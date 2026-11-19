//! Where Kotora opens (Settings → General → On startup): Home or the last page, and the
//! Anime | Manga switch. `plan` decides from `[startup]` and `[display] list_kind`; the page left
//! open is kept in `[startup] last_page` when the window closes or hides.

use kotora_core::config::{Config, OpenOn, StartMode};
use kotora_core::{Kind, Status};

use crate::model::{Page, State};

/// What the start shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The Anime | Manga switch.
    pub manga: bool,
    pub page: Page,
    /// A details page to open once the list has it (an entry, so one that was removed is gone).
    pub detail: Option<(Kind, u32)>,
}

/// The page as `last_page` keeps it; `None` for pages that are never restored (the wizard).
pub fn page_key(page: Page, detail: Option<(Kind, u32)>) -> Option<String> {
    Some(match page {
        Page::Home | Page::Search => "home".into(),
        Page::Setup => return None,
        Page::Profile => "profile".into(),
        Page::NowPlaying => "now-playing".into(),
        Page::Queue => "queue".into(),
        Page::Settings => "settings".into(),
        Page::Airing => "airing".into(),
        Page::Publishing => "publishing".into(),
        Page::Stats => "stats".into(),
        // Feeds may be off by the next start: Home.
        Page::Feeds => "home".into(),
        Page::List(st) => format!("list:{}", st.mal_name()),
        Page::Manga(st) => format!("manga:{}", st.mal_name()),
        Page::Details => match detail {
            Some((k, id)) => format!("details:{}:{id}", k.name()),
            None => "home".into(),
        },
    })
}

/// A saved page in the terms of the switch (`manga`); `None` when it no longer makes sense.
fn restore(key: &str, manga: bool) -> Option<(Page, Option<(Kind, u32)>)> {
    let list = |st: &str| Status::from_mal(st);
    let page = match key {
        "home" => Page::Home,
        "profile" => Page::Profile,
        "now-playing" => Page::NowPlaying,
        "queue" => Page::Queue,
        "settings" => Page::Settings,
        "stats" => Page::Stats,
        // Airing and Publishing are one place in the sidebar: the switch picks.
        "airing" | "publishing" => {
            if manga {
                Page::Publishing
            } else {
                Page::Airing
            }
        }
        _ => {
            if let Some(st) = key
                .strip_prefix("list:")
                .or_else(|| key.strip_prefix("manga:"))
            {
                let st = list(st)?;
                return Some((
                    if manga {
                        Page::Manga(st)
                    } else {
                        Page::List(st)
                    },
                    None,
                ));
            }
            let (k, id) = key.strip_prefix("details:")?.split_once(':')?;
            let (kind, id) = (Kind::from_name(k), id.parse::<u32>().ok()?);
            // Another mode's entry: its page isn't this switch's.
            let known = k == "anime" || k == "manga";
            return (known && id > 0 && (kind == Kind::Manga) == manga)
                .then_some((Page::Home, Some((kind, id))));
        }
    };
    Some((page, None))
}

/// What to open. `manga_on`: Manga is on (`manga:enabled`); `kind_left`: the switch as it was left.
pub fn plan(c: &Config, kind_left: Kind, manga_on: bool) -> Plan {
    let manga = match c.startup.mode() {
        StartMode::Last => kind_left == Kind::Manga,
        StartMode::Anime => false,
        StartMode::Manga => true,
    } && manga_on;
    let (page, detail) = match c.startup.open_on() {
        OpenOn::Home => (Page::Home, None),
        OpenOn::Last => restore(&c.startup.last_page, manga).unwrap_or((Page::Home, None)),
    };
    Plan {
        manga,
        page,
        detail,
    }
}

impl State {
    /// At start (never headless), once the file is read: the switch and the page. Manga is
    /// taken as on until the saved lists say otherwise (`apply_manga`).
    pub fn apply_startup(&mut self, c: &Config) {
        // The wizard (and a signed-out Home) is the start, as always.
        if !self.has_list() || self.page != Page::Home {
            return;
        }
        let p = plan(c, c.display.list_kind, true);
        self.manga.mode = p.manga;
        self.startup_detail = p.detail;
        if p.page != Page::Home {
            self.go(p.page);
        }
    }

