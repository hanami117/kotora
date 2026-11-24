//! Airing, Publishing and the notifications of both, for the phones: the desktop's
//! `app/src/airing.rs`, `app/src/publishing.rs` and `app/src/airnotes.rs`, without their views.
//! Everything shown is in words already (the desktop's wording), so Swift and Kotlin only draw.
//!
//! - **Airing**: [`Kotora::airing_page`] (local: the saved season lists and the list, merged as
//!   on the desktop) and [`Kotora::airing_refresh`] (network: the current season and the one
//!   before, from the active service, at the desktop's pace: [`airing::FRESH_SECS`]).
//! - **Publishing**: [`Kotora::publishing_page`] (local: the saved MangaUpdates / MangaDex
//!   files) and [`Kotora::publishing_refresh`] (network, only what is older than four hours unless
//!   forced), plus the panel's link and reset.
//! - **Notifications**: [`Kotora::notices_due`] (the desktop's `airnotes::pass`, local),
//!   the New chapter ones from [`Kotora::publishing_refresh`], and [`Kotora::upcoming_episodes`]
//!   (the estimated slots ahead, for a phone that can schedule a notification in advance). The
//!   switches are the desktop's `[notifications]` in `config.toml`.

use std::collections::HashMap;

use kotora_core::airing::{self, Season, SeasonAnime, Timetable, WEEK};
use kotora_core::airnotes::{self, What};
use kotora_core::config::{self, Config};
use kotora_core::mangaupdates::{self as mu, Cache, Confidence, Info, Key, ListRun, Matched, Pace, Source, Sources};
use kotora_core::service::ServiceError;
use kotora_core::store::Store;
use kotora_core::time::{self, Clock};
use kotora_core::{ListItem, Scope, Status, mangadex, paths};

use crate::{Kotora, KotoraError, ListStatus, LogLevel, MediaKind, Service, other};

/// Pages of 100 asked for a season (the desktop's `PAGES`: the 400 most followed).
const PAGES: u32 = 4;
const DAY: i64 = 86_400;
const DAY_SHORT: [&str; 7] = airing::DAY_SHORT;
const DAY_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// -------------------------------------------------------------------------------------- types

/// The Airing page's type filter.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiringType {
    All,
    Tv,
    Ona,
    Movie,
}

impl AiringType {
    fn keeps(self, format: &str) -> bool {
        match self {
            AiringType::All => true,
            AiringType::Tv => format == "tv",
            AiringType::Ona => format == "ona",
            AiringType::Movie => format == "movie",
        }
    }
}

/// The Airing page's list filter: Everything, On my list, Not on my list.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiringMine {
    All,
    On,
    Off,
}

impl AiringMine {
    fn keeps(self, on_list: bool) -> bool {
        match self {
            AiringMine::All => true,
            AiringMine::On => on_list,
            AiringMine::Off => !on_list,
        }
    }
}

/// The season grid's order: Most popular, Most to watch.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiringOrder {
    Popular,
    ToWatch,
}

/// One episode in the week.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringSlot {
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub cover_url: Option<String>,
    /// "TV", "ONA", … (empty when unknown).
    pub kind_label: String,
    pub episode: u32,
    /// Unix seconds (estimated, or AniList's exact time).
    pub at: i64,
    /// "16:30", local.
    pub time: String,
    /// The list status (not dropped), when on the list.
    pub status: Option<ListStatus>,
    /// "in 2 d 4 h" (on the list, still to come), "aired", or empty.
    pub note: String,
    /// Watching or On hold: "~3 to watch", "Caught up".
    pub to_watch: Option<String>,
}

/// One of the next seven days (0 today).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringDay {
    /// "Today · Friday Jan 15", "Tomorrow · …", "Sunday Jan 17".
    pub label: String,
    pub today: bool,
    /// "2 on your list", or empty.
    pub on_list: String,
    pub slots: Vec<AiringSlot>,
}

/// An airing show with no time to estimate.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringUntimed {
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub cover_url: Option<String>,
    /// "Saturdays in Japan, time not known", "No broadcast slot on MyAnimeList".
    pub why: String,
    pub status: Option<ListStatus>,
}

/// A card of the season grid.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringCard {
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub cover_url: Option<String>,
    /// "TV · 12 eps · ★ 8.12".
    pub meta: String,
    /// "Sat 16:30 · ep 7 in 2 d 4 h", "Starts Jan 9, 2027", "Fridays", "".
    pub when: String,
    pub status: Option<ListStatus>,
    /// On the list and the next episode is within two days: the line is lit.
    pub lit: bool,
    pub to_watch: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringPage {
    /// "Fall 2026".
    pub season: String,
    /// "MyAnimeList" or "AniList": where the season lists come from.
    pub service_title: String,
    /// When the current season's saved list was fetched; `None`: never.
    pub fetched_at: Option<i64>,
    /// Whether the saved season lists need asking again (old or missing).
    pub stale: bool,
    /// Any show at all (before the filters).
    pub has_shows: bool,
    pub days: Vec<AiringDay>,
    pub untimed: Vec<AiringUntimed>,
    pub cards: Vec<AiringCard>,
}

/// The Publishing page's order.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishingSort {
    /// Soonest estimated release first.
    Next,
    /// Most chapters to read first.
    Behind,
    /// Newest release first.
    Latest,
}

