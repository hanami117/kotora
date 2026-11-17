//! Airing: MyAnimeList's seasonal lists and Kotora's own timetable.
//!
//! MyAnimeList gives each show a start date and a weekly broadcast slot in Japan time
//! (`broadcast { day_of_the_week, start_time }`), not the time of each episode. Kotora estimates:
//! episode N airs at the first slot on or after the start date, plus N - 1 weeks. Japan has no
//! summer time, so JST is always UTC+9; the result is unix seconds, shown in the user's zone
//! (`time::Clock::local`). Breaks, double episodes and delays are unknown, so every time is
//! "estimated", and when the user's progress is past the estimate the episodes move forward
//! ([`Timetable::with_progress`]).

use serde::{Deserialize, Serialize};

use crate::time::{self, Clock};
use crate::{ListItem, Media, Status};

pub const WEEK: i64 = 7 * 86_400;
/// Japan Standard Time, UTC+9 all year.
pub const JST: i64 = 9 * 3600;

const DAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
pub const DAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A weekly slot in Japan time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Broadcast {
    /// Monday 0 … Sunday 6.
    pub day: u8,
    /// Minutes after midnight, JST; `None`: MyAnimeList knows the day only.
    pub minute: Option<u16>,
}

impl Broadcast {
    /// MyAnimeList's `day_of_the_week` (`saturday`) and `start_time` (`23:30`). A time past
    /// 24:00 (late-night TV writes 25:30) moves to the next day.
    pub fn parse(day: &str, time: Option<&str>) -> Option<Broadcast> {
        let day = DAYS.iter().position(|d| d.eq_ignore_ascii_case(day.trim()))? as u8;
        let minute = time.and_then(|t| {
            let (h, m) = t.trim().split_once(':')?;
            let (h, m) = (h.parse::<u16>().ok()?, m.get(..2).unwrap_or(m).parse::<u16>().ok()?);
            (h < 48 && m < 60).then_some(h * 60 + m)
        });
        Some(match minute {
            Some(m) if m >= 24 * 60 => Broadcast { day: (day + 1) % 7, minute: Some(m - 24 * 60) },
            minute => Broadcast { day, minute },
        })
    }

    /// The store's form: `saturday 23:30`, or `saturday`.
    pub fn to_text(self) -> String {
        let day = DAYS[usize::from(self.day % 7)];
        match self.minute {
            Some(m) => format!("{day} {:02}:{:02}", m / 60, m % 60),
            None => day.to_string(),
        }
    }

    pub fn from_text(s: &str) -> Option<Broadcast> {
        let mut it = s.split_whitespace();
        Broadcast::parse(it.next()?, it.next())
    }
}

/// The next episode and the exact time it airs, from the service (AniList's
/// `nextAiringEpisode`). Beats the weekly estimate: delays and breaks are already in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextAiring {
    pub episode: u32,
    /// Unix seconds.
    pub at: i64,
}

impl NextAiring {
    /// The store's form: `13 1767196800`.
    pub fn to_text(self) -> String {
        format!("{} {}", self.episode, self.at)
    }

    pub fn from_text(s: &str) -> Option<NextAiring> {
        let (e, at) = s.trim().split_once(' ')?;
        let episode = e.parse().ok().filter(|e| *e > 0)?;
        Some(NextAiring { episode, at: at.trim().parse().ok()? })
    }

    /// The weekly slot in Japan time this airing falls on (for the timetable's day and time).
    pub fn broadcast(self) -> Broadcast {
        let local = self.at + JST;
        let days = local.div_euclid(86_400);
        let minute = (local.rem_euclid(86_400) / 60) as u16;
        Broadcast { day: time::weekday(days), minute: Some(minute) }
    }
}

/// When the episodes of one show air, as Kotora estimates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timetable {
    /// Unix seconds of episode `shift + 1`'s slot (the first slot on or after the start date).
    pub first: i64,
    /// The episode count, when known.
    pub episodes: Option<u32>,
    /// Episodes known to have aired before `first` (from the user's progress).
    pub shift: u32,
    /// The last day an episode can air (the end date), as unix seconds of its end.
    pub until: Option<i64>,
}

impl Timetable {
    /// From the start date (`2026-10-03`; a shorter date gives no timetable) and the slot.
    ///
    /// The first slot is the broadcast weekday on or after the start date. When the slot's
    /// weekday is the day *before* the start date, the start date is taken to be a day late
    /// (a late-night show listed by the next calendar day, or an early premiere) and the first
    /// slot is that day before: the following weeks stay right either way.
    pub fn new(start_date: &str, b: Broadcast, episodes: Option<u32>, end_date: Option<&str>) -> Option<Timetable> {
        let start = time::parse_date(start_date)?;
        let minute = i64::from(b.minute?);
        let offset = match (i64::from(b.day) - i64::from(time::weekday(start))).rem_euclid(7) {
            6 => -1,
            n => n,
        };
        let first = (start + offset) * 86_400 + minute * 60 - JST;
        let until = end_date.and_then(time::parse_date).map(|d| (d + 2) * 86_400 - JST);
        Some(Timetable { first, episodes: episodes.filter(|&n| n > 0), shift: 0, until })
    }

