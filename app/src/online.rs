//! The workers: load the saved list, sync with the active service (MyAnimeList or AniList), sign
//! in and out, switch services. Each runs on its own
//! short-lived thread and reports to the UI thread with `AppHandle::update_if` (`State::apply`).
//! Nothing here runs in headless mode, and nothing waits when there is no work (no timers).
//!
//! Writes to the account go through the sync queue (`queue.rs`), never from here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kotora_core::anilist::{self, AniList};
use kotora_core::config::Config;
use kotora_core::loopback;
use kotora_core::mal::{self, Client, Mal, Pkce, UreqTransport};
use kotora_core::paths;
use kotora_core::service::{ListService, ServiceError, ServiceId, User};
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::tokens::TokenStore;

use crate::account;

use crate::model::{Auth, Fail, Handle, Msg, State};

/// How long a sign-in waits for the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

fn send(h: &Handle, msg: Msg) {
    h.update_if(move |s, _| s.apply(msg));
}

/// What to show first for the active service: the Client ID and the token file decide.
pub fn initial_auth() -> Auth {
    auth_for(account::active())
}

/// MyAnimeList needs its Client ID for every request (the token refresh); AniList only to sign
/// in, so a saved AniList token is enough.
pub fn auth_for(id: ServiceId) -> Auth {
    let cfg = Config::load(&paths::config_file()).unwrap_or_default();
    let has_id = cfg.has_client_id(id);
    match (id, account::has_token(id), has_id) {
        (ServiceId::AniList, true, _) | (ServiceId::Mal, true, true) => Auth::SignedIn,
        (_, _, true) => Auth::SignedOut,
        _ => Auth::NoClientId,
    }
}

/// Whether the wizard begins with the service choice: not chosen yet, no Client ID in the file and
/// no saved token. Someone who already uses Kotora with MyAnimeList never sees it.
pub fn wants_service_choice() -> bool {
    let has_token = ServiceId::ALL.into_iter().any(account::has_token);
    Config::load(&paths::config_file()).is_ok_and(|c| c.needs_service_choice(has_token))
}

/// At start: show the saved list, then sync if signed in.
pub fn start(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    s.offline = paths::offline_forced();
    s.syncing = s.auth == Auth::SignedIn && !s.offline;
    let online = s.syncing;
    std::thread::spawn(move || {
        let store = match Store::open(&paths::db_file()) {
            Ok(st) => st,
            Err(e) => return send(&h, Msg::SyncFailed(Fail::Other(format!("Can't open the saved list: {e}")))),
        };
        // The saved list first, so the app is useful before (and without) the network.
        let now = now_unix();
        let svc = account::name();
        let _ = store.queue_start(svc, now);
        if let (Ok(items), Ok(user), Ok(synced_at), Ok(ops)) = (store.load_list(svc), store.account(svc), store.synced_at(svc), store.queue(svc, now)) {
            let picture = store.picture(svc).ok().and_then(|(url, _)| url);
            let profile = store.profile(svc).ok().flatten().map(Box::new);
            // A switch meanwhile: this cache is the other service's.
            if svc == account::name() {
                send(&h, Msg::Cache { items, user, picture, profile, synced_at, ops });
            }
        }
        if online { sync_now(&h, store) } else { send(&h, Msg::Idle) }
    });
}

/// Refresh (the button, and Retry): one sync on a worker.
pub fn refresh(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    if s.syncing || s.auth != Auth::SignedIn || paths::offline_forced() {
        return;
    }
    s.syncing = true;
    s.error = None;
    std::thread::spawn(move || match Store::open(&paths::db_file()) {
        Ok(store) => sync_now(&h, store),
        Err(e) => send(&h, Msg::SyncFailed(Fail::Other(format!("Can't open the saved list: {e}")))),
    });
}