/// How a row's series was found.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishingLook {
    /// Not looked up yet.
    Pending,
    NotFound,
    /// Matched, no releases read yet.
    NoReleases,
    Sure,
    /// "Check match".
    Guess,
    /// The user's link.
    Picked,
}

/// One release in a row's panel.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PublishingRelease {
    /// "Ch 74".
    pub label: String,
    /// "3 days ago".
    pub when: String,
}

/// One Reading manga, in words.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PublishingRow {
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub cover_url: Option<String>,
    pub progress: u32,
    pub total: Option<u32>,
    /// "Ch 74", "Vol 9", "Read 80"; empty when unknown.
    pub latest: String,
    /// "1 day ago", "ahead of release data".
    pub when: String,
    pub behind: Option<u32>,
    /// "Weekly", "On hiatus", …
    pub pace: String,
    /// "next ~Mon", "due today", "late", "about every 10 days"; empty when nothing to say.
    pub next: String,
    pub look: PublishingLook,
    /// Out in the last two days and not read (lit).
    pub fresh: bool,
    pub hiatus: bool,
    pub finished: bool,
    pub ahead: bool,
    /// "MangaUpdates" or "MangaDex", the source of the numbers.
    pub source: Option<String>,
    /// The matched series ("Title on MangaUpdates") and its page.
    pub series_title: Option<String>,
    pub series_url: Option<String>,
    pub recent: Vec<PublishingRelease>,
    pub error: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PublishingPage {
    pub rows: Vec<PublishingRow>,
    /// The newest time releases were read.
    pub checked_at: Option<i64>,
    /// "Release data: MangaUpdates, MangaDex".
    pub credit: String,
    /// Something is older than four hours (or was never looked up).
    pub stale: bool,
}

/// What a Publishing refresh did, and the New chapter notifications it found.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PublishingReport {
    pub checked: u32,
    /// "A release source asked Kotora to slow down; …": the saved releases still show.
    pub error: Option<String>,
    pub notices: Vec<Notice>,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    StartedAiring,
    NewEpisode,
    NewChapter,
}

/// A notification to show, in the desktop's words.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Notice {
    /// "kotora-21-ep7": the same id for the same event (one replaces the other).
    pub id: String,
    pub kind: NoticeKind,
    pub media_kind: MediaKind,
    pub media_id: u32,
    pub title: String,
    pub body: String,
    /// When it is due (an upcoming episode's estimated slot), else now.
    pub at: i64,
}

/// Settings → Notifications (the desktop's `[notifications]`).
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifySettings {
    pub started_airing: bool,
    pub new_episode: bool,
    pub new_chapter: bool,
}

// ------------------------------------------------------------------------------------ helpers

fn kind_label(format: &str) -> &'static str {
    match format {
        "tv" => "TV",
        "movie" => "Movie",
        "ova" => "OVA",
        "ona" => "ONA",
        "special" => "Special",
        "tv_special" => "TV special",
        "music" => "Music",
        "cm" => "CM",
        "pv" => "PV",
        _ => "",
    }
}

/// "Jan 9, 2027", "Jan 2027".
fn date_text(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    let month = |m: &str| m.parse::<usize>().ok().filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    match p.as_slice() {
        [y, m, day] => match (month(m), day.parse::<u32>()) {
            (Some(m), Ok(day)) => format!("{m} {day}, {y}"),
            _ => d.to_string(),
        },
        [y, m] => month(m).map_or_else(|| d.to_string(), |m| format!("{m} {y}")),
        _ => d.to_string(),
    }
}

/// "1 day ago", "3 h ago", "just now".
fn ago(at: i64, now: i64) -> String {
    match time::age(at, now).as_str() {
        "now" => "just now".into(),
        a => format!("{a} ago"),
    }
}

/// A release day in words: "today", "~tomorrow", "~Mon" within the week, "~Dec 3" after.
fn day_words(day: i64, today: i64) -> String {
    let d = day.div_euclid(DAY);
    match d - today {
        i64::MIN..=0 => "today".into(),
        1 => "~tomorrow".into(),
        2..=6 => format!("~{}", DAY_SHORT[usize::from(time::weekday(d))]),
        _ => {
            let (_, m, date) = time::civil_from_days(d);
            format!("~{} {date}", MONTHS[(m as usize + 11) % 12])
        }
    }
}

/// Episodes out and not watched (the desktop's `State::to_watch_in`, without the mirror's copy).
fn to_watch_of(it: &ListItem, show: Option<&SeasonAnime>, now: i64) -> Option<airing::ToWatch> {
    if !(matches!(it.status, Status::Watching | Status::OnHold) || it.rewatching) {
        return None;
    }
    let m = &it.media;
    let next = m.next_airing.or_else(|| show.and_then(|s| s.next_airing));
    let estimate = Timetable::of_media(m).or_else(|| show.and_then(Timetable::of));
    let air = if m.air_status.is_empty() { show.map_or("", |s| s.air_status.as_str()) } else { m.air_status.as_str() };
    airing::to_watch(air, m.episodes.or_else(|| show.and_then(|s| s.episodes)), next, estimate, it.progress, now)
}

/// "Release data: MangaUpdates, MangaDex" (the sources turned on).
fn credit(on: Sources) -> String {
    match (on.mu, on.md) {
        (true, true) => "Release data: MangaUpdates, MangaDex".into(),
        (false, true) => "Release data: MangaDex".into(),
        _ => mu::CREDIT.into(),
    }
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::MangaDex => "MangaDex",
        _ => "MangaUpdates",
    }
}

