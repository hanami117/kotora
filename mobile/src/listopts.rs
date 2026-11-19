//! The list page's power features, as the desktop's list pages (`model.rs` `refresh`,
//! `manga.rs` `refresh_manga`): the four orders and their two directions, the chips above the
//! list (Airing / Finished airing / New episode; manga Publishing / Finished / On hiatus), the
//! "Filter your list" box, the view (List / Grid / Large, saved per tab), the "Ep N out" and
//! "On hiatus" tags, the score in the user's own format and the empty states with their words.
//!
//! A module of its own so that `lib.rs` only gets one `mod` line.

use kotora_core::airing::Timetable;
use kotora_core::score::Smiley;
use kotora_core::{Score, Scope, time};

use super::*;

/// How the list is ordered (the desktop's `Sort`, with Progress).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSort {
    Updated,
    Title,
    Score,
    Progress,
}

impl ListSort {
    pub const ALL: [ListSort; 4] = [ListSort::Updated, ListSort::Title, ListSort::Score, ListSort::Progress];
}

/// The chips above the list. Anime: All, Airing, FinishedAiring, NewEpisode. Manga: All,
/// Publishing, Finished, OnHiatus.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickFilter {
    All,
    Airing,
    FinishedAiring,
    NewEpisode,
    Publishing,
    Finished,
    OnHiatus,
}

/// List, Grid (dense posters) or Large (big cards).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListViewMode {
    List,
    Grid,
    Large,
}

/// How a score shows: the number (100-point, 10-point, decimal), five stars, or a face.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreStyle {
    Number,
    Stars,
    Smiley,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreFace {
    Sad,
    Neutral,
    Happy,
}

/// A score in the user's format (the desktop's `scores::mark`).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ScoreMark {
    pub style: ScoreStyle,
    /// The number as the format writes it ("73", "7.3", "7"); for stars, the star count.
    pub text: String,
    /// Stars lit (1-5); 0 for the other styles.
    pub stars: u8,
    pub face: Option<ScoreFace>,
    /// For screen readers: "4 stars", "Happy", "7.3".
    pub words: String,
}

/// What a list page asks for.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ListQuery {
    pub status: ListStatus,
    pub filter: QuickFilter,
    /// The "Filter your list" box. Not empty: the whole list's matches (the page's status first,
    /// then the tabs' order), and the chip is ignored, as on the desktop.
    pub query: String,
    pub sort: ListSort,
    /// The second direction of the order ("Oldest first", "Z to A", …).
    pub reverse: bool,
}

/// One line of the list: the entry and what its tags and score show.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PageLine {
    pub row: Row,
    /// "Ep N out": episode N has aired past the progress (Watching, airing shows).
    pub new_episode: Option<u32>,
    /// Manga on hiatus: the "On hiatus" tag.
    pub on_hiatus: bool,
    pub score_mark: Option<ScoreMark>,
}

/// Why the page has no lines.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEmptyKind {
    /// It has lines.
    None,
    /// Nothing saved yet and never synced: "Loading your list…".
    Loading,
    /// The status has no entries: "No anime in Watching".
    Empty,
    /// Entries, but none match the chip or the filter.
    NoMatch,
}

/// The page: its lines, or why there are none and the words for it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ListPage {
    pub lines: Vec<PageLine>,
    pub empty: ListEmptyKind,
    /// The empty state's heading ("No anime in Watching", "Nothing matches “x”.",
    /// "Loading your list…"); empty when there are lines.
    pub empty_title: String,
    /// The line below it ("Anime you're watching on MyAnimeList show up here."); may be empty.
    pub empty_help: String,
}

/// The order's name in the Sort menu.
#[uniffi::export]
pub fn list_sort_label(sort: ListSort) -> String {
    match sort {
        ListSort::Updated => "Last updated",
        ListSort::Title => "Title",
        ListSort::Score => "Score",
        ListSort::Progress => "Progress",
    }
    .into()
}

