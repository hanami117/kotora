//! Statistics (0.4.5): numbers for the Statistics page and `kot stats`, from what is saved on
//! this computer only (no request): the saved list (totals, statuses, scores), the saved details
//! pages (episode length, genres, studios) and Kotora's activity log (`activity.rs`: what was
//! watched or read in Kotora, and when). Works offline, the same for MyAnimeList and AniList.
//!
//! One call reads it all ([`Store::stats`]); the app runs it on a worker thread when the page
//! opens or the list changes while it is open.

use std::collections::HashMap;

use rusqlite::params;

use crate::activity::Row;
use crate::store::{Result, Store};
use crate::time::{Clock, civil_from_days, days_from_civil};
use crate::{Kind, Scope, Status};

/// Days of activity kept for the calendar: 53 weeks.
pub const DAYS: usize = 371;
/// Genres and studios (authors) shown.
const TOP_TAGS: usize = 8;
const TOP_MAKERS: usize = 6;
/// Titles in "Most watched lately".
const TOP_TITLES: usize = 5;
/// How far back "lately" goes.
const LATELY_DAYS: i64 = 30;

/// What one stretch of time holds: episodes (chapters), volumes, minutes, different titles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Period {
    pub units: u32,
    pub volumes: u32,
    pub minutes: u64,
    pub titles: u32,
}

/// A title on the list, for the lists on the page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Title {
    pub media_id: u32,
    /// The service's main title (the app shows its own, in the chosen language).
    pub title: String,
    pub cover_url: Option<String>,
    pub progress: u32,
    pub total: Option<u32>,
    /// Episodes or chapters in the stretch the list is about.
    pub units: u32,
}

/// The saved list's numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListStats {
    pub titles: u32,
    /// Per status, in `Status::ALL`'s order.
    pub statuses: [u32; 5],
    /// How many entries have each score, 1 to 10 (index 0 is 1), on the 10-point scale.
    pub scores: [u32; 10],
    /// Episodes watched (chapters read), finished rewatches (rereads) included.
    pub units: u64,
    pub volumes: u64,
    /// Anime: time watched, from each title's episode length.
    pub minutes: u64,
    /// Titles with episodes watched whose length isn't saved: a usual length for the type was used.
    pub guessed: u32,
    /// Titles watched (not Plan to watch) and how many of them have saved details (genres).
    pub watched: u32,
    pub with_details: u32,
    /// The most frequent genres among them, most first.
    pub genres: Vec<(String, u32)>,
    /// Anime: studios; manga: authors. Most first.
    pub makers: Vec<(String, u32)>,
    /// Anime being watched that is still airing.
    pub airing: Vec<Title>,
}

/// The activity log's numbers. Days are the user's local days.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogStats {
    /// Episodes (chapters) per day, oldest first, the last one today: [`DAYS`] days.
    pub days: Vec<u32>,
    /// The day number (days since 1970-01-01) of `days[0]`.
    pub first_day: i64,
    /// Since Monday, since the 1st, since January 1, and everything logged.
    pub week: Period,
    pub month: Period,
    pub year: Period,
    pub all: Period,
    /// Days in a row with something watched, up to today (or up to yesterday while today has
    /// nothing yet), and the longest run.
    pub streak: u32,
    pub best_streak: u32,
    /// Episodes per weekday (Monday first) and per hour of the day.
    pub weekdays: [u32; 7],
    pub hours: [u32; 24],
    /// The titles with most episodes in the last 30 days.
    pub lately: Vec<Title>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub kind: Kind,
    pub now: i64,
    /// Today's day number.
    pub today: i64,
    /// When the activity log started (the page says it knows nothing from before).
    pub since: Option<i64>,
    pub list: ListStats,
    pub log: LogStats,
}

impl Stats {
    /// Whether anything at all was logged.
    pub fn has_activity(&self) -> bool {
        self.log.all.units > 0 || self.log.all.volumes > 0
    }
}

/// An episode's usual length when the details aren't saved, from the type.
pub fn usual_minutes(format: &str) -> u32 {
    match format.to_ascii_lowercase().as_str() {
        "movie" => 90,
        "music" => 4,
        "cm" | "pv" => 2,
        "special" | "tv_special" | "ova" => 24,
        "tv_short" => 5,
        _ => 24,
    }
}