fn load_config() -> Config {
    Config::load(&paths::config_file()).unwrap_or_default()
}

fn sources() -> Sources {
    let r = load_config().release_data;
    // At least one stays on (as the desktop's settings make sure).
    if !r.mangaupdates && !r.mangadex { Sources::default() } else { Sources { mu: r.mangaupdates, md: r.mangadex } }
}

/// The words of a network failure, as the desktop's `online::error_text`.
fn error_text(e: &ServiceError, service: Service) -> String {
    let title = service.id().title();
    match e {
        ServiceError::Offline(_) => format!("Can\u{2019}t reach {title}."),
        ServiceError::SignInAgain => format!("{title} needs you to sign in again."),
        ServiceError::RateLimited => format!("{title} asked Kotora to slow down. Try again in a minute."),
        ServiceError::Other(m) => m.clone(),
    }
}

/// A season list, at most [`PAGES`] pages.
fn season_pages(svc: &mut dyn kotora_core::service::ListService, season: Season) -> Result<Vec<SeasonAnime>, ServiceError> {
    let mut all = Vec::new();
    for page in 0..PAGES {
        let (mut v, more) = svc.season(season, page)?;
        all.append(&mut v);
        if !more {
            break;
        }
    }
    Ok(all)
}

fn row_of(it: &ListItem, info: Option<&Info>, on: Sources, now: i64, today: i64) -> (PublishingRow, i64, i64) {
    let m = &it.media;
    let read = it.progress;
    let mut r = PublishingRow {
        id: m.id,
        title: m.title.clone(),
        title_en: m.title_en.clone(),
        cover_url: m.cover_url.clone(),
        progress: read,
        total: m.episodes.filter(|n| *n > 0),
        latest: String::new(),
        when: String::new(),
        behind: None,
        pace: String::new(),
        next: String::new(),
        look: PublishingLook::Pending,
        fresh: false,
        hiatus: false,
        finished: false,
        ahead: false,
        source: None,
        series_title: None,
        series_url: None,
        recent: Vec::new(),
        error: None,
    };
    let (mut next_key, mut latest_key) = (i64::MAX, i64::MIN);
    let Some(info) = info else { return (r, next_key, latest_key) };
    r.error = info.error.clone();
    if !info.found(on) {
        r.look = if info.not_found(on) { PublishingLook::NotFound } else { PublishingLook::Pending };
        return (r, next_key, latest_key);
    }
    match (&info.matched, &info.md.id) {
        (Some(mm), _) if on.mu => {
            r.series_title = Some(mm.title.clone());
            r.series_url = Some(mu::series_url(mm.id));
        }
        (_, Some(id)) => {
            r.series_title = Some(info.md.title.clone());
            r.series_url = Some(mangadex::md_url(id));
        }
        _ => {}
    }
    let Some((s, used)) = info.best(now, ListRun::from_word(&m.air_status), on) else {
        r.look = PublishingLook::NoReleases;
        return (r, next_key, latest_key);
    };
    r.source = Some(source_name(used).into());
    let confidence = match (used, &info.matched) {
        (Source::MangaUpdates, Some(mm)) => mm.confidence,
        _ if info.md.picked => Confidence::Picked,
        _ => Confidence::Sure,
    };
    r.look = match confidence {
        Confidence::Sure => PublishingLook::Sure,
        Confidence::Guess => PublishingLook::Guess,
        Confidence::Picked => PublishingLook::Picked,
    };
    r.recent = s.recent.iter().take(8).map(|(label, at)| PublishingRelease { label: mu::chapter_label(label), when: ago(*at, now) }).collect();
    if s.latest_no.is_some_and(|n| f64::from(read) > n.floor()) {
        r.ahead = true;
        r.latest = format!("Read {read}");
        r.when = "ahead of release data".into();
        r.finished = s.run == mu::Run::Finished;
        r.hiatus = s.run == mu::Run::Hiatus;
        r.pace = match s.run {
            mu::Run::Finished => "Finished".into(),
            mu::Run::Hiatus => "On hiatus".into(),
            mu::Run::Ongoing => "Ongoing".into(),
        };
        if s.run == mu::Run::Ongoing {
            r.next = "release dates not tracked".into();
        }
        return (r, next_key, latest_key);
    }
    if let Some(l) = &s.latest {
        r.latest = mu::chapter_label(l);
    } else if let Some(v) = &s.volume_only {
        r.latest = format!("Vol {v}");
    }
    if let Some(at) = s.latest_at {
        r.when = ago(at, now);
        latest_key = at;
    } else if s.from_record {
        r.when = "date not tracked".into();
    }
    r.behind = mu::behind(s.latest_no, read);
    let new_at = if s.from_record { Some(s.went_up_at).filter(|t| *t > 0) } else { s.latest_at };
    r.fresh = r.behind.is_some_and(|b| b > 0) && new_at.is_some_and(|at| now - at < mu::NEW_FOR);
    let c = s.cadence;
    r.pace = c.pace.label();
    r.hiatus = c.pace == Pace::Hiatus;
    r.finished = c.pace == Pace::Finished;
    r.next = match (c.pace, c.next) {
        (Pace::Untracked, _) => "release dates not tracked".into(),
        (Pace::Hiatus | Pace::Unknown | Pace::Finished, _) => String::new(),
        (Pace::Irregular(d), _) => format!("about every {d} days"),
        (_, Some(n)) if n.div_euclid(DAY) <= today => "due today".into(),
        (_, Some(n)) => format!("next {}", day_words(n, today)),
        (_, None) if c.overdue => "late".into(),
        _ => String::new(),
    };
    next_key = c.sort_key();
    (r, next_key, latest_key)
}

