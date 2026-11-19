//! The Statistics page (sidebar → Kotora → Statistics), for the switch's list (anime or manga):
//! totals, this week / month / year, a calendar of the last year, the streak, when the user
//! watches, what they watched most lately, the statuses and scores, top genres and studios
//! (authors), and the airing shows they follow.
//!
//! Everything comes from what is saved on this computer (`kotora_core::stats`): no request, so
//! it is the same for MyAnimeList and AniList and works offline. The Profile page keeps the
//! service's own numbers (MyAnimeList's days and mean); this page is Kotora's.
//!
//! The numbers are read on a short worker thread when the page opens, when the switch changes
//! and when the list changes while the page is open; the view only draws them. No timer.

use std::sync::Arc;

use kotora_core::stats::{self, Period, Stats, Title};
use kotora_core::time::Clock;
use kotora_core::{Kind, Status};
use rune::{El, ScrollSpec, TextStyle, col, el, row, spacer, text};

use crate::model::{Page, State};
use crate::theme::{self, PAL};
use crate::ui::card;

/// The page's numbers and their loading state.
#[derive(Default)]
pub struct StatsView {
    /// The last numbers read, for the list of their `kind`.
    pub data: Option<Arc<Stats>>,
    /// A read runs; `again`: the list changed meanwhile, read once more when it ends.
    pub loading: bool,
    pub again: bool,
    pub error: Option<String>,
}

/// A calendar cell (it grows with the window between these) and the gap after it.
const CELL_MIN: f32 = 10.0;
const CELL_MAX: f32 = 15.0;
const CELL_GAP: f32 = 3.0;
/// The legend's cells.
const CELL: f32 = 11.0;
/// The weekday labels' column beside the calendar.
const DAY_COL: f32 = 26.0;
const MAX_W: f32 = 1120.0;

impl State {
    pub fn open_stats(&mut self) {
        self.go(Page::Stats);
        self.stats_load();
    }

    /// The list the page shows.
    pub fn stats_kind(&self) -> Kind {
        if self.manga.mode { Kind::Manga } else { Kind::Anime }
    }

    /// The numbers for the switch's list, when they are the ones read.
    pub fn stats_now(&self) -> Option<&Stats> {
        self.stats.data.as_deref().filter(|d| d.kind == self.stats_kind())
    }

    /// Reads the numbers again (never headless: the scenes set them).
    pub fn stats_load(&mut self) {
        if self.stats.loading {
            self.stats.again = true;
            return;
        }
        let Some(h) = self.handle.clone() else { return };
        let Some(service) = self.account.as_ref().map(|(_, svc)| *svc) else { return };
        let kind = self.stats_kind();
        self.stats.loading = true;
        let spawned = std::thread::Builder::new().name("kotora-stats".into()).spawn(move || {
            let r = read(service, kind);
            h.update_if(move |s, _| s.stats_loaded(r));
        });
        if spawned.is_err() {
            self.stats.loading = false;
        }
    }

    /// A read ended.
    pub fn stats_loaded(&mut self, r: Result<Stats, String>) -> bool {
        self.stats.loading = false;
        match r {
            Ok(st) => {
                self.stats.data = Some(Arc::new(st));
                self.stats.error = None;
            }
            Err(e) => self.stats.error = Some(e),
        }
        if std::mem::take(&mut self.stats.again) || self.stats_now().is_none() && self.stats.error.is_none() {
            self.stats_load();
        }
        self.page == Page::Stats
    }

    /// The list changed (a sync, an edit, the switch): the open page reads its numbers again.
    pub fn stats_list_changed(&mut self) {
        if self.page == Page::Stats {
            self.stats_load();
        }
    }

    /// The title in the chosen language, from the list (else the service's main title).
    fn stats_title(&self, t: &Title, kind: Kind) -> String {
        let found = match kind {
            Kind::Anime => self.entries.iter().find(|e| e.id == t.media_id).map(|e| e.title.clone()),
            Kind::Manga => self.manga.entries.iter().find(|e| e.id == t.media_id).map(|e| e.title.clone()),
        };
        found.unwrap_or_else(|| t.title.clone())
    }
}

/// The worker's read: its own store connection, the user's time zone.
fn read(service: &str, kind: Kind) -> Result<Stats, String> {
    let store = kotora_core::store::Store::open(&kotora_core::paths::db_file()).map_err(|e| format!("Can't open the saved list: {e}"))?;
    let now = kotora_core::time::now_unix();
    store.stats((service, kind), now, &kotora_core::time::local_offset).map_err(|e| format!("Can't read the statistics: {e}"))
}