    /// From the service's exact next airing: that episode at that time, then weekly. The
    /// episodes before it have aired.
    pub fn from_next(next: NextAiring, episodes: Option<u32>) -> Timetable {
        Timetable { first: next.at, episodes: episodes.filter(|&n| n > 0), shift: next.episode - 1, until: None }
    }

    /// The show's timetable, from what the season list or the saved list knows: the exact next
    /// airing when the service gave one, else the estimate.
    pub fn of(a: &SeasonAnime) -> Option<Timetable> {
        if let Some(n) = a.next_airing {
            return Some(Timetable::from_next(n, a.episodes));
        }
        Timetable::new(a.start_date.as_deref()?, a.broadcast?, a.episodes, a.end_date.as_deref())
    }

    /// Whether episode `n` exists (by the count, else by the end date's week).
    fn exists(&self, n: u32, at: i64) -> bool {
        self.episodes.is_none_or(|t| n <= t) && self.until.is_none_or(|u| at < u)
    }

    /// Episodes aired at `now` by the estimate (not more than the count).
    pub fn aired(&self, now: i64) -> u32 {
        let mut weeks = if now < self.first { 0 } else { (now - self.first) / WEEK + 1 };
        if let Some(u) = self.until {
            // Slots before the end date only.
            weeks = weeks.min(if u > self.first { (u - self.first - 1) / WEEK + 1 } else { 0 });
        }
        let n = self.shift + weeks as u32;
        self.episodes.map_or(n, |t| n.min(t))
    }

    /// Self-correction: a progress past the estimate means more has aired than estimated
    /// (a double premiere, an early start, a wrong start date). The episodes move forward so
    /// that the next one is `progress + 1`, at the next slot.
    pub fn with_progress(mut self, progress: u32, now: i64) -> Timetable {
        let aired = self.aired(now);
        if progress > aired {
            self.shift += progress - aired;
        }
        self
    }

    /// Unix seconds of episode `n`'s slot; `None` for an episode before the first slot.
    pub fn time_of(&self, n: u32) -> Option<i64> {
        (n > self.shift).then(|| self.first + i64::from(n - self.shift - 1) * WEEK)
    }

    /// The next episode to air after `now` and when, if any is left.
    pub fn next(&self, now: i64) -> Option<(u32, i64)> {
        let n = self.aired(now) + 1;
        let at = self.time_of(n)?;
        let at = if at <= now { at + WEEK } else { at };
        self.exists(n, at).then_some((n, at))
    }

    /// The episode whose slot falls in `[from, to)`, and when (at most one: slots are a week
    /// apart and the range is a week or less).
    pub fn slot_in(&self, from: i64, to: i64) -> Option<(u32, i64)> {
        let k = if from <= self.first { 0 } else { (from - self.first + WEEK - 1) / WEEK };
        let at = self.first + k * WEEK;
        let n = self.shift + k as u32 + 1;
        (at < to && self.exists(n, at)).then_some((n, at))
    }
}

/// An episode past what has aired by the estimate: Kotora asks before it is recorded.
/// Only the total episode count is a hard limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAired {
    pub episode: u32,
    /// Unix seconds of its estimated slot, when there is a timetable.
    pub expected: Option<i64>,
}

impl Timetable {
    /// The timetable of a list entry's anime (the list has no end date).
    pub fn of_media(m: &Media) -> Option<Timetable> {
        if let Some(n) = m.next_airing {
            return Some(Timetable::from_next(n, m.episodes));
        }
        Timetable::new(m.start_date.as_deref()?, m.broadcast?, m.episodes, None)
    }
}

/// Whether `episode` (a new progress) is past what has aired at `now`, by the estimate.
///
/// - Finished airing, or an unknown status: never.
/// - Airing: past the timetable's episodes, moved forward by the progress (a progress past
///   the estimate means more aired than estimated). No timetable (no slot time): never.
/// - Not yet aired: past the timetable, and with no timetable every episode.
/// - At or below the progress: never (nothing new is marked).
pub fn not_aired(air_status: &str, tt: Option<Timetable>, progress: u32, episode: u32, now: i64) -> Option<NotAired> {
    if episode <= progress {
        return None;
    }
    let upcoming = match air_status {
        "currently_airing" => false,
        "not_yet_aired" => true,
        _ => return None,
    };
    match tt {
        Some(t) => {
            let t = t.with_progress(progress, now);
            (episode > t.aired(now)).then(|| NotAired { episode, expected: t.time_of(episode).filter(|at| *at > now) })
        }
        None if upcoming => Some(NotAired { episode, expected: None }),
        None => None,
    }
}

