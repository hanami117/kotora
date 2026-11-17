//! The Airing page: what airs this week (by weekday, in the user's time zone) and this season's
//! anime (a cover grid, most members first). Data from MyAnimeList only; the times are Kotora's
//! own estimate (`kotora_core::airing::Timetable`).
//!
//! - **Data:** the current season and the one before (its shows still airing), from
//!   `GET /anime/season/{year}/{season}` in pages of 100 (at most [`PAGES`] each: 8 requests
//!   through the app's one rate budget), saved in the store's `season` table and asked for again
//!   after [`airing::FRESH_SECS`]. The user's own airing entries come from the saved list (the
//!   list sync asks for their start date and slot). A worker thread does all of it when the page
//!   opens; nothing runs headless, offline or signed out.
//! - **Other seasons:** the picker beside the title (‹ ›, the menu, `[` and `]`) shows one other
//!   season at a time as a plain grid: no week, no countdowns, no estimates ([`Browse`]). The same
//!   worker pattern, one season (at most [`PAGES`] requests), from the saved copy when it is still
//!   good ([`Season::is_fresh`]: a day's half for the near seasons, two weeks for older ones). A
//!   newer pick stops an older worker between two requests ([`Airing::generation`]).
//! - **Derived once** ([`State::airing_rebuild`]): the week's slots per day with their texts, the
//!   grid's order after the filters, and the cards' lines. The view only reads them. While the page
//!   is on screen, `poll` rebuilds once a minute (the countdowns); nothing ticks elsewhere.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kotora_core::airing::{self, DAY_SHORT, Season, SeasonAnime, Timetable, WEEK};
use kotora_core::service::ServiceError;
use kotora_core::store::Store;
use kotora_core::time::{Clock, now_unix};
use kotora_core::{ListItem, Status, paths};
use rune::{El, GrowthSpec, ScrollSpec, TextStyle, Tx, col, el, row, spacer, text};

use crate::model::{Auth, Page, State, kind_label};
use crate::theme::{self, PAL};
use crate::ui::{action, alert, button, cover, tag, tag_tinted, top};

/// Pages of 100 asked for a season (by members: the 400 most followed).
pub const PAGES: u32 = 4;
/// The picker's menu lists this many seasons after the current one (MyAnimeList has the next
/// one's list) and this many before.
pub const AHEAD: i32 = 1;
pub const BEHIND: i32 = 8;

/// A grid card: cover and the text under it.
pub const CARD_W: f32 = 128.0;
pub const CARD_COVER_H: f32 = 181.0;
const CARD_GAP: f32 = 16.0;
/// Title (two lines), type line and schedule (two lines): one height, so the buttons line up.
const CARD_TEXT_H: f32 = 84.0;
const CARD_ROW_H: f32 = CARD_COVER_H + CARD_TEXT_H + 50.0;
const WEEK_ROW_H: f32 = 56.0;
const DAY_HEAD_H: f32 = 40.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Week,
    Season,
}

/// The type filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    All,
    Tv,
    Ona,
    Movie,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::All, Kind::Tv, Kind::Ona, Kind::Movie];

    pub fn label(self) -> &'static str {
        match self {
            Kind::All => "All types",
            Kind::Tv => "TV",
            Kind::Ona => "ONA",
            Kind::Movie => "Movie",
        }
    }

    fn keeps(self, format: &str) -> bool {
        match self {
            Kind::All => true,
            Kind::Tv => matches!(format, "tv" | "tv_special"),
            Kind::Ona => format == "ona",
            Kind::Movie => format == "movie",
        }
    }
}

/// The list filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mine {
    #[default]
    All,
    On,
    Off,
}

impl Mine {
    pub const ALL: [Mine; 3] = [Mine::All, Mine::On, Mine::Off];

    pub fn label(self) -> &'static str {
        match self {
            Mine::All => "Everything",
            Mine::On => "On my list",
            Mine::Off => "Not on my list",
        }
    }

    fn keeps(self, on_list: bool) -> bool {
        match self {
            Mine::All => true,
            Mine::On => on_list,
            Mine::Off => !on_list,
        }
    }
}

/// The season grid's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// Most members first (MyAnimeList's order).
    #[default]
    Popular,
    /// The list's shows with the most episodes out and not watched first.
    ToWatch,
}

impl Order {
    pub const ALL: [Order; 2] = [Order::Popular, Order::ToWatch];

    pub fn label(self) -> &'static str {
        match self {
            Order::Popular => "Most popular",
            Order::ToWatch => "Most to watch",
        }
    }
}

/// One episode in the week view.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    /// Index into `Airing::shows`.
    pub show: usize,
    pub episode: u32,
    /// Unix seconds (estimated).
    pub at: i64,
    /// "16:30", local.
    pub time: String,
    /// On the user's list (not dropped).
    pub mine: bool,
    /// "in 2 d 4 h" (on the list, still to come), "aired", or empty.
    pub note: String,
    /// Watching or On hold: "~3 to watch", "Caught up" ([`ToWatch::label`]).
    pub to_watch: Option<String>,
}

/// A line of the week view (virtual rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekRow {
    /// A weekday's heading: Monday 0 … Sunday 6.
    Day(u8),
    /// Slot `i` of day `d`.
    Slot(u8, usize),
    /// A day with nothing.
    Nothing(u8),
    /// "No broadcast time" heading.
    UntimedHead,
    /// Index into `Airing::untimed`.
    Untimed(usize),
}

/// A card of the season grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub show: usize,
    /// "TV · 12 eps · ★ 8.12".
    pub meta: String,
    /// "Sat 16:30 · ep 7 in 2 d 4 h", "Starts Jan 9", "Fridays", "".
    pub when: String,
    /// The list status, when on the list.
    pub status: Option<Status>,
    /// The next episode is soon and the anime is on the list: the line is lit.
    pub lit: bool,
    /// Watching or On hold: the label and the episodes behind (the "Most to watch" order).
    pub to_watch: Option<(String, u32)>,
}

/// Another season than the current one, as the picker shows it.
#[derive(Debug)]
pub struct Browse {
    pub season: Season,
    pub shows: Vec<SeasonAnime>,
    /// When the saved copy shown was fetched.
    pub fetched_at: Option<i64>,
    /// A worker runs.
    pub loading: bool,
    /// The last fetch failed (the saved copy, if any, still shows).
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Airing {
    pub view: View,
    pub kind: Kind,
    pub mine: Mine,
    /// The season grid's order.
    pub order: Order,
    /// The season lists and the list's airing entries, merged, most members first.
    pub shows: Vec<SeasonAnime>,
    /// The season shown (the current one).
    pub season: Option<Season>,
    /// When the current season was fetched (the saved copy's time).
    pub fetched_at: Option<i64>,
    /// A worker runs.
    pub loading: bool,
    /// The last fetch failed (the saved copy, if any, still shows).
    pub error: Option<String>,
    /// Another season, being looked at (`None`: the current season, the page's two views).
    pub browse: Option<Browse>,
    /// The season menu is open.
    pub menu: bool,
    /// Counts the season picks: a season worker whose number is no longer this one stops.
    pub generation: Arc<AtomicU64>,
    /// Scenes and tests: a fixed UTC offset instead of the system's zone.
    pub tz: Option<i64>,
    // ---- derived by `airing_rebuild`
    /// The next 7 days from today (0 = today), local.
    pub days: [Vec<Slot>; 7],
    /// Shows with no timetable (no slot time or start date), filtered.
    pub untimed: Vec<(usize, String)>,
    pub week: Vec<WeekRow>,
    pub cards: Vec<Card>,
    /// Local midnight that starts today (the first of the 7 days).
    pub day0: i64,
    /// Today's weekday, Monday 0 … Sunday 6.
    pub today: u8,
    /// The minute the texts were made for (`poll` rebuilds when it changes).
    pub minute: i64,
}

/// A worker's answer.
#[derive(Debug)]
pub struct Answer {
    pub season: Season,
    pub shows: Vec<SeasonAnime>,
    pub fetched_at: Option<i64>,
    pub error: Option<String>,
    /// Nothing more comes.
    pub done: bool,
}

impl Airing {
    /// The anime the cards and `Card::show` index: the browsed season's, else the page's.
    pub fn shown(&self) -> &[SeasonAnime] {
        self.browse.as_ref().map_or(&self.shows, |b| &b.shows)
    }

    /// A worker runs for what the page shows.
    pub fn is_loading(&self) -> bool {
        self.browse.as_ref().map_or(self.loading, |b| b.loading)
    }

    /// The error of what the page shows.
    pub fn shown_error(&self) -> Option<&str> {
        self.browse
            .as_ref()
            .map_or(self.error.as_deref(), |b| b.error.as_deref())
    }

    /// The season the page shows: the browsed one, else the current one.
    pub fn viewed(&self, current: Season) -> Season {
        self.browse.as_ref().map_or(current, |b| b.season)
    }

    /// Stops the season worker that runs, if any (its answer is dropped too).
    pub(crate) fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn clock(&self, t: i64) -> Clock {
        match self.tz {
            Some(o) => Clock::at(t, o),
            None => Clock::local(t),
        }
    }

    /// The day number (in this zone) of `t`, counted from today: 0 today, 1 tomorrow, …
    fn day_from_today(&self, t: i64, now: i64) -> i64 {
        self.clock(t).day - self.clock(now).day
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAY_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

impl State {
    /// Opens the page (the sidebar): fetches what is missing or old.
    pub fn open_airing(&mut self) {
        // Coming from another page: this season. The sidebar item on the page keeps the season.
        if self.page != Page::Airing && self.airing.browse.take().is_some() {
            self.airing.cancel();
        }
        self.go(Page::Airing);
        self.airing_rebuild();
        if self.airing.browse.is_some() {
            self.airing_browse_fetch(false);
        } else {
            self.airing_fetch(false);
        }
    }

    /// The present season, by the clock (Japan's calendar).
    pub fn current_season(&self) -> Season {
        Season::of(self.clock())
    }

    /// The season the page shows.
    pub fn viewed_season(&self) -> Season {
        self.airing.viewed(self.current_season())
    }

    /// The seasons the picker reaches: the first TV season to the one after the current one.
    pub fn season_range(&self) -> (Season, Season) {
        (Season::OLDEST, self.current_season().shift(AHEAD))
    }

    /// Shows another season (or the current one again); out of range, the nearest in range.
    /// Returns whether the page changed.
    pub fn airing_goto(&mut self, season: Season) -> bool {
        let (first, last) = self.season_range();
        let season = season.clamp(first, last);
        self.airing.menu = false;
        if season == self.viewed_season() {
            return false;
        }
        self.airing.cancel();
        if season == self.current_season() {
            self.airing.browse = None;
            self.airing_rebuild();
            if self.airing.shows.is_empty() {
                self.airing_fetch(false);
            }
        } else {
            self.airing.browse = Some(Browse {
                season,
                shows: Vec::new(),
                fetched_at: None,
                loading: false,
                error: None,
            });
            self.airing_rebuild();
            self.airing_browse_fetch(false);
        }
        true
    }

    /// The previous (`-1`) or next (`1`) season, or further. At the end of the range: nothing.
    pub fn airing_step(&mut self, by: i32) -> bool {
        let to = self.viewed_season().shift(by);
        self.airing_goto(to)
    }

    /// The Refresh button: asks MyAnimeList again for the season shown.
    pub fn airing_refresh(&mut self) {
        if self.airing.browse.is_some() {
            self.airing_browse_fetch(true);
        } else {
            self.airing_fetch(true);
        }
    }

    /// Asks a worker for the browsed season: the saved copy first, then MyAnimeList when it is
    /// missing or past its time ([`Season::is_fresh`]), or `force`. Without a saved copy the rows
    /// show as the pages come in. Never headless, offline or signed out (the saved copy still
    /// shows). A newer pick stops it between two requests.
    pub fn airing_browse_fetch(&mut self, force: bool) {
        let Some(h) = self.handle.clone() else { return };
        if !self.has_list() {
            return;
        }
        let Some(b) = self.airing.browse.as_mut() else {
            return;
        };
        let season = b.season;
        b.loading = true;
        b.error = None;
        let online = self.auth == Auth::SignedIn && !paths::offline_forced();
        let token = self.airing.generation.clone();
        let mine = token.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            let store = Store::open(&paths::db_file()).ok();
            let run = Run { store: store.as_ref(), now: now_unix(), force, online };
            let mut mal = if online { crate::account::open() } else { None };
            let signed_in = mal.is_some();
            let mut get = |se: Season, page: u32| mal.as_mut().map_or(Err(ServiceError::SignInAgain), |m| m.season(se, page));
            let get: Option<Source> = if signed_in { Some(&mut get) } else { None };
            let send = &mut |a: Answer| h.update_if(move |s, _| s.airing_browse_answer(a));
            run_browse(&run, season, get, &|| token.load(Ordering::SeqCst) == mine, send);
        });
    }

