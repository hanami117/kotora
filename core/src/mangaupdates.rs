//! Release data for the Publishing page, from MangaUpdates' public API (no account, no key).
//!
//! For each Reading manga Kotora finds its MangaUpdates series, reads the series' latest
//! releases, and estimates how often chapters come out ([`cadence`]).
//!
//! - **Finding the series.** MangaUpdates knows no MyAnimeList or AniList ids, so the match is
//!   by title ([`best_match`]): the manga's titles against each hit's title and the title that
//!   matched (an associated name), comics against comics and novels against novels, the year
//!   when both sides know it. One clear candidate is [`Sure`](Confidence::Sure); several, or a
//!   year that doesn't fit, is a [`Guess`](Confidence::Guess) the page asks the user to check.
//!   A link the user pastes ([`series_id_from_link`]) wins over any search and is kept in the
//!   store (not the cache), so it survives a cleared cache.
//! - **Releases.** One request per series (`/releases/search`, newest first, 40 records): enough
//!   history for the estimate. A release counts when it brings a chapter past the highest seen
//!   so far: back-filled old chapters and the same chapter from a second group don't.
//! - **Estimates only.** MangaUpdates lists scanlation and official English releases as groups
//!   report them; the next date is Kotora's guess from the gaps between past releases, and the
//!   page says so.
//! - **Acceptable use** (MangaUpdates' API policy): credit ("Release data: MangaUpdates" wherever
//!   the data shows), reasonable spacing ([`Limiter`] at 20 a minute, 1.5 s apart) and caching
//!   ([`Cache`]: releases for [`RELEASES_TTL`], matches for weeks). Nothing of the user's goes to
//!   MangaUpdates but the titles searched and the series ids asked for.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{Request, Transport};
use crate::ratelimit::Limiter;
use crate::service::ServiceError;

pub const API: &str = "https://api.mangaupdates.com/v1";

/// The credit shown with the data.
pub const CREDIT: &str = "Release data: MangaUpdates";

/// A series' releases are asked again after 4 hours at the earliest.
pub const RELEASES_TTL: i64 = 4 * 3600;
/// A found series (sure or guessed) is searched again after 30 days.
pub const MATCH_TTL: i64 = 30 * 86_400;
/// A manga with no series found is searched again after 3 days.
pub const MISSING_TTL: i64 = 3 * 86_400;

/// Records asked for per series: about 12 release days for a series with two groups.
const RELEASES_PER_PAGE: u32 = 40;

const DAY: i64 = 86_400;

// ------------------------------------------------------------------------------------ series

/// A series' page on mangaupdates.com (the site's links carry the id in base 36).
pub fn series_url(id: u64) -> String {
    format!("https://www.mangaupdates.com/series/{}", base36(id))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The series id in what the user pasted: a series link (`mangaupdates.com/series/pb8uwds/…`,
/// with or without `https://` and `www.`), the API's link (`api.mangaupdates.com/v1/series/
/// 55099564912`), or a bare number. `None` for anything else (the old `series.html?id=` links use
/// other ids, which the API doesn't know).
pub fn series_id_from_link(s: &str) -> Option<u64> {
    let s = s.trim();
    if !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse().ok().filter(|n| *n > 0);
    }
    let lower = s.to_ascii_lowercase();
    let rest = lower.split_once("mangaupdates.com/")?.1;
    let (api, rest) = match rest.strip_prefix("v1/series/") {
        Some(r) => (true, r),
        None => (false, rest.strip_prefix("series/")?),
    };
    let part: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if part.is_empty() || part.len() > 13 {
        return None;
    }
    let n = if api {
        part.parse().ok()?
    } else {
        u64::from_str_radix(&part, 36).ok()?
    };
    (n > 0).then_some(n)
}

/// A search hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: u64,
    pub title: String,
    /// The title that matched the search (the main one or an associated name).
    pub hit_title: String,
    /// MangaUpdates' type: `Manga`, `Manhwa`, `Novel`, `Artbook`, `Doujinshi`, …
    pub kind: String,
    pub year: Option<i32>,
}

/// `/series/search`'s answer.
pub fn parse_search(body: &str) -> Result<Vec<Hit>, String> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(default)]
        results: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        record: Rec,
        #[serde(default)]
        hit_title: Option<String>,
    }
    #[derive(Deserialize)]
    struct Rec {
        series_id: u64,
        #[serde(default)]
        title: String,
        #[serde(default, rename = "type")]
        kind: Option<String>,
        #[serde(default)]
        year: Option<String>,
    }
    let a: Answer = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaUpdates\u{2019} search answer: {e}"))?;
    Ok(a.results
        .into_iter()
        .map(|it| {
            let title = html_unescape(&it.record.title);
            Hit {
                id: it.record.series_id,
                hit_title: it
                    .hit_title
                    .map_or_else(|| title.clone(), |t| html_unescape(&t)),
                title,
                kind: it.record.kind.unwrap_or_default(),
                year: it
                    .record
                    .year
                    .and_then(|y| y.trim().get(..4).and_then(|y| y.parse().ok())),
            }
        })
        .collect())
}

/// What the series record says of the run (`GET /series/{id}`). The release history alone goes
/// stale once groups stop (an official simulpub, a finished series): the record's latest
/// chapter and status don't.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    /// The latest chapter MangaUpdates knows of.
    pub latest_chapter: Option<u32>,
    /// The status text, e.g. "33 Volumes (Ongoing)", "17 Volumes (Complete)", "(Hiatus)".
    pub status: String,
    /// Completely scanlated.
    pub completed: bool,
    /// When the record last changed (unix seconds; 0 unknown).
    pub last_updated: i64,
    /// When Kotora saw `latest_chapter` go up between two looks (0: never; the first look
    /// doesn't count, so a series' first record never notifies).
    pub went_up_at: i64,
}

/// `GET /series/{id}`: the id, the title (a pasted link is checked with it) and the record.
pub fn parse_series(body: &str) -> Result<(u64, String, Series), String> {
    #[derive(Deserialize)]
    struct Rec {
        series_id: u64,
        #[serde(default)]
        title: String,
        #[serde(default)]
        latest_chapter: Option<i64>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        completed: Option<bool>,
        #[serde(default)]
        last_updated: Option<Value>,
    }
    let r: Rec = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaUpdates\u{2019} series: {e}"))?;
    let series = Series {
        latest_chapter: r
            .latest_chapter
            .filter(|n| *n > 0)
            .and_then(|n| u32::try_from(n).ok()),
        status: r
            .status
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        completed: r.completed.unwrap_or(false),
        last_updated: r
            .last_updated
            .as_ref()
            .and_then(|v| v.get("timestamp"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0),
        went_up_at: 0,
    };
    Ok((r.series_id, html_unescape(&r.title), series))
}

/// The list's own status of the manga (MyAnimeList's words; AniList's are mapped to them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListRun {
    Publishing,
    Finished,
    Hiatus,
    #[default]
    Unknown,
}