fn sync_now(h: &Handle, mut store: Store) {
    let active = account::active();
    let Some(mut service) = account::open() else {
        let fail = match Config::load(&paths::config_file()) {
            Err(e) => Fail::Other(e),
            Ok(c) if active == ServiceId::Mal && !c.has_client_id(ServiceId::Mal) => Fail::Other("No MyAnimeList Client ID in config.toml.".into()),
            Ok(_) => Fail::SignInAgain,
        };
        return send(h, Msg::SyncFailed(fail));
    };
    let msg = sync_with(service.as_mut(), &mut store, now_unix());
    // Switched to the other service while this one synced: its answer is not shown.
    if account::active() != active {
        return send(h, Msg::Idle);
    }
    let synced = matches!(msg, Msg::Synced { .. });
    send(h, msg);
    if synced {
        // The manga list, only once Manga mode was turned on (off by default: no request), and
        // only on MyAnimeList (AniList is anime only for now). A failure never fails the sync.
        if active == ServiceId::Mal {
            let _ = kotora_core::sync::sync_manga(service.as_mut(), &mut store, now_unix());
        }
        for m in refresh_user(service.as_mut(), &store, now_unix()) {
            send(h, m);
        }
    }
}

/// After a good sync: the user's name, picture, join date and anime statistics again (one
/// request, through the same rate budget as the list). The picture URL and the Profile page's
/// facts are saved; the image itself is a cover-cache file (`art.rs`).
pub fn refresh_user(service: &mut dyn ListService, store: &Store, now: i64) -> Vec<Msg> {
    let name = service.name();
    let Ok(user) = service.user() else { return Vec::new() };
    let _ = store.set_account(name, &user.name);
    let _ = store.set_picture(name, user.picture.as_deref(), now);
    let _ = store.set_profile(name, &user.profile);
    vec![Msg::Picture(user.picture), Msg::Profile(Box::new(user.profile))]
}

/// Fetches the list, saves it, and says what happened. The list sent to the UI is the saved one,
/// with the changes still in the queue laid over it. Pure of threads, so tests drive it.
pub fn sync_with(service: &mut dyn ListService, store: &mut Store, now: i64) -> Msg {
    let title = service.title();
    let fail = |e: ServiceError| {
        Msg::SyncFailed(match e {
            ServiceError::SignInAgain => Fail::SignInAgain,
            ServiceError::Offline(_) => Fail::Offline(format!("Can't reach {}. Showing your saved list.", title)),
            ServiceError::RateLimited => Fail::Other(format!("{} asked Kotora to slow down. Try again in a minute.", title)),
            ServiceError::Other(m) => Fail::Other(m),
        })
    };
    let items = match service.fetch_list() {
        Ok(i) => i,
        Err(e) => return fail(e),
    };
    let name = service.name();
    if let Err(e) = store.replace_list(name, &items, now) {
        return Msg::SyncFailed(Fail::Other(format!("Can't save the list: {e}")));
    }
    match store.load_list(name) {
        Ok(items) => Msg::Synced { items, synced_at: now },
        Err(e) => Msg::SyncFailed(Fail::Other(format!("Can't read the saved list: {e}"))),
    }
}

/// A service error in the words a page shows.
pub fn error_text(e: ServiceError) -> String {
    let title = account::active().title();
    match e {
        ServiceError::Offline(_) => format!("Can't reach {title}."),
        ServiceError::SignInAgain => format!("{title} needs you to sign in again."),
        ServiceError::RateLimited => format!("{title} asked Kotora to slow down. Try again in a minute."),
        ServiceError::Other(m) => m,
    }
}

// -------------------------------------------------------------------------------------- sign in