    /// The season worker's answer. One for a season no longer shown is dropped; an error keeps
    /// the shows already shown.
    pub fn airing_browse_answer(&mut self, a: Answer) -> bool {
        if !self.has_list() {
            return false;
        }
        let Some(b) = self.airing.browse.as_mut().filter(|b| b.season == a.season) else {
            return false;
        };
        b.loading = !a.done;
        if a.error.is_none() || b.shows.is_empty() {
            if !a.shows.is_empty() || a.error.is_none() {
                b.shows = a.shows;
            }
            b.fetched_at = a.fetched_at.or(b.fetched_at);
        }
        b.error = a.error;
        self.airing_rebuild();
        true
    }

    pub fn airing_view(&mut self, v: View) {
        self.airing.view = v;
        self.airing_rebuild();
    }

    pub fn airing_kind(&mut self, k: Kind) {
        self.airing.kind = k;
        self.airing_rebuild();
    }

    pub fn airing_mine(&mut self, m: Mine) {
        self.airing.mine = m;
        self.airing_rebuild();
    }

    pub fn airing_order(&mut self, o: Order) {
        self.airing.order = o;
        self.airing_rebuild();
    }

    /// Asks the worker for the seasons: the saved copies first (shown at once, even old ones), then
    /// MyAnimeList when they are older than [`FRESH_SECS`] (or `force`, the Refresh button). The
    /// current season comes first and shows as soon as its first page is in; the previous one
    /// follows. Never headless, offline or signed out (the saved copies still show).
    pub fn airing_fetch(&mut self, force: bool) {
        let Some(h) = self.handle.clone() else { return };
        if self.airing.loading || !self.has_list() {
            return;
        }
        self.airing.loading = true;
        self.airing.error = None;
        let online = self.auth == Auth::SignedIn && !paths::offline_forced();
        let token = self.airing.generation.clone();
        let mine = token.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            let store = Store::open(&paths::db_file()).ok();
            let list = store.as_ref().and_then(|st| st.load_list("mal").ok()).unwrap_or_default();
            let run = Run { store: store.as_ref(), now: now_unix(), force, online };
            let mut mal = if online { crate::account::open() } else { None };
            let signed_in = mal.is_some();
            let mut get = |se: Season, page: u32| mal.as_mut().map_or(Err(ServiceError::SignInAgain), |m| m.season(se, page));
            let get: Option<Source> = if signed_in { Some(&mut get) } else { None };
            let send = &mut |a: Answer| h.update_if(move |s, _| s.airing_answer(a));
            // A season picked meanwhile has the budget: the previous season waits.
            run_current(&run, &list, get, &|| token.load(Ordering::SeqCst) == mine, send);
        });
    }

    /// The worker's answer. An error keeps the shows already shown.
    pub fn airing_answer(&mut self, a: Answer) -> bool {
        // Signed out while the worker ran: its answer goes with the account.
        if !self.has_list() {
            return false;
        }
        let a_ = &mut self.airing;
        a_.loading = !a.done;
        a_.season = Some(a.season);
        if a.error.is_none() || a_.shows.is_empty() {
            if !a.shows.is_empty() || a.error.is_none() {
                a_.shows = a.shows;
            }
            a_.fetched_at = a.fetched_at.or(a_.fetched_at);
        }
        a_.error = a.error;
        self.airing_rebuild();
        true
    }

    /// Scenes and tests: shows as if MyAnimeList had answered.
    pub fn airing_show(&mut self, season: Season, shows: Vec<SeasonAnime>) {
        self.airing.season = Some(season);
        self.airing.fetched_at = Some(self.clock() - 3600);
        self.airing.shows = shows;
        self.airing.loading = false;
        self.airing.error = None;
        self.airing_rebuild();
    }

    /// Scenes and tests: another season as if MyAnimeList had answered.
    pub fn airing_browse_show(&mut self, season: Season, shows: Vec<SeasonAnime>) {
        let fetched_at = Some(self.clock() - 3600);
        self.airing.browse = Some(Browse {
            season,
            shows,
            fetched_at,
            loading: false,
            error: None,
        });
        self.airing_rebuild();
    }

    /// Rebuilds everything the page draws: the week, the grid and their texts. Call it after the
    /// data, a filter, the list or the minute changes; the view only reads the result.
    pub fn airing_rebuild(&mut self) {
        let now = self.clock();
        // The list as the page sees it: status and progress (pending edits included).
        let mine: HashMap<u32, (Status, u32)> = self.entries.iter().map(|e| (e.id, (e.status, e.progress))).collect();
        // Episodes out and not watched, of Watching and On hold shows (the browsed seasons too).
        let shows: HashMap<u32, &SeasonAnime> = self.airing.shown().iter().map(|s| (s.id, s)).collect();
        let behind: HashMap<u32, kotora_core::airing::ToWatch> = self.entries.iter().filter_map(|e| Some((e.id, self.to_watch_in(e, shows.get(&e.id).copied())?))).collect();
        let lang = self.prefs.titles;
        let a = &mut self.airing;
        a.minute = now.div_euclid(60);
        let today = a.clock(now);
        a.day0 = today.midnight();
        a.today = today.weekday;
        // A day more than a week: a DST change in between still ends at a midnight below.
        let week_end = a.day0 + WEEK + 3600;
        for d in &mut a.days {
            d.clear();
        }
        a.untimed.clear();
        a.cards.clear();
        // Another season: a plain grid. No timetable (the shows aired, or have not started), so no
        // week, no countdowns and no "airing" notes.
        let browsing = a.browse.is_some();
        let src = a.browse.as_ref().map_or(&a.shows, |b| &b.shows);
        for (i, show) in src.iter().enumerate() {
            let entry = mine.get(&show.id).copied().filter(|(st, _)| *st != Status::Dropped);
            if !a.kind.keeps(&show.format) || !a.mine.keeps(entry.is_some()) {
                continue;
            }
            // Self-correction: a progress past the estimate moves the episodes forward.
            let tt = Timetable::of(show)
                .filter(|_| !browsing)
                .map(|t| match entry {
                    Some((_, p)) => t.with_progress(p, now),
                    None => t,
                });
            let next = tt.and_then(|t| t.next(now));
            // The week.
            match tt {
                Some(t) => {
                    if let Some((ep, at)) = t.slot_in(a.day0, week_end).filter(|(_, at)| (0..7).contains(&a.day_from_today(*at, now))) {
                        let c = a.clock(at);
                        let note = if at <= now {
                            "aired".to_string()
                        } else if entry.is_some() {
                            format!("in {}", airing::countdown(at - now))
                        } else {
                            String::new()
                        };
                        let k = a.day_from_today(at, now) as usize;
                        if let Some(day) = a.days.get_mut(k) {
                            day.push(Slot { show: i, episode: ep, at, time: c.hhmm(), mine: entry.is_some(), note, to_watch: behind.get(&show.id).map(|w| w.label()) });
                        }
                    }
                }
                None if show.is_airing() && !browsing => {
                    let why = match show.broadcast {
                        Some(b) => format!("{}s in Japan, time not known", DAY_LONG[usize::from(b.day % 7)]),
                        None => format!("No broadcast slot on {}", self.service.title()),
                    };
                    a.untimed.push((i, why));
                }
                None => {}
            }
            // The card.
            let mut meta = Vec::new();
            let kind = kind_label(&show.format);
            if !kind.is_empty() {
                meta.push(kind.to_string());
            }
            meta.push(match show.episodes {
                Some(1) => "1 ep".into(),
                Some(n) => format!("{n} eps"),
                None => "? eps".into(),
            });
            if let Some(m) = show.mean {
                meta.push(format!("\u{2605} {m:.2}"));
            }
            let when = match (next, show.air_status.as_str()) {
                (Some((ep, at)), _) => {
                    let c = a.clock(at);
                    let slot = format!("{} {}", DAY_SHORT[usize::from(c.weekday)], c.hhmm());
                    if entry.is_some() { format!("{slot} \u{b7} ep {ep} in {}", airing::countdown(at - now)) } else { format!("{slot} \u{b7} ep {ep}") }
                }
                (None, "not_yet_aired") => match show.start_date.as_deref().map(crate::details::date_text) {
                    Some(d) => format!("Starts {d}"),
                    None => "Not yet aired".into(),
                },
                (None, "finished_airing") => "Finished airing".into(),
                (None, _) => match show.broadcast {
                    Some(b) => format!("{}s", DAY_LONG[usize::from(b.day % 7)]),
                    None => String::new(),
                },
            };
            let lit = entry.is_some() && next.is_some_and(|(_, at)| at - now < 2 * 86_400);
            let to_watch = behind.get(&show.id).map(|w| (w.label(), w.behind));
            a.cards.push(Card { show: i, meta: meta.join(" \u{b7} "), when, status: entry.map(|(st, _)| st), lit, to_watch });
        }
        if a.order == Order::ToWatch {
            // Stable: equal ones keep the popular order; shows not watched last.
            a.cards.sort_by_key(|c| std::cmp::Reverse(c.to_watch.as_ref().map_or(-1, |(_, n)| i64::from(*n))));
        }
        for d in &mut a.days {
            d.sort_by_key(|s| (s.at, !s.mine));
        }
        // The week as lines: each day's heading, its slots (or "nothing"), then the untimed ones.
        a.week.clear();
        for d in 0..7u8 {
            a.week.push(WeekRow::Day(d));
            let n = a.days[usize::from(d)].len();
            if n == 0 {
                a.week.push(WeekRow::Nothing(d));
            }
            a.week.extend((0..n).map(|i| WeekRow::Slot(d, i)));
        }
        if !a.untimed.is_empty() {
            a.week.push(WeekRow::UntimedHead);
            a.week.extend((0..a.untimed.len()).map(WeekRow::Untimed));
        }
        let _ = lang;
    }

    /// The title of show `i` in the chosen language.
    pub fn airing_title(&self, i: usize) -> &str {
        let a = &self.airing.shown()[i];
        self.prefs.titles.pick(&a.title, a.title_en.as_deref(), a.title_ja.as_deref())
    }

    /// "+ Watching" / "+ Plan to watch" on a card or a line: through the queue, as search does.
    pub fn airing_add(&mut self, id: u32, status: Status) {
        let Some(show) = self.airing.shown().iter().find(|a| a.id == id) else { return };
        let media = show.media();
        match self.add_media(media, status) {
            Ok(_) => self.airing_rebuild(),
            Err(e) => self.airing.error = Some(e),
        }
    }

    /// Opens the details page of show `id`.
    pub fn airing_details(&mut self, id: u32) {
        let Some(i) = self.airing.shown().iter().position(|a| a.id == id) else { return };
        let a = &self.airing.shown()[i];
        let stub = crate::details::Stub { id, title: self.airing_title(i).to_string(), cover_url: a.cover_url.clone(), format: a.format.clone(), episodes: a.episodes, kind: kotora_core::Kind::Anime };
        self.open_details(stub);
    }

    /// `kot airing`: the user's list in the next 7 days, soonest first.
    pub fn ctl_airing(&self) -> Vec<kotora_ctl::AiringItem> {
        let now = self.clock();
        let mut out: Vec<kotora_ctl::AiringItem> = Vec::new();
        for (i, show) in self.airing.shows.iter().enumerate() {
            let Some(e) = self.entries.iter().find(|e| e.id == show.id && e.status != Status::Dropped) else { continue };
            let Some(t) = Timetable::of(show).map(|t| t.with_progress(e.progress, now)) else { continue };
            let Some((episode, at)) = t.next(now) else { continue };
            if at - now <= WEEK {
                let (c, today) = (self.airing.clock(at), self.airing.clock(now));
                let day = match c.day - today.day {
                    0 => "Today".to_string(),
                    1 => "Tomorrow".to_string(),
                    _ => DAY_LONG[usize::from(c.weekday)].to_string(),
                };
                out.push(kotora_ctl::AiringItem {
                    title: self.airing_title(i).to_string(),
                    episode,
                    at,
                    day,
                    time: c.hhmm(),
                    countdown: format!("in {}", airing::countdown(at - now)),
                    behind: self.to_watch_in(e, Some(show)).map_or(0, |w| w.behind),
                    estimate: self.to_watch_in(e, Some(show)).is_some_and(|w| !w.exact && w.behind > 0),
                });
            }
        }
        out.sort_by_key(|x| x.at);
        out
    }

    /// The bare `kot` dashboard: what plays, the Watching list with what comes next, the queue
    /// and the last sync. Reads state only (no disk walk, no network).
    pub fn ctl_dash(&self) -> kotora_ctl::Dashboard {
        let now = self.clock();
        let rows = self
            .entries
            .iter()
            .filter(|e| e.status == Status::Watching || e.rewatching)
            .map(|e| {
                let on_disk = crate::library::has_next_file(self, e);
                let timetable = self.airing.shows.iter().find(|a| a.id == e.id).and_then(Timetable::of).map(|t| t.with_progress(e.progress, now));
                let mut row = kotora_ctl::DashRow { title: e.title.clone(), progress: e.progress, episodes: e.episodes, updated_at: e.updated_at, on_disk, ..Default::default() };
                if let Some(t) = timetable {
                    if t.aired(now) > e.progress {
                        row.aired = true;
                    } else if let Some((_, at)) = t.next(now) {
                        let c = self.airing.clock(at);
                        row.air_at = Some(at);
                        row.air_slot = format!("{} {}", DAY_SHORT[usize::from(c.weekday)], c.hhmm());
                    }
                }
                row
            })
            .collect();
        kotora_ctl::Dashboard { now_unix: now, now: self.ctl_now(), rows, queue: self.queue, synced_at: self.synced_at, offline: self.offline }
    }
}