/// `kot stats [--manga]`: the same numbers as text, read on the request's own thread.
pub fn ctl_text(service: &str, kind: Kind) -> Result<String, String> {
    let st = read(service, kind)?;
    Ok(stats::text(&st, &kotora_core::time::local_offset))
}

// ---------------------------------------------------------------------------------------- view

/// Words for one kind of list.
struct Words {
    unit: &'static str,
    units: &'static str,
    verb: &'static str,
}

fn words(kind: Kind) -> Words {
    match kind {
        Kind::Anime => Words { unit: "episode", units: "episodes", verb: "watched" },
        Kind::Manga => Words { unit: "chapter", units: "chapters", verb: "read" },
    }
}

fn count(n: u64, w: &Words) -> String {
    if n == 1 { format!("1 {}", w.unit) } else { format!("{} {}", stats::grouped(n), w.units) }
}

pub fn page(s: &State, width: f32) -> El {
    let kind = s.stats_kind();
    let title = format!("{} statistics", kind.label());
    let inner = (width.min(MAX_W) - 2.0 * theme::PAGE_PAD).max(320.0);
    let wide = inner >= 820.0;
    let body = match s.stats_now() {
        Some(st) => content(s, st, inner, wide),
        None => {
            let line = s.stats.error.clone().unwrap_or_else(|| "Counting\u{2026}".to_string());
            col().key("stats/loading").py(40.0).w_full().items_center().child(text(line).font(theme::BODY).color(PAL.dim).wrap())
        }
    };
    col()
        .key("page/stats")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, &title, None, false))
        .child(
            col()
                .key("stats-scroll")
                .scroll(ScrollSpec { stick_bottom: false })
                .grow(1.0)
                .min_h(0.0)
                .child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).gap(16.0).w(inner + 2.0 * theme::PAGE_PAD).child(body)),
        )
}

fn content(s: &State, st: &Stats, inner: f32, wide: bool) -> El {
    let w = words(st.kind);
    let mut c = col().gap(16.0).w_full();
    c = c.child(figures(st, &w, inner));
    c = c.child(calendar(st, &w, inner));
    let half = (inner - 16.0) / 2.0;
    let pair = |a: El, b: El| if wide { row().gap(16.0).w_full().items_start().child(a.w(half)).child(b.w(half)) } else { col().gap(16.0).w_full().child(a).child(b) };
    c = c.child(pair(when(st, &w), lately(s, st, &w)));
    c = c.child(pair(list_card(st), tags(st)));
    if st.kind == Kind::Anime && !st.list.airing.is_empty() {
        c = c.child(airing(s, st, inner));
    }
    c
}