pub fn sign_in(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    if matches!(s.auth, Auth::SigningIn | Auth::NoClientId) {
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    s.cancel = Some(cancel.clone());
    s.auth = Auth::SigningIn;
    s.error = None;
    s.sign_in_link = None;
    s.sign_in_seq += 1;
    let seq = s.sign_in_seq;
    let service = s.service;
    std::thread::spawn(move || {
        let r = match service {
            ServiceId::Mal => sign_in_flow(&h, seq, &cancel),
            ServiceId::AniList => anilist_flow(&h, seq, &cancel),
        };
        signed_in(&h, seq, r);
    });
}

/// The end of a sign-in: the account shows and the first list is asked for. A sign-in that a
/// pasted token (or a new attempt) took over says nothing.
fn signed_in(h: &Handle, seq: u64, r: Result<(String, Option<String>), String>) {
    h.update_if(move |s, _| {
        if s.sign_in_seq != seq {
            return false;
        }
        match r {
            Ok((user, picture)) => {
                s.apply(Msg::SignedIn { user, picture });
                // The first list after a sign-in.
                s.syncing = false;
                refresh(s);
                true
            }
            Err(m) => s.apply(Msg::SignInFailed(m)),
        }
    });
}

fn send_link(h: &Handle, seq: u64, url: String, opened: bool) {
    h.update_if(move |s, _| s.sign_in_seq == seq && s.apply(Msg::SignInUrl { url, opened }));
}

pub fn cancel_sign_in(s: &mut State) {
    if let Some(c) = &s.cancel {
        c.store(true, Ordering::Relaxed);
    }
}

/// AniList: a token pasted from AniList's page (or the address the browser ended on). Ends
/// the browser wait, checks the token with AniList and saves it.
pub fn sign_in_with_token(s: &mut State) {
    let raw = s.client_id.token.text.clone();
    let token = match anilist::parse_token_input(&raw, now_unix()) {
        Ok(t) => t,
        Err(e) => {
            s.client_id.token_error = Some(e);
            return;
        }
    };
    s.client_id.token_error = None;
    let Some(h) = s.handle.clone() else { return };
    cancel_sign_in(s);
    s.sign_in_seq += 1;
    let seq = s.sign_in_seq;
    s.auth = Auth::SigningIn;
    s.error = None;
    s.client_id.token.clear();
    std::thread::spawn(move || signed_in(&h, seq, finish_anilist(&token)));
}

/// Checks an AniList token, saves it and the account. The user name and picture.
fn finish_anilist(token: &kotora_core::tokens::Token) -> Result<(String, Option<String>), String> {
    let mut service = AniList::new(anilist::Client::new(UreqTransport::default()), TokenStore::new(paths::token_file()));
    let user = service.complete_sign_in(token).map_err(|e| match e {
        ServiceError::SignInAgain => "AniList refused the token. Sign in again.".to_string(),
        other => other.to_string(),
    })?;
    save_account(ServiceId::AniList, &user)
}

fn save_account(id: ServiceId, user: &User) -> Result<(String, Option<String>), String> {
    let store = Store::open(&paths::db_file()).map_err(|e| e.to_string())?;
    store.set_account(id.name(), &user.name).map_err(|e| e.to_string())?;
    let _ = store.set_picture(id.name(), user.picture.as_deref(), now_unix());
    let _ = store.set_profile(id.name(), &user.profile);
    Ok((user.name.clone(), user.picture.clone()))
}

/// AniList's implicit grant: AniList's page, then its redirect to the loopback page, which
/// posts the token back. A client registered with AniList's PIN page instead shows the token
/// there, to paste ([`sign_in_with_token`]).
fn anilist_flow(h: &Handle, seq: u64, cancel: &AtomicBool) -> Result<(String, Option<String>), String> {
    let id = Config::load(&paths::config_file())?.effective_client_id(ServiceId::AniList).ok_or("No AniList Client ID.")?;
    let url = anilist::authorize_url(&id);
    let listener = match loopback::bind() {
        Ok(l) => l,
        Err(e) => {
            // The port is busy: the page still opens, and the address it ends on (or a token)
            // can be pasted. Pasting ends this wait.
            let opened = open_browser(&url).is_ok();
            send_link(h, seq, url, opened);
            let until = std::time::Instant::now() + SIGN_IN_TIMEOUT;
            while !cancel.load(Ordering::Relaxed) && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(200));
            }
            return Err(if cancel.load(Ordering::Relaxed) { "sign-in cancelled".to_string() } else { format!("Can't listen on port {} for AniList's reply: {e}", mal::PORT) });
        }
    };
    let opened = open_browser(&url).is_ok();
    send_link(h, seq, url, opened);
    let posted = loopback::wait_for_token(&listener, SIGN_IN_TIMEOUT, cancel)?;
    drop(listener);
    let token = anilist::parse_token_input(&posted, now_unix())?;
    finish_anilist(&token)
}