// ------------------------------------------------------------------------------------- the API

impl Kotora {
    fn reading(&self, store: &Store, service: Service) -> Vec<ListItem> {
        store.load_list(Scope::new(service.name(), kotora_core::Kind::Manga)).unwrap_or_default().into_iter().filter(|it| it.status == Status::Watching).collect()
    }

    /// The Airing page at `now` (tests pass their own time).
    pub(crate) fn airing_page_at(&self, kind: AiringType, mine_filter: AiringMine, order: AiringOrder, now: i64) -> Result<AiringPage, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let name = service.name();
        let cur = Season::of(now);
        let prev = cur.previous();
        let c = store.season(name, cur)?;
        let p = store.season(name, prev)?;
        let stale = !c.as_ref().is_some_and(|(_, at)| cur.is_fresh(cur, *at, now)) || !p.as_ref().is_some_and(|(_, at)| prev.is_fresh(cur, *at, now));
        let fetched_at = c.as_ref().map(|(_, at)| *at);
        let list = store.load_list(name)?;
        drop(store);
        let shows = airing::merge(&c.map(|x| x.0).unwrap_or_default(), &p.map(|x| x.0).unwrap_or_default(), &list);
        let items: HashMap<u32, &ListItem> = list.iter().map(|it| (it.media.id, it)).collect();
        let by_id: HashMap<u32, &SeasonAnime> = shows.iter().map(|s| (s.id, s)).collect();