/// A big number with its caption and a small line under it.
fn tile(key: &str, big: String, caption: &str, small: String, w: f32) -> El {
    col()
        .key(format!("stats/tile/{key}"))
        .w(w)
        .gap(3.0)
        .padding(14.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(text(caption.to_uppercase()).font(theme::CAPTION).color(PAL.faint).truncate())
        .child(text(big).font(TextStyle::new(22.0, 600.0).features("'tnum' 1")).color(PAL.text).truncate())
        .child(text(small).font(theme::LABEL).color(PAL.dim).truncate())
}

fn period_line(p: &Period, st: &Stats, w: &Words) -> String {
    match st.kind {
        Kind::Anime if p.minutes > 0 => stats::duration(p.minutes),
        Kind::Manga if p.volumes > 0 => format!("{} vol.", p.volumes),
        _ if p.titles == 1 => "1 title".to_string(),
        _ if p.titles > 0 => format!("{} titles", p.titles),
        _ => format!("Nothing {} yet", w.verb),
    }
}

/// The row of totals: the list's, then this week, month, year and the streak from the log.
fn figures(st: &Stats, w: &Words, inner: f32) -> El {
    let l = &st.list;
    let mut tiles: Vec<(&str, String, &str, String)> = Vec::new();
    let units_caption = if st.kind == Kind::Anime { "Episodes watched" } else { "Chapters read" };
    tiles.push(("units", stats::grouped(l.units), units_caption, format!("{} on your list", stats::grouped(u64::from(l.titles)))));
    match st.kind {
        Kind::Anime => {
            let about = if l.guessed > 0 { "Some lengths guessed" } else { "From episode lengths" };
            tiles.push(("time", stats::duration(l.minutes), "Time watched", about.to_string()));
        }
        Kind::Manga => tiles.push(("volumes", stats::grouped(l.volumes), "Volumes read", "From your list".to_string())),
    }
    for (key, caption, p) in [("week", "This week", &st.log.week), ("month", "This month", &st.log.month), ("year", "This year", &st.log.year)] {
        tiles.push((key, stats::grouped(u64::from(p.units)), caption, period_line(p, st, w)));
    }
    let best = if st.log.best_streak > 0 { format!("Best {}", stats::days(st.log.best_streak)) } else { "Days in a row".to_string() };
    tiles.push(("streak", stats::days(st.log.streak), "Streak", best));
    let per_row = if inner >= 960.0 { 6 } else if inner >= 560.0 { 3 } else { 2 };
    let tw = ((inner - 12.0 * (per_row as f32 - 1.0)) / per_row as f32).floor();
    let mut rows = col().key("stats/figures").gap(12.0).w_full();
    for chunk in tiles.chunks(per_row) {
        let mut r = row().gap(12.0);
        for (key, big, caption, small) in chunk {
            r = r.child(tile(key, big.clone(), caption, small.clone(), tw));
        }
        rows = rows.child(r);
    }
    rows
}

/// "Since Nov 19, 2026" of the log's start, in the user's time zone.
fn since_text(st: &Stats) -> Option<String> {
    st.since.map(|t| stats::date_text(Clock::local(t).day))
}

/// The day's colour on the calendar: none, then four steps of orange.
fn shade(n: u32, max: u32) -> rune::Color {
    if n == 0 {
        return PAL.border;
    }
    let step = (4 * n).div_ceil(max.max(1)).clamp(1, 4);
    PAL.accent.alpha([0.3, 0.5, 0.75, 1.0][step as usize - 1])
}

/// The last year as weeks of days (Monday on top), as many weeks as fit.
fn calendar(st: &Stats, w: &Words, inner: f32) -> El {
    let l = &st.log;
    // Cells grow to fill the card (up to 15), and shrink to 10 before weeks drop off.
    let room = inner - 32.0 - DAY_COL;
    let fit = ((room + CELL_GAP) / 53.0 - CELL_GAP).floor();
    let cell = fit.clamp(CELL_MIN, CELL_MAX);
    let weeks = (((room + CELL_GAP) / (cell + CELL_GAP)).floor() as i64).clamp(8, 53);
    // The column of today's week is the last; its Monday:
    let monday = st.today - i64::from(kotora_core::time::weekday(st.today));
    let first = monday - (weeks - 1) * 7;
    let since_day = st.since.map(|t| Clock::local(t).day);
    let max = l.days.iter().copied().max().unwrap_or(0);
    let value = |day: i64| -> u32 {
        let i = day - l.first_day;
        if (0..l.days.len() as i64).contains(&i) { l.days[i as usize] } else { 0 }
    };
    let mut grid = row().key("stats/calendar").gap(CELL_GAP);
    // The weekday labels: Mon, Wed, Fri.
    let mut labels = col().w(DAY_COL - CELL_GAP).gap(CELL_GAP);
    for (i, d) in ["Mon", "", "Wed", "", "Fri", "", ""].iter().enumerate() {
        labels = labels.child(el().key(format!("stats/cal-day/{i}")).h(cell).child(text((*d).to_string()).font(TextStyle::new(9.5, 400.0)).color(PAL.faint)));
    }
    grid = grid.child(labels);
    let mut months = row().key("stats/cal-months").h(14.0).w_full().ml(DAY_COL);
    let mut last_month = 0;
    let mut in_view = 0u64;
    for wk in 0..weeks {
        let start = first + wk * 7;
        let (_, m, _) = kotora_core::time::civil_from_days(start + 6);
        if m != last_month {
            // A month's name over the first week that ends in it (not over the first column).
            if wk > 0 || weeks < 10 {
                months = months.child(el().absolute(wk as f32 * (cell + CELL_GAP), 0.0).child(text(month_name(m)).font(TextStyle::new(10.0, 400.0)).color(PAL.faint)));
            }
            last_month = m;
        }
        let mut column = col().gap(CELL_GAP);
        for d in 0..7 {
            let day = start + d;
            let c = el().size(cell, cell).radius(2.5);
            let c = if day > st.today {
                c
            } else if since_day.is_some_and(|s| day < s) && value(day) == 0 {
                // Before Kotora counted: not known, not "nothing".
                c.border(1.0, PAL.border).label(format!("{}: before Kotora counted", stats::date_text(day)))
            } else {
                let n = value(day);
                in_view += u64::from(n);
                c.fill(shade(n, max)).label(format!("{}: {}", stats::date_text(day), count(u64::from(n), w)))
            };
            column = column.child(c.key(format!("stats/day/{day}")));
        }
        grid = grid.child(column);
    }
    let legend = {
        let mut r = row().gap(3.0).center_y().child(text("Less").font(theme::SMALL).color(PAL.faint).mr(4.0));
        for (i, n) in [0, 1, 2, 3, 4].into_iter().enumerate() {
            r = r.child(el().key(format!("stats/legend/{i}")).size(CELL, CELL).radius(2.5).fill(shade(n, 4)));
        }
        r.child(text("More").font(theme::SMALL).color(PAL.faint).ml(4.0))
    };
    let note = match since_text(st) {
        Some(d) => format!("Counted by Kotora since {d}: what you record here, not what came before or from the website."),
        None => "Counted by Kotora: what you record here, not what came before or from the website.".to_string(),
    };
    let head = row()
        .gap(12.0)
        .center_y()
        .child(text("Activity").font(theme::SECTION).color(PAL.text))
        .child(spacer())
        .child(text(format!("{} in {} weeks", count(in_view, w), weeks)).key("stats/cal-total").font(theme::LABEL_NUM).color(PAL.dim));
    col()
        .key("stats/activity")
        .padding(16.0)
        .gap(10.0)
        .w_full()
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(head)
        .child(col().gap(4.0).child(months).child(grid))
        .child(row().gap(12.0).items_start().child(text(note).key("stats/since").font(theme::LABEL).color(PAL.faint).wrap().grow(1.0).min_w(0.0)).child(legend))
}

fn month_name(m: u32) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m as usize).saturating_sub(1) % 12]
}