    /// Manga turned out to be off: the manga start becomes the anime one.
    pub fn startup_manga_off(&mut self) {
        if matches!(self.startup_detail, Some((Kind::Manga, _))) {
            self.startup_detail = None;
        }
        if self.page == Page::Publishing {
            self.go(Page::Airing);
        }
    }

    /// The details page asked for at start opens once its entry is in the list. `last`: the list
    /// is complete (a sync ended), so an entry still missing was removed. Nothing opens when
    /// the user already moved on.
    pub fn settle_start_detail(&mut self, last: bool) {
        let Some((kind, id)) = self.startup_detail else {
            return;
        };
        if self.page != Page::Home {
            self.startup_detail = None;
            return;
        }
        let found = match kind {
            Kind::Anime => self
                .entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| (crate::details::Stub::from_entry(e), e.status)),
            Kind::Manga => self
                .manga
                .entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| (crate::manga::stub(e), e.status)),
        };
        match found {
            Some((stub, st)) => {
                self.startup_detail = None;
                // Back returns to the entry's list page.
                self.go(self.list_page(st));
                self.open_details(stub);
            }
            None if last => self.startup_detail = None,
            None => {}
        }
    }

    /// Keeps the page left open (the window closes or hides, or Kotora quits). Never headless.
    pub fn remember_page(&self) {
        if self.handle.is_none() {
            return;
        }
        let Some(key) = page_key(self.page, self.detail.as_ref().map(|d| (d.kind(), d.id())))
        else {
            return;
        };
        let path = kotora_core::paths::config_file();
        if Config::load(&path).is_ok_and(|c| c.startup.last_page == key) {
            return;
        }
        let _ = kotora_core::config::write_setting(
            &path,
            "startup",
            "last_page",
            &kotora_core::config::quote(&key),
        );
    }

    /// Settings → General → On startup → Open on.
    pub fn set_open_on(&mut self, v: OpenOn) {
        self.prefs.open_on = v;
        crate::detect::save_setting(
            self,
            "startup",
            "open_on",
            kotora_core::config::quote(v.key()),
        );
    }

    /// Settings → General → On startup → Mode.
    pub fn set_start_mode(&mut self, v: StartMode) {
        self.prefs.start_mode = v;
        crate::detect::save_setting(self, "startup", "mode", kotora_core::config::quote(v.key()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(open_on: &str, mode: &str, last: &str, kind: &str) -> Config {
        Config::parse(&format!("[startup]\nopen_on = \"{open_on}\"\nmode = \"{mode}\"\nlast_page = \"{last}\"\n[display]\nlist_kind = \"{kind}\"\n")).unwrap()
    }

    #[test]
    fn open_on_home_ignores_the_last_page() {
        let c = cfg("home", "last", "list:completed", "anime");
        assert_eq!(
            plan(&c, Kind::Anime, true),
            Plan {
                manga: false,
                page: Page::Home,
                detail: None
            }
        );
        assert_eq!(
            plan(&Config::default(), Kind::Anime, true).page,
            Page::Home,
            "the default"
        );
    }

    #[test]
    fn the_last_page_comes_back() {
        for (key, page) in [
            ("profile", Page::Profile),
            ("queue", Page::Queue),
            ("settings", Page::Settings),
            ("now-playing", Page::NowPlaying),
            ("list:on_hold", Page::List(Status::OnHold)),
            ("airing", Page::Airing),
        ] {
            let p = plan(&cfg("last", "last", key, "anime"), Kind::Anime, true);
            assert_eq!((p.manga, p.page, p.detail), (false, page, None), "{key}");
        }
        let p = plan(
            &cfg("last", "last", "manga:dropped", "manga"),
            Kind::Manga,
            true,
        );
        assert_eq!((p.manga, p.page), (true, Page::Manga(Status::Dropped)));
        let p = plan(
            &cfg("last", "last", "publishing", "manga"),
            Kind::Manga,
            true,
        );
        assert_eq!((p.manga, p.page), (true, Page::Publishing));
        let p = plan(
            &cfg("last", "last", "details:anime:42", "anime"),
            Kind::Anime,
            true,
        );
        assert_eq!(
            (p.page, p.detail),
            (Page::Home, Some((Kind::Anime, 42))),
            "details wait for the list"
        );
    }

    #[test]
    fn nonsense_falls_back_to_home() {
        for key in [
            "",
            "nowhere",
            "list:",
            "list:sleeping",
            "details:anime",
            "details:anime:x",
            "details:book:3",
            "details:anime:0",
        ] {
            let p = plan(&cfg("last", "last", key, "anime"), Kind::Anime, true);
            assert_eq!((p.page, p.detail), (Page::Home, None), "{key:?}");
        }
        assert_eq!(
            page_key(Page::Setup, None),
            None,
            "the wizard is never kept"
        );
        assert_eq!(page_key(Page::Search, None).as_deref(), Some("home"));
        assert_eq!(page_key(Page::Details, None).as_deref(), Some("home"));
        assert_eq!(
            page_key(Page::Details, Some((Kind::Manga, 7))).as_deref(),
            Some("details:manga:7")
        );
    }

    #[test]
    fn the_mode_choices() {
        let at = |mode, kind, on| {
            plan(
                &cfg("home", mode, "home", if kind { "manga" } else { "anime" }),
                if kind { Kind::Manga } else { Kind::Anime },
                on,
            )
            .manga
        };
        // Last used: as it was left (Anime when Manga is off).
        assert!(!at("last", false, true) && at("last", true, true) && !at("last", true, false));
        // Anime: always.
        assert!(!at("anime", true, true) && !at("anime", false, true));
        // Manga: when Manga is on, else Anime.
        assert!(
            at("manga", false, true)
                && at("manga", true, true)
                && !at("manga", false, false)
                && !at("manga", true, false)
        );
        // A bad value reads as Last used.
        assert!(at("sometimes", true, true) && !at("sometimes", false, true));
    }

    #[test]
    fn the_last_page_follows_the_mode() {
        // Left on a manga page, mode Anime: the anime page of the same status.
        let p = plan(
            &cfg("last", "anime", "manga:completed", "manga"),
            Kind::Manga,
            true,
        );
        assert_eq!((p.manga, p.page), (false, Page::List(Status::Completed)));
        // Left on an anime page, mode Manga: the manga one.
        let p = plan(
            &cfg("last", "manga", "list:completed", "anime"),
            Kind::Anime,
            true,
        );
        assert_eq!((p.manga, p.page), (true, Page::Manga(Status::Completed)));
        // Manga is off: Anime, and the manga page becomes the anime page.
        let p = plan(
            &cfg("last", "manga", "manga:watching", "manga"),
            Kind::Manga,
            false,
        );
        assert_eq!((p.manga, p.page), (false, Page::List(Status::Watching)));
        let p = plan(
            &cfg("last", "last", "publishing", "manga"),
            Kind::Manga,
            false,
        );
        assert_eq!(p.page, Page::Airing, "Publishing is Airing without Manga");
        // A manga's details in Anime mode, an anime's in Manga mode: Home.
        assert_eq!(
            plan(
                &cfg("last", "anime", "details:manga:5", "manga"),
                Kind::Manga,
                true
            )
            .detail,
            None
        );
        assert_eq!(
            plan(
                &cfg("last", "manga", "details:anime:5", "anime"),
                Kind::Anime,
                true
            )
            .detail,
            None
        );
        assert_eq!(
            plan(
                &cfg("last", "last", "details:manga:5", "manga"),
                Kind::Manga,
                false
            )
            .detail,
            None,
            "Manga is off"
        );
    }

    fn started() -> State {
        let mut s = State::fake();
        s.page = Page::Home;
        s.manga.enabled = true;
        s
    }

    #[test]
    fn starting_applies_the_plan() {
        let mut s = started();
        s.apply_startup(&cfg("last", "manga", "list:completed", "anime"));
        assert!(s.manga.mode);
        assert_eq!(s.page, Page::Manga(Status::Completed));
        let mut s = started();
        s.apply_startup(&cfg("home", "manga", "list:completed", "anime"));
        assert_eq!(
            (s.manga.mode, s.page),
            (true, Page::Home),
            "Home, in Manga mode"
        );
        // Signed out and the wizard: nothing changes.
        let mut s = State::start(crate::model::Auth::SignedOut);
        s.apply_startup(&cfg("last", "manga", "queue", "manga"));
        assert_eq!((s.manga.mode, s.page), (false, Page::Home));
        let mut s = State::start(crate::model::Auth::NoClientId);
        s.apply_startup(&cfg("last", "last", "queue", "anime"));
        assert_eq!(s.page, Page::Setup);
    }

    #[test]
    fn manga_off_at_start_falls_back() {
        let mut s = started();
        s.apply_startup(&cfg("last", "manga", "publishing", "manga"));
        assert_eq!(s.page, Page::Publishing);
        s.apply_manga(crate::manga::MangaMsg::Cache {
            enabled: false,
            items: vec![],
            synced_at: None,
        });
        assert!(!s.manga.mode);
        assert_eq!(s.page, Page::Airing);
        let mut s = started();
        s.apply_startup(&cfg("last", "manga", "manga:watching", "manga"));
        s.apply_manga(crate::manga::MangaMsg::Cache {
            enabled: false,
            items: vec![],
            synced_at: None,
        });
        assert_eq!(
            (s.manga.mode, s.page),
            (false, Page::List(Status::Watching))
        );
    }

    #[test]
    fn details_open_when_the_entry_is_there_and_not_when_it_is_gone() {
        let id = State::fake()
            .entries
            .iter()
            .map(|e| e.id)
            .find(|&i| i > 0)
            .unwrap();
        let mut s = started();
        s.apply_startup(&cfg(
            "last",
            "last",
            &format!("details:anime:{id}"),
            "anime",
        ));
        assert_eq!(s.page, Page::Home, "waits for the list");
        s.settle_start_detail(false);
        assert_eq!(s.page, Page::Details);
        assert_eq!(s.detail.as_ref().map(|d| d.id()), Some(id));
        assert_eq!(s.startup_detail, None);
        // A removed entry: Home, once the list is complete.
        let mut s = started();
        s.apply_startup(&cfg("last", "last", "details:anime:999999", "anime"));
        s.settle_start_detail(false);
        assert_eq!(
            (s.page, s.startup_detail.is_some()),
            (Page::Home, true),
            "the list may still come"
        );
        s.settle_start_detail(true);
        assert_eq!((s.page, s.startup_detail), (Page::Home, None));
        // The user moved on first: it stays away.
        let mut s = started();
        s.apply_startup(&cfg(
            "last",
            "last",
            &format!("details:anime:{id}"),
            "anime",
        ));
        s.go(Page::Queue);
        s.settle_start_detail(false);
        assert_eq!((s.page, s.startup_detail), (Page::Queue, None));
    }

    #[test]
    fn a_manga_detail_opens_in_manga_mode() {
        let mut s = started();
        crate::manga::fake(&mut s);
        s.go(Page::Home);
        s.manga.mode = false;
        let m = s
            .manga
            .entries
            .iter()
            .map(|e| e.id)
            .find(|&i| i > 0)
            .expect("fake manga");
        s.apply_startup(&cfg("last", "last", &format!("details:manga:{m}"), "manga"));
        assert!(s.manga.mode);
        s.settle_start_detail(false);
        assert_eq!(s.page, Page::Details);
        assert!(s.detail.as_ref().is_some_and(|d| d.is_manga()));
    }

    #[test]
    fn remembering_never_writes_headless() {
        // `handle` is None in tests: nothing is written (the real config is never touched).
        State::fake().remember_page();
    }
}