impl ListRun {
    pub fn from_word(s: &str) -> ListRun {
        match s {
            "currently_publishing" => ListRun::Publishing,
            "finished" | "discontinued" => ListRun::Finished,
            "on_hiatus" => ListRun::Hiatus,
            _ => ListRun::Unknown,
        }
    }
}

/// Where the series is in its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Ongoing,
    Finished,
    Hiatus,
}

/// Finished or on hiatus by the list's status (reliable on MyAnimeList and AniList), else by
/// MangaUpdates' status text or its completed flag; else ongoing. Never from a gap in releases.
pub fn run_of(series: Option<&Series>, list: ListRun) -> Run {
    let text = series.map(|s| s.status.to_lowercase()).unwrap_or_default();
    let complete = series.is_some_and(|s| s.completed)
        || (text.contains("complete") && !text.contains("incomplete"));
    match list {
        ListRun::Finished => Run::Finished,
        ListRun::Hiatus => Run::Hiatus,
        _ if complete => Run::Finished,
        _ if text.contains("hiatus") => Run::Hiatus,
        _ => Run::Ongoing,
    }
}

/// MangaUpdates sends a few titles with HTML entities in them.
fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

// ----------------------------------------------------------------------------------- matching

/// How sure a match is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    /// One series has the title (and the year fits).
    Sure,
    /// Several have it, or the year is off: the page asks the user to check.
    Guess,
    /// The user pasted its link.
    Picked,
}

/// The series a manga is matched with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matched {
    pub id: u64,
    pub title: String,
    pub confidence: Confidence,
}

/// Lowercase letters and digits only, spaces between words: "Salt & Gears!" and "salt and
/// gears" compare equal ("and" and "&" are the same word).
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut words: Vec<String> = Vec::new();
    let mut w = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            w.extend(c.to_lowercase());
        } else {
            if !w.is_empty() {
                words.push(std::mem::take(&mut w));
            }
            if c == '&' {
                words.push("and".into());
            }
        }
    }
    if !w.is_empty() {
        words.push(w);
    }
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// What the manga is, for the type check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Comic,
    Novel,
}

fn kind_fits(kind: &str, form: Form) -> bool {
    match kind {
        "Novel" => form == Form::Novel,
        // Never a list's manga.
        "Artbook" | "Doujinshi" | "Drama CD" => false,
        // Manga, Manhwa, Manhua, OEL and the country types; unknown types too.
        _ => form == Form::Comic,
    }
}

/// The series for a manga titled `titles` (main first), from search hits. `year`: the manga's
/// first year, when known.
pub fn best_match(
    titles: &[String],
    year: Option<i32>,
    form: Form,
    hits: &[Hit],
) -> Option<Matched> {
    let wanted: Vec<String> = titles
        .iter()
        .map(|t| normalize(t))
        .filter(|t| !t.is_empty())
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let same: Vec<&Hit> = hits
        .iter()
        .filter(|h| kind_fits(&h.kind, form))
        .filter(|h| {
            wanted.contains(&normalize(&h.hit_title)) || wanted.contains(&normalize(&h.title))
        })
        .filter(|h| seen.insert(h.id))
        .collect();
    let year_fits = |h: &Hit| match (year, h.year) {
        (Some(a), Some(b)) => (a - b).abs() <= 1,
        _ => true,
    };
    let fitting: Vec<&&Hit> = same.iter().filter(|h| year_fits(h)).collect();
    let m = |h: &Hit, confidence| Matched {
        id: h.id,
        title: h.title.clone(),
        confidence,
    };
    match (fitting.as_slice(), same.first()) {
        ([one], _) => {
            // The only one with the year; still a guess when others share the title and no year
            // told them apart.
            let told_apart = same.len() == 1 || (year.is_some() && one.year.is_some());
            Some(m(
                one,
                if told_apart {
                    Confidence::Sure
                } else {
                    Confidence::Guess
                },
            ))
        }
        ([first, ..], _) => Some(m(first, Confidence::Guess)),
        ([], Some(first)) => Some(m(first, Confidence::Guess)),
        ([], None) => None,
    }
}

// ----------------------------------------------------------------------------------- releases

/// One release of a series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// The chapter as written ("79.1", "Bone 67", "10-12"); empty for a volume-only release.
    pub chapter: String,
    /// The volume as written, when given.
    pub volume: Option<String>,
    /// The release day (unix seconds of its midnight, UTC).
    pub day: i64,
    /// When MangaUpdates added it (unix seconds); 0 when not given.
    pub added: i64,
}

impl Release {
    /// The chapter's number ([`number_in`]).
    pub fn chapter_no(&self) -> Option<f64> {
        number_in(&self.chapter)
    }

    pub fn volume_no(&self) -> Option<f64> {
        self.volume.as_deref().and_then(number_in)
    }

    /// When it came out, as precisely as known.
    pub fn at(&self) -> i64 {
        if self.added > 0 && self.added >= self.day {
            self.added
        } else {
            self.day
        }
    }
}

/// The number a chapter or volume is known by: the last number in the text ("79.1" 79.1, "Bone
/// 67" 67, "Night 259" 259, "10-12" 12, "c.5" 5, "12a" 12). `None` when there is no number.
pub fn number_in(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            last = s[start..i].parse::<f64>().ok().filter(|n| n.is_finite());
        } else {
            i += 1;
        }
    }
    last
}