/// Small vertical bars with a label under each.
fn bars(key: &str, values: &[u32], labels: &[&str], height: f32, gap: f32) -> El {
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let mut r = row().key(key.to_string()).w_full().gap(gap).items_end();
    for (i, v) in values.iter().enumerate() {
        let h = if *v == 0 { 2.0 } else { (height * *v as f32 / max as f32).max(3.0) };
        r = r.child(
            col()
                .key(format!("{key}/{i}"))
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .items_center()
                .child(el().h(height - h))
                .child(el().w_full().h(h).radius(2.0).fill(if *v == 0 { PAL.border } else { PAL.accent }))
                .child(text(labels.get(i).copied().unwrap_or("").to_string()).font(TextStyle::new(10.0, 400.0).features("'tnum' 1")).color(PAL.faint)),
        );
    }
    r
}

/// When the user watches: the weekdays and the hours of the day.
fn when(st: &Stats, w: &Words) -> El {
    let c = card("stats/when", if st.kind == Kind::Anime { "When you watch" } else { "When you read" }).gap(12.0);
    if !st.has_activity() {
        return c.child(empty_line(st, w));
    }
    let l = &st.log;
    let top_day = l.weekdays.iter().enumerate().max_by_key(|(i, v)| (**v, 7 - i)).map(|(i, _)| i).unwrap_or(0);
    let top_hour = l.hours.iter().enumerate().max_by_key(|(i, v)| (**v, 24 - i)).map(|(i, _)| i).unwrap_or(0);
    const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let hours: Vec<String> = (0..24).map(|h| if h % 6 == 0 { h.to_string() } else { String::new() }).collect();
    let hour_labels: Vec<&str> = hours.iter().map(String::as_str).collect();
    c.child(text(format!("Mostly on {}s, around {:02}:00.", DAYS[top_day], top_hour)).key("stats/when-line").font(theme::BODY).color(PAL.dim).wrap())
        .child(text("BY DAY").font(theme::CAPTION).color(PAL.faint))
        .child(bars("stats/weekdays", &l.weekdays, &["M", "T", "W", "T", "F", "S", "S"], 56.0, 8.0))
        .child(text("BY HOUR").font(theme::CAPTION).color(PAL.faint).pt(4.0))
        .child(bars("stats/hours", &l.hours, &hour_labels, 56.0, 2.0))
}

