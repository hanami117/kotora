//! The Airing page: what airs this week (by weekday, in the user's time zone) and this season's
//! anime (a cover grid, most members first). Data from MyAnimeList only; the times are Kotora's
//! own estimate (`kotora_core::airing::Timetable`, DESIGN.md §5.8).
//!
//! - **Data:** the current season and the one before (its shows still airing), from
//!   `GET /anime/season/{year}/{season}` in pages of 100 (at most [`PAGES_NOW`] and
//!   [`PAGES_BEFORE`]: 6 requests through the app's one rate budget), saved in the store's `season`
//!   table and asked for again after [`FRESH_SECS`]. The user's own airing entries come from the
//!   saved list (the list sync asks for their start date and slot). A worker thread does all of it
//!   when the page opens; nothing runs headless, offline or signed out.
//! - **Derived once** ([`State::airing_rebuild`]): the week's slots per day with their texts, the
//!   grid's order after the filters, and the cards' lines. The view only reads them. While the page
//!   is on screen, `poll` rebuilds once a minute (the countdowns); nothing ticks elsewhere.

use std::collections::HashMap;

use kotora_core::airing::{self, DAY_SHORT, Season, SeasonAnime, Timetable, WEEK};
use kotora_core::service::ListService;
use kotora_core::store::Store;
use kotora_core::time::{Clock, now_unix};
use kotora_core::{Status, paths};
use rune::{El, GrowthSpec, ScrollSpec, TextStyle, Tx, col, el, row, spacer, text};

use crate::model::{Auth, Page, State, kind_label};
use crate::theme::{self, PAL};
use crate::ui::{action, alert, button, cover, tag, tag_tinted, top};

/// A saved season list younger than this is not asked for again.
pub const FRESH_SECS: i64 = 12 * 3600;
/// Pages of 100 asked for the current season (by members: the 400 most followed).
pub const PAGES_NOW: u32 = 4;
/// Pages of the season before (only its shows still airing are kept).
pub const PAGES_BEFORE: u32 = 2;

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
}