fn sign_in_flow(h: &Handle, seq: u64, cancel: &AtomicBool) -> Result<(String, Option<String>), String> {
    let id = Config::load(&paths::config_file())?.effective_client_id(ServiceId::Mal).ok_or("No MyAnimeList Client ID in config.toml.")?;
    // Bind first: if the port is busy there is no point in opening the browser.
    let listener = loopback::bind().map_err(|e| format!("Can't listen on port {} for MyAnimeList's reply: {e}", mal::PORT))?;
    let pkce = Pkce::new().map_err(|e| format!("Can't make a sign-in code: {e}"))?;
    let url = mal::authorize_url(&id, &pkce);
    // The link shows while Kotora waits, for a browser that didn't open (or opened elsewhere).
    let opened = open_browser(&url).is_ok();
    send_link(h, seq, url, opened);
    let code = loopback::wait_for_code(&listener, &pkce.state, SIGN_IN_TIMEOUT, cancel)?;
    drop(listener);
    let mut service = Mal::new(Client::new(id, UreqTransport::default()), TokenStore::new(paths::token_file()));
    let err = |e: ServiceError| match e {
        ServiceError::SignInAgain => "MyAnimeList refused the sign-in. Try again.".to_string(),
        other => other.to_string(),
    };
    service.complete_sign_in(&code, &pkce.verifier).map_err(err)?;
    let user = service.user().map_err(err)?;
    save_account(ServiceId::Mal, &user)
}

/// A link in Settings: opened by the system browser on a short thread (never in headless runs).
pub fn open_link(s: &mut State, url: &'static str) {
    if s.handle.is_none() {
        return;
    }
    std::thread::spawn(move || {
        let _ = open_browser(url);
    });
}

/// [`open_link`] for a URL made at run time (an anime's MyAnimeList page).
pub fn open_url(s: &mut State, url: String) {
    if s.handle.is_none() {
        return;
    }
    std::thread::spawn(move || {
        let _ = open_browser(&url);
    });
}

fn open_browser(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| format!("Can't open your browser ({program} is missing)."))?;
    let _ = child.wait();
    Ok(())
}

/// Sign out of the active service: its token goes, and so does its saved list (the MAL API
/// licence; AniList's terms: no copy of the list of our own). The other service's token and
/// list stay.
pub fn sign_out(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let id = s.service;
    std::thread::spawn(move || {
        let _ = TokenStore::new(paths::token_file()).forget(id.name());
        kotora_core::covers::clear(&kotora_core::covers::dir());
        if let Ok(mut store) = Store::open(&paths::db_file()) {
            let _ = store.clear_service(id.name());
        }
        send(&h, Msg::SignedOut);
    });
}

/// Settings → Account → "Use AniList" (or MyAnimeList): that service becomes the active one.
/// Nothing is deleted: each service keeps its token, saved list and waiting changes, and
/// switching back shows them again. The choice is saved in `[account] service`.
pub fn switch_service(s: &mut State, id: ServiceId) {
    if s.service == id {
        return;
    }
    cancel_sign_in(s);
    s.sign_in_seq += 1;
    s.forget_account();
    s.service = id;
    if s.handle.is_none() {
        // Tests and scenes: the state only.
        s.auth = Auth::SignedOut;
        return;
    }
    account::set_active(id);
    std::thread::spawn(move || {
        if let Err(e) = kotora_core::config::save_service(&paths::config_file(), id.name()) {
            eprintln!("kotora: {e}");
        }
    });
    s.auth = initial_auth();
    s.client_id.hint = crate::clientid::saved_hint(id);
    s.loading = s.auth == Auth::SignedIn;
    s.refresh();
    start(s);
    s.queue_cmd(crate::queue::Cmd::Kick { refresh: true });
}

/// "Check again" on the no-Client-ID screen: read `config.toml` once more.
pub fn recheck_config(s: &mut State) {
    if s.auth == Auth::NoClientId && initial_auth() != Auth::NoClientId {
        s.auth = Auth::SignedOut;
    }
}