/// Nothing logged yet: what will count.
fn empty_line(st: &Stats, w: &Words) -> El {
    let start = since_text(st).map_or_else(|| "Kotora counts from now on".to_string(), |d| format!("Kotora counts from {d}"));
    text(format!("{start}, and nothing is {} yet. {} you record (by hand, with kot or from your player) show up here.", w.verb, capital(w.units)))
    .key("stats/empty")
    .font(theme::BODY)
    .color(PAL.dim)
    .wrap()
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A title with a small poster and a number on the right.
fn title_row(s: &State, st: &Stats, t: &Title, right: String, key: &str) -> El {
    let title = s.stats_title(t, st.kind);
    row()
        .key(format!("{key}/{}", t.media_id))
        .gap(10.0)
        .center_y()
        .w_full()
        .child(crate::look::poster(s, t.media_id, &title, t.cover_url.as_deref(), 26.0, 38.0, theme::R_XS))
        .child(text(title).font(theme::BODY_MED).color(PAL.text).truncate().grow(1.0).min_w(0.0))
        .child(text(right).font(theme::LABEL_NUM).color(PAL.dim))
}

/// The titles with most episodes in the last 30 days.
fn lately(s: &State, st: &Stats, w: &Words) -> El {
    let c = card("stats/lately", if st.kind == Kind::Anime { "Most watched lately" } else { "Most read lately" }).gap(10.0);
    if st.log.lately.is_empty() {
        let line = if st.has_activity() { format!("Nothing {} in the last 30 days.", w.verb) } else { format!("Your most {} titles of the last 30 days show here.", w.verb) };
        return c.child(text(line).key("stats/lately-empty").font(theme::BODY).color(PAL.dim).wrap());
    }
    let mut c = c.child(text("LAST 30 DAYS").font(theme::CAPTION).color(PAL.faint));
    for t in &st.log.lately {
        c = c.child(title_row(s, st, t, count(u64::from(t.units), w), "stats/lately"));
    }
    c
}

/// The list's statuses as one stacked bar, and the scores 1 to 10.
fn list_card(st: &Stats) -> El {
    let l = &st.list;
    let counts: Vec<(Status, u32)> = Status::ALL.iter().copied().zip(l.statuses).collect();
    let mut bar = row().key("stats/status-bar").w_full().h(10.0).gap(2.0).radius(5.0).clip_rounded(5.0);
    if l.titles == 0 {
        bar = bar.fill(PAL.border);
    }
    for (stt, n) in counts.iter().filter(|c| c.1 > 0) {
        bar = bar.child(el().key(format!("stats/status/{}", stt.slug())).grow(*n as f32).h_full().fill(PAL.status(*stt)));
    }
    let mut legend = row().key("stats/status-legend").flex_wrap().gap(4.0);
    for (stt, n) in &counts {
        legend = legend.child(
            row()
                .w(150.0)
                .gap(8.0)
                .py(2.0)
                .center_y()
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(*stt)))
                .child(text(stt.label_in(st.kind)).font(theme::LABEL).color(PAL.dim))
                .child(spacer())
                .child(text(stats::grouped(u64::from(*n))).font(theme::LABEL_NUM).color(PAL.text))
                .mr(12.0),
        );
    }
    let scored: u32 = l.scores.iter().sum();
    let labels: Vec<String> = (1..=10).map(|i| i.to_string()).collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mean = (scored > 0).then(|| l.scores.iter().enumerate().map(|(i, c)| f64::from(*c) * (i as f64 + 1.0)).sum::<f64>() / f64::from(scored));
    let score_line = match mean {
        Some(m) => format!("{} scored \u{b7} mean {m:.2} (out of 10)", stats::grouped(u64::from(scored))),
        None => "No scores on your list yet.".to_string(),
    };
    card("stats/list", "Your list")
        .gap(10.0)
        .child(text("BY STATUS").font(theme::CAPTION).color(PAL.faint))
        .child(bar)
        .child(legend)
        .child(text("SCORES").font(theme::CAPTION).color(PAL.faint).pt(6.0))
        .child(bars("stats/scores", &l.scores, &label_refs, 64.0, 6.0))
        .child(text(score_line).key("stats/scored").font(theme::LABEL).color(PAL.dim))
}