#[derive(Default)]
pub struct Airing {
    pub view: View,
    pub kind: Kind,
    pub mine: Mine,
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
    fn clock(&self, t: i64) -> Clock {
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
        self.go(Page::Airing);
        self.airing_rebuild();
        self.airing_fetch(false);
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

    /// Asks the worker for the seasons: the saved copies first, then MyAnimeList when they are
    /// older than [`FRESH_SECS`] (or `force`, the Refresh button). Never headless, offline or
    /// signed out (the saved copies still show).
    pub fn airing_fetch(&mut self, force: bool) {
        let Some(h) = self.handle.clone() else { return };
        if self.airing.loading || !self.has_list() {
            return;
        }
        self.airing.loading = true;
        self.airing.error = None;
        let online = self.auth == Auth::SignedIn && !paths::offline_forced();
        std::thread::spawn(move || {
            let send = |a: Answer| h.update_if(move |s, _| s.airing_answer(a));
            let now = now_unix();
            let cur = Season::of(now);
            let prev = cur.previous();
            let store = Store::open(&paths::db_file()).ok();
            let saved = |se: Season| store.as_ref().and_then(|st| st.season("mal", se).ok().flatten());
            let list = store.as_ref().and_then(|st| st.load_list("mal").ok()).unwrap_or_default();
            let (c, p) = (saved(cur), saved(prev));
            let fresh = |x: &Option<(Vec<SeasonAnime>, i64)>| x.as_ref().is_some_and(|(_, at)| (0..FRESH_SECS).contains(&(now - at)));
            let fetched_at = c.as_ref().map(|(_, at)| *at);
            let need = force || !fresh(&c) || !fresh(&p);
            if c.is_some() || p.is_some() {
                let shows = airing::merge(c.as_ref().map_or(&[][..], |x| &x.0), p.as_ref().map_or(&[][..], |x| &x.0), &list);
                send(Answer { season: cur, shows, fetched_at, error: None, done: !need || !online });
            }
            if !need {
                return;
            }
            if !online {
                if c.is_none() && p.is_none() {
                    send(Answer { season: cur, shows: Vec::new(), fetched_at: None, error: Some("Offline: the season lists come from MyAnimeList.".into()), done: true });
                }
                return;
            }
            let Some(mut mal) = crate::online::mal_service() else {
                return send(Answer { season: cur, shows: Vec::new(), fetched_at, error: Some("Sign in to MyAnimeList to see the seasons.".into()), done: true });
            };
            let mut get = |se: Season, pages: u32, airing_only: bool| -> Result<Vec<SeasonAnime>, String> {
                let mut all = Vec::new();
                for page in 0..pages {
                    let (mut v, more) = mal.season(se, page).map_err(crate::online::error_text)?;
                    if airing_only {
                        v.retain(SeasonAnime::is_airing);
                    }
                    all.append(&mut v);
                    if !more {
                        break;
                    }
                }
                Ok(all)
            };
            let got = get(cur, PAGES_NOW, false).and_then(|c| Ok((c, get(prev, PAGES_BEFORE, true)?)));
            match got {
                Ok((c, p)) => {
                    if let Some(st) = &store {
                        let _ = st.save_season("mal", cur, &c, now, &[prev]);
                        let _ = st.save_season("mal", prev, &p, now, &[cur]);
                    }
                    let shows = airing::merge(&c, &p, &list);
                    send(Answer { season: cur, shows, fetched_at: Some(now), error: None, done: true });
                }
                Err(e) => send(Answer { season: cur, shows: Vec::new(), fetched_at, error: Some(e), done: true }),
            }
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

    /// Rebuilds everything the page draws: the week, the grid and their texts. Call it after the
    /// data, a filter, the list or the minute changes; the view only reads the result.
    pub fn airing_rebuild(&mut self) {
        let now = self.clock();
        // The list as the page sees it: status and progress (pending edits included).
        let mine: HashMap<u32, (Status, u32)> = self.entries.iter().map(|e| (e.id, (e.status, e.progress))).collect();
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
        for (i, show) in a.shows.iter().enumerate() {
            let entry = mine.get(&show.id).copied().filter(|(st, _)| *st != Status::Dropped);
            if !a.kind.keeps(&show.format) || !a.mine.keeps(entry.is_some()) {
                continue;
            }
            // Self-correction: a progress past the estimate moves the episodes forward.
            let tt = Timetable::of(show).map(|t| match entry {
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
                            day.push(Slot { show: i, episode: ep, at, time: c.hhmm(), mine: entry.is_some(), note });
                        }
                    }
                }
                None if show.is_airing() => {
                    let why = match show.broadcast {
                        Some(b) => format!("{}s in Japan, time not known", DAY_LONG[usize::from(b.day % 7)]),
                        None => "No broadcast slot on MyAnimeList".to_string(),
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
            a.cards.push(Card { show: i, meta: meta.join(" \u{b7} "), when, status: entry.map(|(st, _)| st), lit });
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
        let a = &self.airing.shows[i];
        self.prefs.titles.pick(&a.title, a.title_en.as_deref(), a.title_ja.as_deref())
    }

    /// "+ Watching" / "+ Plan to watch" on a card or a line: through the queue, as search does.
    pub fn airing_add(&mut self, id: u32, status: Status) {
        let Some(show) = self.airing.shows.iter().find(|a| a.id == id) else { return };
        let media = show.media();
        match self.add_media(media, status) {
            Ok(_) => self.airing_rebuild(),
            Err(e) => self.airing.error = Some(e),
        }
    }

    /// Opens the details page of show `id`.
    pub fn airing_details(&mut self, id: u32) {
        let Some(i) = self.airing.shows.iter().position(|a| a.id == id) else { return };
        let a = &self.airing.shows[i];
        let stub = crate::details::Stub { id, title: self.airing_title(i).to_string(), cover_url: a.cover_url.clone(), format: a.format.clone(), episodes: a.episodes };
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
                    behind: t.aired(now).saturating_sub(e.progress),
                });
            }
        }
        out.sort_by_key(|x| x.at);
        out
    }
}

/// Once a minute while the page is on screen: the countdowns. Returns whether anything changed.
pub fn tick(s: &mut State, tx: &mut Tx) -> bool {
    if s.page != Page::Airing || !s.on_screen() || s.airing.shows.is_empty() {
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
    let season = a.season.map_or_else(String::new, Season::label);
    let mut head = top(s, "Airing", None, false);
    if !season.is_empty() {
        head = head.child(text(season).key("airing/season").font(theme::LABEL).color(PAL.faint).pb(3.0));
    }
    head = head.child(spacer()).child(views(a.view));
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
                    s.airing_fetch(true);
                })
                .child(rune::icon(&crate::icons::SYNC, 16.0).color(if a.loading { PAL.faint } else { PAL.dim })),
        );
    }
    let mut page = col().key("page/airing").w_full().h_full().child(head);
    if let Some(e) = &a.error {
        let retry = button("airing/retry", "Retry", false, Some(|s: &mut State| s.airing_fetch(true)));
        let msg = if a.shows.is_empty() { e.clone() } else { format!("{e} Showing the saved season.") };
        page = page.child(alert("airing/error", PAL.status(Status::Dropped), msg, Some(retry)));
    }
    if a.shows.is_empty() {
        return page.child(if a.loading || a.error.is_none() && a.season.is_none() { loading() } else { empty(a.error.is_some()) });
    }
    page = page.child(filters(s));
    match a.view {
        View::Week => page.child(week(s)),
        View::Season => page.child(grid(s, width)),
    }
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
    r.child(spacer()).child(text("Times are estimated".to_string()).key("airing/estimated").font(theme::SMALL).color(PAL.faint))
}