        let clock = |t: i64| Clock::local(t);
        let today = clock(now);
        let day0 = today.midnight();
        let week_end = day0 + WEEK + 3600;
        let day_from_today = |t: i64| clock(t).day - today.day;
        let mut days: [Vec<AiringSlot>; 7] = Default::default();
        let mut untimed = Vec::new();
        let mut cards: Vec<(AiringCard, i64)> = Vec::new();
        for show in &shows {
            let item = items.get(&show.id).copied().filter(|it| it.status != Status::Dropped);
            if !kind.keeps(&show.format) || !mine_filter.keeps(item.is_some()) {
                continue;
            }
            let status = item.map(|it| ListStatus::from(it.status));
            let behind = items.get(&show.id).and_then(|it| to_watch_of(it, by_id.get(&show.id).copied(), now));
            let tt = Timetable::of(show).map(|t| match item {
                Some(it) => t.with_progress(it.progress, now),
                None => t,
            });
            let next = tt.and_then(|t| t.next(now));
            match tt {
                Some(t) => {
                    if let Some((ep, at)) = t.slot_in(day0, week_end).filter(|(_, at)| (0..7).contains(&day_from_today(*at))) {
                        let note = if at <= now {
                            "aired".to_string()
                        } else if item.is_some() {
                            format!("in {}", airing::countdown(at - now))
                        } else {
                            String::new()
                        };
                        days[day_from_today(at) as usize].push(AiringSlot {
                            id: show.id,
                            title: show.title.clone(),
                            title_en: show.title_en.clone(),
                            cover_url: show.cover_url.clone(),
                            kind_label: kind_label(&show.format).into(),
                            episode: ep,
                            at,
                            time: clock(at).hhmm(),
                            status,
                            note,
                            to_watch: behind.map(|w| w.label()),
                        });
                    }
                }
                None if show.is_airing() => {
                    let why = match show.broadcast {
                        Some(b) => format!("{}s in Japan, time not known", DAY_LONG[usize::from(b.day % 7)]),
                        None => format!("No broadcast slot on {}", service.id().title()),
                    };
                    untimed.push(AiringUntimed { id: show.id, title: show.title.clone(), title_en: show.title_en.clone(), cover_url: show.cover_url.clone(), why, status });
                }
                None => {}
            }
            let mut meta = Vec::new();
            let k = kind_label(&show.format);
            if !k.is_empty() {
                meta.push(k.to_string());
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
                    let c = clock(at);
                    let slot = format!("{} {}", DAY_SHORT[usize::from(c.weekday)], c.hhmm());
                    if item.is_some() { format!("{slot} \u{b7} ep {ep} in {}", airing::countdown(at - now)) } else { format!("{slot} \u{b7} ep {ep}") }
                }
                (None, "not_yet_aired") => match show.start_date.as_deref().map(date_text) {
                    Some(d) => format!("Starts {d}"),
                    None => "Not yet aired".into(),
                },
                (None, "finished_airing") => "Finished airing".into(),
                (None, _) => match show.broadcast {
                    Some(b) => format!("{}s", DAY_LONG[usize::from(b.day % 7)]),
                    None => String::new(),
                },
            };
            let lit = item.is_some() && next.is_some_and(|(_, at)| at - now < 2 * DAY);
            let key = behind.map_or(-1, |w| i64::from(w.behind));
            cards.push((AiringCard { id: show.id, title: show.title.clone(), title_en: show.title_en.clone(), cover_url: show.cover_url.clone(), meta: meta.join(" \u{b7} "), when, status, lit, to_watch: behind.map(|w| w.label()) }, key));
        }
        if order == AiringOrder::ToWatch {
            cards.sort_by_key(|(_, k)| std::cmp::Reverse(*k));
        }
        let days = days
            .into_iter()
            .enumerate()
            .map(|(d, mut slots)| {
                slots.sort_by_key(|s| (s.at, s.status.is_none()));
                let c = clock(day0 + d as i64 * DAY + 12 * 3600);
                let name = format!("{} {} {}", DAY_LONG[usize::from(c.weekday)], MONTHS[(c.month as usize + 11) % 12], c.date);
                let label = match d {
                    0 => format!("Today \u{b7} {name}"),
                    1 => format!("Tomorrow \u{b7} {name}"),
                    _ => name,
                };
                let n = slots.iter().filter(|s| s.status.is_some()).count();
                AiringDay { label, today: d == 0, on_list: if n > 0 { format!("{n} on your list") } else { String::new() }, slots }
            })
            .collect();
        Ok(AiringPage { season: cur.label(), service_title: service.id().title().into(), fetched_at, stale, has_shows: !shows.is_empty(), days, untimed, cards: cards.into_iter().map(|c| c.0).collect() })
    }

    /// New Started airing and New episode notifications at `now` (each once).
    pub(crate) fn notices_due_at(&self, now: i64) -> Result<Vec<Notice>, KotoraError> {
        let service = self.active_or_err()?;
        let n = load_config().notifications;
        let mut store = self.own_store()?;
        let due = airnotes::pass(&mut store, service.name(), now)?;
        let list = store.load_list(service.name())?;
        let mut out = Vec::new();
        for d in due {
            let Some(it) = list.iter().find(|it| it.media.id == d.media_id) else { continue };
            let title = &it.media.title;
            match d.what {
                What::Started if n.started_airing && it.status == Status::PlanToWatch => out.push(Notice {
                    id: format!("kotora-{}-started", it.media.id),
                    kind: NoticeKind::StartedAiring,
                    media_kind: MediaKind::Anime,
                    media_id: it.media.id,
                    title: format!("{title} started airing"),
                    body: "It\u{2019}s on your Plan to watch list.".into(),
                    at: now,
                }),
                What::Episode(e) if n.new_episode && it.status == Status::Watching && it.progress < e => out.push(episode_notice(it, e, now)),
                _ => {}
            }
        }
        Ok(out)
    }

    /// The next estimated episode of each Watching show, within `within` seconds of `now`.
    pub(crate) fn upcoming_at(&self, within: i64, now: i64) -> Result<Vec<Notice>, KotoraError> {
        let service = self.active_or_err()?;
        if !load_config().notifications.new_episode {
            return Ok(Vec::new());
        }
        let list = self.store().load_list(service.name())?;
        let mut out: Vec<Notice> = list
            .iter()
            .filter(|it| it.status == Status::Watching && it.media.is_airing())
            .filter_map(|it| {
                let tt = Timetable::of_media(&it.media)?.with_progress(it.progress, now);
                let (ep, at) = tt.next(now)?;
                (ep > it.progress && at - now <= within).then(|| episode_notice(it, ep, at))
            })
            .collect();
        out.sort_by_key(|n| n.at);
        Ok(out)
    }

    pub(crate) fn publishing_page_at(&self, sort: PublishingSort, now: i64) -> Result<PublishingPage, KotoraError> {
        let service = self.active_or_err()?;
        let on = sources();
        let (mu_cache, md_cache) = (Cache::default_dir(), Cache::mangadex_dir());
        let store = self.store();
        let items = self.reading(&store, service);
        let today = Clock::local(now).day;
        let mut stale = false;
        let mut checked_at: Option<i64> = None;
        let mut rows: Vec<(PublishingRow, i64, i64)> = items
            .iter()
            .map(|it| {
                let key = Key { service: service.id(), media_id: it.media.id };
                let info = mu::saved_all(&mu_cache, &md_cache, &store, key, on);
                stale |= mu::stale_all(&mu_cache, &md_cache, &store, key, on, now);
                checked_at = checked_at.max(info.checked_at());
                row_of(it, Some(&info), on, now, today)
            })
            .collect();
        drop(store);
        let title = |r: &PublishingRow| r.title.to_lowercase();
        match sort {
            PublishingSort::Next => rows.sort_by_cached_key(|(r, n, _)| (*n, title(r))),
            PublishingSort::Behind => rows.sort_by_cached_key(|(r, n, _)| (std::cmp::Reverse(r.behind.map_or(-1, i64::from)), *n, title(r))),
            PublishingSort::Latest => rows.sort_by_cached_key(|(r, _, l)| (std::cmp::Reverse(*l), title(r))),
        }
        Ok(PublishingPage { rows: rows.into_iter().map(|r| r.0).collect(), checked_at, credit: credit(on), stale })
    }
}

fn episode_notice(it: &ListItem, ep: u32, at: i64) -> Notice {
    Notice {
        // The desktop's id: one notification per episode.
        id: format!("kotora-{}-ep{ep}", it.media.id),
        kind: NoticeKind::NewEpisode,
        media_kind: MediaKind::Anime,
        media_id: it.media.id,
        title: format!("{} \u{b7} Ep {ep} is out", it.media.title),
        body: format!("Estimated from the broadcast time. You\u{2019}re at episode {}.", it.progress),
        at,
    }
}