/// Horizontal bars: a name, a bar, a count.
fn ranked(key: &str, items: &[(String, u32)]) -> El {
    let max = items.iter().map(|i| i.1).max().unwrap_or(1).max(1);
    let mut c = col().key(key.to_string()).gap(6.0).w_full();
    for (i, (name, n)) in items.iter().enumerate() {
        c = c.child(
            row()
                .key(format!("{key}/{i}"))
                .gap(10.0)
                .center_y()
                .w_full()
                .child(text(name.clone()).font(theme::LABEL).color(PAL.text).truncate().w(120.0))
                .child(
                    row()
                        .grow(1.0)
                        .min_w(0.0)
                        .h(8.0)
                        .radius(4.0)
                        .fill(PAL.border)
                        .child(el().grow(*n as f32).h_full().radius(4.0).fill(PAL.accent))
                        .child(el().grow((max - n) as f32)),
                )
                .child(text(n.to_string()).font(theme::LABEL_NUM).color(PAL.dim).w(28.0)),
        );
    }
    c
}

/// Top genres, and studios (anime) or authors (manga).
fn tags(st: &Stats) -> El {
    let l = &st.list;
    let makers = if st.kind == Kind::Anime { "STUDIOS" } else { "AUTHORS" };
    let mut c = card("stats/tags", "Genres and studios").gap(10.0);
    if st.kind == Kind::Manga {
        c = card("stats/tags", "Genres and authors").gap(10.0);
    }
    c = c.child(text("GENRES").font(theme::CAPTION).color(PAL.faint));
    c = if l.genres.is_empty() {
        c.child(text("Genres come from the details pages Kotora has saved: open a title's page and it counts here.").key("stats/genres-empty").font(theme::LABEL).color(PAL.dim).wrap())
    } else {
        c.child(ranked("stats/genres", &l.genres))
    };
    c = c.child(text(makers).font(theme::CAPTION).color(PAL.faint).pt(6.0));
    c = if l.makers.is_empty() {
        c.child(text(if st.kind == Kind::Anime { "None saved yet." } else { "None on your list yet." }).font(theme::LABEL).color(PAL.dim))
    } else {
        c.child(ranked("stats/makers", &l.makers))
    };
    if !l.genres.is_empty() || st.kind == Kind::Anime {
        let of = format!(
            "From the {} of your {} titles (not Plan to {}) whose details are saved: open a title's page to add it.",
            stats::grouped(u64::from(l.with_details)),
            stats::grouped(u64::from(l.watched)),
            if st.kind == Kind::Anime { "watch" } else { "read" }
        );
        c = c.child(text(of).key("stats/coverage").font(theme::SMALL).color(PAL.faint).wrap());
    }
    c
}

/// The airing shows the user is watching, with how far they are.
fn airing(s: &State, st: &Stats, inner: f32) -> El {
    let per_row = ((inner - 32.0 + 12.0) / 252.0).floor().max(1.0) as usize;
    let cw = ((inner - 32.0 - 12.0 * (per_row as f32 - 1.0)) / per_row as f32).floor();
    let mut c = card("stats/airing", "Airing that you watch").gap(12.0);
    let items = &st.list.airing;
    for chunk in items.chunks(per_row) {
        let mut r = row().gap(12.0);
        for t in chunk {
            let ep = match t.total {
                Some(n) => format!("Ep {} of {n}", t.progress),
                None => format!("Ep {}", t.progress),
            };
            r = r.child(title_row(s, st, t, ep, "stats/airing").w(cw));
        }
        c = c.child(r);
    }
    c.child(text(format!("{} airing \u{b7} Airing shows when the next ones come out.", items.len())).font(theme::SMALL).color(PAL.faint))
}

// --------------------------------------------------------------------------------------- fake

