//! The background refresh of both phones (iOS `BGAppRefreshTask`, Android `WorkManager`): the
//! waiting changes go, then the active list is fetched again when the saved copy is older than
//! [`LIST_EVERY`]. The desktop fetches the list each time it starts (it starts with the
//! computer); a phone app starts rarely and stays suspended, so the OS wakes it for this instead.
//!
//! One run per wake-up, for every job the app does in the background: the platform's single
//! periodic task calls [`Kotora::background_refresh`] first, then its own checks (Airing and
//! Publishing notifications) over the fresh list. No second scheduler.

use kotora_core::time;

use crate::{FlushReport, Kotora, KotoraError, MediaKind};

/// How old the saved list may get before a background run fetches it again (seconds).
pub const LIST_EVERY: i64 = 60 * 60;

/// What one background run did.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct BackgroundReport {
    /// The queue's run (nothing sent when nothing waited).
    pub flush: FlushReport,
    /// The active list (and the manga list in Manga mode) was fetched again.
    pub synced: bool,
    /// The active service wants a new sign-in: the list can't be fetched, changes wait.
    pub sign_in_again: bool,
    /// Why the list wasn't fetched (offline, rate limited), when it was due.
    pub problem: Option<String>,
}

#[uniffi::export]
impl Kotora {
    /// Is the active list due for a background fetch (never fetched, or older than [`LIST_EVERY`])?
    pub fn list_sync_due(&self) -> bool {
        self.active().is_some() && self.synced_at(MediaKind::Anime).is_none_or(|at| time::now_unix() - at >= LIST_EVERY)
    }

    /// The background run: the waiting changes of every signed-in service, then the active list
    /// when [`Kotora::list_sync_due`]. Network: call it from the worker, off the main thread.
    pub fn background_refresh(&self) -> Result<BackgroundReport, KotoraError> {
        let mut report = BackgroundReport::default();
        if self.active().is_none() {
            return Ok(report);
        }
        if self.waiting_count() > 0 {
            report.flush = self.flush_queue()?;
            report.sign_in_again = report.flush.sign_in_again;
        }
        if !self.list_sync_due() {
            return Ok(report);
        }
        match self.sync() {
            Ok(_) => report.synced = true,
            Err(KotoraError::SignInAgain) => report.sign_in_again = true,
            Err(e @ (KotoraError::Offline { .. } | KotoraError::RateLimited)) => report.problem = Some(e.to_string()),
            Err(e) => return Err(e),
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    //! The stale rule, and the activity log behind the phones' Statistics: mobile edits go
    //! through the core's `Store::enqueue`, which writes the `activity` rows as on the desktop.

    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use kotora_core::{Kind, ListItem, Media, Score, Status};

    use crate::*;

    #[derive(Default)]
    struct Vault(Mutex<HashMap<String, Vec<u8>>>);

    impl Platform for Vault {
        fn utc_offset(&self, _at_unix: i64) -> i32 {
            crate::testenv::OFFSET
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, service: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&service).cloned()
        }
        fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(service, bytes);
            true
        }
        fn token_delete(&self, service: String) {
            self.0.lock().unwrap().remove(&service);
        }
    }

    fn open(name: &str) -> (Arc<Kotora>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-bg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config_dir: dir.join("files").display().to_string(),
            data_dir: dir.join("nobackup").display().to_string(),
            cache_dir: dir.join("cache").display().to_string(),
        };
        let k = Kotora::new(paths, ClientIds { mal: "fake".into(), anilist: "fake".into() }, Arc::new(Vault::default())).unwrap();
        (k, dir)
    }

    fn sign_in(k: &Kotora) {
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save("mal", &t).unwrap();
        k.store().set_account("mal", "made-up-user").unwrap();
    }

    fn show(id: u32, progress: u32, synced_at: i64) -> (ListItem, i64) {
        let media = Media {
            kind: Kind::Anime,
            id,
            title: "Hoshikuzu Relay".into(),
            title_en: None,
            title_ja: None,
            synonyms: Vec::new(),
            episodes: Some(12),
            format: "tv".into(),
            air_status: "currently_airing".into(),
            cover_url: None,
            start_date: None,
            broadcast: None,
            volumes: None,
            authors: Vec::new(),
            next_airing: None,
            id_mal: None,
        };
        let it = ListItem { media, status: Status::Watching, progress, score: Score::from_ten(8), rewatching: false, times_rewatched: 0, volumes: 0, updated_at: 100 };
        (it, synced_at)
    }

    #[test]
    fn the_list_is_due_when_never_fetched_or_older_than_an_hour() {
        let (k, dir) = open("due");
        assert!(!k.list_sync_due(), "signed out: nothing to fetch");
        assert_eq!(k.background_refresh().unwrap(), BackgroundReport::default());
        sign_in(&k);
        assert!(k.list_sync_due(), "never fetched");
        let now = time::now_unix();
        let (it, at) = show(1, 6, now - 60);
        k.store().replace_list("mal", std::slice::from_ref(&it), at).unwrap();
        assert!(!k.list_sync_due(), "fetched a minute ago");
        // Nothing waits and the list is fresh: no request at all (the fake token would fail one).
        assert_eq!(k.background_refresh().unwrap(), BackgroundReport::default());
        k.store().replace_list("mal", &[it], now - LIST_EVERY).unwrap();
        assert!(k.list_sync_due(), "an hour old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plus_one_the_editor_and_undo_write_the_activity_log() {
        let (k, dir) = open("activity");
        sign_in(&k);
        let (it, at) = show(1, 6, 1_000);
        k.store().replace_list("mal", &[it], at).unwrap();
        let ups = |k: &Kotora| k.store().activity(("mal", Kind::Anime)).unwrap().iter().map(|r| (r.from, r.to)).collect::<Vec<_>>();

        let e = k.plus_one(MediaKind::Anime, 1).unwrap();
        assert_eq!(ups(&k), [(6, 7)], "+1 is one episode watched");
        k.undo(e).unwrap();
        assert!(ups(&k).is_empty(), "Undo takes it back");

        let mut ed = k.editor(MediaKind::Anime, 1).unwrap();
        ed = editor_step(ed, 3);
        k.save_editor(ed).unwrap();
        assert_eq!(ups(&k), [(6, 9)], "the editor's Save counts what it raised");
        assert!(k.store().activity_since().unwrap().is_some(), "the log has started (Statistics' 'since')");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