// ------------------------------------------------------------------------------------ workers

/// Where season pages come from: a season and a page number (from 0) give that page's anime and
/// whether more follow. MyAnimeList in the app, a fake in tests.
pub type Source<'a> = &'a mut dyn FnMut(Season, u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError>;

/// What a season worker needs besides its source.
pub struct Run<'a> {
    pub store: Option<&'a Store>,
    pub now: i64,
    pub force: bool,
    pub online: bool,
}

/// Asks for the pages of `season`, calling `each` with everything so far after each page but the
/// last. `Ok(None)`: `wanted` turned false, so it stopped between two requests.
fn pages(get: &mut dyn FnMut(Season, u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError>, season: Season, wanted: &dyn Fn() -> bool, each: &mut dyn FnMut(&[SeasonAnime])) -> Result<Option<Vec<SeasonAnime>>, ServiceError> {
    let mut all = Vec::new();
    for page in 0..PAGES {
        if !wanted() {
            return Ok(None);
        }
        let (mut v, more) = get(season, page)?;
        all.append(&mut v);
        if !more {
            break;
        }
        if page + 1 < PAGES {
            each(&all);
        }
    }
    Ok(Some(all))
}

/// The worker of the page's own season (see [`State::airing_fetch`]), without the thread:
/// 1. the saved copies, merged, at once (even stale ones);
/// 2. the current season from MyAnimeList. With no saved copy of it the rows show after the first
///    page and grow with each one; with an old copy the page keeps it until the new list is whole;
/// 3. the previous season (the shows of it still airing) at low priority: only while `wanted`.
pub fn run_current(run: &Run, list: &[ListItem], get: Option<Source>, wanted: &dyn Fn() -> bool, send: &mut dyn FnMut(Answer)) {
    let (now, force) = (run.now, run.force);
    let cur = Season::of(now);
    let prev = cur.previous();
    let saved = |se: Season| run.store.and_then(|st| st.season(crate::account::name(), se).ok().flatten());
    let (c, p) = (saved(cur), saved(prev));
    let fresh = |se: Season, x: &Option<(Vec<SeasonAnime>, i64)>| x.as_ref().is_some_and(|(_, at)| se.is_fresh(cur, *at, now));
    let (need_c, need_p) = (force || !fresh(cur, &c), force || !fresh(prev, &p));
    let mut fetched_at = c.as_ref().map(|(_, at)| *at);
    let mut c_shows = c.map_or_else(Vec::new, |x| x.0);
    let had_c = fetched_at.is_some();
    let mut p_shows = p.as_ref().map_or_else(Vec::new, |x| x.0.clone());
    let had_any = had_c || p.is_some();
    if had_any {
        send(Answer { season: cur, shows: airing::merge(&c_shows, &p_shows, list), fetched_at, error: None, done: !(need_c || need_p) || !run.online });
    }
    if !(need_c || need_p) {
        return;
    }
    if !run.online {
        if !had_any {
            send(Answer { season: cur, shows: Vec::new(), fetched_at: None, error: Some(format!("Offline: the season lists come from {}.", crate::account::active().title())), done: true });
        }
        return;
    }
    let Some(get) = get else {
        return send(Answer { season: cur, shows: Vec::new(), fetched_at, error: Some(format!("Sign in to {} to see the seasons.", crate::account::active().title())), done: true });
    };
    let fail = |e: ServiceError, fetched_at| Answer { season: cur, shows: Vec::new(), fetched_at, error: Some(crate::online::error_text(e)), done: true };
    if need_c {
        let mut partial = |v: &[SeasonAnime]| {
            if !had_c {
                send(Answer { season: cur, shows: airing::merge(v, &p_shows, list), fetched_at: None, error: None, done: false });
            }
        };
        match pages(get, cur, &|| true, &mut partial) {
            Ok(all) => {
                let all = all.unwrap_or_default();
                if let Some(st) = run.store {
                    let _ = st.save_season(crate::account::name(), cur, &all, now, &[prev]);
                }
                (c_shows, fetched_at) = (all, Some(now));
                send(Answer { season: cur, shows: airing::merge(&c_shows, &p_shows, list), fetched_at, error: None, done: !need_p });
            }
            Err(e) => return send(fail(e, fetched_at)),
        }
    }
    if need_p {
        if !wanted() {
            // Another season was picked: it has the budget. The next visit asks again.
            return send(Answer { season: cur, shows: airing::merge(&c_shows, &p_shows, list), fetched_at, error: None, done: true });
        }
        match pages(get, prev, wanted, &mut |_| {}) {
            Ok(Some(all)) => {
                if let Some(st) = run.store {
                    let _ = st.save_season(crate::account::name(), prev, &all, now, &[cur]);
                }
                p_shows = all;
                send(Answer { season: cur, shows: airing::merge(&c_shows, &p_shows, list), fetched_at, error: None, done: true });
            }
            Ok(None) => send(Answer { season: cur, shows: airing::merge(&c_shows, &p_shows, list), fetched_at, error: None, done: true }),
            Err(e) => send(fail(e, fetched_at)),
        }
    }
}

/// The worker of a browsed season (see [`State::airing_browse_fetch`]), without the thread. A
/// season picked since (`wanted` false) stops it between two requests, and its answers are dropped.
pub fn run_browse(run: &Run, season: Season, get: Option<Source>, wanted: &dyn Fn() -> bool, send: &mut dyn FnMut(Answer)) {
    let (now, force) = (run.now, run.force);
    let cur = Season::of(now);
    let saved = run.store.and_then(|st| st.season(crate::account::name(), season).ok().flatten());
    let fresh = saved.as_ref().is_some_and(|(_, at)| season.is_fresh(cur, *at, now));
    let fetched_at = saved.as_ref().map(|(_, at)| *at);
    if let Some((shows, at)) = saved {
        send(Answer { season, shows, fetched_at: Some(at), error: None, done: (fresh && !force) || !run.online });
    }
    if fresh && !force {
        return;
    }
    if !run.online {
        if fetched_at.is_none() {
            send(Answer { season, shows: Vec::new(), fetched_at: None, error: Some(format!("Offline: the season lists come from {}.", crate::account::active().title())), done: true });
        }
        return;
    }
    let Some(get) = get else {
        return send(Answer { season, shows: Vec::new(), fetched_at, error: Some(format!("Sign in to {} to see the seasons.", crate::account::active().title())), done: true });
    };
    let mut partial = |v: &[SeasonAnime]| {
        if fetched_at.is_none() {
            send(Answer { season, shows: v.to_vec(), fetched_at: None, error: None, done: false });
        }
    };
    match pages(get, season, wanted, &mut partial) {
        Ok(Some(all)) => {
            if let Some(st) = run.store {
                let _ = st.save_season(crate::account::name(), season, &all, now, &[cur, cur.previous()]);
            }
            send(Answer { season, shows: all, fetched_at: Some(now), error: None, done: true });
        }
        Ok(None) => {}
        Err(e) => send(Answer { season, shows: Vec::new(), fetched_at, error: Some(crate::online::error_text(e)), done: true }),
    }
}

/// Once a minute while the page is on screen: the countdowns. Returns whether anything changed.
pub fn tick(s: &mut State, tx: &mut Tx) -> bool {
    if s.page != Page::Airing
        || !s.on_screen()
        || s.airing.shows.is_empty()
        || s.airing.browse.is_some()
    {
        return false;
    }
    let now = s.clock();
    tx.wake_after(std::time::Duration::from_secs((60 - now.rem_euclid(60)).clamp(1, 60) as u64));
    if now.div_euclid(60) == s.airing.minute {
        return false;
    }
    s.airing_rebuild();
    true
}

// ------------------------------------------------------------------------------------ view

/// The page.
pub fn page(s: &State, width: f32) -> El {
    let a = &s.airing;
    let mut head = top(s, "Airing", None, false).child(season_picker(s));
    // The rows shown are saved or partial ones and a worker still asks: say so, small.
    if a.is_loading() && !a.shown().is_empty() {
        head = head.child(text("Updating\u{2026}").key("airing/updating").font(theme::LABEL).color(PAL.faint).px(12.0));
    }
    head = head.child(spacer());
    // Another season is a plain grid: no week to switch to.
    if a.browse.is_none() {
        head = head.child(views(a.view));
    }
    if s.auth == Auth::SignedIn {
        head = head.child(
            el().key("airing/refresh")
                .role("button")
                .label("Refresh the season")
                .focusable(true)
                .size(32.0, 32.0)
                .radius(theme::R_SM)
                .fill(PAL.field)
                .border(1.0, PAL.border)
                .center()
                .hover(PAL.hover())
                .press(PAL.press())
                .on_press_tx(|s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    s.airing_refresh();
                })
                .child(rune::icon(&crate::icons::SYNC, 16.0).color(if a.is_loading() { PAL.faint } else { PAL.dim })),
        );
    }
    let mut page = col().key("page/airing").w_full().h_full().child(head);
    let shown = a.shown();
    if let Some(e) = a.shown_error() {
        let retry = button("airing/retry", "Retry", false, Some(|s: &mut State| s.airing_refresh()));
        let msg = if shown.is_empty() { e.to_string() } else { format!("{e} Showing the saved season.") };
        page = page.child(alert("airing/error", PAL.status(Status::Dropped), msg, Some(retry)));
    }
    if shown.is_empty() {
        let first = a.browse.is_none() && a.error.is_none() && a.season.is_none();
        return page.child(if a.is_loading() || first { loading(&s.viewed_season().label()) } else { empty(a.shown_error().is_some(), s.service.title()) });
    }
    page = page.child(filters(s));
    match a.view {
        _ if a.browse.is_some() => page.child(grid(s, width)),
        View::Week => page.child(week(s)),
        View::Season => page.child(grid(s, width)),
    }
}

// ---------------------------------------------------------------------------- season picker

/// The keys that step the season on the page: `[` and `]`, Cmd or Ctrl `[` and `]`, and Cmd or
/// Alt with Left and Right. -1 the previous season, 1 the next, `None` other keys.
pub fn step_key(k: &rune::KeyEvent) -> Option<i32> {
    let bare = !(k.mods.cmd || k.mods.ctrl || k.mods.alt);
    match &k.key {
        rune::Key::Text(t) if bare && t == "[" => Some(-1),
        rune::Key::Text(t) if bare && t == "]" => Some(1),
        rune::Key::Left if k.mods.cmd || k.mods.alt => Some(-1),
        rune::Key::Right if k.mods.cmd || k.mods.alt => Some(1),
        _ if k.is_command('[') => Some(-1),
        _ if k.is_command(']') => Some(1),
        _ => None,
    }
}

/// The seasons of the menu, newest first: [`AHEAD`] after the current one, the current one and
/// [`BEHIND`] before it. The viewed season follows when it is further back (the arrows go on).
pub fn menu_seasons(current: Season, viewed: Season) -> Vec<Season> {
    let mut v: Vec<Season> = (-BEHIND..=AHEAD)
        .rev()
        .map(|n| current.shift(n))
        .filter(|se| *se >= Season::OLDEST)
        .collect();
    if !v.contains(&viewed) {
        v.push(viewed);
    }
    v
}