fn loading() -> El {
    let mut c = col().key("airing/loading").px(theme::PAGE_PAD).pt(4.0).gap(8.0).child(text("Loading the season\u{2026}").font(theme::LABEL).color(PAL.faint).pb(2.0));
    for i in 0..4 {
        c = c.child(el().key(format!("airing/skel/{i}")).w_full().h(44.0).radius(theme::R_MD).fill(PAL.raised));
    }
    c
}

fn empty(failed: bool) -> El {
    let (title, help) = if failed {
        ("No season to show", "Kotora shows the season once MyAnimeList answers.")
    } else {
        ("Nothing airing", "MyAnimeList lists no anime for this season yet.")
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
                Some(WeekRow::UntimedHead) => head_line("No broadcast time".into(), false, Some("airing on MyAnimeList, no time to estimate".into())),
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
            move |s: &State, r| s.airing.cards.get(r * per).map_or_else(|| format!("gone-{r}"), |c| format!("cards/{}", s.airing.shows[c.show].id)).into(),
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
    let a = &s.airing.shows[c.show];
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
    let words = col()
        .h(CARD_TEXT_H)
        .gap(3.0)
        .child(text(title.to_string()).font(TextStyle::new(12.0, 500.0)).color(PAL.text).group_color(PAL.accent_hi).max_lines(2).wrap())
        .child(text(c.meta.clone()).font(theme::SMALL).color(PAL.faint).truncate())
        .child(when);
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
        assert_eq!(v.iter().find(|x| x.title == "Tidepool Radio").map(|x| (x.episode, x.behind)), Some((10, 1)));
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

    #[test]
    fn grid_rows_fit_the_width() {
        // The main area of a 1280 and a 960 pt window.
        assert_eq!(per_row(1055.0), 7);
        assert_eq!(per_row(735.0), 4);
        assert_eq!(per_row(100.0), 1);
    }
}