/// `/releases/search`'s answer, newest first as asked.
pub fn parse_releases(body: &str) -> Result<Vec<Release>, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("can\u{2019}t read MangaUpdates\u{2019} releases: {e}"))?;
    let Some(results) = v.get("results").and_then(Value::as_array) else {
        return Err("MangaUpdates\u{2019} releases answer has no results".into());
    };
    let text = |x: Option<&Value>| match x {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let mut out = Vec::new();
    for r in results {
        let Some(rec) = r.get("record") else { continue };
        let added = rec
            .get("time_added")
            .and_then(|t| t.get("timestamp"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let day = crate::time::parse_date(&text(rec.get("release_date")))
            .map(|d| d * DAY)
            .or_else(|| (added > 0).then(|| added.div_euclid(DAY) * DAY));
        let Some(day) = day else { continue };
        let volume = Some(text(rec.get("volume"))).filter(|v| !v.is_empty());
        out.push(Release {
            chapter: text(rec.get("chapter")),
            volume,
            day,
            added,
        });
    }
    Ok(out)
}

/// What the page shows of a series' releases.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The highest chapter out, as written ("Ch 74", "Bone 67" shows as written).
    pub latest: Option<String>,
    /// Its number.
    pub latest_no: Option<f64>,
    /// When it came out.
    pub latest_at: Option<i64>,
    /// When it was added to MangaUpdates (the notifications' "new").
    pub latest_added: i64,
    /// A series with volume releases only: the highest volume, as written.
    pub volume_only: Option<String>,
    pub cadence: Cadence,
    /// The newest release days with what they brought, newest first (the row's panel).
    pub recent: Vec<(String, i64)>,
    pub run: Run,
    /// The latest chapter comes from the series record, past the release history: its date
    /// isn't known.
    pub from_record: bool,
    /// The release history stopped (groups gave up after an official release, say): no estimate.
    pub stale: bool,
    /// When the record's latest chapter was seen going up (notifications; 0 never).
    pub went_up_at: i64,
}

/// The release days that moved the series on: oldest first, each with the highest number it
/// brought (chapters; volumes for a series with no chapter numbers) and the release that brought it.
fn new_highs(releases: &[Release]) -> (Vec<(i64, f64, usize)>, bool) {
    let chapters = releases.iter().any(|r| r.chapter_no().is_some());
    let mut order: Vec<usize> = (0..releases.len()).collect();
    // Oldest first; on one day, the lower number first.
    let no = |r: &Release| {
        if chapters {
            r.chapter_no()
        } else {
            r.volume_no()
        }
    };
    order.sort_by(|&a, &b| {
        releases[a].day.cmp(&releases[b].day).then(
            no(&releases[a])
                .unwrap_or(0.0)
                .total_cmp(&no(&releases[b]).unwrap_or(0.0)),
        )
    });
    let mut high = f64::NEG_INFINITY;
    let mut out: Vec<(i64, f64, usize)> = Vec::new();
    for i in order {
        let Some(n) = no(&releases[i]) else { continue };
        if n <= high {
            continue;
        }
        high = n;
        match out.last_mut() {
            Some(last) if last.0 == releases[i].day => {
                last.1 = n;
                last.2 = i;
            }
            _ => out.push((releases[i].day, n, i)),
        }
    }
    (out, chapters)
}

/// [`summarize_with`] with no series record and no list status.
pub fn summarize(releases: &[Release], now: i64) -> Summary {
    summarize_with(releases, None, ListRun::Unknown, now)
}

/// The record ahead of the releases by more than this many chapters: the history is stale.
const AHEAD: f64 = 2.0;

/// The latest chapter (the higher of the releases and the series record), its date when the
/// releases have it, the run (finished, on hiatus, ongoing: [`run_of`]), and the estimate. A
/// release history the record has left behind, or with nothing for a long time on an ongoing
/// series, is stale: no estimate ([`Pace::Untracked`]), never a hiatus by itself.
pub fn summarize_with(
    releases: &[Release],
    series: Option<&Series>,
    list: ListRun,
    now: i64,
) -> Summary {
    let (highs, chapters) = new_highs(releases);
    let last = highs.last().map(|&(_, _, i)| &releases[i]);
    let recent = highs
        .iter()
        .rev()
        .take(6)
        .map(|&(day, _, i)| {
            let r = &releases[i];
            let label = if chapters {
                chapter_label(&r.chapter)
            } else {
                format!("Vol {}", r.volume.as_deref().unwrap_or("?"))
            };
            (label, day.max(0))
        })
        .collect();
    let mut s = Summary {
        latest: last.filter(|_| chapters).map(|r| r.chapter.clone()),
        latest_no: highs.last().filter(|_| chapters).map(|h| h.1),
        latest_at: last.map(Release::at),
        latest_added: last.map_or(0, |r| r.added),
        volume_only: last.filter(|_| !chapters).and_then(|r| r.volume.clone()),
        cadence: cadence_of(&highs.iter().map(|h| h.0).collect::<Vec<_>>(), now),
        recent,
        run: run_of(series, list),
        from_record: false,
        stale: false,
        went_up_at: series.map_or(0, |s| s.went_up_at),
    };
    let rel_high = s.latest_no;
    if let Some(n) = series.and_then(|x| x.latest_chapter).map(f64::from)
        && rel_high.is_none_or(|h| n > h)
    {
        s.stale = rel_high.is_none_or(|h| n - h > AHEAD)
            || s.cadence.overdue
            || s.cadence.pace == Pace::Hiatus;
        s.latest = Some(format!("{n}"));
        s.latest_no = Some(n);
        s.latest_at = None;
        s.latest_added = 0;
        s.volume_only = None;
        s.from_record = true;
    }
    // A long quiet spell is a stale history, not a hiatus: only a status says hiatus.
    if s.cadence.pace == Pace::Hiatus {
        s.stale = true;
    }
    let c = &mut s.cadence;
    match s.run {
        Run::Finished => {
            *c = Cadence {
                pace: Pace::Finished,
                gap_days: 0,
                next: None,
                overdue: false,
                last: c.last,
            }
        }
        Run::Hiatus => {
            *c = Cadence {
                pace: Pace::Hiatus,
                gap_days: 0,
                next: None,
                overdue: false,
                last: c.last,
            }
        }
        Run::Ongoing if s.stale => {
            *c = Cadence {
                pace: Pace::Untracked,
                gap_days: 0,
                next: None,
                overdue: false,
                last: c.last,
            }
        }
        Run::Ongoing => {}
    }
    s
}

/// "Ch 74" for a plain number, the text as written otherwise ("Bone 67", "Ch 10-12").
pub fn chapter_label(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with(|c: char| c.is_ascii_digit()) {
        format!("Ch {raw}")
    } else {
        raw.to_string()
    }
}

// ------------------------------------------------------------------------------------ cadence

/// How often chapters come out, by the gaps between past release days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Weekly,
    Biweekly,
    /// Every three weeks.
    Triweekly,
    Monthly,
    /// Regular, every so many days (another gap).
    Every(u32),
    /// No clear rhythm; about every so many days.
    Irregular(u32),
    /// On hiatus (the series' status says so; [`cadence_of`] alone also says it after a long
    /// quiet spell, which [`summarize_with`] turns into [`Pace::Untracked`]).
    Hiatus,
    /// The series is finished.
    Finished,
    /// Ongoing, but the release history stopped: no dates to estimate from.
    Untracked,
    /// Too few releases to tell.
    Unknown,
}