/// `‹  Fall 2026 ▾  ›` beside the title: the arrows step one season, the title opens the menu.
fn season_picker(s: &State) -> El {
    let (first, last) = s.season_range();
    let viewed = s.viewed_season();
    let open = s.airing.menu;
    let arrow = |key: &str, label: &str, icon: &'static rune::Icon, by: i32, on: bool| {
        el().key(key.to_string())
            .role("button")
            .label(label.to_string())
            .focusable(on)
            .size(28.0, 28.0)
            .radius(theme::R_SM)
            .center()
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.airing_step(by);
            })
            .child(rune::icon(icon, 16.0).color(if on { PAL.dim } else { PAL.faint.alpha(0.4) }))
    };
    let toggle = |s: &mut State, tx: &mut Tx| {
        tx.instant();
        s.airing.menu = !s.airing.menu;
    };
    let title = row()
        .key("airing/season")
        .role("button")
        .label(format!("Season: {}. Choose a season", viewed.label()))
        .focusable(true)
        .h(28.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(theme::R_SM)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(toggle)
        .on_key(move |s: &mut State, k: &rune::KeyEvent, tx: &mut Tx| {
            if matches!(k.key, rune::Key::Enter | rune::Key::Down) && !s.airing.menu {
                toggle(s, tx);
                return true;
            }
            false
        })
        .child(text(viewed.label()).font(theme::SECTION).color(PAL.text))
        .child(rune::icon(&crate::icons::DOWN, 14.0).color(PAL.dim));
    let title = if open { title.fill(PAL.raised) } else { title };
    row()
        .key("airing/picker")
        .gap(2.0)
        .center_y()
        .pb(2.0)
        .child(arrow(
            "airing/prev",
            "Previous season",
            &crate::icons::LEFT,
            -1,
            viewed > first,
        ))
        .child(title)
        .child(arrow(
            "airing/next",
            "Next season",
            &crate::icons::RIGHT,
            1,
            viewed < last,
        ))
}

/// The season menu under the title: "This season" to jump back, then the seasons near the
/// current one, newest first, with a check on the one shown. It holds the keyboard while open
/// (arrows move, Return or Space picks, Escape or a press outside closes it and the focus goes
/// back to the title). Mounted at the window's root so nothing clips it.
pub(crate) fn season_menu(s: &State) -> El {
    let current = s.current_season();
    let viewed = s.viewed_season();
    let item = |key: String,
                role: &'static str,
                label: String,
                note: Option<&'static str>,
                on: bool,
                go: Season| {
        let r = row()
            .key(key)
            .role(role)
            .label(label.clone())
            .checked(on)
            .focusable(true)
            .focus_selected(on)
            .focus_highlight(PAL.hover())
            .h(30.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(theme::R_SM)
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.airing_goto(go);
                s.airing.menu = false;
            });
        let mark = el().size(14.0, 14.0).center().child(if on {
            rune::icon(&crate::icons::CHECK, 14.0).color(PAL.accent_hi)
        } else {
            el()
        });
        let r = r.child(mark).child(
            text(label)
                .font(if on { theme::BODY_MED } else { theme::BODY })
                .color(PAL.text),
        );
        match note {
            Some(n) => r
                .child(spacer())
                .child(text(n.to_string()).font(theme::SMALL).color(PAL.faint)),
            None => r,
        }
    };
    let mut m = col()
        .key("season-menu")
        .role("menu")
        .label("Season")
        .focus_group()
        .w(220.0)
        .padding(4.0)
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border_strong)
        .shadow(
            8.0,
            24.0,
            rune::Color::BLACK.alpha(if PAL.light { 0.14 } else { 0.4 }),
        )
        .anchored("airing/season", rune::Side::Bottom, rune::Align::Start, 6.0)
        .modal(true, |s: &mut State| s.airing.menu = false)
        .focus_return("airing/season")
        .child(item(
            "airing/season/this".into(),
            "menuitem",
            "This season".into(),
            None,
            false,
            current,
        ))
        .child(el().w_full().h(1.0).my(4.0).fill(PAL.border));
    for se in menu_seasons(current, viewed) {
        let note = match se.cmp(&current) {
            std::cmp::Ordering::Greater => Some("Next"),
            std::cmp::Ordering::Equal => Some("Now"),
            std::cmp::Ordering::Less => None,
        };
        m = m.child(item(
            format!("airing/season/{}", se.key().replace('/', "-")),
            "menuitemradio",
            se.label(),
            note,
            se == viewed,
            se,
        ));
    }
    m
}

fn views(v: View) -> El {
    let mut r = row().key("airing/views").gap(2.0).padding(2.0).radius(theme::R_SM).fill(PAL.field).border(1.0, PAL.border);
    for (view, label) in [(View::Week, "This week"), (View::Season, "This season")] {
        let on = v == view;
        let c = el()
            .key(format!("airing/view/{}", if view == View::Week { "week" } else { "season" }))
            .role("button")
            .label(label)
            .focusable(true)
            .h(26.0)
            .px(10.0)
            .center()
            .radius(theme::R_XS)
            .hover(PAL.hover())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.airing_view(view);
            })
            .child(text(label).font(theme::LABEL_BTN).color(if on { PAL.text } else { PAL.dim }));
        r = r.child(if on { c.fill(PAL.raised) } else { c });
    }
    r
}

fn chip(key: String, label: &str, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    let c = row()
        .key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(26.0)
        .px(10.0)
        .center_y()
        .radius(13.0)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s);
        })
        .child(text(label.to_string()).font(theme::LABEL).color(if on { PAL.accent_hi } else { PAL.dim }));
    if on { c.fill(PAL.accent.alpha(0.14)) } else { c.border(1.0, PAL.border) }
}

fn filters(s: &State) -> El {
    let a = &s.airing;
    let mut r = row().key("airing/filters").flex_wrap().gap(6.0).px(theme::PAGE_PAD).pb(10.0).center_y();
    for k in Kind::ALL {
        r = r.child(chip(format!("airing/kind/{}", k.label()), k.label(), a.kind == k, move |s: &mut State| s.airing_kind(k)));
    }
    r = r.child(el().w(1.0).h(18.0).mx(4.0).fill(PAL.border));
    for m in Mine::ALL {
        r = r.child(chip(format!("airing/mine/{}", m.label()), m.label(), a.mine == m, move |s: &mut State| s.airing_mine(m)));
    }
    if a.browse.is_some() {
        return r;
    }
    if a.view == View::Season {
        r = r.child(el().w(1.0).h(18.0).mx(4.0).fill(PAL.border));
        for o in Order::ALL {
            r = r.child(chip(format!("airing/order/{}", o.label()), o.label(), a.order == o, move |s: &mut State| s.airing_order(o)));
        }
    }
    r.child(spacer()).child(text("Times are estimated; ~ marks estimated counts".to_string()).key("airing/estimated").font(theme::SMALL).color(PAL.faint))
}

fn loading(season: &str) -> El {
    let mut c = col().key("airing/loading").px(theme::PAGE_PAD).pt(4.0).gap(8.0).child(text(format!("Loading {season}\u{2026}")).font(theme::LABEL).color(PAL.faint).pb(2.0));
    for i in 0..4 {
        c = c.child(el().key(format!("airing/skel/{i}")).w_full().h(44.0).radius(theme::R_MD).fill(PAL.raised));
    }
    c
}

fn empty(failed: bool, svc: &str) -> El {
    let (title, help) = if failed {
        ("No season to show", format!("Kotora shows the season once {svc} answers."))
    } else {
        ("Nothing airing", format!("{svc} lists no anime for this season yet."))
    };
    col()
        .key("airing/empty")
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(4.0)
        .pb(60.0)
        .child(text(title).font(theme::SECTION).color(PAL.text))
        .child(text(help).font(theme::LABEL).color(PAL.dim))
}

// ------------------------------------------------------------------------------- this week

fn week(s: &State) -> El {
    let n = s.airing.week.len();
    if s.airing.days.iter().all(Vec::is_empty) && s.airing.untimed.is_empty() {
        return col()
            .key("airing/week-none")
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(4.0)
            .pb(60.0)
            .child(text("Nothing airs this week with these filters").font(theme::SECTION).color(PAL.text))
            .child(text("Try Everything, or All types.").font(theme::LABEL).color(PAL.dim));
    }
    col().key("airing/week-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
        col().key("airing/week").growth(GrowthSpec::default()).virtual_rows(
            n,
            WEEK_ROW_H,
            |s: &State, i| match s.airing.week.get(i) {
                Some(WeekRow::Day(d)) => format!("day/{d}").into(),
                Some(WeekRow::Slot(d, k)) => s.airing.days[usize::from(*d)].get(*k).map_or_else(|| format!("gone-{i}"), |sl| format!("slot/{}", s.airing.shows[sl.show].id)).into(),
                Some(WeekRow::Nothing(d)) => format!("nothing/{d}").into(),
                Some(WeekRow::UntimedHead) => "untimed".into(),
                Some(WeekRow::Untimed(k)) => s.airing.untimed.get(*k).map_or_else(|| format!("gone-{i}"), |(j, _)| format!("untimed/{}", s.airing.shows[*j].id)).into(),
                None => format!("gone-{i}").into(),
            },
            |s: &State, i| match s.airing.week.get(i) {
                Some(WeekRow::Day(d)) => day_head(s, *d),
                Some(WeekRow::Slot(d, k)) => s.airing.days[usize::from(*d)].get(*k).map_or_else(el, |sl| slot_row(s, sl)),
                Some(WeekRow::Nothing(_)) => row().h(36.0).px(theme::PAGE_PAD).pl(theme::PAGE_PAD + 64.0).center_y().child(text("Nothing airs").font(theme::LABEL).color(PAL.faint)),
                Some(WeekRow::UntimedHead) => head_line("No broadcast time".into(), false, Some(format!("airing on {}, no time to estimate", s.service.title()))),
                Some(WeekRow::Untimed(k)) => s.airing.untimed.get(*k).map_or_else(el, |(j, why)| untimed_row(s, *j, why)),
                None => el(),
            },
        ),
    )
}

fn head_line(label: String, today: bool, end: Option<String>) -> El {
    let mut r = row()
        .h(DAY_HEAD_H)
        .px(theme::PAGE_PAD)
        .pt(14.0)
        .gap(8.0)
        .center_y()
        .child(text(label.to_uppercase()).font(theme::CAPTION).color(if today { PAL.accent_hi } else { PAL.faint }));
    if let Some(e) = end {
        r = r.child(text(e).font(theme::SMALL).color(PAL.faint));
    }
    r
}

/// Day `d` of the 7 (0 today): "Today · Friday Jan 15", "Tomorrow · …", "Sunday Jan 17".
fn day_head(s: &State, d: u8) -> El {
    let a = &s.airing;
    let c = a.clock(a.day0 + i64::from(d) * 86_400 + 12 * 3600);
    let name = format!("{} {} {}", DAY_LONG[usize::from(c.weekday)], MONTHS[(c.month - 1) as usize % 12], c.date);
    let today = d == 0;
    let label = match d {
        0 => format!("Today \u{b7} {name}"),
        1 => format!("Tomorrow \u{b7} {name}"),
        _ => name,
    };
    let n = a.days[usize::from(d)].iter().filter(|x| x.mine).count();
    let end = (n > 0).then(|| format!("{n} on your list"));
    let h = head_line(label, today, end).key(format!("airing/day/{d}"));
    if today { h.child(el().key("airing/today").size(6.0, 6.0).radius(3.0).fill(PAL.accent)) } else { h }
}

fn small_cover(s: &State, i: usize) -> El {
    let a = &s.airing.shows[i];
    let tile = cover(a.id, s.airing_title(i), 30.0, 42.0, 11.0);
    match s.covers.get(a.id, a.cover_url.as_deref(), 30.0, 42.0) {
        Some(img) => tile.child(el().absolute(0.0, 0.0).size(30.0, 42.0).radius(theme::R_XS).pixels(img.pixels())),
        None => tile,
    }
}

/// "+ Watching" and "+ Plan" for an anime not on the list; its status tag when it is.
fn add_or_status(s: &State, id: u32, status: Option<Status>) -> El {
    match status {
        Some(st) => tag(st.label(), PAL.status(st), true),
        None if s.has_list() => row()
            .gap(6.0)
            .child(action(format!("airing/add/{id}"), "+ Watching", false, move |s: &mut State, _| s.airing_add(id, Status::Watching)))
            .child(action(format!("airing/plan/{id}"), "+ Plan", false, move |s: &mut State, _| s.airing_add(id, Status::PlanToWatch))),
        None => el(),
    }
}

fn slot_row(s: &State, sl: &Slot) -> El {
    let a = &s.airing.shows[sl.show];
    let id = a.id;
    let title = s.airing_title(sl.show);
    let status = s.entries.iter().find(|e| e.id == id).map(|e| e.status);
    let fg = if sl.mine { PAL.text } else { PAL.dim };
    let mut info = row().gap(8.0).center_y().child(text(format!("Ep {}", sl.episode)).font(theme::LABEL_NUM).color(PAL.dim));
    let kind = kind_label(&a.format);
    if !kind.is_empty() {
        info = info.child(tag_tinted(kind, PAL.dim, PAL.raised));
    }
    if !sl.note.is_empty() {
        let c = if sl.note == "aired" { PAL.faint } else { PAL.accent_hi };
        info = info.child(text(sl.note.clone()).key("note").font(theme::LABEL_NUM).color(c));
    }
    if let Some(w) = &sl.to_watch {
        let c = if w.contains("to watch") { PAL.text } else { PAL.faint };
        info = info.child(text("\u{b7}").font(theme::LABEL).color(PAL.faint)).child(text(w.clone()).key("to-watch").font(theme::LABEL_NUM).color(c));
    }
    let line = row()
        .key("line")
        .h(WEEK_ROW_H)
        .px(theme::PAGE_PAD)
        .gap(theme::COL_GAP)
        .center_y()
        .role("button")
        .label(format!("About {title}"))
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.airing_details(id);
        })
        .child(text(sl.time.clone()).font(theme::NUM_MED).color(if sl.mine { PAL.text } else { PAL.dim }).w(50.0))
        .child(small_cover(s, sl.show))
        .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(title.to_string()).font(theme::ROW_TITLE).color(fg).truncate()).child(info))
        .child(add_or_status(s, id, status));
    let line = if sl.mine { line.fill(PAL.accent.alpha(0.06)).child(el().absolute(0.0, 8.0).size(3.0, WEEK_ROW_H - 16.0).radius(1.5).fill(PAL.accent)) } else { line };
    col().w_full().child(line).child(el().w_full().h(1.0).fill(PAL.border.alpha(0.5)))
}