/// Made-up numbers for the scenes: the fake list's titles, a year of evenings.
pub fn fake(kind: Kind, entries: &[Title], today: i64, empty: bool) -> Stats {
    use kotora_core::stats::{DAYS, ListStats, LogStats};
    let mut days = vec![0u32; DAYS];
    let start = DAYS - 120;
    if !empty {
        // A made-up rhythm: more at weekends, a quiet spell, a streak to today.
        for (i, d) in days.iter_mut().enumerate().skip(start) {
            let day = today - (DAYS - 1 - i) as i64;
            let wd = kotora_core::time::weekday(day);
            let seed = (i as u32).wrapping_mul(2_654_435_761) >> 28;
            *d = match (wd, seed) {
                (5 | 6, s) => 1 + s % 5,
                (_, s) if s < 6 => 0,
                (_, s) => s % 3,
            };
            if (60..70).contains(&(i - start)) {
                *d = 0;
            }
        }
        for d in days.iter_mut().rev().take(6) {
            *d = (*d).max(1);
        }
    }
    let sum = |n: usize| days.iter().rev().take(n).sum::<u32>();
    let per = if kind == Kind::Anime { 24 } else { 0 };
    let p = |n: usize, titles: u32| Period { units: sum(n), volumes: if kind == Kind::Manga { sum(n) / 8 } else { 0 }, minutes: u64::from(sum(n) * per), titles };
    let titles = |n: usize| -> Vec<Title> {
        entries.iter().take(n).enumerate().map(|(i, t)| Title { units: 14u32.saturating_sub(i as u32 * 3), ..t.clone() }).collect()
    };
    let log = if empty {
        LogStats { days, first_day: today - (DAYS as i64 - 1), ..LogStats::default() }
    } else {
        LogStats {
            first_day: today - (DAYS as i64 - 1),
            week: p(4, 3),
            month: p(19, 6),
            year: p(120, 14),
            all: p(DAYS, 14),
            streak: 6,
            best_streak: 11,
            weekdays: [9, 7, 11, 8, 13, 31, 27],
            hours: [6, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 1, 2, 4, 6, 9, 14, 17, 15, 10],
            lately: titles(5),
            days,
        }
    };
    let list = ListStats {
        titles: 304,
        statuses: [11, 48, 236, 7, 2],
        scores: [0, 1, 2, 4, 9, 22, 48, 71, 40, 15],
        units: if kind == Kind::Anime { 3790 } else { 6120 },
        volumes: if kind == Kind::Manga { 512 } else { 0 },
        minutes: if kind == Kind::Anime { 90_870 } else { 0 },
        guessed: 12,
        watched: 256,
        with_details: 61,
        genres: [("Drama", 34), ("Comedy", 29), ("Action", 25), ("Slice of Life", 19), ("Romance", 16), ("Fantasy", 12), ("Mystery", 9), ("Sci-Fi", 7)].map(|(g, n)| (g.to_string(), n)).to_vec(),
        makers: if kind == Kind::Anime {
            [("Studio Lantern", 9), ("Harbour Works", 7), ("Iron Garden Pictures", 5), ("Studio Kite", 4), ("Paper Moon", 3), ("Ninefold", 2)].map(|(g, n)| (g.to_string(), n)).to_vec()
        } else {
            [("Aoi Kurata", 4), ("Ren Mizuno", 3), ("Sora Ikeda", 3), ("Mei Tachibana", 2)].map(|(g, n)| (g.to_string(), n)).to_vec()
        },
        airing: if kind == Kind::Anime { titles(4).into_iter().map(|t| Title { units: 0, ..t }).collect() } else { Vec::new() },
    };
    Stats { kind, now: today * 86_400 + 12 * 3600, today, since: Some((today - if empty { 0 } else { 118 }) * 86_400 + 12 * 3600), list, log }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_step_by_quarters() {
        assert_eq!(shade(0, 8), PAL.border);
        assert_eq!(shade(1, 8), PAL.accent.alpha(0.3));
        assert_eq!(shade(8, 8), PAL.accent.alpha(1.0));
        assert_eq!(shade(5, 8), PAL.accent.alpha(0.75));
    }

    #[test]
    fn the_page_follows_the_switch() {
        let mut s = State::fake();
        s.stats.data = Some(Arc::new(fake(Kind::Anime, &[], 20_000, false)));
        assert!(s.stats_now().is_some());
        s.manga.mode = true;
        assert!(s.stats_now().is_none(), "anime numbers aren't shown for manga");
        assert!(!s.stats_loaded(Ok(fake(Kind::Manga, &[], 20_000, true))), "not on the page: nothing to draw");
        assert_eq!(s.stats_now().map(|d| d.kind), Some(Kind::Manga));
    }

    #[test]
    fn headless_reads_nothing() {
        let mut s = State::fake();
        s.open_stats();
        assert_eq!(s.page, Page::Stats);
        assert!(!s.stats.loading, "no handle, no thread");
    }
}
