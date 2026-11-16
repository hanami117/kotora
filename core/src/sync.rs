//! The manga half of a sync. The anime list is fetched on every sync as before; the manga list
//! only once the user turned Manga mode on ([`Store::manga_enabled`], off by default), so
//! people with no manga pay no extra requests. When on, it costs one request per 100 manga.
//!
//! [`complete_media`] fills in list anime saved with only a title (an add from Now Playing).

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

/// The most details requests one [`complete_media`] call makes (the rest wait for the next).
pub const COMPLETE_MAX: usize = 5;

/// Anime on the list saved with only a title ([`Store::bare_media`]) get their whole media
/// (episodes, format, cover, titles): from the saved details when there are any, else one
/// details request each, at most [`COMPLETE_MAX`]. `only`: just these ids (an add just made).
/// Returns the ids filled in. A failed request leaves the row for the next try.
pub fn complete_media(service: &mut dyn ListService, store: &Store, only: Option<&[u32]>, now: i64) -> Vec<u32> {
    let name = service.name();
    let Ok(bare) = store.bare_media(name) else { return Vec::new() };
    let mut done = Vec::new();
    let mut asked = 0;
    for id in bare.into_iter().filter(|id| only.is_none_or(|o| o.contains(id))) {
        let details = match store.details(name, id) {
            // Saved details without a type either: the service has no more to give.
            Ok(Some((d, _))) if d.format.is_empty() => continue,
            Ok(Some((d, _))) => d,
            _ if asked >= COMPLETE_MAX => continue,
            _ => {
                asked += 1;
                match service.details(id) {
                    Ok(d) => {
                        let _ = store.save_details(name, &d, now);
                        d
                    }
                    Err(_) => continue,
                }
            }
        };
        if store.refresh_media(name, &details.media(), now).unwrap_or(false) {
            done.push(id);
        }
    }
    done
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

    /// Answers details for any id, and counts the requests.
    #[derive(Default)]
    struct Info {
        asked: Vec<u32>,
        fail: bool,
    }

    impl ListService for Info {
        fn name(&self) -> &'static str {
            "anilist"
        }
        fn title(&self) -> &'static str {
            "AniList"
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
        fn details(&mut self, id: u32) -> Result<crate::service::Details, ServiceError> {
            self.asked.push(id);
            if self.fail {
                return Err(ServiceError::Offline("no network".into()));
            }
            Ok(crate::service::Details {
                id,
                title: format!("Show {id}"),
                episodes: Some(25),
                format: "TV".into(),
                air_status: "finished_airing".into(),
                cover_url: Some(format!("https://img.example/{id}.jpg")),
                ..Default::default()
            })
        }
    }

    /// The bug: "Add as Watching" from Now Playing saved only the title, so the list showed
    /// "3 / ?", an initials tile and no type until... never (a waiting add isn't in the fetched
    /// list). The add is filled in from the service; a full row is left alone.
    #[test]
    fn an_add_with_only_a_title_is_filled_in() {
        let mut s = Store::open_memory().unwrap();
        let full = Media { id: 2, title: "Full".into(), episodes: Some(12), format: "tv".into(), cover_url: Some("https://img.example/2.jpg".into()), ..Default::default() };
        s.add_to_list("anilist", &full, Status::Watching, None, 10).unwrap();
        // As Now Playing adds it: the title, and the cover recognition saw.
        let bare = Media { id: 21355, title: "Re:Zero".into(), cover_url: Some("https://img.example/seen.jpg".into()), ..Default::default() };
        s.add_to_list("anilist", &bare, Status::Watching, Some(3), 10).unwrap();
        assert_eq!(s.bare_media("anilist").unwrap(), [21355]);
        let mut svc = Info { fail: true, ..Default::default() };
        assert!(complete_media(&mut svc, &s, None, 20).is_empty(), "offline: left for the next try");
        assert_eq!(s.bare_media("anilist").unwrap(), [21355]);
        svc.fail = false;
        assert_eq!(complete_media(&mut svc, &s, Some(&[21355]), 30), [21355]);
        assert_eq!(svc.asked, [21355, 21355], "only the bare one is asked for");
        let it = s.load_item("anilist", 21355).unwrap().unwrap();
        assert_eq!((it.media.episodes, it.media.format.as_str(), it.media.cover_url.as_deref(), it.progress), (Some(25), "TV", Some("https://img.example/21355.jpg"), 3));
        assert!(s.bare_media("anilist").unwrap().is_empty());
        assert_eq!(s.load_item("anilist", 2).unwrap().unwrap().media.title, "Full", "the full row is untouched");
        // Nothing bare: no request.
        assert!(complete_media(&mut svc, &s, None, 40).is_empty());
        assert_eq!(svc.asked.len(), 2);
        // A show the service has no type for: asked once, then its saved details say so.
        s.add_to_list("anilist", &Media { id: 9, title: "TBA".into(), ..Default::default() }, Status::PlanToWatch, None, 50).unwrap();
        s.save_details("anilist", &crate::service::Details { id: 9, title: "TBA".into(), ..Default::default() }, 50).unwrap();
        assert!(complete_media(&mut svc, &s, None, 60).is_empty());
        assert_eq!(svc.asked.len(), 2, "no request for it");
    }

    /// Saved details fill a bare row without a request; requests stop at COMPLETE_MAX.
    #[test]
    fn saved_details_first_and_few_requests_at_once() {
        let mut s = Store::open_memory().unwrap();
        for id in 1..=8 {
            s.add_to_list("anilist", &Media { id, title: format!("T{id}"), ..Default::default() }, Status::Watching, None, 1).unwrap();
        }
        let saved = crate::service::Details { id: 1, title: "T1".into(), format: "Movie".into(), episodes: Some(1), cover_url: Some("https://img.example/1.jpg".into()), ..Default::default() };
        s.save_details("anilist", &saved, 1).unwrap();
        let mut svc = Info::default();
        let done = complete_media(&mut svc, &s, None, 2);
        assert_eq!(done.len(), 1 + COMPLETE_MAX);
        assert!(!svc.asked.contains(&1), "the saved details were used");
        assert_eq!(svc.asked.len(), COMPLETE_MAX);
        assert_eq!(s.load_item("anilist", 1).unwrap().unwrap().media.format, "Movie");
        assert_eq!(s.bare_media("anilist").unwrap().len(), 8 - 1 - COMPLETE_MAX, "the rest wait for the next call");
    }
}