fn untimed_row(s: &State, i: usize, why: &str) -> El {
    let a = &s.airing.shows[i];
    let id = a.id;
    let title = s.airing_title(i);
    let status = s.entries.iter().find(|e| e.id == id).map(|e| e.status);
    row()
        .key("line")
        .h(WEEK_ROW_H)
        .px(theme::PAGE_PAD)
        .gap(theme::COL_GAP)
        .center_y()
        .role("button")
        .label(format!("About {title}"))
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.airing_details(id);
        })
        .child(text("\u{2013}").font(theme::NUM_MED).color(PAL.faint).w(50.0))
        .child(small_cover(s, i))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(title.to_string()).font(theme::ROW_TITLE).color(PAL.dim).truncate())
                .child(text(why.to_string()).font(theme::LABEL).color(PAL.faint).truncate()),
        )
        .child(add_or_status(s, id, status))
}

// ----------------------------------------------------------------------------- this season

/// Cards a grid row holds at `width` (the main area's).
pub fn per_row(width: f32) -> usize {
    (((width - 2.0 * theme::PAGE_PAD + CARD_GAP) / (CARD_W + CARD_GAP)).floor() as usize).max(1)
}

fn grid(s: &State, width: f32) -> El {
    let per = per_row(width);
    let n = s.airing.cards.len();
    if n == 0 {
        return col()
            .key("airing/grid-none")
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(4.0)
            .pb(60.0)
            .child(text("No anime match these filters").font(theme::SECTION).color(PAL.text));
    }
    let rows = n.div_ceil(per);
    col().key("airing/grid-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
        col().key("airing/grid").growth(GrowthSpec::default()).virtual_rows(
            rows,
            CARD_ROW_H,
            move |s: &State, r| s.airing.cards.get(r * per).map_or_else(|| format!("gone-{r}"), |c| format!("cards/{}", s.airing.shown()[c.show].id)).into(),
            move |s: &State, r| {
                let mut line = row().px(theme::PAGE_PAD).pb(CARD_GAP).gap(CARD_GAP).items_start();
                for c in s.airing.cards.iter().skip(r * per).take(per) {
                    line = line.child(card(s, c));
                }
                line
            },
        ),
    )
}

fn card(s: &State, c: &Card) -> El {
    let a = &s.airing.shown()[c.show];
    let id = a.id;
    let title = s.airing_title(c.show);
    let tile = cover(id, title, CARD_W, CARD_COVER_H, 24.0).radius(theme::R_MD);
    let mut art = match s.covers.get(id, a.cover_url.as_deref(), CARD_W, CARD_COVER_H) {
        Some(img) => tile.child(el().absolute(0.0, 0.0).size(CARD_W, CARD_COVER_H).radius(theme::R_MD).pixels(img.pixels())),
        None => tile,
    };
    if let Some(st) = c.status {
        art = art.child(el().absolute(6.0, 6.0).child(tag_tinted(st.label(), PAL.text, PAL.bg.alpha(0.85))));
    }
    let when = if c.when.is_empty() {
        el()
    } else {
        text(c.when.clone()).key("when").font(TextStyle::new(11.0, if c.lit { 600.0 } else { 400.0 }).features("'tnum' 1")).color(if c.lit { PAL.accent_hi } else { PAL.dim }).max_lines(2).wrap()
    };
    let mut words = col()
        .h(CARD_TEXT_H)
        .gap(3.0)
        .child(text(title.to_string()).font(TextStyle::new(12.0, 500.0)).color(PAL.text).group_color(PAL.accent_hi).max_lines(2).wrap())
        .child(text(c.meta.clone()).font(theme::SMALL).color(PAL.faint).truncate())
        .child(when);
    if let Some((w, n)) = &c.to_watch {
        words = words.child(text(w.clone()).key("to-watch").font(TextStyle::new(11.0, if *n > 0 { 600.0 } else { 400.0 }).features("'tnum' 1")).color(if *n > 0 { PAL.text } else { PAL.faint }).truncate());
    }
    let open = col()
        .key("open")
        .role("button")
        .label(format!("About {title}"))
        .gap(6.0)
        .radius(theme::R_MD)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.airing_details(id);
        })
        .child(art)
        .child(words);
    let mut card = col().key(format!("card/{id}")).w(CARD_W).gap(4.0).child(open);
    if c.status.is_none() && s.has_list() {
        card = card.child(
            row()
                .w_full()
                .gap(4.0)
                .child(small_add(format!("airing/add/{id}"), "+ Watching", id, Status::Watching).grow(1.0))
                .child(small_add(format!("airing/plan/{id}"), "+ Plan", id, Status::PlanToWatch)),
        );
    }
    card
}

/// A card's add button: smaller than `ui::action`, so two fit under a cover.
fn small_add(key: String, label: &str, id: u32, status: Status) -> El {
    el().key(key)
        .role("button")
        .label(format!("Add as {}", status.label()))
        .focusable(true)
        .h(24.0)
        .px(7.0)
        .radius(theme::R_SM)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.airing_add(id, status);
        })
        .child(text(label.to_string()).font(TextStyle::new(11.0, 600.0)).color(PAL.text))
}

// ------------------------------------------------------------------------------ fake data

/// A made-up season for the scenes and tests (titles invented, no real data). The clock is
/// `model::FAKE_NOW` (Friday 2027-01-15, 17:00 in Japan): Winter 2027. The fake list's airing
/// entries (ids 0, 1, 3, 6, 10) are here too, with slots that match their "Ep 7 on Sat" notes.
pub fn fake_season() -> (Season, Vec<SeasonAnime>) {
    use kotora_core::airing::Broadcast;
    let b = |day: u8, h: u16, m: u16| Some(Broadcast { day, minute: Some(h * 60 + m) });
    #[allow(clippy::too_many_arguments)]
    let a = |id: u32, title: &str, en: Option<&str>, format: &str, eps: Option<u32>, air: &str, start: &str, bc: Option<Broadcast>, members: u32, mean: Option<f32>| SeasonAnime {
        id,
        title: title.into(),
        title_en: en.map(Into::into),
        format: format.into(),
        episodes: eps,
        air_status: air.into(),
        start_date: Some(start.into()).filter(|s: &String| !s.is_empty()),
        broadcast: bc,
        members: Some(members),
        mean,
        cover_url: Some(format!("https://cdn.example.invalid/s{id}.jpg")),
        ..Default::default()
    };
    let airing = "currently_airing";
    let shows = vec![
        // On the fake list (the Watching page's airing rows).
        a(0, "Lanterns of Kirigaoka", Some("The Lanterns on Kirigaoka Hill"), "tv", Some(12), airing, "2026-12-05", b(5, 23, 30), 412_000, Some(8.44)),
        a(1, "Starfall Delivery Service", Some("Starfall Express"), "tv", Some(13), airing, "2026-11-25", b(2, 22, 0), 268_000, Some(8.1)),
        a(3, "Slow Life on Hoshizora Island", Some("Island Under the Stars"), "tv", Some(12), airing, "2026-12-27", b(6, 1, 0), 120_500, Some(7.6)),
        a(6, "Mecha Bento Club", None, "tv", Some(10), airing, "2026-12-18", b(4, 23, 0), 98_000, Some(7.2)),
        a(10, "Tidepool Radio", None, "tv", Some(12), airing, "2026-11-17", b(1, 0, 30), 64_000, Some(6.9)),
        // Winter 2027, not on the list.
        a(9301, "Moonlit Harbor Academy", Some("Moonlit Harbor"), "tv", Some(12), airing, "2027-01-08", b(4, 0, 0), 350_000, Some(8.31)),
        a(9302, "Crimson Signal", None, "tv", Some(24), airing, "2027-01-10", b(6, 17, 0), 301_000, Some(8.02)),
        a(9303, "Quiet Orchard Detective Agency", None, "tv", Some(12), airing, "2027-01-07", b(3, 23, 30), 233_000, Some(7.88)),
        a(9304, "Electric Lighthouse", Some("The Electric Lighthouse"), "ona", Some(10), airing, "2027-01-13", b(2, 18, 0), 187_000, Some(7.5)),
        a(9305, "Silver Comet Festival", None, "movie", Some(1), "not_yet_aired", "2027-02-19", None, 150_000, None),
        a(9306, "Hidden Garden Workshop", None, "tv", Some(13), airing, "2027-01-11", b(0, 21, 0), 142_000, Some(7.41)),
        a(9307, "Northern Station Rebirth", None, "tv", Some(12), airing, "2027-01-09", b(5, 1, 30), 131_000, Some(7.33)),
        a(9308, "Velvet Market Zero", None, "ona", None, airing, "2027-01-05", Some(Broadcast { day: 1, minute: None }), 99_000, Some(6.95)),
        a(9309, "Gentle Valley Side Stories", None, "tv", Some(12), airing, "2027-01-06", b(2, 0, 0), 87_000, Some(7.05)),
        a(9310, "Endless Bridge", None, "tv", Some(12), "not_yet_aired", "2027-01-24", b(6, 22, 0), 82_000, None),
        a(9311, "Winter Theatre", None, "tv", Some(12), airing, "2027-01-08", b(4, 23, 45), 76_000, Some(7.18)),
        a(9312, "Bright Archive", None, "ona", Some(8), airing, "2027-01-15", None, 52_000, None),
        a(9313, "Lost Academy Final Act", None, "tv", Some(12), airing, "2027-01-04", b(0, 23, 0), 49_000, Some(6.81)),
        a(9314, "Hollow Garden", None, "movie", Some(1), "finished_airing", "2027-01-01", None, 47_000, Some(7.9)),
        a(9315, "Paper Lantern Market", None, "tv", Some(12), airing, "2027-01-12", b(1, 23, 0), 41_000, Some(7.02)),
        a(9316, "Second Signal", None, "tv", Some(12), airing, "2027-01-09", b(5, 9, 0), 36_000, Some(6.6)),
        a(9317, "Electric Valley", None, "ona", Some(12), airing, "2027-01-14", b(3, 12, 0), 33_000, Some(6.77)),
        a(9318, "Moonlit Workshop", None, "tv", Some(24), airing, "2027-01-03", b(6, 9, 30), 30_000, Some(6.4)),
        a(9319, "Quiet Comet", None, "tv", Some(12), airing, "2027-01-10", b(6, 23, 0), 28_000, Some(6.9)),
        a(9320, "Northern Lantern", None, "tv", Some(12), airing, "2027-01-15", b(4, 12, 0), 21_000, None),
    ];
    (Season { year: 2027, quarter: 0 }, shows)
}

