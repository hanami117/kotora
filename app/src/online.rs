//! The workers: load the saved list, sync with MyAnimeList, sign in and out. Each runs on its own
//! short-lived thread and reports to the UI thread with `AppHandle::update_if` (`State::apply`).
//! Nothing here runs in headless mode, and nothing waits when there is no work (no timers).
//!
//! Read only: stage 2 never writes to the MAL account.

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

/// What to show first: the Client ID and the token file decide.
pub fn initial_auth() -> Auth {
    match Config::load(&paths::config_file()) {
        Ok(c) if c.mal_client_id.is_some() => {
            if TokenStore::new(paths::token_file()).load_mal().is_some() { Auth::SignedIn } else { Auth::SignedOut }
        }
        _ => Auth::NoClientId,
    }
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
        if let (Ok(items), Ok(user), Ok(synced_at)) = (store.load_list("mal"), store.account("mal"), store.synced_at("mal")) {
            send(&h, Msg::Cache { items, user, synced_at });
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
        Ok(Config { mal_client_id: Some(id) }) => id,
        Ok(_) => return send(h, Msg::SyncFailed(Fail::Other("No MyAnimeList Client ID in config.toml.".into()))),
        Err(e) => return send(h, Msg::SyncFailed(Fail::Other(e))),
    };
    let mut service = Mal::new(Client::new(id, UreqTransport::default()), TokenStore::new(paths::token_file()));
    send(h, sync_with(&mut service, &mut store, now_unix()));
}

/// Fetches the list, saves it, and says what happened. Pure of threads, so tests drive it.
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
    Msg::Synced { items, synced_at: now }
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
    std::thread::spawn(move || match sign_in_flow(&h, &cancel) {
        Ok(user) => {
            send(&h, Msg::SignedIn { user });
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

fn sign_in_flow(_h: &Handle, cancel: &AtomicBool) -> Result<String, String> {
    let id = Config::load(&paths::config_file())?.mal_client_id.ok_or("No MyAnimeList Client ID in config.toml.")?;
    // Bind first: if the port is busy there is no point in opening the browser.
    let listener = loopback::bind().map_err(|e| format!("Can't listen on port {} for MyAnimeList's reply: {e}", mal::PORT))?;
    let pkce = Pkce::new().map_err(|e| format!("Can't make a sign-in code: {e}"))?;
    open_browser(&mal::authorize_url(&id, &pkce))?;
    let code = loopback::wait_for_code(&listener, &pkce.state, SIGN_IN_TIMEOUT, cancel)?;
    drop(listener);
    let mut service = Mal::new(Client::new(id, UreqTransport::default()), TokenStore::new(paths::token_file()));
    let err = |e: ServiceError| match e {
        ServiceError::SignInAgain => "MyAnimeList refused the sign-in. Try again.".to_string(),
        other => other.to_string(),
    };
    service.complete_sign_in(&code, &pkce.verifier).map_err(err)?;
    let user = service.user_name().map_err(err)?;
    let store = Store::open(&paths::db_file()).map_err(|e| e.to_string())?;
    store.set_account("mal", &user).map_err(|e| e.to_string())?;
    Ok(user)
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

/// Sign out: the token file goes, and so does the saved list (the MAL API licence).
pub fn sign_out(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    std::thread::spawn(move || {
        let _ = TokenStore::new(paths::token_file()).delete();
        if let Ok(mut store) = Store::open(&paths::db_file()) {
            let _ = store.clear_service("mal");
        }
        send(&h, Msg::SignedOut);
    });
}

/// "Check again" on the no-Client-ID screen: read `config.toml` once more.
pub fn recheck_config(s: &mut State) {
    if s.auth == Auth::NoClientId && initial_auth() != Auth::NoClientId {
        s.auth = Auth::SignedOut;
    }
}

#[cfg(test)]
mod tests {
    use kotora_core::{ListItem, Media, Status};

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
            },
            status: Status::Watching,
            progress: 1,
            score: None,
            rewatching: false,
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