impl Pace {
    /// "Weekly", "Every 2 weeks", "Monthly", "Every 10 days", "Irregular", "On hiatus", …
    pub fn label(self) -> String {
        match self {
            Pace::Weekly => "Weekly".into(),
            Pace::Biweekly => "Every 2 weeks".into(),
            Pace::Triweekly => "Every 3 weeks".into(),
            Pace::Monthly => "Monthly".into(),
            Pace::Every(d) if d >= 14 && d % 7 <= 1 => format!("Every {} weeks", d / 7),
            Pace::Every(d) if d >= 50 => format!("Every {} months", (d + 15) / 30),
            Pace::Every(d) => format!("Every {d} days"),
            Pace::Irregular(_) => "Irregular".into(),
            Pace::Hiatus => "On hiatus".into(),
            Pace::Finished => "Finished".into(),
            Pace::Untracked => "Ongoing".into(),
            Pace::Unknown => "Too few releases".into(),
        }
    }
}

/// The estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub pace: Pace,
    /// The typical gap, in days (0 when unknown).
    pub gap_days: u32,
    /// The next release day by the estimate (unix seconds of a UTC midnight); `None` when
    /// irregular, on hiatus, unknown or overdue.
    pub next: Option<i64>,
    /// Later than the usual gap allows, but not yet a hiatus.
    pub overdue: bool,
    /// The last release day.
    pub last: Option<i64>,
}

impl Cadence {
    /// For sorting by next release: the estimate, else (irregular) the last day plus the gap.
    /// Hiatus and unknown sort last.
    pub fn sort_key(&self) -> i64 {
        match (self.pace, self.next, self.last) {
            (_, Some(n), _) => n,
            (Pace::Hiatus | Pace::Unknown | Pace::Finished | Pace::Untracked, _, _) => i64::MAX,
            (_, None, Some(l)) if self.overdue => l + i64::from(self.gap_days) * DAY,
            (_, None, Some(l)) => l + i64::from(self.gap_days.max(1)) * DAY,
            _ => i64::MAX,
        }
    }
}

/// The release days used for the estimate: the last 12.
const HISTORY: usize = 12;

/// The estimate from release days (oldest first, one per day).
pub fn cadence_of(days: &[i64], now: i64) -> Cadence {
    let none = |pace, last| Cadence {
        pace,
        gap_days: 0,
        next: None,
        overdue: false,
        last,
    };
    let Some(&last) = days.last() else {
        return none(Pace::Unknown, None);
    };
    let since = (now - last).max(0);
    let days = &days[days.len().saturating_sub(HISTORY)..];
    if days.len() < 3 {
        return none(
            if since > 120 * DAY {
                Pace::Hiatus
            } else {
                Pace::Unknown
            },
            Some(last),
        );
    }
    let mut gaps: Vec<i64> = days
        .windows(2)
        .map(|w| (w[1] - w[0]) / DAY)
        .filter(|g| *g > 0)
        .collect();
    if gaps.is_empty() {
        return none(Pace::Unknown, Some(last));
    }
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2].max(1);
    let gap = median * DAY;
    if since > (4 * gap).max(56 * DAY) {
        return Cadence {
            pace: Pace::Hiatus,
            gap_days: median as u32,
            next: None,
            overdue: false,
            last: Some(last),
        };
    }
    let tol = (median * 3 / 10).max(2);
    let close = gaps.iter().filter(|g| (**g - median).abs() <= tol).count();
    let regular = close * 10 >= gaps.len() * 6;
    let overdue = since * 2 > gap * 3;
    let pace = if !regular {
        Pace::Irregular(median as u32)
    } else {
        match median {
            6..=8 => Pace::Weekly,
            13..=16 => Pace::Biweekly,
            20..=23 => Pace::Triweekly,
            27..=35 => Pace::Monthly,
            d => Pace::Every(d as u32),
        }
    };
    let next = (regular && !overdue).then(|| {
        // The first slot from today on (a release a day late still shows today).
        let today = now.div_euclid(DAY) * DAY;
        let mut n = last + gap;
        while n < today {
            n += gap;
        }
        n
    });
    Cadence {
        pace,
        gap_days: median as u32,
        next,
        overdue,
        last: Some(last),
    }
}

/// Chapters out that the user hasn't read: the latest chapter's whole number past `read`
/// (79.1 out, 79 read: 0). `None` when the latest is unknown.
pub fn behind(latest: Option<f64>, read: u32) -> Option<u32> {
    let n = latest?;
    Some(
        (n.floor().max(0.0) as u64)
            .saturating_sub(u64::from(read))
            .min(u64::from(u32::MAX)) as u32,
    )
}

// ------------------------------------------------------------------------------------- client