/// The menu's two directions: the order's own first, then the reverse.
#[uniffi::export]
pub fn list_sort_directions(sort: ListSort) -> Vec<String> {
    let [a, b] = match sort {
        ListSort::Updated => ["Newest first", "Oldest first"],
        ListSort::Title => ["A to Z", "Z to A"],
        ListSort::Score => ["Highest first", "Lowest first"],
        ListSort::Progress => ["Most watched first", "Least watched first"],
    };
    vec![a.into(), b.into()]
}

/// The orders, in the menu's order.
#[uniffi::export]
pub fn list_sorts() -> Vec<ListSort> {
    ListSort::ALL.to_vec()
}

/// The chips of a kind, in order (All first).
#[uniffi::export]
pub fn quick_filters(kind: MediaKind) -> Vec<QuickFilter> {
    match kind {
        MediaKind::Anime => vec![QuickFilter::All, QuickFilter::Airing, QuickFilter::FinishedAiring, QuickFilter::NewEpisode],
        MediaKind::Manga => vec![QuickFilter::All, QuickFilter::Publishing, QuickFilter::Finished, QuickFilter::OnHiatus],
    }
}

#[uniffi::export]
pub fn quick_filter_label(filter: QuickFilter) -> String {
    match filter {
        QuickFilter::All => "All",
        QuickFilter::Airing => "Airing",
        QuickFilter::FinishedAiring => "Finished airing",
        QuickFilter::NewEpisode => "New episode",
        QuickFilter::Publishing => "Publishing",
        QuickFilter::Finished => "Finished",
        QuickFilter::OnHiatus => "On hiatus",
    }
    .into()
}

#[uniffi::export]
pub fn list_view_label(view: ListViewMode) -> String {
    match view {
        ListViewMode::List => "List",
        ListViewMode::Grid => "Grid",
        ListViewMode::Large => "Large",
    }
    .into()
}

/// The filter box's placeholder.
#[uniffi::export]
pub fn list_filter_hint(kind: MediaKind) -> String {
    match kind {
        MediaKind::Anime => "Filter your list",
        MediaKind::Manga => "Filter your manga",
    }
    .into()
}

fn status_label(kind: MediaKind, st: ListStatus) -> &'static str {
    match (st, kind) {
        (ListStatus::Watching, MediaKind::Anime) => "Watching",
        (ListStatus::Watching, MediaKind::Manga) => "Reading",
        (ListStatus::PlanToWatch, MediaKind::Anime) => "Plan to watch",
        (ListStatus::PlanToWatch, MediaKind::Manga) => "Plan to read",
        (ListStatus::Completed, _) => "Completed",
        (ListStatus::OnHold, _) => "On hold",
        (ListStatus::Dropped, _) => "Dropped",
    }
}

fn service_title(s: Service) -> &'static str {
    match s {
        Service::Mal => "MyAnimeList",
        Service::AniList => "AniList",
    }
}

/// The score as the format shows it.
pub(crate) fn score_mark(fmt: ScoreFormat, score: Score) -> ScoreMark {
    match fmt {
        ScoreFormat::Point5 => {
            let n = fmt.value(score);
            ScoreMark { style: ScoreStyle::Stars, text: n.to_string(), stars: n, face: None, words: fmt.words(score) }
        }
        ScoreFormat::Point3 => {
            let face = match fmt.smiley(score).unwrap_or(Smiley::Neutral) {
                Smiley::Sad => ScoreFace::Sad,
                Smiley::Neutral => ScoreFace::Neutral,
                Smiley::Happy => ScoreFace::Happy,
            };
            ScoreMark { style: ScoreStyle::Smiley, text: fmt.show(score), stars: 0, face: Some(face), words: fmt.words(score) }
        }
        _ => ScoreMark { style: ScoreStyle::Number, text: fmt.show(score), stars: 0, face: None, words: fmt.words(score) },
    }
}