/// One list entry as the numbers need it.
struct Item {
    media_id: u32,
    title: String,
    cover_url: Option<String>,
    status: Status,
    progress: u32,
    total: Option<u32>,
    volumes: u32,
    score: Option<crate::Score>,
    times: u32,
    air_status: String,
    /// The saved length, else `None`.
    minutes: Option<u32>,
    format: String,
}

impl Item {
    fn minutes(&self) -> u32 {
        self.minutes.filter(|m| *m > 0).unwrap_or_else(|| usual_minutes(&self.format))
    }
}

impl Store {
    /// Everything the Statistics page shows for one list. `offset`: the user's UTC offset at a
    /// time, in seconds (`time::local_offset`; tests pass a fixed one).
    pub fn stats<'a>(&self, scope: impl Into<Scope<'a>>, now: i64, offset: &dyn Fn(i64) -> i64) -> Result<Stats> {
        let sc = scope.into();
        let items = self.stat_items(sc)?;
        let rows = crate::activity::rows(&self.conn, sc)?;
        let today = Clock::at(now, offset(now)).day;
        let list = self.list_stats(sc, &items)?;
        let log = log_stats(&items, &rows, now, today, sc.kind, offset);
        Ok(Stats { kind: sc.kind, now, today, since: crate::activity::since(&self.conn)?, list, log })
    }

    fn stat_items(&self, sc: Scope) -> Result<Vec<Item>> {
        let mut st = self.conn.prepare(
            "SELECT e.media_id, m.title, m.cover_url, e.status, e.progress, m.episodes, e.volumes, e.score, e.times_rewatched,
                    m.air_status, m.format,
                    (SELECT json_extract(d.body, '$.minutes') FROM details d
                      WHERE d.service = e.service AND d.kind = e.kind AND d.media_id = e.media_id AND json_valid(d.body))
             FROM entry e JOIN media m ON m.service = e.service AND m.kind = e.kind AND m.id = e.media_id
             WHERE e.service = ?1 AND e.kind = ?2",
        )?;
        let rows = st.query_map(params![sc.service, sc.kind.name()], |r| {
            Ok(Item {
                media_id: r.get(0)?,
                title: r.get(1)?,
                cover_url: r.get(2)?,
                status: Status::from_mal(&r.get::<_, String>(3)?).unwrap_or(Status::PlanToWatch),
                progress: r.get(4)?,
                total: r.get(5)?,
                volumes: r.get(6)?,
                score: crate::Score::from_sql_points(r.get(7)?),
                times: r.get(8)?,
                air_status: r.get(9)?,
                format: r.get(10)?,
                minutes: r.get::<_, Option<i64>>(11)?.and_then(|m| u32::try_from(m).ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn list_stats(&self, sc: Scope, items: &[Item]) -> Result<ListStats> {
        let mut l = ListStats { titles: items.len() as u32, ..ListStats::default() };
        for it in items {
            let i = Status::ALL.iter().position(|s| *s == it.status).unwrap_or(1);
            l.statuses[i] += 1;
            if let Some(sc) = it.score {
                l.scores[usize::from(sc.to_ten().clamp(1, 10)) - 1] += 1;
            }
            let units = u64::from(it.progress) + u64::from(it.times) * u64::from(it.total.unwrap_or(0));
            l.units += units;
            l.volumes += u64::from(it.volumes);
            if sc.kind == Kind::Anime && units > 0 {
                l.minutes += units * u64::from(it.minutes());
                if it.minutes.is_none_or(|m| m == 0) {
                    l.guessed += 1;
                }
            }
            if it.status != Status::PlanToWatch {
                l.watched += 1;
            }
            if sc.kind == Kind::Anime && it.status == Status::Watching && it.air_status == "currently_airing" {
                l.airing.push(Title {
                    media_id: it.media_id,
                    title: it.title.clone(),
                    cover_url: it.cover_url.clone(),
                    progress: it.progress,
                    total: it.total,
                    units: 0,
                });
            }
        }
        l.airing.sort_by_key(|a| a.title.to_lowercase());
        let args = params![sc.service, sc.kind.name()];
        let watched = "e.service = ?1 AND e.kind = ?2 AND e.status != 'plan_to_watch'";
        l.with_details = self.conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM entry e JOIN details d ON d.service = e.service AND d.kind = e.kind AND d.media_id = e.media_id
                 WHERE {watched} AND json_valid(d.body)"
            ),
            args,
            |r| r.get(0),
        )?;
        l.genres = self.top(
            &format!(
                "SELECT g.value, COUNT(*) FROM entry e JOIN details d ON d.service = e.service AND d.kind = e.kind AND d.media_id = e.media_id,
                 json_each(d.body, '$.genres') g WHERE {watched} AND json_valid(d.body) AND g.type = 'text'
                 GROUP BY g.value ORDER BY 2 DESC, 1 LIMIT {TOP_TAGS}"
            ),
            sc,
        )?;
        l.makers = match sc.kind {
            Kind::Anime => self.top(
                &format!(
                    "SELECT g.value, COUNT(*) FROM entry e JOIN details d ON d.service = e.service AND d.kind = e.kind AND d.media_id = e.media_id,
                     json_each(d.body, '$.studios') g WHERE {watched} AND json_valid(d.body) AND g.type = 'text'
                     GROUP BY g.value ORDER BY 2 DESC, 1 LIMIT {TOP_MAKERS}"
                ),
                sc,
            )?,
            // Authors come with the list (media.authors), no details needed.
            Kind::Manga => self.top(
                &format!(
                    "SELECT g.value, COUNT(*) FROM entry e JOIN media m ON m.service = e.service AND m.kind = e.kind AND m.id = e.media_id,
                     json_each(m.authors) g WHERE {watched} AND json_valid(m.authors) AND g.type = 'text'
                     GROUP BY g.value ORDER BY 2 DESC, 1 LIMIT {TOP_MAKERS}"
                ),
                sc,
            )?,
        };
        Ok(l)
    }

    fn top(&self, sql: &str, sc: Scope) -> Result<Vec<(String, u32)>> {
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map(params![sc.service, sc.kind.name()], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// The first day of `today`'s week (Monday), month and year.
fn starts(today: i64) -> (i64, i64, i64) {
    let (y, m, _) = civil_from_days(today);
    (today - i64::from(crate::time::weekday(today)), days_from_civil(y, i64::from(m), 1), days_from_civil(y, 1, 1))
}

fn log_stats(items: &[Item], rows: &[Row], now: i64, today: i64, kind: Kind, offset: &dyn Fn(i64) -> i64) -> LogStats {
    let first_day = today - (DAYS as i64 - 1);
    let mut l = LogStats { days: vec![0; DAYS], first_day, ..LogStats::default() };
    let minutes: HashMap<u32, u32> = items.iter().map(|it| (it.media_id, it.minutes())).collect();
    let (week, month, year) = starts(today);
    let mut seen: [std::collections::HashSet<u32>; 4] = Default::default();
    let mut lately: HashMap<u32, u32> = HashMap::new();
    let mut active_days = std::collections::BTreeSet::new();
    for r in rows.iter().filter(|r| r.at <= now) {
        let c = Clock::at(r.at, offset(r.at));
        let (units, vols) = (r.units(), r.volumes());
        let mins = if kind == Kind::Anime { u64::from(units) * u64::from(minutes.get(&r.media_id).copied().unwrap_or(24)) } else { 0 };
        for (i, (from, p)) in [(i64::MIN, &mut l.all), (year, &mut l.year), (month, &mut l.month), (week, &mut l.week)].into_iter().enumerate() {
            if c.day >= from {
                p.units += units;
                p.volumes += vols;
                p.minutes += mins;
                if seen[i].insert(r.media_id) {
                    p.titles += 1;
                }
            }
        }
        if units > 0 || vols > 0 {
            active_days.insert(c.day);
        }
        // Manga: a volume-only edit still counts on the calendar (as one).
        let mark = if units > 0 { units } else { u32::from(vols > 0) };
        if c.day >= first_day && c.day <= today {
            l.days[(c.day - first_day) as usize] += mark;
        }
        l.weekdays[usize::from(c.weekday)] += mark;
        l.hours[(c.minute / 60) as usize % 24] += mark;
        if c.day > today - LATELY_DAYS && units > 0 {
            *lately.entry(r.media_id).or_default() += units;
        }
    }
    // Streaks: runs of consecutive active days.
    let mut run = 0u32;
    let mut prev: Option<i64> = None;
    for d in &active_days {
        run = if prev == Some(d - 1) { run + 1 } else { 1 };
        l.best_streak = l.best_streak.max(run);
        prev = Some(*d);
    }
    l.streak = match prev {
        Some(last) if last == today || last == today - 1 => run,
        _ => 0,
    };
    let by_id: HashMap<u32, &Item> = items.iter().map(|it| (it.media_id, it)).collect();
    let mut lately: Vec<Title> = lately
        .into_iter()
        .filter_map(|(id, units)| {
            let it = by_id.get(&id)?;
            Some(Title { media_id: id, title: it.title.clone(), cover_url: it.cover_url.clone(), progress: it.progress, total: it.total, units })
        })
        .collect();
    lately.sort_by(|a, b| b.units.cmp(&a.units).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    lately.truncate(TOP_TITLES);
    l.lately = lately;
    l
}

/// "3 d 4 h", "5 h 20 min", "45 min".
pub fn duration(minutes: u64) -> String {
    let (d, h, m) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) if m == 0 => format!("{h} h"),
        (0, _) => format!("{h} h {m} min"),
        _ if h == 0 => format!("{d} d"),
        _ => format!("{d} d {h} h"),
    }
}

/// "1,234".
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "Nov 19, 2026" of a day number.
pub fn date_text(day: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = civil_from_days(day);
    format!("{} {d}, {y}", MONTHS[(m as usize).saturating_sub(1) % 12])
}

/// `kot stats`: the numbers as plain text.
pub fn text(s: &Stats, offset: &dyn Fn(i64) -> i64) -> String {
    let (unit, units, verb) = match s.kind {
        Kind::Anime => ("episode", "episodes", "watched"),
        Kind::Manga => ("chapter", "chapters", "read"),
    };
    let n = |x: u64| if x == 1 { format!("1 {unit}") } else { format!("{} {units}", grouped(x)) };
    let period = |p: &Period| {
        let mut t = n(u64::from(p.units));
        if s.kind == Kind::Manga && p.volumes > 0 {
            t.push_str(&format!(", {} vol.", p.volumes));
        }
        if s.kind == Kind::Anime && p.minutes > 0 {
            t.push_str(&format!(" ({})", duration(p.minutes)));
        }
        t
    };
    let mut out = String::new();
    let l = &s.list;
    out.push_str(&format!("{} statistics\n\n", s.kind.label()));
    out.push_str(&format!("On your list     {} titles\n", grouped(u64::from(l.titles))));
    for (st, c) in Status::ALL.iter().zip(l.statuses) {
        out.push_str(&format!("  {:<15}{}\n", st.label_in(s.kind), grouped(u64::from(c))));
    }
    out.push_str(&format!("{:<17}{}\n", format!("{} {verb}", capital(units)), grouped(l.units)));
    if s.kind == Kind::Manga {
        out.push_str(&format!("Volumes read     {}\n", grouped(l.volumes)));
    }
    if s.kind == Kind::Anime {
        let about = if l.guessed > 0 { "about " } else { "" };
        out.push_str(&format!("Time watched     {about}{}\n", duration(l.minutes)));
    }
    let scored: u32 = l.scores.iter().sum();
    if scored > 0 {
        let mean = l.scores.iter().enumerate().map(|(i, c)| f64::from(*c) * (i as f64 + 1.0)).sum::<f64>() / f64::from(scored);
        out.push_str(&format!("Mean score       {mean:.2} ({} scored, out of 10)\n", grouped(u64::from(scored))));
    }
    out.push('\n');
    let since = s.since.map(|t| date_text(Clock::at(t, offset(t)).day));
    match &since {
        Some(d) => out.push_str(&format!("In Kotora, since {d}:\n")),
        None => out.push_str("In Kotora:\n"),
    }
    out.push_str(&format!("  This week      {}\n", period(&s.log.week)));
    out.push_str(&format!("  This month     {}\n", period(&s.log.month)));
    out.push_str(&format!("  This year      {}\n", period(&s.log.year)));
    out.push_str(&format!("  Streak         {} (best {})\n", days(s.log.streak), days(s.log.best_streak)));
    if !s.has_activity() {
        out.push_str(&format!("  Nothing {verb} in Kotora yet: what you record from now on counts here.\n"));
    }
    if !l.genres.is_empty() {
        let g: Vec<String> = l.genres.iter().take(5).map(|(g, c)| format!("{g} ({c})")).collect();
        out.push_str(&format!("\nTop genres       {}\n", g.join(", ")));
    }
    if !l.makers.is_empty() {
        let g: Vec<String> = l.makers.iter().take(3).map(|(g, c)| format!("{g} ({c})")).collect();
        out.push_str(&format!("{:<17}{}\n", if s.kind == Kind::Anime { "Top studios" } else { "Top authors" }, g.join(", ")));
    }
    out
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// "1 day", "3 days".
pub fn days(n: u32) -> String {
    if n == 1 { "1 day".into() } else { format!("{n} days") }
}

#[cfg(test)]
mod tests;