/// A made-up other season for the scenes and tests: `season` is a quarter of the fake year, its
/// shows invented (ids 94xx). The fake list's Starfall Delivery Service (1) and Tidepool Radio
/// (10) are in Fall 2026 too, as long runners still airing.
pub fn fake_other_season(season: Season) -> Vec<SeasonAnime> {
    use kotora_core::airing::Broadcast;
    let b = |day: u8, h: u16, m: u16| {
        Some(Broadcast {
            day,
            minute: Some(h * 60 + m),
        })
    };
    #[allow(clippy::too_many_arguments)]
    let a = |id: u32,
             title: &str,
             en: Option<&str>,
             format: &str,
             eps: Option<u32>,
             air: &str,
             start: &str,
             bc: Option<Broadcast>,
             members: u32,
             mean: Option<f32>| SeasonAnime {
        id,
        title: title.into(),
        title_en: en.map(Into::into),
        format: format.into(),
        episodes: eps,
        air_status: air.into(),
        start_date: Some(start.into()),
        end_date: None,
        broadcast: bc,
        members: Some(members),
        mean,
        cover_url: Some(format!("https://cdn.example.invalid/s{id}.jpg")),
        ..Default::default()
    };
    let (done, soon) = ("finished_airing", "not_yet_aired");
    if season > Season::of(crate::model::FAKE_NOW) {
        return vec![
            a(
                9501,
                "Cloudline Railway Diaries",
                Some("Diaries of the Cloudline Railway"),
                "tv",
                Some(12),
                soon,
                "2027-04-04",
                b(6, 17, 30),
                88_000,
                None,
            ),
            a(
                9502,
                "Salt Garden Sorcery",
                None,
                "tv",
                Some(24),
                soon,
                "2027-04-10",
                b(5, 23, 0),
                64_000,
                None,
            ),
            a(
                9503,
                "Paper Moon Orchestra",
                None,
                "ona",
                Some(8),
                soon,
                "2027-04-17",
                None,
                41_000,
                None,
            ),
            a(
                9504,
                "Evening Bell Mystery",
                None,
                "tv",
                None,
                soon,
                "2027-04",
                None,
                27_000,
                None,
            ),
            a(
                9505,
                "Harbor Light Movie",
                None,
                "movie",
                Some(1),
                soon,
                "2027-05-14",
                None,
                19_000,
                None,
            ),
        ];
    }
    vec![
        a(
            9401,
            "Ember Tower Chronicle",
            Some("Chronicle of Ember Tower"),
            "tv",
            Some(24),
            done,
            "2026-10-05",
            b(0, 23, 30),
            604_000,
            Some(8.71),
        ),
        a(
            9402,
            "Quiet Afternoon Club",
            None,
            "tv",
            Some(12),
            done,
            "2026-10-02",
            b(4, 22, 0),
            411_000,
            Some(8.24),
        ),
        a(
            1,
            "Starfall Delivery Service",
            Some("Starfall Express"),
            "tv",
            Some(13),
            "currently_airing",
            "2026-11-25",
            b(2, 22, 0),
            268_000,
            Some(8.1),
        ),
        a(
            9403,
            "Glass Coast Detective",
            None,
            "tv",
            Some(12),
            done,
            "2026-10-08",
            b(3, 0, 0),
            255_000,
            Some(7.96),
        ),
        a(
            9404,
            "Mountain Pass Spirits",
            None,
            "ona",
            Some(6),
            done,
            "2026-10-15",
            None,
            190_000,
            Some(7.71),
        ),
        a(
            9405,
            "Neon Bakery",
            Some("The Neon Bakery"),
            "tv",
            Some(12),
            done,
            "2026-10-04",
            b(6, 17, 0),
            176_000,
            Some(7.55),
        ),
        a(
            9406,
            "Last Train to Hoshimi",
            None,
            "movie",
            Some(1),
            done,
            "2026-10-23",
            None,
            140_000,
            Some(8.05),
        ),
        a(
            9407,
            "Ninth Street Orchestra",
            None,
            "tv",
            Some(13),
            done,
            "2026-10-06",
            b(1, 23, 0),
            121_000,
            Some(7.4),
        ),
        a(
            10,
            "Tidepool Radio",
            None,
            "tv",
            Some(12),
            "currently_airing",
            "2026-11-17",
            b(1, 0, 30),
            64_000,
            Some(6.9),
        ),
        a(
            9408,
            "Violet Harbor Games",
            None,
            "tv",
            Some(12),
            done,
            "2026-10-09",
            b(4, 23, 30),
            98_000,
            Some(7.1),
        ),
        a(
            9409,
            "Small Planet Bakery",
            None,
            "tv",
            Some(12),
            done,
            "2026-10-03",
            b(5, 9, 0),
            77_000,
            Some(6.88),
        ),
        a(
            9410,
            "Clockwork Orchard",
            None,
            "ona",
            Some(10),
            done,
            "2026-10-12",
            None,
            52_000,
            Some(6.5),
        ),
    ]
}

/// Shows another season, as if MyAnimeList had answered (scenes and tests).
pub fn fake_browse(s: &mut State, season: Season) {
    s.covers.fake();
    s.airing_browse_show(season, fake_other_season(season));
}