/// The MangaUpdates client: its own rate budget, and a 429 waited out once.
pub struct Client<T: Transport> {
    transport: T,
    limiter: Limiter,
    sleep: fn(Duration),
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T) -> Self {
        Client {
            transport,
            limiter: Limiter::new(20, Duration::from_millis(1500)),
            sleep: std::thread::sleep,
        }
    }

    /// Tests: no real waiting.
    pub fn without_sleep(transport: T) -> Self {
        Client {
            sleep: |_| {},
            ..Client::new(transport)
        }
    }

    fn send(
        &mut self,
        method: &'static str,
        url: String,
        json: Option<String>,
    ) -> Result<String, ServiceError> {
        for attempt in 0..2 {
            let wait = self.limiter.acquire(Instant::now());
            (self.sleep)(wait);
            let r = self
                .transport
                .send(&Request {
                    method,
                    url: url.clone(),
                    bearer: None,
                    form: None,
                    json: json.clone(),
                })
                .map_err(|e| {
                    ServiceError::Offline(format!("can\u{2019}t reach MangaUpdates: {e}"))
                })?;
            match r.status {
                200..=299 => return Ok(r.body),
                429 if attempt == 0 => {
                    let wait = Duration::from_secs(r.retry_after.unwrap_or(10).min(60));
                    self.limiter.hold(Instant::now(), wait);
                }
                429 => return Err(ServiceError::RateLimited),
                404 => {
                    return Err(ServiceError::Other(
                        "MangaUpdates has no such series".into(),
                    ));
                }
                s => {
                    return Err(ServiceError::Other(format!(
                        "MangaUpdates answered HTTP {s}"
                    )));
                }
            }
        }
        Err(ServiceError::RateLimited)
    }

    /// Series whose title (or an associated name) matches `q`.
    pub fn search(&mut self, q: &str) -> Result<Vec<Hit>, ServiceError> {
        let q: String = q.trim().chars().take(100).collect();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let body = serde_json::json!({ "search": q, "perpage": 25 }).to_string();
        parse_search(&self.send("POST", format!("{API}/series/search"), Some(body))?)
            .map_err(ServiceError::Other)
    }

    /// The newest releases of series `id`.
    pub fn releases(&mut self, id: u64) -> Result<Vec<Release>, ServiceError> {
        let body = serde_json::json!({ "search": id.to_string(), "search_type": "series", "orderby": "date", "asc": "desc", "perpage": RELEASES_PER_PAGE }).to_string();
        parse_releases(&self.send("POST", format!("{API}/releases/search"), Some(body))?)
            .map_err(ServiceError::Other)
    }

    /// Series `id`: its id, title (a pasted link is checked with it) and record.
    pub fn series(&mut self, id: u64) -> Result<(u64, String, Series), ServiceError> {
        parse_series(&self.send("GET", format!("{API}/series/{id}"), None)?)
            .map_err(ServiceError::Other)
    }

    /// Searches each title in turn until one gives a match.
    pub fn find(
        &mut self,
        titles: &[String],
        year: Option<i32>,
        form: Form,
    ) -> Result<Option<Matched>, ServiceError> {
        let mut seen = std::collections::HashSet::new();
        let mut guess = None;
        for t in titles
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .take(3)
        {
            if !seen.insert(normalize(t)) {
                continue;
            }
            let hits = self.search(t)?;
            match best_match(titles, year, form, &hits) {
                Some(m) if m.confidence == Confidence::Sure => return Ok(Some(m)),
                Some(m) => {
                    guess.get_or_insert(m);
                }
                None => {}
            }
        }
        Ok(guess)
    }
}

impl Client<crate::http::UreqTransport> {
    /// The real client (`User-Agent: Kotora/<version>`).
    pub fn online() -> Self {
        Client::new(crate::http::UreqTransport::default())
    }
}

// -------------------------------------------------------------------------------------- cache

/// Files in `<cache>/kotora/mangaupdates/` (safe to delete): one per manga's match, one per
/// series' releases.
pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct CachedMatch {
    at: i64,
    found: Option<Matched>,
}

#[derive(Serialize, Deserialize)]
struct CachedReleases {
    at: i64,
    releases: Vec<Release>,
    /// The series record read with them (files from before it have none: read again).
    #[serde(default)]
    series: Option<Series>,
}

/// The manga a match belongs to: the service and its id there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub service: crate::service::ServiceId,
    pub media_id: u32,
}

impl Key {
    fn file(self) -> String {
        format!("match-{}-{}.json", self.service.name(), self.media_id)
    }

    /// The store's key for a link the user picked (under the service: sign out forgets it).
    pub fn kv(self) -> String {
        format!("{}:mu-link:{}", self.service.name(), self.media_id)
    }
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Cache { dir }
    }

    /// Kotora's own cache folder.
    pub fn default_dir() -> Self {
        Cache::new(crate::paths::cache_dir().join("mangaupdates"))
    }

    fn read<D: for<'a> Deserialize<'a>>(&self, name: &str) -> Option<D> {
        serde_json::from_str(&std::fs::read_to_string(self.dir.join(name)).ok()?).ok()
    }

    fn write<S: Serialize>(&self, name: &str, v: &S) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string(v) {
            let tmp = self.dir.join(format!("{name}.tmp"));
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, self.dir.join(name));
            }
        }
    }

    /// The saved match and when it was made (`Some(None)`: searched, nothing found).
    pub fn get_match(&self, key: Key) -> Option<(Option<Matched>, i64)> {
        self.read::<CachedMatch>(&key.file())
            .map(|c| (c.found, c.at))
    }

    pub fn put_match(&self, key: Key, found: Option<&Matched>, now: i64) {
        self.write(
            &key.file(),
            &CachedMatch {
                at: now,
                found: found.cloned(),
            },
        );
    }

    /// The saved releases of a series and when they were read.
    pub fn get_releases(&self, id: u64) -> Option<(Vec<Release>, i64)> {
        self.read::<CachedReleases>(&format!("rel-{id}.json"))
            .map(|c| (c.releases, c.at))
    }

    pub fn put_releases(&self, id: u64, releases: &[Release], now: i64) {
        self.put_releases_with(id, releases, None, now);
    }

    /// The releases with the series record.
    pub fn put_releases_with(
        &self,
        id: u64,
        releases: &[Release],
        series: Option<&Series>,
        now: i64,
    ) {
        self.write(
            &format!("rel-{id}.json"),
            &CachedReleases {
                at: now,
                releases: releases.to_vec(),
                series: series.cloned(),
            },
        );
    }

    /// The series record saved with the releases.
    pub fn get_series(&self, id: u64) -> Option<Series> {
        self.read::<CachedReleases>(&format!("rel-{id}.json"))
            .and_then(|c| c.series)
    }
}

/// Whether a saved match is still good at `now`.
pub fn match_fresh(found: Option<&Matched>, at: i64, now: i64) -> bool {
    let ttl = if found.is_some() {
        MATCH_TTL
    } else {
        MISSING_TTL
    };
    now - at < ttl
}

/// Whether saved releases are still good at `now`.
pub fn releases_fresh(at: i64, now: i64) -> bool {
    now - at < RELEASES_TTL && at <= now
}

/// The manga's first year, from its start date (`2019-04-02`, `2019`).
pub fn year_of(start_date: Option<&str>) -> Option<i32> {
    start_date
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
}

