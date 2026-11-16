//! The manga half of a sync. The anime list is fetched on every sync as before; the manga list
//! only once the user turned Manga mode on ([`Store::manga_enabled`], off by default), so
//! people with no manga pay no extra requests. When on, it costs one request per 100 manga.

use crate::service::{ListService, ServiceError};
use crate::store::Store;
use crate::{Kind, ListItem, Scope};

#[derive(Debug, PartialEq)]
pub enum MangaSync {
    /// Manga mode is off: nothing was asked.
    Off,
    /// The saved manga list, with the waiting changes laid over it.
    Synced { items: Vec<ListItem>, synced_at: i64 },
    /// The service failed; the saved manga list is kept as it was.
    Failed(ServiceError),
    /// The store failed (the list could not be saved or read).
    NotSaved(String),
}

/// Fetches and saves the manga list of `service` if Manga mode is on. Pure of threads: the
/// caller runs it on its sync worker, after the anime list.
pub fn sync_manga(service: &mut dyn ListService, store: &mut Store, now: i64) -> MangaSync {
    match store.manga_enabled() {
        Ok(true) => {}
        Ok(false) => return MangaSync::Off,
        Err(e) => return MangaSync::NotSaved(e.to_string()),
    }
    let items = match service.fetch_manga_list() {
        Ok(i) => i,
        Err(e) => return MangaSync::Failed(e),
    };
    let scope = Scope::new(service.name(), Kind::Manga);
    if let Err(e) = store.replace_list(scope, &items, now) {
        return MangaSync::NotSaved(format!("can't save the manga list: {e}"));
    }
    match store.load_list(scope) {
        Ok(items) => MangaSync::Synced { items, synced_at: now },
        Err(e) => MangaSync::NotSaved(format!("can't read the saved manga list: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Change, Media, Remote, Status};

    /// Counts the list requests; the manga list holds one made-up manga.
    #[derive(Default)]
    struct Svc {
        manga_calls: u32,
        fail: bool,
    }

    impl ListService for Svc {
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
            Ok(vec![])
        }
        fn get_entry(&mut self, _: u32) -> Result<Option<Remote>, ServiceError> {
            Ok(None)
        }
        fn update_entry(&mut self, _: u32, _: &Change) -> Result<Remote, ServiceError> {
            Err(ServiceError::Other("no".into()))
        }
        fn fetch_manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
            self.manga_calls += 1;
            if self.fail {
                return Err(ServiceError::Offline("no network".into()));
            }
            Ok(vec![ListItem {
                media: Media { kind: Kind::Manga, id: 2001, title: "Paper Lanterns".into(), volumes: Some(14), ..Default::default() },
                status: Status::Watching,
                progress: 21,
                score: None,
                rewatching: false,
                times_rewatched: 0,
                volumes: 3,
                updated_at: 50,
            }])
        }
    }

    #[test]
    fn off_by_default_asks_nothing() {
        let mut s = Store::open_memory().unwrap();
        let mut svc = Svc::default();
        assert_eq!(sync_manga(&mut svc, &mut s, 100), MangaSync::Off);
        assert_eq!(svc.manga_calls, 0, "no manga request");
        assert_eq!(s.synced_at(("mal", Kind::Manga)).unwrap(), None);
    }

    #[test]
    fn on_it_fetches_and_saves_the_manga_list() {
        let mut s = Store::open_memory().unwrap();
        s.set_manga_enabled(true).unwrap();
        let mut svc = Svc::default();
        let MangaSync::Synced { items, synced_at } = sync_manga(&mut svc, &mut s, 100) else { panic!("not synced") };
        assert_eq!((items.len(), items[0].volumes, items[0].media.kind, synced_at), (1, 3, Kind::Manga, 100));
        assert_eq!(svc.manga_calls, 1);
        assert!(s.load_list("mal").unwrap().is_empty(), "the anime list is not touched");
        // Offline: the saved manga list stays.
        svc.fail = true;
        assert!(matches!(sync_manga(&mut svc, &mut s, 200), MangaSync::Failed(ServiceError::Offline(_))));
        assert_eq!(s.load_list(("mal", Kind::Manga)).unwrap().len(), 1);
        assert_eq!(s.synced_at(("mal", Kind::Manga)).unwrap(), Some(100));
    }
}