/// The aired episode past the progress, for a show being watched that airs now.
fn new_episode(it: &ListItem, now: i64) -> Option<u32> {
    let m = &it.media;
    if m.kind != Kind::Anime || m.air_status != "currently_airing" || !(it.status == Status::Watching || it.rewatching) {
        return None;
    }
    let aired = Timetable::of_media(m)?.aired(now);
    (aired > it.progress).then_some(aired)
}

fn keeps(filter: QuickFilter, it: &ListItem, new_ep: Option<u32>) -> bool {
    let air = it.media.air_status.as_str();
    match filter {
        QuickFilter::All => true,
        QuickFilter::Airing => air == "currently_airing",
        QuickFilter::FinishedAiring => air == "finished_airing",
        QuickFilter::NewEpisode => new_ep.is_some(),
        QuickFilter::Publishing => air == "currently_publishing" || air == "currently_airing",
        QuickFilter::Finished => air == "finished" || air == "finished_airing",
        QuickFilter::OnHiatus => air == "on_hiatus",
    }
}

fn view_key(service: Service, kind: MediaKind, st: ListStatus) -> String {
    let k = match kind {
        MediaKind::Anime => "anime",
        MediaKind::Manga => "manga",
    };
    let s = match st {
        ListStatus::Watching => "watching",
        ListStatus::PlanToWatch => "plan_to_watch",
        ListStatus::Completed => "completed",
        ListStatus::OnHold => "on_hold",
        ListStatus::Dropped => "dropped",
    };
    format!("mobile:view:{}:{k}:{s}", service.name())
}