/// One anime of a season list (MAL `GET /anime/season/{year}/{season}`), with what the Airing
/// page shows. Cached as JSON in the store's `season` table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeasonAnime {
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    /// MAL's `media_type`: `tv`, `ona`, `movie`, …
    pub format: String,
    pub episodes: Option<u32>,
    /// `currently_airing`, `finished_airing`, `not_yet_aired`.
    pub air_status: String,
    /// `2026-10-03` (or shorter).
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub broadcast: Option<Broadcast>,
    pub mean: Option<f32>,
    /// Members (`num_list_users`): the popularity order.
    pub members: Option<u32>,
    pub cover_url: Option<String>,
    /// The user's entry on MyAnimeList when it was fetched (MAL `status`, progress).
    pub mine: Option<(String, u32)>,
    /// The exact next airing, when the service gives it (AniList).
    pub next_airing: Option<NextAiring>,
}

impl SeasonAnime {
    pub fn media(&self) -> Media {
        Media {
            id: self.id,
            title: self.title.clone(),
            title_en: self.title_en.clone(),
            title_ja: self.title_ja.clone(),
            synonyms: self.synonyms.clone(),
            episodes: self.episodes,
            format: self.format.clone(),
            air_status: self.air_status.clone(),
            cover_url: self.cover_url.clone(),
            start_date: self.start_date.clone(),
            broadcast: self.broadcast,
            authors: Vec::new(),
            kind: Default::default(),
            volumes: None,
            next_airing: self.next_airing,
            id_mal: None,
        }
    }

    /// A list entry as a season anime (a long-running show the season lists don't have).
    pub fn from_item(it: &ListItem) -> SeasonAnime {
        let m = &it.media;
        SeasonAnime {
            id: m.id,
            title: m.title.clone(),
            title_en: m.title_en.clone(),
            title_ja: m.title_ja.clone(),
            synonyms: m.synonyms.clone(),
            format: m.format.clone(),
            episodes: m.episodes,
            air_status: m.air_status.clone(),
            start_date: m.start_date.clone(),
            end_date: None,
            broadcast: m.broadcast,
            mean: None,
            members: None,
            cover_url: m.cover_url.clone(),
            mine: Some((it.status.mal_name().into(), it.progress)),
            next_airing: m.next_airing,
        }
    }

    pub fn is_airing(&self) -> bool {
        self.air_status == "currently_airing"
    }
}

/// A saved season list younger than this is not asked for again (the current and the coming
/// season, and the one before: the Airing page merges its shows still airing).
pub const FRESH_SECS: i64 = 12 * 3600;
/// An older season changes slowly (scores, members), so its saved list stays good this long.
pub const PAST_FRESH_SECS: i64 = 14 * 86_400;
/// A saved season list is forgotten after this, whatever the page shows (no hoarding).
pub const SEASON_MAX_AGE: i64 = 30 * 86_400;
/// Season lists kept at most, the newest fetched (the current and the previous season always).
pub const SEASONS_KEPT: usize = 6;

/// A season: year and `winter`, `spring`, `summer`, `fall`. Ordered by time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Season {
    pub year: u16,
    /// 0 winter (Jan–Mar), 1 spring, 2 summer, 3 fall (Oct–Dec).
    pub quarter: u8,
}

const SEASONS: [&str; 4] = ["winter", "spring", "summer", "fall"];

impl Season {
    /// The first season the picker goes back to (the first TV anime aired in 1963).
    pub const OLDEST: Season = Season {
        year: 1963,
        quarter: 0,
    };

    /// The season of `t` in Japan (where the seasons are counted).
    pub fn of(t: i64) -> Season {
        let c = Clock::at(t, JST);
        Season { year: c.year as u16, quarter: ((c.month - 1) / 3) as u8 }
    }

    /// Seasons since year 0's winter: one number that counts across the year roll-over.
    pub fn index(self) -> i32 {
        i32::from(self.year) * 4 + i32::from(self.quarter % 4)
    }

    /// The season `n` seasons from this one (negative: back). Stops at year 0 and 65535.
    pub fn shift(self, n: i32) -> Season {
        let i = (self.index() + n).clamp(0, i32::from(u16::MAX) * 4 + 3);
        Season {
            year: (i / 4) as u16,
            quarter: (i % 4) as u8,
        }
    }

    pub fn previous(self) -> Season {
        self.shift(-1)
    }

    pub fn next(self) -> Season {
        self.shift(1)
    }

    /// The store's key for the season: `2026/fall`.
    pub fn key(self) -> String {
        format!("{}/{}", self.year, self.name())
    }

    /// How long a saved list of this season is good, with `current` the present season: the
    /// current one, the one after it and the one before it are asked for again after
    /// [`FRESH_SECS`]; an older one after [`PAST_FRESH_SECS`].
    pub fn fresh_secs(self, current: Season) -> i64 {
        if self < current.previous() { PAST_FRESH_SECS } else { FRESH_SECS }
    }