/// Fills the page with the fake season, as the scenes show it (Central European winter time).
pub fn fake(s: &mut State) {
    let (season, shows) = fake_season();
    s.airing.tz = Some(3600);
    s.airing_show(season, shows);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> State {
        let mut s = State::fake();
        fake(&mut s);
        s.go(Page::Airing);
        s
    }

    #[test]
    fn the_week_follows_the_users_zone_and_list() {
        let s = page();
        let a = &s.airing;
        // FAKE_NOW is Friday 2027-01-15 09:00 in Central Europe; the 7 days start today.
        assert_eq!(a.today, 4);
        // Lanterns (on the list, Saturdays 23:30 JST): tomorrow 15:30 CET, episode 7, counting down.
        let sat = &a.days[1];
        let l = sat.iter().find(|x| a.shows[x.show].id == 0).unwrap();
        assert_eq!((l.episode, l.time.as_str(), l.mine), (7, "15:30", true));
        assert_eq!(l.note, "in 1 d 6 h");
        // Sundays 01:00 JST is Saturday 17:00 in Europe: the date moves across midnight.
        let h = sat.iter().find(|x| a.shows[x.show].id == 3).unwrap();
        assert_eq!((h.episode, h.time.as_str()), (4, "17:00"));
        // Mecha Bento Club (Fridays 23:00 JST, progress 4): episode 5 today at 15:00, in 6 h.
        let m = a.days[0].iter().find(|x| a.shows[x.show].id == 6).unwrap();
        assert_eq!((m.episode, m.time.as_str(), m.note.as_str()), (5, "15:00", "in 6 h"));
        // Earlier today: aired. Not on the list: no countdown.
        let early = a.days[0].iter().find(|x| a.shows[x.show].id == 9320).unwrap();
        assert_eq!((early.time.as_str(), early.note.as_str(), early.mine), ("04:00", "aired", false));
        let later = a.days[2].iter().find(|x| a.shows[x.show].id == 9302).unwrap();
        assert_eq!((later.time.as_str(), later.note.as_str(), later.mine), ("09:00", "", false));
        // Tidepool Radio, Tuesdays 00:30 JST: Monday 16:30 in Europe, episode 10 (9 aired).
        let mon = a.days[3].iter().find(|x| a.shows[x.show].id == 10).unwrap();
        assert_eq!((mon.episode, mon.time.as_str(), mon.note.as_str()), (10, "16:30", "in 3 d 7 h"));
        // Yesterday's slots are next week's: Moonlit Harbor (Thursdays 16:00 CET) is day 6, ep 3.
        let thu = a.days[6].iter().find(|x| a.shows[x.show].id == 9301).unwrap();
        assert_eq!(thu.episode, 3);
        // Days are in time order.
        for d in &a.days {
            assert!(d.windows(2).all(|w| w[0].at <= w[1].at));
        }
        // No slot time: listed apart, with why.
        assert!(a.untimed.iter().any(|(i, why)| a.shows[*i].id == 9308 && why.contains("Tuesdays")));
        assert!(a.untimed.iter().any(|(i, why)| a.shows[*i].id == 9312 && why.contains("No broadcast")));
        assert!(!a.untimed.iter().any(|(i, _)| a.shows[*i].id == 9305), "not yet aired: not in the week");
    }

    #[test]
    fn progress_ahead_moves_the_week_forward() {
        let mut s = page();
        // Two episodes ahead of the estimate on Lanterns: episode 9 is next, on Saturday.
        s.edit(0, kotora_core::Change { progress: Some(8), ..Default::default() }).unwrap();
        s.airing_rebuild();
        let a = &s.airing;
        let l = a.days[1].iter().find(|x| a.shows[x.show].id == 0).unwrap();
        assert_eq!(l.episode, 9);
    }

    #[test]
    fn filters_and_cards() {
        let mut s = page();
        let all = s.airing.cards.len();
        assert_eq!(all, 25);
        s.airing_kind(Kind::Movie);
        assert!(s.airing.cards.iter().all(|c| s.airing.shows[c.show].format == "movie"));
        assert_eq!(s.airing.cards.len(), 2);
        s.airing_kind(Kind::All);
        s.airing_mine(Mine::On);
        assert_eq!(s.airing.cards.iter().map(|c| s.airing.shows[c.show].id).collect::<Vec<_>>(), [0, 1, 3, 6, 10]);
        assert!(s.airing.days.iter().flatten().all(|x| x.mine));
        s.airing_mine(Mine::Off);
        assert_eq!(s.airing.cards.len(), all - 5);
        s.airing_mine(Mine::All);
        // Most members first; the lines.
        assert_eq!(s.airing.shows[s.airing.cards[0].show].id, 0);
        let c = &s.airing.cards[0];
        assert_eq!((c.meta.as_str(), c.when.as_str(), c.status, c.lit), ("TV \u{b7} 12 eps \u{b7} \u{2605} 8.44", "Sat 15:30 \u{b7} ep 7 in 1 d 6 h", Some(Status::Watching), true));
        let movie = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == 9305).unwrap();
        assert_eq!(movie.when, "Starts Feb 19, 2027");
        let other = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == 9302).unwrap();
        assert_eq!((other.when.as_str(), other.lit), ("Sun 09:00 \u{b7} ep 2", false));
    }

    #[test]
    fn adding_goes_through_the_queue() {
        let mut s = page();
        let queued = s.queue;
        s.airing_add(9302, Status::PlanToWatch);
        let e = s.entries.iter().find(|e| e.id == 9302).unwrap();
        assert_eq!((e.status, e.waiting), (Status::PlanToWatch, Some("Waiting to sync")));
        assert!(s.ops.len() >= queued as usize);
        let c = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == 9302).unwrap();
        assert_eq!(c.status, Some(Status::PlanToWatch));
        s.airing_details(9302);
        assert_eq!(s.page, Page::Details);
        assert_eq!(s.lit_page(), Page::Airing);
    }

    #[test]
    fn kot_airing_lists_the_week_for_the_list() {
        let s = page();
        let v = s.ctl_airing();
        assert_eq!(v.iter().map(|x| (x.title.as_str(), x.episode)).collect::<Vec<_>>()[..2], [("Mecha Bento Club", 5), ("Lanterns of Kirigaoka", 7)]);
        assert!(v.iter().all(|x| x.at > FAKE && x.at - FAKE <= WEEK));
        // Tidepool Radio: episode 9 aired Monday night, progress 8: one behind.
        assert_eq!(v.iter().find(|x| x.title == "Tidepool Radio").map(|x| (x.episode, x.behind, x.estimate)), Some((10, 1, true)), "MyAnimeList: an estimate");
    }

    #[test]
    fn the_week_and_cards_say_what_is_left_to_watch() {
        let mut s = page();
        let tide = s.entries.iter().find(|e| e.title == "Tidepool Radio").unwrap().id;
        let slot = s.airing.days.iter().flatten().find(|sl| s.airing.shows[sl.show].id == tide).cloned().unwrap();
        assert_eq!(slot.to_watch.as_deref(), Some("~1 to watch"), "MyAnimeList's estimate is marked");
        let card = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == tide).cloned().unwrap();
        assert_eq!(card.to_watch, Some(("~1 to watch".to_string(), 1)));
        // Plan to watch and shows off the list: nothing.
        assert!(s.airing.cards.iter().filter(|c| c.status.is_none() || c.status == Some(Status::PlanToWatch)).all(|c| c.to_watch.is_none()));
        // On hold counts too.
        let e = s.entries.iter_mut().find(|e| e.id == tide).unwrap();
        e.status = Status::OnHold;
        s.airing_rebuild();
        assert!(s.airing.cards.iter().any(|c| s.airing.shows[c.show].id == tide && c.to_watch.is_some()));
        // Caught up after +1.
        s.entries.iter_mut().find(|e| e.id == tide).unwrap().progress += 1;
        s.airing_rebuild();
        let card = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == tide).unwrap();
        assert_eq!(card.to_watch.as_ref().map(|w| w.0.as_str()), Some("Caught up"));
    }

    #[test]
    fn most_to_watch_puts_the_list_s_backlog_first() {
        let mut s = page();
        s.airing_view(View::Season);
        // Two shows behind by different amounts.
        let ids: Vec<u32> = s.entries.iter().filter(|e| e.status == Status::Watching && s.airing.shows.iter().any(|x| x.id == e.id)).map(|e| e.id).take(2).collect();
        assert_eq!(ids.len(), 2);
        for (k, id) in ids.iter().enumerate() {
            s.entries.iter_mut().find(|e| e.id == *id).unwrap().progress = k as u32;
        }
        s.airing_order(Order::ToWatch);
        let first = &s.airing.cards[0];
        let max = s.airing.cards.iter().filter_map(|c| c.to_watch.as_ref().map(|w| w.1)).max().unwrap();
        assert_eq!(first.to_watch.as_ref().map(|w| w.1), Some(max));
        // Shows not watched come after every watched one.
        let pos = s.airing.cards.iter().position(|c| c.to_watch.is_none()).unwrap();
        assert!(s.airing.cards[pos..].iter().all(|c| c.to_watch.is_none()));
        s.airing_order(Order::Popular);
        assert_ne!(s.airing.cards[0].show, usize::MAX);
    }

    #[test]
    fn the_mirrors_anilist_copy_makes_it_exact_on_myanimelist() {
        let mut s = page();
        let tide = s.entries.iter().find(|e| e.title == "Tidepool Radio").unwrap().id;
        // AniList knows episode 12 airs tomorrow: 11 out (more than the estimate's 9).
        s.mirror.next.insert(tide, kotora_core::airing::NextAiring { episode: 12, at: FAKE + 86_400 });
        s.airing_rebuild();
        let card = s.airing.cards.iter().find(|c| s.airing.shows[c.show].id == tide).unwrap();
        assert_eq!(card.to_watch, Some(("3 to watch".to_string(), 3)), "exact: no ~");
        let e = s.entries.iter().find(|e| e.id == tide).unwrap();
        assert_eq!(s.to_watch(e).map(|w| (w.out, w.exact)), Some((11, true)));
    }

    const FAKE: i64 = crate::model::FAKE_NOW;

    #[test]
    fn the_countdown_ticks_once_a_minute_only_on_screen() {
        let mut s = page();
        let mut tx = rune::Tx::default();
        assert!(!tick(&mut s, &mut tx), "same minute: nothing");
        s.fixed_now = Some(FAKE + 61);
        assert!(tick(&mut s, &mut tx), "a new minute redraws");
        s.fixed_now = Some(FAKE + 200);
        s.go(Page::Settings);
        assert!(!tick(&mut s, &mut tx), "other pages never tick");
    }

    fn browse_page(quarter: u8, year: u16) -> State {
        let mut s = page();
        fake_browse(&mut s, Season { year, quarter });
        s
    }

    fn label(s: &State) -> String {
        s.viewed_season().label()
    }

    #[test]
    fn the_picker_steps_across_the_year_and_stays_in_range() {
        let mut s = page();
        assert_eq!(label(&s), "Winter 2027");
        assert_eq!(
            s.season_range(),
            (
                Season::OLDEST,
                Season {
                    year: 2027,
                    quarter: 1
                }
            )
        );
        // Back from Winter: Fall of the year before.
        assert!(s.airing_step(-1));
        assert_eq!(label(&s), "Fall 2026");
        assert_eq!(
            s.airing.browse.as_ref().map(|b| b.season),
            Some(Season {
                year: 2026,
                quarter: 3
            })
        );
        assert!(s.airing_step(-1));
        assert_eq!(label(&s), "Summer 2026");
        // Forward again: Fall, then Winter is the current season, the plain page.
        assert!(s.airing_step(1) && s.airing_step(1));
        assert_eq!(label(&s), "Winter 2027");
        assert!(
            s.airing.browse.is_none(),
            "the current season is not a browsed one"
        );
        // One more: the next season, the end of the range.
        assert!(s.airing_step(1));
        assert_eq!(label(&s), "Spring 2027");
        assert!(!s.airing_step(1), "no season past the next one");
        assert_eq!(label(&s), "Spring 2027");
        // Far back lands on the first TV season and stops there.
        assert!(s.airing_goto(Season {
            year: 1900,
            quarter: 2
        }));
        assert_eq!(s.viewed_season(), Season::OLDEST);
        assert!(!s.airing_step(-1));
        assert!(s.airing_goto(Season {
            year: 2999,
            quarter: 0
        }));
        assert_eq!(
            label(&s),
            "Spring 2027",
            "past the range: the last season in it"
        );
    }

    #[test]
    fn every_pick_stops_the_worker_before_it() {
        let mut s = page();
        let g = |s: &State| s.airing.generation.load(Ordering::SeqCst);
        let g0 = g(&s);
        s.airing_step(-1);
        let g1 = g(&s);
        assert!(g1 > g0, "a worker of the old pick sees a new number");
        assert!(!s.airing_step(0), "same season: nothing changes");
        assert_eq!(g(&s), g1, "and nobody is stopped");
        s.airing_step(1);
        assert!(
            g(&s) > g1,
            "back to this season stops the worker of the other one"
        );
    }

    #[test]
    fn the_menu_lists_the_nearby_seasons_newest_first() {
        let cur = Season {
            year: 2027,
            quarter: 0,
        };
        let labels = |viewed| {
            menu_seasons(cur, viewed)
                .iter()
                .map(|s| s.label())
                .collect::<Vec<_>>()
        };
        let near = [
            "Spring 2027",
            "Winter 2027",
            "Fall 2026",
            "Summer 2026",
            "Spring 2026",
            "Winter 2026",
            "Fall 2025",
            "Summer 2025",
            "Spring 2025",
            "Winter 2025",
        ];
        assert_eq!(labels(cur), near);
        assert_eq!(
            labels(Season {
                year: 2026,
                quarter: 3
            }),
            near
        );
        // Further back than the list: that season ends it, so the check has a row.
        let far = labels(Season {
            year: 2019,
            quarter: 1,
        });
        assert_eq!(far.len(), 11);
        assert_eq!(far.last().map(String::as_str), Some("Spring 2019"));
        // Near the first season nothing before it is listed.
        assert!(
            menu_seasons(Season::OLDEST.shift(2), Season::OLDEST)
                .iter()
                .all(|s| *s >= Season::OLDEST)
        );
    }

    #[test]
    fn menu_actions_pick_a_season_and_close_the_menu() {
        let mut s = page();
        s.airing.menu = true;
        assert!(s.airing_goto(Season {
            year: 2026,
            quarter: 3
        }));
        assert!(!s.airing.menu, "picking closes the menu");
        s.airing.menu = true;
        assert!(
            !s.airing_goto(Season {
                year: 2026,
                quarter: 3
            }),
            "the one shown: no change"
        );
        assert!(!s.airing.menu);
        // "This season": back to the current one.
        s.airing.menu = true;
        let cur = s.current_season();
        assert!(s.airing_goto(cur));
        assert!(s.airing.browse.is_none() && !s.airing.menu);
        // Leaving the page closes the menu; the season stays for a way back from Details.
        s.airing_step(-1);
        s.airing.menu = true;
        s.go(Page::Settings);
        assert!(!s.airing.menu);
        s.go(Page::Airing);
        assert_eq!(label(&s), "Fall 2026");
        // The sidebar's Airing from another page starts at this season again.
        s.go(Page::Settings);
        s.open_airing();
        assert_eq!(label(&s), "Winter 2027");
        // The sidebar item on the page itself keeps the season.
        s.airing_step(-2);
        s.open_airing();
        assert_eq!(label(&s), "Summer 2026");
    }

    #[test]
    fn keys_step_the_season() {
        let key = |k: rune::Key, mods: rune::Mods| rune::KeyEvent { key: k, mods };
        let none = rune::Mods::default();
        let text = |t: &str| rune::Key::Text(t.into());
        assert_eq!(step_key(&key(text("["), none)), Some(-1));
        assert_eq!(step_key(&key(text("]"), none)), Some(1));
        assert_eq!(
            step_key(&key(rune::Key::Left, rune::Mods { cmd: true, ..none })),
            Some(-1)
        );
        assert_eq!(
            step_key(&key(rune::Key::Right, rune::Mods { alt: true, ..none })),
            Some(1)
        );
        assert_eq!(
            step_key(&key(
                rune::Key::Command('['),
                rune::Mods { cmd: true, ..none }
            )),
            Some(-1)
        );
        assert_eq!(
            step_key(&key(
                rune::Key::Command(']'),
                rune::Mods { cmd: true, ..none }
            )),
            Some(1)
        );
        // Plain arrows, letters and a text key held with Cmd are not ours.
        assert_eq!(step_key(&key(rune::Key::Left, none)), None);
        assert_eq!(step_key(&key(text("p"), none)), None);
        assert_eq!(
            step_key(&key(text("["), rune::Mods { cmd: true, ..none })),
            None
        );
        // Through the window's key handler: only on this page, and not under the menu.
        let mut s = page();
        let mut tx = rune::Tx::default();
        assert!(crate::ui::on_key(&mut s, &key(text("["), none), &mut tx));
        assert_eq!(label(&s), "Fall 2026");
        assert!(crate::ui::on_key(&mut s, &key(text("]"), none), &mut tx));
        assert_eq!(label(&s), "Winter 2027");
        s.airing.menu = true;
        assert!(
            !crate::ui::on_key(&mut s, &key(text("["), none), &mut tx),
            "the menu has the keyboard"
        );
        assert_eq!(label(&s), "Winter 2027");
        s.airing.menu = false;
        s.go(Page::Settings);
        assert!(!crate::ui::on_key(&mut s, &key(text("["), none), &mut tx));
        assert_eq!(label(&s), "Winter 2027");
    }

    #[test]
    fn another_season_is_a_plain_grid_with_no_airing_marks() {
        let s = browse_page(3, 2026);
        let a = &s.airing;
        assert_eq!(a.cards.len(), 12);
        assert!(
            a.days.iter().all(Vec::is_empty) && a.untimed.is_empty(),
            "no week for it"
        );
        assert!(
            a.cards
                .iter()
                .all(|c| !c.lit && !c.when.contains("ep ") && !c.when.contains(" in ")),
            "no countdowns: {:?}",
            a.cards.iter().map(|c| &c.when).collect::<Vec<_>>()
        );
        let whens: Vec<&str> = a.cards.iter().map(|c| c.when.as_str()).collect();
        assert!(
            whens.contains(&"Finished airing") && whens.contains(&"Wednesdays"),
            "{whens:?}"
        );
        // Cards index the browsed shows, and the list's status is on them.
        let first = &a.shown()[a.cards[0].show];
        assert_eq!(first.id, 9401);
        let starfall = a.cards.iter().find(|c| a.shown()[c.show].id == 1).unwrap();
        assert_eq!(starfall.status, Some(Status::Watching));
        // The filters still work.
        let mut s = s;
        s.airing_mine(Mine::On);
        assert_eq!(
            s.airing
                .cards
                .iter()
                .map(|c| s.airing.shown()[c.show].id)
                .collect::<Vec<_>>(),
            [1, 10]
        );
        // The page's own season, and `kot airing`, are untouched while looking at another one.
        assert_eq!(s.airing.shows.len(), 25);
        assert!(!s.ctl_airing().is_empty());
        // The coming season: "Starts …", no countdown.
        let n = browse_page(1, 2027);
        assert!(
            n.airing
                .cards
                .iter()
                .all(|c| c.when.starts_with("Starts ") || c.when == "Not yet aired"),
            "{:?}",
            n.airing.cards.iter().map(|c| &c.when).collect::<Vec<_>>()
        );
    }

    #[test]
    fn another_season_follows_the_title_language_and_adds_through_the_queue() {
        let mut s = browse_page(3, 2026);
        let title = |s: &State, id: u32| {
            let i = s.airing.shown().iter().position(|a| a.id == id).unwrap();
            s.airing_title(i).to_string()
        };
        assert_eq!(title(&s, 9401), "Ember Tower Chronicle");
        s.set_titles(kotora_core::config::TitleLang::English);
        assert_eq!(title(&s, 9401), "Chronicle of Ember Tower");
        assert_eq!(
            title(&s, 9402),
            "Quiet Afternoon Club",
            "no English title: the usual one"
        );
        s.airing_add(9402, Status::PlanToWatch);
        let c = s
            .airing
            .cards
            .iter()
            .find(|c| s.airing.shown()[c.show].id == 9402)
            .unwrap();
        assert_eq!(c.status, Some(Status::PlanToWatch));
        s.airing_details(9403);
        assert_eq!((s.page, s.lit_page()), (Page::Details, Page::Airing));
    }

    #[test]
    fn a_season_answer_for_another_season_is_dropped() {
        let mut s = page();
        let fall = Season {
            year: 2026,
            quarter: 3,
        };
        let ans = |season, shows: Vec<SeasonAnime>, error: Option<&str>, done| Answer {
            season,
            shows,
            fetched_at: Some(5),
            error: error.map(Into::into),
            done,
        };
        // Nothing browsed: an answer has nowhere to go.
        assert!(!s.airing_browse_answer(ans(fall, fake_other_season(fall), None, true)));
        s.airing_goto(fall);
        // Headless: no worker ran, so start the way the page does with one.
        s.airing.browse.as_mut().unwrap().loading = true;
        assert!(s.airing.is_loading() && s.airing.shown().is_empty());
        // The answer of a season the user already left.
        assert!(!s.airing_browse_answer(ans(fall.previous(), fake_other_season(fall), None, true)));
        assert!(s.airing.shown().is_empty() && s.airing.is_loading());
        // The saved copy first (the worker goes on), then the fresh list.
        assert!(s.airing_browse_answer(ans(
            fall,
            fake_other_season(fall)[..3].to_vec(),
            None,
            false
        )));
        assert_eq!(
            (
                s.airing.shown().len(),
                s.airing.is_loading(),
                s.airing.cards.len()
            ),
            (3, true, 3)
        );
        assert!(s.airing_browse_answer(ans(fall, fake_other_season(fall), None, true)));
        assert_eq!((s.airing.shown().len(), s.airing.is_loading()), (12, false));
        // An error keeps the saved list and says so; it does not touch the page's own season.
        assert!(s.airing_browse_answer(ans(
            fall,
            Vec::new(),
            Some("Can't reach MyAnimeList."),
            true
        )));
        assert_eq!(
            (s.airing.shown().len(), s.airing.shown_error()),
            (12, Some("Can't reach MyAnimeList."))
        );
        assert_eq!(s.airing.error, None);
        // Back to this season: a late answer for the other one is dropped.
        let cur = s.current_season();
        s.airing_goto(cur);
        assert!(!s.airing_browse_answer(ans(fall, fake_other_season(fall), None, true)));
        assert_eq!(s.airing.shown().len(), 25);
        // Signed out: nothing lands.
        s.airing_goto(fall);
        s.auth = crate::model::Auth::SignedOut;
        assert!(!s.airing_browse_answer(ans(fall, fake_other_season(fall), None, true)));
    }

    #[test]
    fn grid_rows_fit_the_width() {
        // The main area of a 1280 and a 960 pt window.
        assert_eq!(per_row(1055.0), 7);
        assert_eq!(per_row(735.0), 4);
        assert_eq!(per_row(100.0), 1);
    }
}