/// The titles searched for a manga: main, English, then its other names.
pub fn titles_of(m: &crate::Media) -> Vec<String> {
    std::iter::once(m.title.clone())
        .chain(m.title_en.clone())
        .chain(m.synonyms.iter().cloned())
        .chain(m.title_ja.clone())
        .filter(|t| !t.trim().is_empty())
        .collect()
}

pub fn form_of(m: &crate::Media) -> Form {
    if m.is_novel() {
        Form::Novel
    } else {
        Form::Comic
    }
}

// ------------------------------------------------------------------------------- one manga

/// What is known of one manga's series.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// `None`: no series found (or not looked up yet, with `looked` false).
    pub matched: Option<Matched>,
    /// Searched at least once (a `None` match means "not found").
    pub looked: bool,
    pub releases: Vec<Release>,
    /// The series record read with them.
    pub series: Option<Series>,
    /// When the releases were read (`None`: never).
    pub fetched_at: Option<i64>,
    /// The last lookup's failure, if it failed.
    pub error: Option<String>,
    /// The same manga on MangaDex (the second source).
    pub md: MdSide,
    /// The user's choice of source for this manga.
    pub pref: Source,
}

/// Which release data a manga uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// Whichever has the higher latest chapter.
    #[default]
    Auto,
    MangaUpdates,
    MangaDex,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::Auto, Source::MangaUpdates, Source::MangaDex];

    pub fn word(self) -> &'static str {
        match self {
            Source::Auto => "auto",
            Source::MangaUpdates => "mu",
            Source::MangaDex => "md",
        }
    }

    pub fn from_word(s: &str) -> Source {
        Source::ALL
            .into_iter()
            .find(|x| x.word() == s.trim())
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Auto => "Auto",
            Source::MangaUpdates => "MangaUpdates",
            Source::MangaDex => "MangaDex",
        }
    }
}

/// The release sources turned on (Settings → Manga → Release data; at least one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sources {
    pub mu: bool,
    pub md: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Sources { mu: true, md: true }
    }
}

/// A manga's series on MangaDex: found only through its MyAnimeList / AniList link there (or
/// picked by the user), never by title alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MdSide {
    /// The series id (a UUID).
    pub id: Option<String>,
    pub title: String,
    /// The user pasted its link.
    pub picked: bool,
    /// Searched at least once (`id` `None`: none links to this manga).
    pub looked: bool,
    pub releases: Vec<Release>,
    pub fetched_at: Option<i64>,
    /// Its newest chapter is an official external one (MANGA Plus, …).
    pub external: bool,
}

impl Info {
    /// The summary of the source in use ([`Info::best`], both sources on).
    pub fn summary(&self, now: i64, list: ListRun) -> Option<Summary> {
        self.best(now, list, Sources::default()).map(|b| b.0)
    }

    /// The summary of each source turned on and found, and the one used: the user's choice when
    /// it has data, else the one with the higher latest chapter (a dated one wins a tie).
    pub fn best(&self, now: i64, list: ListRun, on: Sources) -> Option<(Summary, Source)> {
        let mu = (on.mu && self.matched.is_some() && self.fetched_at.is_some())
            .then(|| summarize_with(&self.releases, self.series.as_ref(), list, now));
        let md = (on.md && self.md.id.is_some() && self.md.fetched_at.is_some())
            .then(|| summarize_with(&self.md.releases, None, list, now));
        let rank = |s: &Summary| (s.latest_no.unwrap_or(-1.0), s.latest_at.is_some());
        match (self.pref, mu, md) {
            (Source::MangaUpdates, Some(a), _) => Some((a, Source::MangaUpdates)),
            (Source::MangaDex, _, Some(b)) => Some((b, Source::MangaDex)),
            (_, Some(a), Some(b)) => {
                let (ra, rb) = (rank(&a), rank(&b));
                if rb.0 > ra.0 || (rb.0 == ra.0 && rb.1 && !ra.1) {
                    Some((b, Source::MangaDex))
                } else {
                    Some((a, Source::MangaUpdates))
                }
            }
            (_, Some(a), None) => Some((a, Source::MangaUpdates)),
            (_, None, Some(b)) => Some((b, Source::MangaDex)),
            (_, None, None) => None,
        }
    }

    /// When either source's releases were last read.
    pub fn checked_at(&self) -> Option<i64> {
        self.fetched_at.max(self.md.fetched_at)
    }

    /// Found on a source turned on.
    pub fn found(&self, on: Sources) -> bool {
        (on.mu && self.matched.is_some()) || (on.md && self.md.id.is_some())
    }

    /// Looked up on every source turned on, and found on none.
    pub fn not_found(&self, on: Sources) -> bool {
        !self.found(on) && (!on.mu || self.looked) && (!on.md || self.md.looked)
    }
}

/// The series the user picked for `key`, kept in the store.
pub fn picked(store: &crate::store::Store, key: Key) -> Option<Matched> {
    let v = store.kv_get(&key.kv()).ok().flatten()?;
    let (id, title) = v.split_once(' ').map_or((v.as_str(), ""), |(a, b)| (a, b));
    Some(Matched {
        id: id.parse().ok()?,
        title: title.to_string(),
        confidence: Confidence::Picked,
    })
}

/// Keeps (or, `None`, forgets) the series the user picked for `key`.
pub fn set_picked(
    store: &crate::store::Store,
    key: Key,
    m: Option<&Matched>,
) -> crate::store::Result<()> {
    match m {
        Some(m) => store.kv_set(&key.kv(), &format!("{} {}", m.id, m.title)),
        None => store.kv_del(&key.kv()),
    }
}

/// What the saved files say, without asking MangaUpdates.
pub fn saved(cache: &Cache, key: Key, picked: Option<Matched>) -> Info {
    let (matched, looked) = match picked {
        Some(p) => (Some(p), true),
        None => match cache.get_match(key) {
            Some((m, _)) => (m, true),
            None => (None, false),
        },
    };
    let (releases, fetched_at) = match matched.as_ref().and_then(|m| cache.get_releases(m.id)) {
        Some((r, at)) => (r, Some(at)),
        None => (Vec::new(), None),
    };
    let series = matched.as_ref().and_then(|m| cache.get_series(m.id));
    Info {
        matched,
        looked,
        releases,
        series,
        fetched_at,
        error: None,
        ..Info::default()
    }
}

