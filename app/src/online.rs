//! The workers: load the saved list, sync with MyAnimeList, sign in and out. Each runs on its own
//! short-lived thread and reports to the UI thread with `AppHandle::update_if` (`State::apply`).
//! Nothing here runs in headless mode, and nothing waits when there is no work (no timers).
//!
//! Writes to the account go through the sync queue (`queue.rs`), never from here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kotora_core::config::Config;
use kotora_core::loopback;
use kotora_core::mal::{self, Client, Mal, Pkce, UreqTransport};
use kotora_core::paths;
use kotora_core::service::{ListService, ServiceError};
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::tokens::TokenStore;

use crate::model::{Auth, Fail, Handle, Msg, State};

/// How long a sign-in waits for the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

fn send(h: &Handle, msg: Msg) {
    h.update_if(move |s, _| s.apply(msg));
}

/// What to show first: a saved token means signed in. Kotora has its own Client ID, so signed
/// out is only one click from signed in. `NoClientId` is the first run now (nothing chosen, no
/// own ID, no token): the setup wizard is the page.
pub fn initial_auth() -> Auth {
    let has_token = TokenStore::new(paths::token_file()).load_mal().is_some();
    if has_token {
        return Auth::SignedIn;
    }
    match Config::load(&paths::config_file()) {
        Ok(c) if c.needs_service_choice(false) => Auth::NoClientId,
        _ => Auth::SignedOut,
    }
}

/// Whether the wizard begins with the service choice: not chosen yet, no Client ID in the file and
/// no saved token. Someone who already uses Kotora with MyAnimeList never sees it.
pub fn wants_service_choice() -> bool {
    let has_token = TokenStore::new(paths::token_file()).load_mal().is_some();
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
        let _ = store.queue_start("mal", now);
        if let (Ok(items), Ok(user), Ok(synced_at), Ok(ops)) = (store.load_list("mal"), store.account("mal"), store.synced_at("mal"), store.queue("mal", now)) {
            let picture = store.picture("mal").ok().and_then(|(url, _)| url);
            let profile = store.profile("mal").ok().flatten().map(Box::new);
            send(&h, Msg::Cache { items, user, picture, profile, synced_at, ops });
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
    let id = match Config::load(&paths::config_file()) {
        Ok(c) => match c.effective_client_id() {
            Some(id) => id,
            None => return send(h, Msg::SyncFailed(Fail::Other("No MyAnimeList Client ID.".into()))),
        },
        Err(e) => return send(h, Msg::SyncFailed(Fail::Other(e))),
    };
    let mut service = Mal::new(Client::new(id, UreqTransport::default()), TokenStore::new(paths::token_file()));
    let msg = sync_with(&mut service, &mut store, now_unix());
    let synced = matches!(msg, Msg::Synced { .. });
    send(h, msg);
    if synced {
        for m in refresh_user(&mut service, &store, now_unix()) {
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

/// A signed-in MyAnimeList client for a short worker (search, details, `kot add`): `None` when
/// offline by choice (`KOTORA_OFFLINE`), with no Client ID, or with no token. Shares the app's
/// one rate budget.
pub fn mal_service() -> Option<Mal<UreqTransport>> {
    if paths::offline_forced() {
        return None;
    }
    let id = Config::load(&paths::config_file()).ok()?.effective_client_id()?;
    let tokens = TokenStore::new(paths::token_file());
    tokens.load_mal()?;
    Some(Mal::new(Client::new(id, UreqTransport::default()), tokens))
}

/// A service error in the words a page shows.
pub fn error_text(e: ServiceError) -> String {
    match e {
        ServiceError::Offline(_) => "Can't reach MyAnimeList.".into(),
        ServiceError::SignInAgain => "MyAnimeList needs you to sign in again.".into(),
        ServiceError::RateLimited => "MyAnimeList asked Kotora to slow down. Try again in a minute.".into(),
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
    std::thread::spawn(move || match sign_in_flow(&h, &cancel) {
        Ok((user, picture)) => {
            send(&h, Msg::SignedIn { user, picture });
            // The first list after a sign-in.
            h.update_if(|s, _| {
                s.syncing = false;
                refresh(s);
                true
            });
        }
        Err(m) => send(&h, Msg::SignInFailed(m)),
    });
}

pub fn cancel_sign_in(s: &mut State) {
    if let Some(c) = &s.cancel {
        c.store(true, Ordering::Relaxed);
    }
}

fn sign_in_flow(h: &Handle, cancel: &AtomicBool) -> Result<(String, Option<String>), String> {
    let id = Config::load(&paths::config_file())?.effective_client_id().ok_or("No MyAnimeList Client ID.")?;
    // Bind first: if the port is busy there is no point in opening the browser.
    let listener = loopback::bind().map_err(|e| format!("Can't listen on port {} for MyAnimeList's reply: {e}", mal::PORT))?;
    let pkce = Pkce::new().map_err(|e| format!("Can't make a sign-in code: {e}"))?;
    let url = mal::authorize_url(&id, &pkce);
    // The link shows while Kotora waits, for a browser that didn't open (or opened elsewhere).
    let opened = open_browser(&url).is_ok();
    send(h, Msg::SignInUrl { url, opened });
    let code = loopback::wait_for_code(&listener, &pkce.state, SIGN_IN_TIMEOUT, cancel)?;
    drop(listener);
    let mut service = Mal::new(Client::new(id, UreqTransport::default()), TokenStore::new(paths::token_file()));
    let err = |e: ServiceError| match e {
        ServiceError::SignInAgain => "MyAnimeList refused the sign-in. Try again.".to_string(),
        other => other.to_string(),
    };
    service.complete_sign_in(&code, &pkce.verifier).map_err(err)?;
    let user = service.user().map_err(err)?;
    let store = Store::open(&paths::db_file()).map_err(|e| e.to_string())?;
    store.set_account("mal", &user.name).map_err(|e| e.to_string())?;
    let _ = store.set_picture("mal", user.picture.as_deref(), now_unix());
    Ok((user.name, user.picture))
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

#[cfg(windows)]
fn open_browser(url: &str) -> Result<(), String> {
    crate::win::open(url)
}

#[cfg(not(windows))]
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

/// Sign out: the token file goes, and so does the saved list (the MAL API licence).
pub fn sign_out(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    std::thread::spawn(move || {
        let _ = TokenStore::new(paths::token_file()).delete();
        kotora_core::covers::clear(&kotora_core::covers::dir());
        if let Ok(mut store) = Store::open(&paths::db_file()) {
            let _ = store.clear_service("mal");
        }
        send(&h, Msg::SignedOut);
    });
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
            },
            status: Status::Watching,
            progress: 1,
            score: None,
            rewatching: false,
            times_rewatched: 0,
            updated_at: 10,
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