#[uniffi::export]
impl Kotora {
    /// A list page: the status (or the filter's matches), the chip, the order and direction,
    /// with each line's tags and score, or the empty state's words. Local: no network.
    pub fn list_page(&self, kind: MediaKind, q: ListQuery) -> Result<ListPage, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let k: Kind = kind.into();
        let scope = Scope::new(service.name(), k);
        let fmt = self.score_format(&store, service);
        let waiting = waiting_ids(&store, service, k);
        let items = store.load_list(scope)?;
        let synced = store.synced_at(scope).ok().flatten();
        drop(store);
        let now = time::now_unix();
        let needle = q.query.trim().to_lowercase();
        let new_eps: Vec<Option<u32>> = items.iter().map(|it| new_episode(it, now)).collect();
        let on_page = |it: &ListItem| shows_in(it.status.into(), it.rewatching, q.status);
        let mut idx: Vec<usize> = (0..items.len())
            .filter(|&i| {
                let it = &items[i];
                if needle.is_empty() { on_page(it) && keeps(q.filter, it, new_eps[i]) } else { it.media.all_titles().any(|t| t.to_lowercase().contains(&needle)) }
            })
            .collect();
        // Stable sorts, as the desktop: equal keys keep the saved list's order.
        match q.sort {
            ListSort::Updated => idx.sort_by_key(|&i| std::cmp::Reverse(items[i].updated_at)),
            ListSort::Title => idx.sort_by_cached_key(|&i| items[i].media.title.to_lowercase()),
            ListSort::Score => idx.sort_by_key(|&i| std::cmp::Reverse(items[i].score)),
            ListSort::Progress => match kind {
                MediaKind::Anime => idx.sort_by_key(|&i| std::cmp::Reverse(items[i].progress)),
                MediaKind::Manga => idx.sort_by_key(|&i| std::cmp::Reverse((items[i].progress, items[i].volumes))),
            },
        }
        if q.reverse {
            idx.reverse();
        }
        if !needle.is_empty() {
            let rank = |it: &ListItem| {
                let st: ListStatus = it.status.into();
                if st == q.status { 0 } else { 1 + ListStatus::ALL.iter().position(|x| *x == st).unwrap_or(0) }
            };
            idx.sort_by_key(|&i| rank(&items[i]));
        }
        let lines: Vec<PageLine> = idx
            .iter()
            .map(|&i| {
                let it = &items[i];
                PageLine {
                    row: row(it, fmt, waiting.contains(&it.media.id)),
                    new_episode: new_eps[i],
                    on_hiatus: kind == MediaKind::Manga && it.media.air_status == "on_hiatus",
                    score_mark: it.score.map(|s| score_mark(fmt, s)),
                }
            })
            .collect();
        let (empty, title, help) = if !lines.is_empty() {
            (ListEmptyKind::None, String::new(), String::new())
        } else if items.is_empty() && synced.is_none() {
            let t = match kind {
                MediaKind::Anime => "Loading your list\u{2026}".to_string(),
                MediaKind::Manga => format!("Reading your manga list from {}\u{2026}", service_title(service)),
            };
            (ListEmptyKind::Loading, t, String::new())
        } else if needle.is_empty() && !items.iter().any(on_page) {
            let svc = service_title(service);
            let help = match (kind, q.status) {
                (MediaKind::Anime, ListStatus::Watching) => format!("Anime you're watching on {svc} show up here."),
                (MediaKind::Anime, ListStatus::PlanToWatch) => format!("Anime you plan to watch on {svc} show up here."),
                (MediaKind::Anime, ListStatus::Completed) => format!("Anime you finish on {svc} show up here."),
                (MediaKind::Anime, ListStatus::OnHold) => format!("Anime you put on hold on {svc} show up here."),
                (MediaKind::Anime, ListStatus::Dropped) => format!("Entries you drop on {svc} show up here."),
                (MediaKind::Manga, ListStatus::Watching) => format!("Manga you're reading on {svc} show up here."),
                (MediaKind::Manga, ListStatus::PlanToWatch) => format!("Manga you plan to read on {svc} show up here."),
                (MediaKind::Manga, ListStatus::Completed) => format!("Manga you finish on {svc} show up here."),
                (MediaKind::Manga, ListStatus::OnHold) => format!("Manga you put on hold on {svc} show up here."),
                (MediaKind::Manga, ListStatus::Dropped) => format!("Manga you drop on {svc} show up here."),
            };
            let noun = if kind == MediaKind::Anime { "anime" } else { "manga" };
            (ListEmptyKind::Empty, format!("No {noun} in {}", status_label(kind, q.status)), help)
        } else {
            let t = if needle.is_empty() {
                if kind == MediaKind::Anime { "No anime match this filter.".to_string() } else { "No manga match this filter.".to_string() }
            } else {
                format!("Nothing matches \u{201c}{}\u{201d}.", q.query.trim())
            };
            (ListEmptyKind::NoMatch, t, String::new())
        };
        Ok(ListPage { lines, empty, empty_title: title, empty_help: help })
    }

    /// The view a tab shows (saved per service, kind and status); List at first.
    pub fn list_view(&self, kind: MediaKind, status: ListStatus) -> ListViewMode {
        let Some(service) = self.active() else { return ListViewMode::List };
        match self.store().kv_get(&view_key(service, kind, status)).ok().flatten().as_deref() {
            Some("grid") => ListViewMode::Grid,
            Some("large") => ListViewMode::Large,
            _ => ListViewMode::List,
        }
    }

    pub fn set_list_view(&self, kind: MediaKind, status: ListStatus, view: ListViewMode) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let v = match view {
            ListViewMode::List => "list",
            ListViewMode::Grid => "grid",
            ListViewMode::Large => "large",
        };
        Ok(self.store().kv_set(&view_key(service, kind, status), v)?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kotora_core::Media;

    use super::*;

    #[derive(Default)]
    struct Fake {
        tokens: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl Platform for Fake {
        fn utc_offset(&self, _at_unix: i64) -> i32 {
            crate::testenv::OFFSET
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, service: String) -> Option<Vec<u8>> {
            self.tokens.lock().unwrap().get(&service).cloned()
        }
        fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
            self.tokens.lock().unwrap().insert(service, bytes);
            true
        }
        fn token_delete(&self, service: String) {
            self.tokens.lock().unwrap().remove(&service);
        }
    }

    fn open(name: &str) -> Arc<Kotora> {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-listopts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config_dir: dir.join("files").display().to_string(),
            data_dir: dir.join("nobackup").display().to_string(),
            cache_dir: dir.join("cache").display().to_string(),
        };
        let k = Kotora::new(paths, ClientIds { mal: "x".into(), anilist: "x".into() }, Arc::new(Fake::default())).unwrap();
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save("mal", &t).unwrap();
        k.store().set_account("mal", "made-up-user").unwrap();
        k
    }

    #[allow(clippy::too_many_arguments)]
    fn item(kind: Kind, id: u32, title: &str, status: Status, progress: u32, score: u8, air: &str, updated_at: i64) -> ListItem {
        ListItem {
            media: Media {
                kind,
                id,
                title: title.into(),
                title_en: Some(format!("{title} English")),
                title_ja: None,
                synonyms: Vec::new(),
                episodes: Some(24),
                format: "tv".into(),
                air_status: air.into(),
                cover_url: None,
                start_date: None,
                broadcast: None,
                volumes: None,
                authors: Vec::new(),
                next_airing: None,
                id_mal: None,
            },
            status,
            progress,
            score: kotora_core::Score::from_ten(u32::from(score)),
            rewatching: false,
            times_rewatched: 0,
            volumes: 0,
            updated_at,
        }
    }

    fn q(status: ListStatus, sort: ListSort) -> ListQuery {
        ListQuery { status, filter: QuickFilter::All, query: String::new(), sort, reverse: false }
    }

    fn titles(p: &ListPage) -> Vec<&str> {
        p.lines.iter().map(|l| l.row.title.as_str()).collect()
    }

    fn fill(k: &Kotora) {
        let now = time::now_unix();
        let mut airing = item(Kind::Anime, 1, "Alpha", Status::Watching, 3, 7, "currently_airing", 100);
        // AniList's exact next airing: episode 6 next week, so 5 have aired.
        airing.media.next_airing = Some(kotora_core::airing::NextAiring { episode: 6, at: now + 3 * 86_400 });
        let list = [
            airing,
            item(Kind::Anime, 2, "bravo", Status::Watching, 10, 9, "finished_airing", 300),
            item(Kind::Anime, 3, "Charlie", Status::Watching, 1, 0, "currently_airing", 200),
            item(Kind::Anime, 4, "Delta", Status::Completed, 24, 6, "finished_airing", 50),
        ];
        k.store().replace_list("mal", &list, 1_000).unwrap();
    }

    #[test]
    fn orders_and_directions_as_the_desktop() {
        let k = open("sort");
        fill(&k);
        let page = |s, rev| titles(&k.list_page(MediaKind::Anime, ListQuery { reverse: rev, ..q(ListStatus::Watching, s) }).unwrap()).join(",");
        assert_eq!(page(ListSort::Updated, false), "bravo,Charlie,Alpha");
        assert_eq!(page(ListSort::Updated, true), "Alpha,Charlie,bravo");
        assert_eq!(page(ListSort::Title, false), "Alpha,bravo,Charlie");
        assert_eq!(page(ListSort::Score, false), "bravo,Alpha,Charlie", "unscored last");
        assert_eq!(page(ListSort::Score, true), "Charlie,Alpha,bravo");
        assert_eq!(page(ListSort::Progress, false), "bravo,Alpha,Charlie");
        assert_eq!(list_sort_directions(ListSort::Progress), vec!["Most watched first", "Least watched first"]);
    }

    #[test]
    fn chips_filter_new_episode_and_score_marks() {
        let k = open("chips");
        fill(&k);
        let with = |f| titles(&k.list_page(MediaKind::Anime, ListQuery { filter: f, ..q(ListStatus::Watching, ListSort::Title) }).unwrap()).join(",");
        assert_eq!(with(QuickFilter::Airing), "Alpha,Charlie");
        assert_eq!(with(QuickFilter::FinishedAiring), "bravo");
        assert_eq!(with(QuickFilter::NewEpisode), "Alpha");
        let p = k.list_page(MediaKind::Anime, q(ListStatus::Watching, ListSort::Title)).unwrap();
        assert_eq!(p.lines[0].new_episode, Some(5));
        assert_eq!(p.lines[0].score_mark.as_ref().map(|m| m.text.as_str()), Some("7"));
        assert_eq!(score_mark(ScoreFormat::Point5, Score::from_ten(8).unwrap()).style, ScoreStyle::Stars);
        assert_eq!(score_mark(ScoreFormat::Point3, Score::from_ten(9).unwrap()).face, Some(ScoreFace::Happy));
        let none = k.list_page(MediaKind::Anime, ListQuery { filter: QuickFilter::NewEpisode, ..q(ListStatus::Completed, ListSort::Title) }).unwrap();
        assert_eq!((none.empty, none.empty_title.as_str()), (ListEmptyKind::NoMatch, "No anime match this filter."));
    }

    #[test]
    fn the_filter_box_looks_at_every_status_and_every_title() {
        let k = open("query");
        fill(&k);
        let p = k.list_page(MediaKind::Anime, ListQuery { query: "  DELTA eng ".into(), filter: QuickFilter::Airing, ..q(ListStatus::Watching, ListSort::Title) }).unwrap();
        assert_eq!(titles(&p), vec!["Delta"]);
        let p = k.list_page(MediaKind::Anime, ListQuery { query: "a".into(), ..q(ListStatus::Completed, ListSort::Title) }).unwrap();
        assert_eq!(titles(&p), vec!["Delta", "Alpha", "bravo", "Charlie"], "the page's status first");
        let p = k.list_page(MediaKind::Anime, ListQuery { query: "zzz".into(), ..q(ListStatus::Watching, ListSort::Title) }).unwrap();
        assert_eq!(p.empty_title, "Nothing matches \u{201c}zzz\u{201d}.");
    }

    #[test]
    fn empty_states_and_views_per_tab() {
        let k = open("empty");
        let p = k.list_page(MediaKind::Anime, q(ListStatus::Watching, ListSort::Updated)).unwrap();
        assert_eq!((p.empty, p.empty_title.as_str()), (ListEmptyKind::Loading, "Loading your list\u{2026}"));
        fill(&k);
        let p = k.list_page(MediaKind::Anime, q(ListStatus::Dropped, ListSort::Updated)).unwrap();
        assert_eq!(p.empty, ListEmptyKind::Empty);
        assert_eq!(p.empty_title, "No anime in Dropped");
        assert_eq!(p.empty_help, "Entries you drop on MyAnimeList show up here.");
        let m = k.list_page(MediaKind::Manga, q(ListStatus::Watching, ListSort::Updated)).unwrap();
        assert_eq!(m.empty_title, "Reading your manga list from MyAnimeList\u{2026}");
        let mut hiatus = item(Kind::Manga, 9, "Echo", Status::Watching, 3, 7, "on_hiatus", 1);
        hiatus.media.format = "manga".into();
        k.store().replace_list(Scope::new("mal", Kind::Manga), &[hiatus], 1_000).unwrap();
        let m = k.list_page(MediaKind::Manga, ListQuery { filter: QuickFilter::OnHiatus, ..q(ListStatus::Watching, ListSort::Updated) }).unwrap();
        assert!(m.lines[0].on_hiatus);
        assert_eq!(k.list_view(MediaKind::Anime, ListStatus::Watching), ListViewMode::List);
        k.set_list_view(MediaKind::Anime, ListStatus::Watching, ListViewMode::Grid).unwrap();
        assert_eq!(k.list_view(MediaKind::Anime, ListStatus::Watching), ListViewMode::Grid);
        assert_eq!(k.list_view(MediaKind::Anime, ListStatus::Completed), ListViewMode::List);
    }
}