/// Whether [`fetch`] would ask MangaUpdates for anything.
pub fn stale(cache: &Cache, key: Key, picked: Option<&Matched>, now: i64) -> bool {
    let m = match picked {
        Some(p) => Some(p.clone()),
        None => match cache.get_match(key) {
            Some((m, at)) if match_fresh(m.as_ref(), at, now) => m,
            Some(_) | None => return true,
        },
    };
    match m {
        // Saved before the series record was read: read again.
        Some(m) => {
            cache
                .get_releases(m.id)
                .is_none_or(|(_, at)| !releases_fresh(at, now))
                || cache.get_series(m.id).is_none()
        }
        None => false,
    }
}

/// Brings one manga's series up to date: the match (the picked one, else a fresh saved one,
/// else a search), then its releases unless fresh. `force`: releases asked again even when fresh
/// (a guess or a not-found is searched again too). Saves what it learns.
pub fn fetch<T: Transport>(
    client: &mut Client<T>,
    cache: &Cache,
    key: Key,
    picked: Option<Matched>,
    media: &crate::Media,
    now: i64,
    force: bool,
) -> Result<Info, ServiceError> {
    let matched = match picked {
        Some(p) => Some(p),
        None => match cache.get_match(key) {
            Some((m, at))
                if match_fresh(m.as_ref(), at, now)
                    && !(force && m.as_ref().is_none_or(|m| m.confidence != Confidence::Sure)) =>
            {
                m
            }
            _ => {
                let m = client.find(
                    &titles_of(media),
                    year_of(media.start_date.as_deref()),
                    form_of(media),
                )?;
                cache.put_match(key, m.as_ref(), now);
                m
            }
        },
    };
    let Some(m) = matched else {
        return Ok(Info {
            looked: true,
            ..Info::default()
        });
    };
    let old = cache.get_series(m.id);
    if !force
        && old.is_some()
        && let Some((r, at)) = cache
            .get_releases(m.id)
            .filter(|(_, at)| releases_fresh(*at, now))
    {
        return Ok(Info {
            matched: Some(m),
            looked: true,
            releases: r,
            series: old,
            fetched_at: Some(at),
            ..Info::default()
        });
    }
    let r = client.releases(m.id)?;
    let series = match client.series(m.id) {
        Ok((_, _, s)) => Some(went_up(old.as_ref(), s, now)),
        Err(e @ (ServiceError::Offline(_) | ServiceError::RateLimited)) => return Err(e),
        // No record (an odd answer): the releases alone, asked again next time.
        Err(_) => old.or_else(|| Some(Series::default())),
    };
    cache.put_releases_with(m.id, &r, series.as_ref(), now);
    Ok(Info {
        matched: Some(m),
        looked: true,
        releases: r,
        series,
        fetched_at: Some(now),
        ..Info::default()
    })
}

/// The new record, with when its latest chapter went up since the saved one (kept from the saved
/// one otherwise; never on a first look).
pub fn went_up(old: Option<&Series>, mut new: Series, now: i64) -> Series {
    new.went_up_at = match (old.and_then(|o| o.latest_chapter), new.latest_chapter) {
        (Some(a), Some(b)) if b > a => now,
        _ => old.map_or(0, |o| o.went_up_at),
    };
    new
}

// --------------------------------------------------------------------- MangaDex, the second source

#[derive(Serialize, Deserialize)]
struct CachedMd {
    at: i64,
    id: Option<String>,
    #[serde(default)]
    title: String,
}

#[derive(Serialize, Deserialize)]
struct CachedFeed {
    at: i64,
    releases: Vec<Release>,
    #[serde(default)]
    external: bool,
}

impl Cache {
    /// Kotora's cache folder for MangaDex release data.
    pub fn mangadex_dir() -> Self {
        Cache::new(crate::paths::cache_dir().join("mangadex-releases"))
    }

    fn get_md_match(&self, key: Key) -> Option<(Option<String>, String, i64)> {
        self.read::<CachedMd>(&format!(
            "md-match-{}-{}.json",
            key.service.name(),
            key.media_id
        ))
        .map(|c| (c.id, c.title, c.at))
    }

    fn put_md_match(&self, key: Key, id: Option<&str>, title: &str, now: i64) {
        self.write(
            &format!("md-match-{}-{}.json", key.service.name(), key.media_id),
            &CachedMd {
                at: now,
                id: id.map(str::to_string),
                title: title.to_string(),
            },
        );
    }

    fn get_feed(&self, id: &str) -> Option<(Vec<Release>, bool, i64)> {
        self.read::<CachedFeed>(&format!("feed-{id}.json"))
            .map(|c| (c.releases, c.external, c.at))
    }

    fn put_feed(&self, id: &str, releases: &[Release], external: bool, now: i64) {
        self.write(
            &format!("feed-{id}.json"),
            &CachedFeed {
                at: now,
                releases: releases.to_vec(),
                external,
            },
        );
    }
}

fn md_kv(key: Key) -> String {
    format!("{}:md-link:{}", key.service.name(), key.media_id)
}

fn pref_kv(key: Key) -> String {
    format!("{}:pub-source:{}", key.service.name(), key.media_id)
}

/// The MangaDex series the user picked: (id, title).
pub fn md_picked(store: &crate::store::Store, key: Key) -> Option<(String, String)> {
    let v = store.kv_get(&md_kv(key)).ok().flatten()?;
    let (id, title) = v.split_once(' ').map_or((v.as_str(), ""), |(a, b)| (a, b));
    crate::mangadex::md_id_from_link(id).map(|id| (id, title.to_string()))
}

pub fn set_md_picked(
    store: &crate::store::Store,
    key: Key,
    m: Option<(&str, &str)>,
) -> crate::store::Result<()> {
    match m {
        Some((id, title)) => store.kv_set(&md_kv(key), &format!("{id} {title}")),
        None => store.kv_del(&md_kv(key)),
    }
}

/// The user's source for a manga (Auto when none was chosen).
pub fn source_pref(store: &crate::store::Store, key: Key) -> Source {
    store
        .kv_get(&pref_kv(key))
        .ok()
        .flatten()
        .map_or(Source::Auto, |v| Source::from_word(&v))
}

pub fn set_source_pref(
    store: &crate::store::Store,
    key: Key,
    s: Source,
) -> crate::store::Result<()> {
    match s {
        Source::Auto => store.kv_del(&pref_kv(key)),
        s => store.kv_set(&pref_kv(key), s.word()),
    }
}

/// The saved MangaDex side, without a request.
fn md_match_fresh(found: bool, at: i64, now: i64) -> bool {
    now - at < if found { MATCH_TTL } else { MISSING_TTL }
}