#[cfg(test)]
mod fetch_tests {
    use std::cell::{Cell, RefCell};
    use std::time::{Duration, Instant};

    use super::*;

    const NOW: i64 = crate::model::FAKE_NOW;

    /// 100 anime a page: ids are the season's index, the page and the place, so no two collide.
    fn page_of(season: Season, page: u32, last: bool) -> (Vec<SeasonAnime>, bool) {
        let base = (season.index() as u32 % 100) * 10_000 + page * 100;
        let v = (0..100).map(|i| SeasonAnime { id: base + i, title: format!("S{base} {i}"), members: Some(1_000_000 - (page * 100 + i)), air_status: "currently_airing".into(), ..Default::default() }).collect();
        (v, !last)
    }

    /// A season of `PAGES` pages, each `latency` away, and a count of the requests made.
    struct Slow {
        latency: Duration,
        calls: Cell<u32>,
        asked: RefCell<Vec<(Season, u32)>>,
    }

    impl Slow {
        fn new(latency_ms: u64) -> Slow {
            Slow { latency: Duration::from_millis(latency_ms), calls: Cell::new(0), asked: RefCell::new(Vec::new()) }
        }

        fn get(&self, se: Season, page: u32) -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
            self.calls.set(self.calls.get() + 1);
            self.asked.borrow_mut().push((se, page));
            std::thread::sleep(self.latency);
            Ok(page_of(se, page, page + 1 == PAGES))
        }
    }

    /// An answer, with how many requests had been made and when it came.
    struct Seen {
        calls: u32,
        at: Duration,
        shows: usize,
        done: bool,
        fetched: Option<i64>,
        error: Option<String>,
    }

    fn run_current_with(src: &Slow, store: &Store, force: bool) -> Vec<Seen> {
        let t0 = Instant::now();
        let seen = RefCell::new(Vec::new());
        let run = Run { store: Some(store), now: NOW, force, online: true };
        let mut get = |se, pg| src.get(se, pg);
        run_current(&run, &[], Some(&mut get), &|| true, &mut |a: Answer| {
            seen.borrow_mut().push(Seen { calls: src.calls.get(), at: t0.elapsed(), shows: a.shows.len(), done: a.done, fetched: a.fetched_at, error: a.error });
        });
        seen.into_inner()
    }

    fn cur() -> Season {
        Season::of(NOW)
    }

    #[test]
    fn without_a_cache_the_rows_show_after_the_first_page_and_grow() {
        let src = Slow::new(40);
        let store = Store::open_memory().unwrap();
        let seen = run_current_with(&src, &store, false);
        // The first rows: after page 1 of the current season, nothing else asked yet.
        let first = seen.first().expect("an answer");
        assert_eq!((first.calls, first.shows, first.done), (1, 100, false));
        assert!(first.at < src.latency * 2, "page 1 is shown before page 2 is in ({:?})", first.at);
        // Then 200, 300 and the whole 400, each as its page comes in.
        let grown: Vec<(u32, usize)> = seen.iter().map(|x| (x.calls, x.shows)).collect();
        assert_eq!(grown[..4], [(1, 100), (2, 200), (3, 300), (4, 400)]);
        // The previous season comes after the current one is whole: only its airing shows merge in.
        let asked = src.asked.borrow();
        assert_eq!(asked[..4].iter().map(|x| x.0).collect::<Vec<_>>(), [cur(); 4]);
        assert_eq!(asked[4..].iter().map(|x| x.0).collect::<Vec<_>>(), [cur().previous(); 4]);
        assert!(seen.iter().rev().skip(1).all(|x| !x.done), "only the last answer is final");
        let last = seen.last().unwrap();
        assert!(last.done && last.error.is_none() && last.shows == 800);
        // Both seasons are saved whole.
        assert_eq!(store.season("mal", cur()).unwrap().map(|(v, at)| (v.len(), at)), Some((400, NOW)));
        assert_eq!(store.season("mal", cur().previous()).unwrap().map(|(v, _)| v.len()), Some(400));
    }

    #[test]
    fn a_saved_season_shows_at_once_even_stale_and_refreshes_in_the_background() {
        let store = Store::open_memory().unwrap();
        let old = NOW - 3 * 86_400;
        let (stale, _) = page_of(cur(), 0, true);
        store.save_season("mal", cur(), &stale, old, &[]).unwrap();
        // 2 days old: stale for the current season (12 h), nothing for the previous one.
        let src = Slow::new(40);
        let seen = run_current_with(&src, &store, false);
        let first = &seen[0];
        assert_eq!((first.calls, first.shows, first.fetched), (0, 100, Some(old)), "the saved rows, before any request");
        assert!(!first.done, "the refresh goes on");
        assert!(first.at < Duration::from_millis(30));
        // The old rows stay while the new list comes: no 100-row flicker down from the saved ones, no partials.
        assert!(seen[1..].iter().all(|x| x.shows >= 100));
        assert_eq!(seen[1].calls, 4, "the next answer is the whole current season");
        let last = seen.last().unwrap();
        assert_eq!((last.done, last.fetched), (true, Some(NOW)));
        assert_eq!(store.season("mal", cur()).unwrap().map(|(v, at)| (v.len(), at)), Some((400, NOW)));
    }

    #[test]
    fn fresh_saved_seasons_make_no_request() {
        let store = Store::open_memory().unwrap();
        let (a, _) = page_of(cur(), 0, true);
        store.save_season("mal", cur(), &a, NOW - 60, &[]).unwrap();
        store.save_season("mal", cur().previous(), &a, NOW - 60, &[cur()]).unwrap();
        let src = Slow::new(10);
        let seen = run_current_with(&src, &store, false);
        assert_eq!((seen.len(), src.calls.get(), seen[0].done), (1, 0, true));
        // The Refresh button asks again.
        let seen = run_current_with(&src, &store, true);
        assert_eq!(src.calls.get(), 8);
        assert!(seen.last().unwrap().done);
    }

    #[test]
    fn the_previous_season_waits_when_another_one_was_picked() {
        let store = Store::open_memory().unwrap();
        let src = Slow::new(1);
        let picked = Cell::new(false);
        let run = Run { store: Some(&store), now: NOW, force: false, online: true };
        let mut get = |se, pg| src.get(se, pg);
        let mut last = None;
        run_current(&run, &[], Some(&mut get), &|| !picked.get(), &mut |a: Answer| {
            picked.set(a.shows.len() == 400);
            last = Some(a);
        });
        assert_eq!(src.calls.get(), 4, "only the current season was asked for");
        let last = last.unwrap();
        assert!(last.done && last.error.is_none() && last.shows.len() == 400);
        assert!(store.season("mal", cur().previous()).unwrap().is_none());
    }

    #[test]
    fn the_page_draws_rows_while_the_worker_still_asks() {
        let mut s = State::fake();
        s.go(Page::Airing);
        let season = cur();
        let rows = |n: u32, done| Answer { season, shows: page_of(season, 0, true).0.into_iter().take(n as usize).collect(), fetched_at: None, error: None, done };
        // Nothing yet: the skeleton. Then the first page: rows, a small "Updating", the worker goes on.
        assert!(s.airing_answer(rows(100, false)));
        assert_eq!((s.airing.cards.len(), s.airing.is_loading()), (100, true));
        assert!(s.airing_answer(rows(100, true)));
        assert!(!s.airing.is_loading());
    }

    #[test]
    fn a_failed_page_keeps_what_is_shown() {
        let store = Store::open_memory().unwrap();
        let mut n = 0;
        let mut get = |se, pg| {
            n += 1;
            if n == 3 { Err(ServiceError::Offline("down".into())) } else { Ok(page_of(se, pg, false)) }
        };
        let seen = RefCell::new(Vec::new());
        let run = Run { store: Some(&store), now: NOW, force: false, online: true };
        run_current(&run, &[], Some(&mut get), &|| true, &mut |a: Answer| seen.borrow_mut().push((a.shows.len(), a.error, a.done)));
        let seen = seen.into_inner();
        assert_eq!(seen[..2].iter().map(|x| x.0).collect::<Vec<_>>(), [100, 200]);
        assert_eq!(seen[2], (0, Some("Can't reach MyAnimeList.".into()), true));
        assert!(store.season("mal", cur()).unwrap().is_none(), "a partial list is never saved");
    }

    #[test]
    fn a_browsed_season_streams_too() {
        let store = Store::open_memory().unwrap();
        let src = Slow::new(20);
        let season = cur().shift(-3);
        let seen = RefCell::new(Vec::new());
        let run = Run { store: Some(&store), now: NOW, force: false, online: true };
        let mut get = |se, pg| src.get(se, pg);
        run_browse(&run, season, Some(&mut get), &|| true, &mut |a: Answer| seen.borrow_mut().push((src.calls.get(), a.shows.len(), a.done)));
        assert_eq!(seen.into_inner(), [(1, 100, false), (2, 200, false), (3, 300, false), (4, 400, true)]);
        // Saved: the next visit shows it before any request, and asks nothing (14 days).
        let seen = RefCell::new(Vec::new());
        let before = src.calls.get();
        run_browse(&run, season, Some(&mut get), &|| true, &mut |a: Answer| seen.borrow_mut().push((a.shows.len(), a.done)));
        assert_eq!((seen.into_inner(), src.calls.get() - before), (vec![(400, true)], 0));
        // Another pick stops the worker between two requests.
        let other = cur().shift(-4);
        let n = Cell::new(0);
        run_browse(&run, other, Some(&mut get), &|| { n.set(n.get() + 1); n.get() < 3 }, &mut |_| {});
        assert!(store.season("mal", other).unwrap().is_none());
    }

    /// Time to the first rows with real pacing (1 s between requests, as the app's budget) and
    /// 250 ms a request. Slow (about 20 s): `cargo test -p kotora --release first_rows -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn first_rows_before_and_after() {
        use kotora_core::ratelimit::Limiter;
        let mut limiter = Limiter::mal();
        let latency = Duration::from_millis(250);
        let mut paced = |_se: Season, pg: u32| -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
            let wait = limiter.acquire(Instant::now());
            std::thread::sleep(wait + latency);
            Ok(page_of(Season::of(NOW), pg, pg + 1 == PAGES))
        };
        // Before: both seasons whole, one answer at the end.
        let t0 = Instant::now();
        for se in [cur(), cur().previous()] {
            for page in 0..PAGES {
                let _ = paced(se, page);
            }
        }
        let before = t0.elapsed();
        // After: the same source, with the answers timed.
        let mut limiter2 = Limiter::mal();
        let mut paced2 = |se: Season, pg: u32| -> Result<(Vec<SeasonAnime>, bool), ServiceError> {
            let wait = limiter2.acquire(Instant::now());
            std::thread::sleep(wait + latency);
            Ok(page_of(se, pg, pg + 1 == PAGES))
        };
        let store = Store::open_memory().unwrap();
        let t1 = Instant::now();
        let first = Cell::new(None);
        let whole = Cell::new(None);
        let run = Run { store: Some(&store), now: NOW, force: false, online: true };
        run_current(&run, &[], Some(&mut paced2), &|| true, &mut |a: Answer| {
            if first.get().is_none() && !a.shows.is_empty() {
                first.set(Some(t1.elapsed()));
            }
            if a.shows.len() >= 400 && whole.get().is_none() {
                whole.set(Some(t1.elapsed()));
            }
        });
        println!("first rows: before {before:?}, after {:?}; current season whole after {:?}; all done {:?}", first.get().unwrap(), whole.get().unwrap(), t1.elapsed());
    }
}