    /// Whether a list fetched at `fetched_at` is still good at `now`. A fetch time in the
    /// future (the clock was set back) is not.
    pub fn is_fresh(self, current: Season, fetched_at: i64, now: i64) -> bool {
        (0..self.fresh_secs(current)).contains(&(now - fetched_at))
    }

    /// MyAnimeList's name: `fall`.
    pub fn name(self) -> &'static str {
        SEASONS[usize::from(self.quarter % 4)]
    }

    /// "Fall 2026".
    pub fn label(self) -> String {
        let n = self.name();
        format!("{}{} {}", n[..1].to_uppercase(), &n[1..], self.year)
    }
}

/// What the Airing page lists: the current season, the previous season's shows still airing
/// (long-running ones), and the user's own airing entries the two lists don't have. One row per
/// anime, most members first (list-only entries, which have no count, last).
pub fn merge(current: &[SeasonAnime], previous: &[SeasonAnime], list: &[ListItem]) -> Vec<SeasonAnime> {
    let mut out: Vec<SeasonAnime> = Vec::with_capacity(current.len() + previous.len());
    let mut seen = std::collections::HashSet::new();
    for a in current.iter().chain(previous.iter().filter(|a| a.is_airing())) {
        if seen.insert(a.id) {
            out.push(a.clone());
        }
    }
    for it in list.iter().filter(|it| it.media.is_airing() && it.status != Status::Dropped) {
        if seen.insert(it.media.id) {
            out.push(SeasonAnime::from_item(it));
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.members.unwrap_or(0)));
    out
}

/// "2 d 4 h", "5 h 12 min", "8 min", "now".
pub fn countdown(secs: i64) -> String {
    let m = secs.max(0) / 60;
    match m {
        0 => "now".into(),
        1..=59 => format!("{m} min"),
        60..=1439 if m % 60 == 0 => format!("{} h", m / 60),
        60..=1439 => format!("{} h {} min", m / 60, m % 60),
        _ if (m % 1440) / 60 == 0 => format!("{} d", m / 1440),
        _ => format!("{} d {} h", m / 1440, (m % 1440) / 60),
    }
}

/// Episodes out that the user hasn't watched (the Airing page, Home, `kot airing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToWatch {
    /// Episodes out, by the service (exact) or by the timetable (estimate).
    pub out: u32,
    /// Out and not watched.
    pub behind: u32,
    /// From the service's own data (AniList's next airing still ahead, or a finished show's
    /// count); else the weekly estimate, which can't know of breaks and delays.
    pub exact: bool,
    /// Not aired yet.
    pub upcoming: bool,
}

impl ToWatch {
    /// "3 to watch", "~3 to watch" (an estimate), "Caught up", "Not aired yet".
    pub fn label(self) -> String {
        match (self.behind, self.upcoming && self.out == 0) {
            (_, true) => "Not aired yet".into(),
            (0, _) => "Caught up".into(),
            (n, _) if self.exact => format!("{n} to watch"),
            (n, _) => format!("~{n} to watch"),
        }
    }
}

