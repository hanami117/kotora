//! Airing: MyAnimeList's seasonal lists and Kotora's own timetable (DESIGN.md §5.8).
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

    /// The show's timetable, from what the season list or the saved list knows.
    pub fn of(a: &SeasonAnime) -> Option<Timetable> {
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

/// An episode past what has aired by the estimate: Kotora asks before it is recorded
/// (DESIGN.md §5.8). Only the total episode count is a hard limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAired {
    pub episode: u32,
    /// Unix seconds of its estimated slot, when there is a timetable.
    pub expected: Option<i64>,
}

impl Timetable {
    /// The timetable of a list entry's anime (the list has no end date).
    pub fn of_media(m: &Media) -> Option<Timetable> {
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
        }
    }

    pub fn is_airing(&self) -> bool {
        self.air_status == "currently_airing"
    }
}

/// A season: year and `winter`, `spring`, `summer`, `fall`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Season {
    pub year: u16,
    /// 0 winter (Jan–Mar), 1 spring, 2 summer, 3 fall (Oct–Dec).
    pub quarter: u8,
}

const SEASONS: [&str; 4] = ["winter", "spring", "summer", "fall"];

impl Season {
    /// The season of `t` in Japan (where the seasons are counted).
    pub fn of(t: i64) -> Season {
        let c = Clock::at(t, JST);
        Season { year: c.year as u16, quarter: ((c.month - 1) / 3) as u8 }
    }

    pub fn previous(self) -> Season {
        if self.quarter == 0 { Season { year: self.year - 1, quarter: 3 } } else { Season { year: self.year, quarter: self.quarter - 1 } }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(date: &str, hh: i64, mm: i64, offset: i64) -> i64 {
        time::parse_date(date).unwrap() * 86_400 + hh * 3600 + mm * 60 - offset
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