pub fn md_saved(cache: &Cache, key: Key, picked: Option<(String, String)>) -> MdSide {
    let was_picked = picked.is_some();
    let (id, title, looked) = match picked {
        Some((id, title)) => (Some(id), title, true),
        None => match cache.get_md_match(key) {
            Some((id, title, _)) => (id, title, true),
            None => (None, String::new(), false),
        },
    };
    let (releases, external, fetched_at) = match id.as_deref().and_then(|i| cache.get_feed(i)) {
        Some((r, e, at)) => (r, e, Some(at)),
        None => (Vec::new(), false, None),
    };
    MdSide {
        id,
        title,
        picked: was_picked,
        looked,
        releases,
        fetched_at,
        external,
    }
}

/// Whether [`md_fetch`] would ask MangaDex for anything.
pub fn md_stale(cache: &Cache, key: Key, picked: Option<&(String, String)>, now: i64) -> bool {
    let id = match picked {
        Some((id, _)) => Some(id.clone()),
        None => match cache.get_md_match(key) {
            Some((id, _, at)) if md_match_fresh(id.is_some(), at, now) => id,
            Some(_) | None => return true,
        },
    };
    id.is_some_and(|i| {
        cache
            .get_feed(&i)
            .is_none_or(|(_, _, at)| !releases_fresh(at, now))
    })
}

/// Brings the MangaDex side up to date: the picked series, else a fresh saved match, else the
/// series linking to the manga's MyAnimeList (or AniList) id; then its chapter feed unless fresh.
#[allow(clippy::too_many_arguments)]
pub fn md_fetch<T: Transport>(
    client: &mut crate::mangadex::Client<T>,
    cache: &Cache,
    key: Key,
    picked: Option<(String, String)>,
    media: &crate::Media,
    now: i64,
    force: bool,
) -> Result<MdSide, ServiceError> {
    let was_picked = picked.is_some();
    let (id, title) = match picked {
        Some((id, title)) => (Some(id), title),
        None => match cache.get_md_match(key) {
            Some((id, title, at))
                if !(force && id.is_none()) && md_match_fresh(id.is_some(), at, now) =>
            {
                (id, title)
            }
            _ => {
                let hit = client.find_linked(
                    crate::mangadex::key_of(key.service, media),
                    &crate::mangadex::titles_of(media),
                )?;
                let (id, title) = hit.map_or((None, String::new()), |h| (Some(h.id), h.title));
                cache.put_md_match(key, id.as_deref(), &title, now);
                (id, title)
            }
        },
    };
    let Some(id) = id else {
        return Ok(MdSide {
            looked: true,
            ..MdSide::default()
        });
    };
    if !force
        && let Some((r, e, at)) = cache
            .get_feed(&id)
            .filter(|(_, _, at)| releases_fresh(*at, now))
    {
        return Ok(MdSide {
            id: Some(id),
            title,
            picked: was_picked,
            looked: true,
            releases: r,
            fetched_at: Some(at),
            external: e,
        });
    }
    let feed = client.feed(&id)?;
    let releases = crate::mangadex::feed_releases(&feed);
    let top = releases
        .iter()
        .filter_map(|r| r.chapter_no())
        .fold(f64::NEG_INFINITY, f64::max);
    let external = feed
        .iter()
        .any(|c| c.external && number_in(&c.chapter) == Some(top));
    cache.put_feed(&id, &releases, external, now);
    Ok(MdSide {
        id: Some(id),
        title,
        picked: was_picked,
        looked: true,
        releases,
        fetched_at: Some(now),
        external,
    })
}

/// Everything saved for a manga, from the sources turned on (no request).
pub fn saved_all(
    mu: &Cache,
    md: &Cache,
    store: &crate::store::Store,
    key: Key,
    on: Sources,
) -> Info {
    let mut i = if on.mu {
        saved(mu, key, picked(store, key))
    } else {
        Info::default()
    };
    if on.md {
        i.md = md_saved(md, key, md_picked(store, key));
    }
    i.pref = source_pref(store, key);
    i
}

/// Whether a pass would ask either source turned on.
pub fn stale_all(
    mu: &Cache,
    md: &Cache,
    store: &crate::store::Store,
    key: Key,
    on: Sources,
    now: i64,
) -> bool {
    (on.mu && stale(mu, key, picked(store, key).as_ref(), now))
        || (on.md && md_stale(md, key, md_picked(store, key).as_ref(), now))
}

/// Both sources turned on, brought up to date (see [`fetch`] and [`md_fetch`]).
#[allow(clippy::too_many_arguments)]
pub fn fetch_all<T: Transport, U: Transport>(
    mu: (&mut Client<T>, &Cache),
    md: (&mut crate::mangadex::Client<U>, &Cache),
    store: &crate::store::Store,
    key: Key,
    media: &crate::Media,
    on: Sources,
    now: i64,
    force: bool,
) -> Result<Info, ServiceError> {
    let mut i = if on.mu {
        fetch(mu.0, mu.1, key, picked(store, key), media, now, force)?
    } else {
        Info::default()
    };
    if on.md {
        i.md = md_fetch(md.0, md.1, key, md_picked(store, key), media, now, force)?;
    }
    i.pref = source_pref(store, key);
    Ok(i)
}

// ----------------------------------------------------------------------------- notifications

/// A chapter added this long ago or less is "new" (as airing's [`RECENT`](crate::airnotes::RECENT)).
pub const NEW_FOR: i64 = 2 * 86_400;

/// The chapter to announce for a manga read up to `read`: the latest one, when it is past the
/// progress and is new: a release MangaUpdates got in the last [`NEW_FOR`] seconds, or the series
/// record's latest chapter seen going up between two looks that recently. A record that is simply
/// ahead of a stale history (the first look) never counts. Its whole number (the key).
pub fn new_chapter(s: &Summary, read: u32, now: i64) -> Option<u32> {
    let n = behind(s.latest_no, read)
        .filter(|b| *b > 0)
        .map(|_| s.latest_no.unwrap_or(0.0).floor() as u32)?;
    let at = if s.from_record {
        Some(s.went_up_at).filter(|t| *t > 0)?
    } else if s.latest_added > 0 {
        s.latest_added
    } else {
        s.latest_at?
    };
    (at <= now + 3600 && now - at < NEW_FOR).then_some(n)
}

#[cfg(test)]
mod tests;