/// How many episodes of a show are out and not watched at `now`.
///
/// - Finished airing: the episode count (exact); no count, nothing known.
/// - The exact next airing (`next`: AniList's own, or the mirror's AniList copy on MyAnimeList)
///   still ahead: the episodes before it (exact). One already past: weeks counted on from it.
/// - Else the timetable's estimate (`estimate`: start date + weekly slot, MyAnimeList).
/// - Not yet aired: none out, unless the timetable says the start has passed.
/// - A progress past the estimate means more is out: never negative, "Caught up".
///
/// `None`: nothing to go on (airing with no slot time, say).
pub fn to_watch(air_status: &str, episodes: Option<u32>, next: Option<NextAiring>, estimate: Option<Timetable>, progress: u32, now: i64) -> Option<ToWatch> {
    let episodes = episodes.filter(|n| *n > 0);
    let cap = |n: u32| episodes.map_or(n, |t| n.min(t));
    let (out, exact) = match (air_status, next) {
        ("finished_airing", _) => (episodes?, true),
        (_, Some(n)) if now < n.at => (cap(n.episode.saturating_sub(1)), true),
        (_, Some(n)) => (Timetable::from_next(n, episodes).aired(now), false),
        ("not_yet_aired", None) => (estimate.map_or(0, |t| t.aired(now)), estimate.is_none_or(|t| t.aired(now) == 0)),
        (_, None) => (estimate?.aired(now), false),
    };
    let upcoming = air_status == "not_yet_aired" && out == 0;
    Some(ToWatch { out, behind: out.saturating_sub(progress), exact, upcoming })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_watch_from_anilist_exactly() {
        // AniList: episode 5 airs next week: 4 are out.
        let next = NextAiring { episode: 5, at: at("2026-11-21", 23, 30, JST) };
        let now = at("2026-11-20", 12, 0, JST);
        let w = to_watch("currently_airing", Some(12), Some(next), Some(show()), 1, now).unwrap();
        assert_eq!((w.out, w.behind, w.exact), (4, 3, true), "the estimate (7) loses to AniList's 4");
        assert_eq!(w.label(), "3 to watch");
        assert_eq!(to_watch("currently_airing", Some(12), Some(next), None, 4, now).unwrap().label(), "Caught up");
        // Its time passed (the saved copy is old): counted on weekly, now an estimate.
        let w = to_watch("currently_airing", Some(12), Some(next), None, 1, next.at + WEEK + 60).unwrap();
        assert_eq!((w.out, w.exact), (6, false));
        assert_eq!(w.label(), "~5 to watch");
    }

    #[test]
    fn to_watch_on_myanimelist_is_an_estimate_from_the_season_start() {
        let t = show();
        // Before the first slot: none out.
        let w = to_watch("currently_airing", Some(12), None, Some(t), 0, t.first - 60).unwrap();
        assert_eq!((w.out, w.behind, w.label().as_str()), (0, 0, "Caught up"));
        // At the first slot: one.
        assert_eq!(to_watch("currently_airing", Some(12), None, Some(t), 0, t.first).unwrap().label(), "~1 to watch");
        // Week 7, at 2: ~5.
        let w = to_watch("currently_airing", Some(12), None, Some(t), 2, at("2026-11-15", 12, 0, JST)).unwrap();
        assert_eq!((w.out, w.behind, w.exact), (7, 5, false));
        assert_eq!(w.label(), "~5 to watch");
        // The user ahead of the estimate (a double premiere): caught up, never negative.
        assert_eq!(to_watch("currently_airing", Some(12), None, Some(t), 9, at("2026-11-15", 12, 0, JST)).unwrap().behind, 0);
        // No slot time: nothing to say.
        assert_eq!(to_watch("currently_airing", Some(12), None, None, 2, t.first), None);
    }

    #[test]
    fn to_watch_finished_and_not_yet_aired() {
        let now = at("2027-03-01", 0, 0, 0);
        let w = to_watch("finished_airing", Some(12), None, Some(show()), 5, now).unwrap();
        assert_eq!((w.out, w.behind, w.exact, w.label().as_str()), (12, 7, true, "7 to watch"));
        assert_eq!(to_watch("finished_airing", Some(12), None, None, 12, now).unwrap().label(), "Caught up");
        assert_eq!(to_watch("finished_airing", None, None, None, 3, now), None, "no count");
        // Not yet aired, start date ahead: none out.
        let t = show();
        let w = to_watch("not_yet_aired", Some(12), None, Some(t), 0, t.first - 86_400).unwrap();
        assert_eq!((w.out, w.upcoming, w.label().as_str()), (0, true, "Not aired yet"));
        assert_eq!(to_watch("not_yet_aired", None, None, None, 0, now).unwrap().label(), "Not aired yet");
        // MyAnimeList still says not yet aired, but the first slot has passed: ~1.
        assert_eq!(to_watch("not_yet_aired", Some(12), None, Some(t), 0, t.first + 60).unwrap().label(), "~1 to watch");
    }

    fn at(date: &str, hh: i64, mm: i64, offset: i64) -> i64 {
        time::parse_date(date).unwrap() * 86_400 + hh * 3600 + mm * 60 - offset
    }

    #[test]
    fn the_exact_next_airing_beats_the_estimate() {
        // The estimate says 7 aired by now, but AniList knows of breaks: 5 is next.
        let next = NextAiring { episode: 5, at: at("2026-11-21", 23, 30, JST) };
        assert_eq!(NextAiring::from_text(&next.to_text()), Some(next));
        assert_eq!(NextAiring::from_text("0 5"), None);
        assert_eq!(next.broadcast(), Broadcast { day: 5, minute: Some(1410) }, "Saturday 23:30 JST");
        let m = Media {
            air_status: "currently_airing".into(),
            episodes: Some(12),
            start_date: Some("2026-10-03".into()),
            broadcast: Some(Broadcast { day: 5, minute: Some(1410) }),
            next_airing: Some(next),
            ..Default::default()
        };
        let now = at("2026-11-20", 12, 0, JST);
        let tt = Timetable::of_media(&m).unwrap();
        assert_eq!(tt.aired(now), 4);
        assert_eq!(tt.next(now), Some((5, next.at)));
        assert_eq!(tt.time_of(6), Some(next.at + WEEK));
        assert!(not_aired(&m.air_status, Some(tt), 4, 5, now).is_some_and(|n| n.expected == Some(next.at)));
        assert_eq!(not_aired(&m.air_status, Some(tt), 4, 4, now), None);
        // Without it, the same show is estimated from the start date.
        let est = Timetable::of_media(&Media { next_airing: None, ..m.clone() }).unwrap();
        assert_eq!(est.aired(now), 7);
        // After it airs, the weeks go on from it.
        assert_eq!(tt.aired(next.at + 1), 5);
        assert_eq!(Timetable::of(&SeasonAnime { next_airing: Some(next), episodes: Some(12), ..Default::default() }).map(|t| t.aired(now)), Some(4));
    }

    #[test]
    fn broadcasts_read_and_write() {
        assert_eq!(Broadcast::parse("saturday", Some("23:30")), Some(Broadcast { day: 5, minute: Some(1410) }));
        assert_eq!(Broadcast::parse("Friday", None), Some(Broadcast { day: 4, minute: None }));
        assert_eq!(Broadcast::parse("saturday", Some("25:30")), Some(Broadcast { day: 6, minute: Some(90) }), "late-night 25:30 is Sunday 01:30");
        assert_eq!(Broadcast::parse("sunday", Some("24:00")), Some(Broadcast { day: 0, minute: Some(0) }));
        assert_eq!(Broadcast::parse("someday", Some("10:00")), None);
        assert_eq!(Broadcast::parse("monday", Some("ten")), Some(Broadcast { day: 0, minute: None }));
        for b in [Broadcast { day: 5, minute: Some(1410) }, Broadcast { day: 6, minute: None }] {
            assert_eq!(Broadcast::from_text(&b.to_text()), Some(b));
        }
        assert_eq!(Broadcast { day: 2, minute: Some(65) }.to_text(), "wednesday 01:05");
    }

    /// A made-up fall show: starts Saturday 2026-10-03, Saturdays 23:30 JST, 12 episodes.
    fn show() -> Timetable {
        Timetable::new("2026-10-03", Broadcast { day: 5, minute: Some(1410) }, Some(12), None).unwrap()
    }

    #[test]
    fn episodes_air_weekly_from_the_first_slot() {
        let t = show();
        assert_eq!(t.first, at("2026-10-03", 23, 30, JST));
        assert_eq!(t.time_of(1), Some(t.first));
        assert_eq!(t.time_of(7), Some(at("2026-11-14", 23, 30, JST)));
        assert_eq!(t.aired(t.first - 1), 0);
        assert_eq!(t.aired(t.first), 1);
        assert_eq!(t.aired(at("2026-11-15", 12, 0, JST)), 7);
        assert_eq!(t.next(at("2026-11-15", 12, 0, JST)), Some((8, at("2026-11-21", 23, 30, JST))));
        assert_eq!(t.aired(at("2027-06-01", 0, 0, 0)), 12, "never more than the count");
        assert_eq!(t.next(at("2027-06-01", 0, 0, 0)), None, "all aired");
        // In Central Europe (UTC+1 in November) episode 7 is Saturday 15:30; in New York (UTC-5)
        // it is Saturday 09:30; in Honolulu (UTC-10) 04:30, still Saturday.
        let ep7 = t.time_of(7).unwrap();
        assert_eq!(Clock::at(ep7, 3600).hhmm(), "15:30");
        assert_eq!(Clock::at(ep7, -5 * 3600).hhmm(), "09:30");
    }

    #[test]
    fn a_late_night_slot_moves_the_date_across_midnight() {
        // Sundays 01:00 JST is Saturday afternoon in Europe and Saturday morning in America.
        let t = Timetable::new("2026-10-04", Broadcast { day: 6, minute: Some(60) }, None, None).unwrap();
        let c = Clock::at(t.first, -4 * 3600);
        assert_eq!((c.weekday, c.date, c.hhmm().as_str()), (5, 3, "12:00"));
        let c = Clock::at(t.first, 2 * 3600);
        assert_eq!((c.weekday, c.hhmm().as_str()), (5, "18:00"));
    }

    #[test]
    fn the_first_slot_follows_the_start_date() {
        // Start on a Thursday, slot on Saturday: two days later.
        let t = Timetable::new("2026-10-01", Broadcast { day: 5, minute: Some(600) }, None, None).unwrap();
        assert_eq!(t.first, at("2026-10-03", 10, 0, JST));
        // Slot the day before the start date: that day (late-night listing or early premiere).
        let t = Timetable::new("2026-10-04", Broadcast { day: 5, minute: Some(1380) }, None, None).unwrap();
        assert_eq!(t.first, at("2026-10-03", 23, 0, JST));
        // No time, or a partial date: no timetable.
        assert_eq!(Timetable::new("2026-10-03", Broadcast { day: 5, minute: None }, None, None), None);
        assert_eq!(Timetable::new("2026-10", Broadcast { day: 5, minute: Some(0) }, None, None), None);
    }

    #[test]
    fn progress_ahead_moves_the_estimate_forward() {
        let now = at("2026-11-15", 12, 0, JST);
        let t = show();
        assert_eq!(t.aired(now), 7);
        // Behind or equal: unchanged.
        assert_eq!(t.with_progress(5, now), t);
        assert_eq!(t.with_progress(7, now), t);
        // Two ahead (a double premiere): episode 10 is next, at the next slot.
        let t2 = t.with_progress(9, now);
        assert_eq!(t2.aired(now), 9);
        assert_eq!(t2.next(now), Some((10, at("2026-11-21", 23, 30, JST))));
        assert_eq!(t2.next(at("2027-01-30", 0, 0, 0)), None, "12 episodes end two weeks sooner");
    }

    #[test]
    fn slots_of_a_week() {
        let t = show();
        let mon = at("2026-11-09", 0, 0, 3600);
        assert_eq!(t.slot_in(mon, mon + WEEK), Some((7, at("2026-11-14", 23, 30, JST))));
        let before = at("2026-09-28", 0, 0, 3600);
        assert_eq!(t.slot_in(before, before + WEEK), Some((1, t.first)));
        assert_eq!(t.slot_in(before - WEEK, before), None, "not started");
        assert_eq!(t.slot_in(at("2027-01-04", 0, 0, 0), at("2027-01-11", 0, 0, 0)), None, "ended");
        // An unknown count stops at the end date.
        let open = Timetable::new("2026-10-03", Broadcast { day: 5, minute: Some(1410) }, None, Some("2026-10-17")).unwrap();
        assert!(open.slot_in(at("2026-10-12", 0, 0, 0), at("2026-10-19", 0, 0, 0)).is_some());
        assert_eq!(open.slot_in(at("2026-10-19", 0, 0, 0), at("2026-10-26", 0, 0, 0)), None);
        assert_eq!(open.aired(at("2026-12-01", 0, 0, 0)), 3);
    }

    #[test]
    fn episodes_past_the_estimate_have_not_aired() {
        let now = at("2026-11-15", 12, 0, JST);
        let t = Some(show());
        // 7 aired: 7 is fine, 8 is not yet, expected at its slot.
        assert_eq!(not_aired("currently_airing", t, 6, 7, now), None);
        assert_eq!(not_aired("currently_airing", t, 7, 8, now), Some(NotAired { episode: 8, expected: Some(at("2026-11-21", 23, 30, JST)) }));
        // A jump from behind asks too (the editor, kot set).
        assert_eq!(not_aired("currently_airing", t, 3, 10, now).map(|n| n.episode), Some(10));
        // Progress already past the estimate (a double premiere): the next one is past it again.
        assert_eq!(not_aired("currently_airing", t, 9, 10, now).and_then(|n| n.expected), Some(at("2026-11-21", 23, 30, JST)));
        // Nothing new, finished or unknown status, or no timetable while airing: never asks.
        assert_eq!(not_aired("currently_airing", t, 8, 8, now), None);
        assert_eq!(not_aired("finished_airing", t, 7, 8, now), None);
        assert_eq!(not_aired("", t, 7, 8, now), None);
        assert_eq!(not_aired("currently_airing", None, 7, 8, now), None);
        // Not yet aired: before the first slot every episode asks; with no timetable too.
        let early = at("2026-09-20", 0, 0, 0);
        assert_eq!(not_aired("not_yet_aired", t, 0, 1, early), Some(NotAired { episode: 1, expected: Some(show().first) }));
        assert_eq!(not_aired("not_yet_aired", None, 0, 1, early), Some(NotAired { episode: 1, expected: None }));
    }

    #[test]
    fn a_list_entry_has_a_timetable_from_its_media() {
        let m = Media { start_date: Some("2026-10-03".into()), broadcast: Some(Broadcast { day: 5, minute: Some(1410) }), episodes: Some(12), ..Default::default() };
        assert_eq!(Timetable::of_media(&m), Some(show()));
        assert_eq!(Timetable::of_media(&Media { start_date: None, ..m }), None);
    }

    #[test]
    fn seasons() {
        let s = Season::of(at("2026-11-15", 12, 0, 0));
        assert_eq!((s.year, s.name(), s.label().as_str()), (2026, "fall", "Fall 2026"));
        assert_eq!(s.previous().label(), "Summer 2026");
        assert_eq!(Season { year: 2027, quarter: 0 }.previous().label(), "Fall 2026");
        // 2026-12-31 20:00 UTC is already 2027 in Japan.
        assert_eq!(Season::of(at("2026-12-31", 20, 0, 0)).label(), "Winter 2027");
    }

    #[test]
    fn seasons_step_across_the_year() {
        let s = |year, quarter| Season { year, quarter };
        // Fall -> Winter rolls the year forward, Winter -> Fall back.
        assert_eq!(s(2026, 3).next(), s(2027, 0));
        assert_eq!(s(2027, 0).previous(), s(2026, 3));
        assert_eq!(s(2026, 1).next(), s(2026, 2));
        assert_eq!(s(2026, 2).previous(), s(2026, 1));
        // Longer steps, both ways, and back to where we were.
        assert_eq!(s(2026, 3).shift(5), s(2028, 0));
        assert_eq!(s(2026, 3).shift(-8), s(2024, 3));
        assert_eq!(s(2026, 3).shift(-9), s(2024, 2));
        assert_eq!(s(2026, 3).shift(7).shift(-7), s(2026, 3));
        assert_eq!(s(2026, 3).shift(0), s(2026, 3));
        // The index counts the seasons; the order is by time.
        assert_eq!(s(2027, 0).index() - s(2026, 3).index(), 1);
        assert!(s(2026, 3) < s(2027, 0) && s(2026, 0) < s(2026, 1));
        // The ends don't wrap.
        assert_eq!(s(0, 0).previous(), s(0, 0));
        assert_eq!(s(65535, 3).next(), s(65535, 3));
        assert_eq!(Season::OLDEST.label(), "Winter 1963");
    }

    #[test]
    fn season_keys_and_lifetimes() {
        let cur = Season {
            year: 2026,
            quarter: 3,
        };
        assert_eq!(cur.key(), "2026/fall");
        assert_eq!(
            Season {
                year: 2027,
                quarter: 0
            }
            .key(),
            "2027/winter"
        );
        // The coming, current and previous seasons are asked for again twice a day.
        for se in [cur.next(), cur, cur.previous()] {
            assert_eq!(se.fresh_secs(cur), FRESH_SECS, "{}", se.key());
        }
        // Older ones keep for two weeks, right across the year roll-over too.
        for se in [cur.shift(-2), cur.shift(-8), Season::OLDEST] {
            assert_eq!(se.fresh_secs(cur), PAST_FRESH_SECS, "{}", se.key());
        }
        let winter = Season {
            year: 2027,
            quarter: 0,
        };
        assert_eq!(
            Season {
                year: 2026,
                quarter: 3
            }
            .fresh_secs(winter),
            FRESH_SECS,
            "fall is the previous season in winter"
        );
        assert_eq!(
            Season {
                year: 2026,
                quarter: 2
            }
            .fresh_secs(winter),
            PAST_FRESH_SECS
        );
        // Fresh until the lifetime ends; never from the future.
        let old = cur.shift(-4);
        assert!(old.is_fresh(cur, 1000, 1000 + PAST_FRESH_SECS - 1));
        assert!(!old.is_fresh(cur, 1000, 1000 + PAST_FRESH_SECS));
        assert!(cur.is_fresh(cur, 1000, 1000 + FRESH_SECS - 1));
        assert!(!cur.is_fresh(cur, 1000, 1000 + FRESH_SECS));
        assert!(
            !cur.is_fresh(cur, 2000, 1000),
            "saved in the future: ask again"
        );
        const {
            assert!(
                SEASON_MAX_AGE > PAST_FRESH_SECS,
                "a list is asked for again before it is forgotten"
            )
        };
    }

    #[test]
    fn merging_keeps_one_row_per_anime() {
        let a = |id, members: Option<u32>, air: &str| SeasonAnime { id, members, air_status: air.into(), title: format!("A{id}"), ..Default::default() };
        let cur = vec![a(1, Some(500), "currently_airing"), a(2, Some(900), "not_yet_aired")];
        let prev = vec![a(3, Some(700), "currently_airing"), a(4, Some(800), "finished_airing"), a(1, Some(500), "currently_airing")];
        let mut item = crate::store::tests::item(5, "Long Runner", Status::Watching, 300, 0);
        item.media.air_status = "currently_airing".into();
        let mut dropped = crate::store::tests::item(6, "Dropped Runner", Status::Dropped, 3, 0);
        dropped.media.air_status = "currently_airing".into();
        let on_season = crate::store::tests::item(3, "A3", Status::Watching, 2, 0);
        let m = merge(&cur, &prev, &[item, dropped, on_season]);
        assert_eq!(m.iter().map(|a| a.id).collect::<Vec<_>>(), [2, 3, 1, 5], "finished ones of last season and dropped entries left out");
        assert_eq!(m[3].mine, Some(("watching".into(), 300)));
    }

    #[test]
    fn countdowns_read_plainly() {
        assert_eq!(countdown(-5), "now");
        assert_eq!(countdown(30), "now");
        assert_eq!(countdown(8 * 60 + 59), "8 min");
        assert_eq!(countdown(5 * 3600 + 12 * 60), "5 h 12 min");
        assert_eq!(countdown(3 * 3600), "3 h");
        assert_eq!(countdown(2 * 86_400 + 4 * 3600 + 59), "2 d 4 h");
        assert_eq!(countdown(6 * 86_400 + 1800), "6 d");
    }
}