#[cfg(test)]
mod tests {
    use kotora_core::{Change, ListItem, Media, Remote, Status};

    use super::*;

    struct Fake(Result<Vec<ListItem>, ServiceError>);

    impl ListService for Fake {
        fn name(&self) -> &'static str {
            "mal"
        }
        fn title(&self) -> &'static str {
            "MyAnimeList"
        }
        fn user_name(&mut self) -> Result<String, ServiceError> {
            Ok("demo_user".into())
        }
        fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
            self.0.clone()
        }
        fn get_entry(&mut self, _: u32) -> Result<Option<Remote>, ServiceError> {
            unreachable!("a sync reads the whole list")
        }
        fn update_entry(&mut self, _: u32, _: &Change) -> Result<Remote, ServiceError> {
            unreachable!("a sync never writes")
        }
    }

    fn item(id: u32) -> ListItem {
        ListItem {
            media: Media {
                id,
                title: format!("Show {id}"),
                title_en: None,
                title_ja: None,
                synonyms: vec![],
                episodes: Some(12),
                format: "tv".into(),
                air_status: "finished_airing".into(),
                cover_url: None,
                start_date: None,
                broadcast: None,
                authors: Vec::new(),
                kind: Default::default(),
                volumes: None,
                next_airing: None,
                id_mal: None,
            },
            status: Status::Watching,
            progress: 1,
            score: None,
            rewatching: false,
            times_rewatched: 0,
            updated_at: 10,
            volumes: 0,
        }
    }

    #[test]
    fn a_good_sync_saves_and_reports() {
        let mut store = Store::open_memory().unwrap();
        let msg = sync_with(&mut Fake(Ok(vec![item(1), item(2)])), &mut store, 500);
        assert!(matches!(&msg, Msg::Synced { items, synced_at: 500 } if items.len() == 2));
        assert_eq!(store.load_list("mal").unwrap().len(), 2);
        assert_eq!(store.synced_at("mal").unwrap(), Some(500));
    }

    #[test]
    fn a_sync_shows_changes_that_still_wait() {
        let mut store = Store::open_memory().unwrap();
        store.replace_list("mal", &[item(1)], 1).unwrap();
        store.enqueue("mal", 1, Change { progress: Some(4), ..Default::default() }, 2).unwrap();
        match sync_with(&mut Fake(Ok(vec![item(1)])), &mut store, 500) {
            Msg::Synced { items, .. } => assert_eq!(items[0].progress, 4),
            m => panic!("{m:?}"),
        }
    }

    /// The picture is asked for after a sync at most once a day, and saved.
    #[test]
    fn the_user_refreshes_with_each_sync() {
        struct Who(u32);
        impl ListService for Who {
            fn name(&self) -> &'static str {
                "mal"
            }
            fn title(&self) -> &'static str {
                "MyAnimeList"
            }
            fn user_name(&mut self) -> Result<String, ServiceError> {
                unreachable!()
            }
            fn user(&mut self) -> Result<kotora_core::service::User, ServiceError> {
                self.0 += 1;
                let stats = kotora_core::service::AnimeStats { completed: self.0, ..Default::default() };
                let profile = kotora_core::service::Profile { joined: Some("2019-03-02".into()), stats: Some(stats) };
                Ok(kotora_core::service::User { name: "demo_user".into(), picture: Some(format!("https://cdn.example.invalid/u.jpg?t={}", self.0)), profile })
            }
            fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
                unreachable!()
            }
            fn get_entry(&mut self, _: u32) -> Result<Option<Remote>, ServiceError> {
                unreachable!()
            }
            fn update_entry(&mut self, _: u32, _: &Change) -> Result<Remote, ServiceError> {
                unreachable!()
            }
        }
        let store = Store::open_memory().unwrap();
        let mut who = Who(0);
        let m = refresh_user(&mut who, &store, 1000);
        assert!(matches!(&m[0], Msg::Picture(Some(u)) if u.ends_with("t=1")));
        assert!(matches!(&m[1], Msg::Profile(p) if p.stats.unwrap().completed == 1));
        let m = refresh_user(&mut who, &store, 1060);
        assert_eq!(who.0, 2, "one request per sync");
        assert!(matches!(&m[0], Msg::Picture(Some(u)) if u.ends_with("t=2")));
        assert_eq!(store.picture("mal").unwrap(), (Some("https://cdn.example.invalid/u.jpg?t=2".into()), Some(1060)));
        assert_eq!(store.profile("mal").unwrap().and_then(|p| p.stats).map(|s| s.completed), Some(2));
        assert_eq!(store.account("mal").unwrap().as_deref(), Some("demo_user"));
    }

    /// The same fake under another service's name.
    struct Named(&'static str, Vec<ListItem>);

    impl ListService for Named {
        fn name(&self) -> &'static str {
            self.0
        }
        fn title(&self) -> &'static str {
            "Fake"
        }
        fn user_name(&mut self) -> Result<String, ServiceError> {
            Ok("demo_user".into())
        }
        fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
            Ok(self.1.clone())
        }
        fn get_entry(&mut self, _: u32) -> Result<Option<Remote>, ServiceError> {
            unreachable!()
        }
        fn update_entry(&mut self, _: u32, _: &Change) -> Result<Remote, ServiceError> {
            unreachable!()
        }
    }

    #[test]
    fn each_service_keeps_its_own_list() {
        let mut store = Store::open_memory().unwrap();
        assert!(matches!(sync_with(&mut Named("mal", vec![item(1)]), &mut store, 100), Msg::Synced { .. }));
        assert!(matches!(sync_with(&mut Named("anilist", vec![item(1), item(2)]), &mut store, 200), Msg::Synced { items, .. } if items.len() == 2));
        assert_eq!((store.load_list("mal").unwrap().len(), store.load_list("anilist").unwrap().len()), (1, 2), "the same id on both, kept apart");
        store.enqueue("anilist", 2, Change { progress: Some(5), ..Default::default() }, 300).unwrap();
        assert!(store.queue("mal", 300).unwrap().is_empty(), "each service its own queue");
        // Signing out of AniList keeps MyAnimeList's list.
        store.clear_service("anilist").unwrap();
        assert_eq!((store.load_list("mal").unwrap().len(), store.load_list("anilist").unwrap().len()), (1, 0));
        assert_eq!(store.synced_at("mal").unwrap(), Some(100));
    }

    #[test]
    fn a_switch_shows_nothing_of_the_other_service() {
        let mut s = State::fake();
        assert_eq!(s.service, ServiceId::Mal);
        let seq = s.sign_in_seq;
        switch_service(&mut s, ServiceId::AniList);
        assert_eq!(s.service, ServiceId::AniList);
        assert!(s.entries.is_empty() && s.account.is_none() && s.ops.is_empty() && s.queue == 0);
        assert!(s.sign_in_seq > seq, "an AniList sign-in under way would be dropped");
        switch_service(&mut s, ServiceId::AniList);
        assert_eq!(s.service, ServiceId::AniList, "the same service: nothing happens");
    }

    #[test]
    fn failures_keep_the_saved_list() {
        let mut store = Store::open_memory().unwrap();
        store.replace_list("mal", &[item(1)], 1).unwrap();
        for (err, want) in [
            (ServiceError::Offline("x".into()), Fail::Offline("Can't reach MyAnimeList. Showing your saved list.".into())),
            (ServiceError::SignInAgain, Fail::SignInAgain),
            (ServiceError::Other("boom".into()), Fail::Other("boom".into())),
        ] {
            match sync_with(&mut Fake(Err(err)), &mut store, 9) {
                Msg::SyncFailed(f) => assert_eq!(f, want),
                m => panic!("{m:?}"),
            }
        }
        assert!(matches!(sync_with(&mut Fake(Err(ServiceError::RateLimited)), &mut store, 9), Msg::SyncFailed(Fail::Other(m)) if m.contains("slow down")));
        assert_eq!(store.load_list("mal").unwrap().len(), 1);
        assert_eq!(store.synced_at("mal").unwrap(), Some(1));
    }
}