#[uniffi::export]
impl Kotora {
    /// The Airing page from the saved season lists and the list. Local: no network.
    pub fn airing_page(&self, kind: AiringType, mine: AiringMine, order: AiringOrder) -> Result<AiringPage, KotoraError> {
        self.airing_page_at(kind, mine, order, time::now_unix())
    }

    /// Asks the active service for the current season and the one before, when their saved
    /// lists are old (or always, `force`). Network: call it off the main thread.
    pub fn airing_refresh(&self, force: bool) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let now = time::now_unix();
        let cur = Season::of(now);
        let prev = cur.previous();
        let store = self.own_store()?;
        let mut svc = self.service(service)?;
        for (se, keep) in [(cur, prev), (prev, cur)] {
            let fresh = store.season(service.name(), se)?.is_some_and(|(_, at)| se.is_fresh(cur, at, now));
            if fresh && !force {
                continue;
            }
            match season_pages(svc.as_mut(), se) {
                Ok(all) => store.save_season(service.name(), se, &all, now, &[keep])?,
                Err(e) => {
                    let text = error_text(&e, service);
                    return Err(match e {
                        ServiceError::Offline(_) => KotoraError::Offline { message: text },
                        ServiceError::SignInAgain => KotoraError::SignInAgain,
                        ServiceError::RateLimited => KotoraError::RateLimited,
                        ServiceError::Other(_) => KotoraError::Other { message: text },
                    });
                }
            }
        }
        Ok(())
    }

    /// Started airing and New episode notifications due now, each returned once (whatever the
    /// switches say, as on the desktop: a switch turned on later never brings old ones back).
    /// Local: looks at the saved list (sync first for news of a start).
    pub fn notices_due(&self) -> Result<Vec<Notice>, KotoraError> {
        self.notices_due_at(time::now_unix())
    }

    /// The next estimated episode of each Watching show airing within `within_secs`, for a
    /// notification scheduled at its time. Empty when New episode is off.
    pub fn upcoming_episodes(&self, within_secs: i64) -> Result<Vec<Notice>, KotoraError> {
        self.upcoming_at(within_secs, time::now_unix())
    }

    pub fn notify_settings(&self) -> NotifySettings {
        let n = load_config().notifications;
        NotifySettings { started_airing: n.started_airing, new_episode: n.new_episode, new_chapter: n.new_chapter }
    }

    /// Saves one switch of `[notifications]` in `config.toml`.
    pub fn set_notify(&self, kind: NoticeKind, on: bool) -> Result<(), KotoraError> {
        let key = match kind {
            NoticeKind::StartedAiring => "started_airing",
            NoticeKind::NewEpisode => "new_episode",
            NoticeKind::NewChapter => "new_chapter",
        };
        config::write_setting(&paths::config_file(), "notifications", key, &on.to_string()).map_err(|e| other(format!("Can\u{2019}t save the setting: {e}")))
    }

    /// The Publishing page (Reading manga) from the saved release files. Local: no network.
    pub fn publishing_page(&self, sort: PublishingSort) -> Result<PublishingPage, KotoraError> {
        self.publishing_page_at(sort, time::now_unix())
    }

    /// Asks MangaUpdates (and MangaDex) for the Reading manga whose saved releases are older than
    /// four hours (all, `force`), then finds the New chapter notifications (under the switch).
    /// Network, at the sources' pace: call it off the main thread.
    pub fn publishing_refresh(&self, force: bool) -> Result<PublishingReport, KotoraError> {
        let service = self.active_or_err()?;
        let on = sources();
        let (mu_cache, md_cache) = (Cache::default_dir(), Cache::mangadex_dir());
        let (mut mu_client, mut md_client) = (mu::Client::online(), mangadex::Client::online());
        let store = self.own_store()?;
        let items = self.reading(&store, service);
        let key = |it: &ListItem| Key { service: service.id(), media_id: it.media.id };
        let mut infos: HashMap<u32, Info> = HashMap::new();
        let mut error = None;
        let mut checked = 0;
        for it in &items {
            let now = time::now_unix();
            if !force && !mu::stale_all(&mu_cache, &md_cache, &store, key(it), on, now) {
                infos.insert(it.media.id, mu::saved_all(&mu_cache, &md_cache, &store, key(it), on));
                continue;
            }
            match mu::fetch_all((&mut mu_client, &mu_cache), (&mut md_client, &md_cache), &store, key(it), &it.media, on, now, force) {
                Ok(info) => {
                    checked += 1;
                    infos.insert(it.media.id, info);
                }
                Err(e @ (ServiceError::Offline(_) | ServiceError::RateLimited)) => {
                    error = Some(match e {
                        ServiceError::RateLimited => "A release source asked Kotora to slow down; the rest comes on the next refresh.".to_string(),
                        ServiceError::Offline(m) => format!("{}.", m.split(':').next().unwrap_or("Can\u{2019}t reach the release sources").replace("can\u{2019}t", "Can\u{2019}t")),
                        _ => String::new(),
                    });
                    break;
                }
                Err(e) => self.log(LogLevel::Warn, format!("publishing: {}: {e:?}", it.media.title)),
            }
        }
        // Announced once each, whatever the switch says (as on the desktop); offline nothing.
        let notify = load_config().notifications.new_chapter;
        let mut notices = Vec::new();
        if error.is_none() {
            let now = time::now_unix();
            for it in &items {
                let Some((s, _)) = infos.get(&it.media.id).and_then(|i| i.best(now, ListRun::from_word(&it.media.air_status), on)) else { continue };
                let Some(n) = mu::new_chapter(&s, it.progress, now) else { continue };
                if !store.announce_once(service.name(), it.media.id, What::Chapter(n), now).unwrap_or(false) || !notify {
                    continue;
                }
                let label = s.latest.as_deref().map_or_else(|| format!("Ch {n}"), mu::chapter_label);
                let behind = n - it.progress;
                let c = credit(on);
                notices.push(Notice {
                    id: format!("kotora-m{}-ch{n}", it.media.id),
                    kind: NoticeKind::NewChapter,
                    media_kind: MediaKind::Manga,
                    media_id: it.media.id,
                    title: format!("New chapter of {}: {label}", it.media.title),
                    body: if behind > 1 { format!("{behind} chapters to read. {c}.") } else { format!("{c}.") },
                    at: now,
                });
            }
        }
        Ok(PublishingReport { checked, error, notices })
    }

    /// The panel's link: a mangaupdates.com/series/… or mangadex.org/title/… address picked for
    /// this manga (kept until Reset), then its releases. Network.
    pub fn publishing_link(&self, media_id: u32, link: String) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let on = sources();
        let store = self.own_store()?;
        let key = Key { service: service.id(), media_id };
        let item = store.load_item(Scope::new(service.name(), kotora_core::Kind::Manga), media_id)?.ok_or(KotoraError::NotFound)?;
        let md_link = link.to_ascii_lowercase().contains("mangadex");
        if let Some(id) = md_link.then(|| mangadex::md_id_from_link(&link)).flatten() {
            if !on.md {
                return Err(other("MangaDex is turned off in Settings \u{2192} Manga \u{2192} Release data."));
            }
            let (id, title) = mangadex::Client::online().title_of(&id).map_err(|e| match e {
                ServiceError::Other(m) if m.contains("no such") => other("MangaDex has no series with that link."),
                e => e.into(),
            })?;
            mu::set_md_picked(&store, key, Some((&id, &title)))?;
        } else if let Some(series) = mu::series_id_from_link(&link) {
            if !on.mu {
                return Err(other("MangaUpdates is turned off in Settings \u{2192} Manga \u{2192} Release data."));
            }
            let (id, title, _) = mu::Client::online().series(series).map_err(|e| match e {
                ServiceError::Other(m) if m.contains("no such") => other("MangaUpdates has no series with that link."),
                e => e.into(),
            })?;
            mu::set_picked(&store, key, Some(&Matched { id, title, confidence: Confidence::Picked }))?;
        } else {
            return Err(other("That isn\u{2019}t a series link Kotora knows. Open the series on mangaupdates.com or mangadex.org and copy the address (mangaupdates.com/series/\u{2026} or mangadex.org/title/\u{2026})."));
        }
        mu::fetch_all((&mut mu::Client::online(), &Cache::default_dir()), (&mut mangadex::Client::online(), &Cache::mangadex_dir()), &store, key, &item.media, on, time::now_unix(), true)?;
        Ok(())
    }

    /// The panel's Reset: back to the title search (MangaUpdates) and the id link (MangaDex).
    pub fn publishing_reset(&self, media_id: u32) -> Result<(), KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let key = Key { service: service.id(), media_id };
        mu::set_picked(&store, key, None)?;
        mu::set_md_picked(&store, key, None)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{ClientIds, Paths, Platform};
    use kotora_core::airing::Broadcast;
    use kotora_core::Media;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Fake(Mutex<HashMap<String, Vec<u8>>>);

    impl Platform for Fake {
        fn utc_offset(&self, _at_unix: i64) -> i32 {
            crate::testenv::OFFSET
        }
        fn log(&self, _level: LogLevel, _message: String) {}
        fn token_load(&self, service: String) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(&service).cloned()
        }
        fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
            self.0.lock().unwrap().insert(service, bytes);
            true
        }
        fn token_delete(&self, service: String) {
            self.0.lock().unwrap().remove(&service);
        }
    }

    /// A core signed in to MyAnimeList (a made-up token) in a scratch folder. The folders are
    /// the process's (the first `Kotora::new`), so the store is per test, `config.toml` shared.
    pub(crate) fn signed_in_mal(name: &str) -> (Arc<Kotora>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kotora-mobile-pages-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths { config_dir: dir.join("files").display().to_string(), data_dir: dir.join("nobackup").display().to_string(), cache_dir: dir.join("cache").display().to_string() };
        let k = Kotora::new(paths, ClientIds { mal: "fake".into(), anilist: "fake".into() }, Arc::new(Fake::default())).unwrap();
        let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
        k.tokens.save("mal", &t).unwrap();
        k.store().set_account("mal", "made-up-user").unwrap();
        (k, dir)
    }

    pub(crate) fn fixture(id: u32, title: &str, status: Status, progress: u32) -> ListItem {
        let media = Media { kind: kotora_core::Kind::Anime, id, title: title.into(), title_en: None, title_ja: None, synonyms: Vec::new(), episodes: None, format: "manga".into(), air_status: String::new(), cover_url: None, start_date: None, broadcast: None, volumes: None, authors: Vec::new(), next_airing: None, id_mal: None };
        ListItem { media, status, progress, score: None, rewatching: false, times_rewatched: 0, volumes: 0, updated_at: 1 }
    }

    /// Saturday 2026-10-03 23:30 JST = 14:30 UTC (the core's airnotes test).
    pub(crate) const EP1: i64 = 1_791_037_800;

    pub(crate) fn show(id: u32, title: &str, status: Status, progress: u32) -> ListItem {
        let mut it = fixture(id, title, status, progress);
        it.media.air_status = "currently_airing".into();
        it.media.format = "tv".into();
        it.media.episodes = Some(12);
        it.media.start_date = Some("2026-10-03".into());
        it.media.broadcast = Broadcast::parse("saturday", Some("23:30"));
        it
    }

    #[test]
    fn the_week_lists_the_list_s_airing_shows_with_their_estimated_slot() {
        let (k, dir) = signed_in_mal("airing-week");
        k.store().replace_list("mal", &[show(1, "Hoshikuzu Relay", Status::Watching, 4)], EP1).unwrap();
        // Friday before episode 6 (episode 5 aired last Saturday) (the zone is JST in the fake platform).
        let now = EP1 + 4 * WEEK + 6 * 86_400 - 3600 * 10;
        let page = k.airing_page_at(AiringType::All, AiringMine::All, AiringOrder::Popular, now).unwrap();
        assert_eq!(page.service_title, "MyAnimeList");
        assert!(page.stale, "no season saved yet");
        assert!(page.days[0].label.starts_with("Today \u{b7} Friday"));
        let tomorrow = &page.days[1];
        assert_eq!(tomorrow.slots.len(), 1);
        let s = &tomorrow.slots[0];
        assert_eq!((s.episode, s.time.as_str(), s.status), (6, "23:30", Some(ListStatus::Watching)));
        assert!(s.note.starts_with("in "), "{}", s.note);
        assert_eq!(s.to_watch.as_deref(), Some("~1 to watch"), "5 out by the estimate, 4 watched");
        assert_eq!(tomorrow.on_list, "1 on your list");
        assert_eq!(page.cards.len(), 1);
        assert_eq!(page.cards[0].meta, "TV \u{b7} 12 eps");
        // Not on my list: nothing.
        let none = k.airing_page_at(AiringType::All, AiringMine::Off, AiringOrder::Popular, now).unwrap();
        assert!(none.days.iter().all(|d| d.slots.is_empty()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn notices_follow_the_desktop_rules_and_switches() {
        let (k, dir) = signed_in_mal("airing-notes");
        let _config = crate::testenv::fresh_config();
        let mut plan = show(2, "Tide Line", Status::PlanToWatch, 0);
        plan.media.air_status = "not_yet_aired".into();
        k.store().replace_list("mal", &[show(1, "Hoshikuzu Relay", Status::Watching, 5), plan.clone()], EP1).unwrap();
        // First look: remembers the statuses, no start.
        let ep6 = EP1 + 5 * WEEK;
        assert!(k.notices_due_at(ep6 - 3600).unwrap().is_empty());
        // The show starts; episode 6 is out.
        plan.media.air_status = "currently_airing".into();
        k.store().replace_list("mal", &[show(1, "Hoshikuzu Relay", Status::Watching, 5), plan], EP1).unwrap();
        let due = k.notices_due_at(ep6 + 60).unwrap();
        let ids: Vec<&str> = due.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["kotora-1-ep6", "kotora-2-started"]);
        assert_eq!(due[1].title, "Tide Line started airing");
        assert_eq!(due[0].title, "Hoshikuzu Relay \u{b7} Ep 6 is out");
        assert!(k.notices_due_at(ep6 + 120).unwrap().is_empty(), "once each");
        // Upcoming: episode 7, a week on; nothing with New episode off.
        let up = k.upcoming_at(8 * 86_400, ep6 + 60).unwrap();
        assert_eq!((up[0].id.as_str(), up[0].at), ("kotora-1-ep7", ep6 + WEEK));
        assert!(k.notify_settings().new_episode && !k.notify_settings().new_chapter);
        k.set_notify(NoticeKind::NewEpisode, false).unwrap();
        assert!(!k.notify_settings().new_episode);
        assert!(k.upcoming_at(8 * 86_400, ep6 + 60).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn publishing_lists_reading_manga_waiting_for_their_lookup() {
        let (k, dir) = signed_in_mal("publishing");
        let mut m = fixture(9, "Bone Garden", Status::Watching, 40);
        m.media.kind = kotora_core::Kind::Manga;
        let mut done = fixture(10, "Old One", Status::Completed, 10);
        done.media.kind = kotora_core::Kind::Manga;
        k.store().replace_list(Scope::new("mal", kotora_core::Kind::Manga), &[m, done], EP1).unwrap();
        let page = k.publishing_page_at(PublishingSort::Next, EP1).unwrap();
        assert_eq!(page.rows.len(), 1, "Reading only");
        assert_eq!((page.rows[0].look, page.rows[0].latest.as_str()), (PublishingLook::Pending, ""));
        assert!(page.stale);
        assert_eq!(page.credit, "Release data: MangaUpdates, MangaDex");
        let _ = std::fs::remove_dir_all(dir);
    }
}
