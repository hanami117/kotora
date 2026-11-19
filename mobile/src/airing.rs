//! The Airing page's week and the airing and chapter notifications, as on the desktop
//! (`app/src/airing.rs`, `app/src/airnotes.rs`, `app/src/publishing.rs`).
//!
//! - **Week:** [`Kotora::airing_fetch`] asks the active service for the current season and the
//!   one before (pages of 100, at most [`PAGES`] each) when the saved copies are older than
//!   [`airing::FRESH_SECS`], and saves them in the store's `season` table. [`Kotora::airing_week`]
//!   then derives the next 7 days from the saved copies and the list, locally: one slot per show,
//!   estimated by the core's [`Timetable`], with the desktop's texts.
//! - **Notifications:** [`Kotora::air_notes`] is one look of core [`airnotes::pass`] (database only,
//!   no request): what is due, each marked announced whatever the switches say, then kept only
//!   under its switch and the entry's status, with the desktop's words. [`Kotora::chapter_notes`] is
//!   the Publishing worker's pass: MangaUpdates and MangaDex for the Reading manga whose saved
//!   releases are stale, then the new chapters. The switches are kept in the store, with the
//!   desktop's defaults (Started airing and New episode on, New chapter off).

use std::collections::{HashMap, HashSet};

use kotora_core::airing::{self, Season, SeasonAnime, Timetable, WEEK};
use kotora_core::airnotes::{self, What};
use kotora_core::mangaupdates::{self as mu, Cache, Key, ListRun, Sources};
use kotora_core::service::ServiceError;
use kotora_core::store::Store;
use kotora_core::time::Clock;
use kotora_core::{Kind, ListItem, Scope, Status, mangadex, time};

use crate::{Kotora, KotoraError, ListStatus, MediaKind, Service};

/// Pages of 100 asked for a season (by members: the 400 most followed), as on the desktop.
pub const PAGES: u32 = 4;

const DAY_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const NOTIFY_STARTED: &str = "mobile.notify.started_airing";
const NOTIFY_EPISODE: &str = "mobile.notify.new_episode";
const NOTIFY_CHAPTER: &str = "mobile.notify.new_chapter";

// ------------------------------------------------------------------------------------- types

/// The type filter (the desktop's chips: All types, TV, ONA, Movie).
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
            AiringType::Tv => matches!(format, "tv" | "tv_special"),
            AiringType::Ona => format == "ona",
            AiringType::Movie => format == "movie",
        }
    }
}

/// The list filter (Everything, On my list, Not on my list).
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

/// One episode in the week.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringSlot {
    pub id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    /// "TV", "ONA", … or empty.
    pub format_label: String,
    pub episode: u32,
    /// Unix seconds (estimated).
    pub at: i64,
    /// "16:30", local.
    pub time: String,
    /// On the user's list (not dropped).
    pub mine: bool,
    /// The list status, when on the list (dropped included).
    pub status: Option<ListStatus>,
    /// "in 2 d 4 h" (on the list, still to come), "aired", or empty.
    pub note: String,
    /// Watching or On hold: "~3 to watch", "Caught up".
    pub to_watch: Option<String>,
}

/// One of the 7 days, today first.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringDay {
    /// "Today · Friday Jan 15", "Tomorrow · Saturday Jan 16", "Sunday Jan 17".
    pub label: String,
    pub today: bool,
    /// How many of its slots are on the list.
    pub on_list: u32,
    pub slots: Vec<AiringSlot>,
}

/// An airing show with no time to estimate.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringUntimed {
    pub id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    /// "Saturdays in Japan, time not known", "No broadcast slot on MyAnimeList".
    pub why: String,
    pub status: Option<ListStatus>,
}

/// The week as the page shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiringWeek {
    /// "Fall 2026".
    pub season: String,
    /// The service the seasons come from ("MyAnimeList").
    pub service_title: String,
    /// When the current season's saved copy was fetched; `None`: never.
    pub fetched_at: Option<i64>,
    /// Shows in the saved copies (before the filters). 0: nothing to show yet.
    pub shows: u32,
    /// The saved copies are missing or old: [`Kotora::airing_fetch`] would ask.
    pub stale: bool,
    pub days: Vec<AiringDay>,
    pub untimed: Vec<AiringUntimed>,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// A Plan to watch show started airing.
    Started,
    /// The next episode of a Watching show is out (estimated).
    Episode,
    /// A new chapter of a Reading manga.
    Chapter,
}

/// A notification to show.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Note {
    /// The desktop's id ("kotora-123-ep7"): the same one replaces the other.
    pub id: String,
    pub kind: NoteKind,
    pub media_kind: MediaKind,
    pub media_id: u32,
    pub summary: String,
    pub body: String,
}

/// Settings → Notifications (the desktop's `[notifications]` switches of this kind).
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifySettings {
    pub started_airing: bool,
    pub new_episode: bool,
    /// Off by default: the one switch that makes Kotora ask a third service in the background.
    pub new_chapter: bool,
}

impl Default for NotifySettings {
    fn default() -> Self {
        NotifySettings { started_airing: true, new_episode: true, new_chapter: false }
    }
}

// ------------------------------------------------------------------------------------- texts

pub(crate) fn kind_label(format: &str) -> &'static str {
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

/// The desktop's `online::error_text`.
fn error_text(service: Service, e: ServiceError) -> String {
    let title = service.id().title();
    match e {
        ServiceError::Offline(_) => format!("Can\u{2019}t reach {title}."),
        ServiceError::SignInAgain => format!("{title} needs you to sign in again."),
        ServiceError::RateLimited => format!("{title} asked Kotora to slow down. Try again in a minute."),
        ServiceError::Other(m) => m,
    }
}

/// Episodes out and not watched of a list entry (the desktop's `State::to_watch_in`).
fn to_watch(it: &ListItem, show: Option<&SeasonAnime>, now: i64) -> Option<airing::ToWatch> {
    if !(matches!(it.status, Status::Watching | Status::OnHold) || it.rewatching) {
        return None;
    }
    let m = &it.media;
    let next = m.next_airing.or_else(|| show.and_then(|s| s.next_airing));
    let estimate = Timetable::of_media(m).or_else(|| show.and_then(Timetable::of));
    let air = if m.air_status.is_empty() { show.map_or("", |s| s.air_status.as_str()) } else { m.air_status.as_str() };
    airing::to_watch(air, m.episodes.or_else(|| show.and_then(|s| s.episodes)), next, estimate, it.progress, now)
}

/// The week from the merged shows and the list at `now`, in the zone of `clock` (the user's; a
/// fixed offset in tests). The desktop's `State::airing_rebuild`, week part.
pub(crate) fn week(shows: &[SeasonAnime], list: &[ListItem], service: Service, kind: AiringType, mine_f: AiringMine, now: i64, clock: &dyn Fn(i64) -> Clock) -> (Vec<AiringDay>, Vec<AiringUntimed>) {
    let by_id: HashMap<u32, &ListItem> = list.iter().map(|it| (it.media.id, it)).collect();
    let today = clock(now);
    let day0 = today.midnight();
    // A day more than a week: a DST change in between still ends at a midnight below.
    let week_end = day0 + WEEK + 3600;
    let from_today = |t: i64| clock(t).day - today.day;
    let mut days: Vec<Vec<AiringSlot>> = vec![Vec::new(); 7];
    let mut untimed = Vec::new();
    for show in shows {
        let it = by_id.get(&show.id).copied();
        let entry = it.filter(|it| it.status != Status::Dropped);
        if !kind.keeps(&show.format) || !mine_f.keeps(entry.is_some()) {
            continue;
        }
        let status = it.map(|it| ListStatus::from(it.status));
        let tt = Timetable::of(show).map(|t| match entry {
            Some(it) => t.with_progress(it.progress, now),
            None => t,
        });
        match tt {
            Some(t) => {
                if let Some((ep, at)) = t.slot_in(day0, week_end).filter(|(_, at)| (0..7).contains(&from_today(*at))) {
                    let note = if at <= now {
                        "aired".to_string()
                    } else if entry.is_some() {
                        format!("in {}", airing::countdown(at - now))
                    } else {
                        String::new()
                    };
                    let k = from_today(at) as usize;
                    days[k].push(AiringSlot {
                        id: show.id,
                        title: show.title.clone(),
                        cover_url: show.cover_url.clone(),
                        format_label: kind_label(&show.format).into(),
                        episode: ep,
                        at,
                        time: clock(at).hhmm(),
                        mine: entry.is_some(),
                        status,
                        note,
                        to_watch: entry.and_then(|it| to_watch(it, Some(show), now)).map(|w| w.label()),
                    });
                }
            }
            None if show.is_airing() => {
                let why = match show.broadcast {
                    Some(b) => format!("{}s in Japan, time not known", DAY_LONG[usize::from(b.day % 7)]),
                    None => format!("No broadcast slot on {}", service.id().title()),
                };
                untimed.push(AiringUntimed { id: show.id, title: show.title.clone(), cover_url: show.cover_url.clone(), why, status });
            }
            None => {}
        }
    }
    let out = days
        .into_iter()
        .enumerate()
        .map(|(d, mut slots)| {
            slots.sort_by_key(|s| (s.at, !s.mine));
            let c = clock(day0 + d as i64 * 86_400 + 12 * 3600);
            let name = format!("{} {} {}", DAY_LONG[usize::from(c.weekday)], MONTHS[(c.month.max(1) - 1) as usize % 12], c.date);
            let label = match d {
                0 => format!("Today \u{b7} {name}"),
                1 => format!("Tomorrow \u{b7} {name}"),
                _ => name,
            };
            AiringDay { label, today: d == 0, on_list: slots.iter().filter(|s| s.mine).count() as u32, slots }
        })
        .collect();
    (out, untimed)
}

/// Asks for the pages of `season`.
fn pages(svc: &mut dyn kotora_core::service::ListService, season: Season) -> Result<Vec<SeasonAnime>, ServiceError> {
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

/// The words of what is due (the desktop's `State::air_notes`): each under its switch, and only
/// while the entry is still Plan to watch (Started) or Watching and behind (Episode).
pub(crate) fn air_note(it: &ListItem, what: What, on: NotifySettings) -> Option<Note> {
    let title = it.media.title.clone();
    let id = it.media.id;
    match what {
        What::Started if on.started_airing && it.status == Status::PlanToWatch => Some(Note {
            id: format!("kotora-{id}-started"),
            kind: NoteKind::Started,
            media_kind: MediaKind::Anime,
            media_id: id,
            summary: format!("{title} started airing"),
            body: "It\u{2019}s on your Plan to watch list.".into(),
        }),
        What::Episode(n) if on.new_episode && it.status == Status::Watching && it.progress < n => Some(Note {
            id: format!("kotora-{id}-ep{n}"),
            kind: NoteKind::Episode,
            media_kind: MediaKind::Anime,
            media_id: id,
            summary: format!("{title} \u{b7} Ep {n} is out"),
            body: format!("Estimated from the broadcast time. You\u{2019}re at episode {}.", it.progress),
        }),
        _ => None,
    }
}

/// The publishing page's credit line, both sources on (the phone has no source settings yet).
fn credit(on: Sources) -> String {
    match (on.mu, on.md) {
        (true, true) => "Release data: MangaUpdates, MangaDex".into(),
        (false, true) => "Release data: MangaDex".into(),
        _ => mu::CREDIT.into(),
    }
}

/// A New chapter notification (the desktop's `State::publishing_notes`).
pub(crate) fn chapter_note(it: &ListItem, n: u32, label: &str, on: Sources) -> Option<Note> {
    if it.status != Status::Watching || it.progress >= n {
        return None;
    }
    let behind = n - it.progress;
    let credit = credit(on);
    let body = if behind > 1 { format!("{behind} chapters to read. {credit}.") } else { format!("{credit}.") };
    let id = it.media.id;
    Some(Note { id: format!("kotora-m{id}-ch{n}"), kind: NoteKind::Chapter, media_kind: MediaKind::Manga, media_id: id, summary: format!("New chapter of {}: {label}", it.media.title), body })
}

fn kv_bool(store: &Store, key: &str, default: bool) -> bool {
    match store.kv_get(key).ok().flatten().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => default,
    }
}

// ------------------------------------------------------------------------------------ Kotora

#[uniffi::export]
impl Kotora {
    /// The week of the current season from the saved copies and the list. Local: no network.
    pub fn airing_week(&self, kind: AiringType, mine: AiringMine) -> Result<AiringWeek, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let now = time::now_unix();
        let cur = Season::of(now);
        let prev = cur.previous();
        let c = store.season(service.name(), cur)?;
        let p = store.season(service.name(), prev)?;
        let stale = !c.as_ref().is_some_and(|(_, at)| cur.is_fresh(cur, *at, now)) || !p.as_ref().is_some_and(|(_, at)| prev.is_fresh(cur, *at, now));
        let fetched_at = c.as_ref().map(|(_, at)| *at);
        let list = store.load_list(Scope::new(service.name(), Kind::Anime))?;
        drop(store);
        let (c_shows, p_shows) = (c.map(|x| x.0).unwrap_or_default(), p.map(|x| x.0).unwrap_or_default());
        let shows = if c_shows.is_empty() && p_shows.is_empty() { Vec::new() } else { airing::merge(&c_shows, &p_shows, &list) };
        let (days, untimed) = week(&shows, &list, service, kind, mine, now, &Clock::local);
        Ok(AiringWeek { season: cur.label(), service_title: service.id().title().into(), fetched_at, shows: shows.len() as u32, stale, days, untimed })
    }

    /// Asks the active service for the current and the previous season when their saved copies
    /// are missing or old (or `force`: the Refresh button), and saves them. Network: call it off
    /// the main thread. True when something was fetched.
    pub fn airing_fetch(&self, force: bool) -> Result<bool, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.own_store()?;
        let now = time::now_unix();
        let cur = Season::of(now);
        let prev = cur.previous();
        let need = |se: Season| -> Result<bool, KotoraError> { Ok(force || !store.season(service.name(), se)?.is_some_and(|(_, at)| se.is_fresh(cur, at, now))) };
        let (need_c, need_p) = (need(cur)?, need(prev)?);
        if !(need_c || need_p) {
            return Ok(false);
        }
        let mut svc = self.service(service)?;
        let fail = |e: ServiceError| KotoraError::Other { message: error_text(service, e) };
        if need_c {
            let all = pages(svc.as_mut(), cur).map_err(fail)?;
            store.save_season(service.name(), cur, &all, now, &[prev])?;
        }
        if need_p {
            let all = pages(svc.as_mut(), prev).map_err(fail)?;
            store.save_season(service.name(), prev, &all, now, &[cur])?;
        }
        Ok(true)
    }

    pub fn notify_settings(&self) -> NotifySettings {
        let store = self.store();
        let d = NotifySettings::default();
        NotifySettings {
            started_airing: kv_bool(&store, NOTIFY_STARTED, d.started_airing),
            new_episode: kv_bool(&store, NOTIFY_EPISODE, d.new_episode),
            new_chapter: kv_bool(&store, NOTIFY_CHAPTER, d.new_chapter),
        }
    }

    pub fn set_notify_settings(&self, settings: NotifySettings) -> Result<(), KotoraError> {
        let store = self.store();
        let b = |v: bool| if v { "1" } else { "0" };
        store.kv_set(NOTIFY_STARTED, b(settings.started_airing))?;
        store.kv_set(NOTIFY_EPISODE, b(settings.new_episode))?;
        store.kv_set(NOTIFY_CHAPTER, b(settings.new_chapter))?;
        Ok(())
    }

    /// One look at the saved list: the Started airing and New episode notifications due now
    /// (each announced once, whatever the switches say: a switch turned on later never brings old
    /// ones back). Database only. Signed out: none.
    pub fn air_notes(&self) -> Result<Vec<Note>, KotoraError> {
        let Some(service) = self.active() else { return Ok(Vec::new()) };
        let on = self.notify_settings();
        let mut store = self.own_store()?;
        let due = airnotes::pass(&mut store, service.name(), time::now_unix())?;
        if due.is_empty() {
            return Ok(Vec::new());
        }
        let list = store.load_list(Scope::new(service.name(), Kind::Anime))?;
        Ok(due.iter().filter_map(|d| air_note(list.iter().find(|it| it.media.id == d.media_id)?, d.what, on)).collect())
    }

    /// The New chapter notifications due now: MangaUpdates and MangaDex for the Reading manga
    /// whose saved releases are stale, then the chapters to announce. Only with Manga and the
    /// switch on. Network: call it off the main thread (a worker).
    pub fn chapter_notes(&self) -> Result<Vec<Note>, KotoraError> {
        let Some(service) = self.active() else { return Ok(Vec::new()) };
        if !self.notify_settings().new_chapter || !self.manga_enabled() {
            return Ok(Vec::new());
        }
        let store = self.own_store()?;
        let svc = service.id();
        let on = Sources { mu: true, md: true };
        let items: Vec<ListItem> = store.load_list(Scope::new(svc.name(), Kind::Manga))?.into_iter().filter(|it| it.status == Status::Watching).collect();
        let (mut mu_client, mu_cache) = (mu::Client::online(), Cache::default_dir());
        let (mut md_client, md_cache) = (mangadex::Client::online(), Cache::mangadex_dir());
        let mut notes = Vec::new();
        let mut seen = HashSet::new();
        for it in &items {
            let key = Key { service: svc, media_id: it.media.id };
            let now = time::now_unix();
            let info = if mu::stale_all(&mu_cache, &md_cache, &store, key, on, now) {
                match mu::fetch_all((&mut mu_client, &mu_cache), (&mut md_client, &md_cache), &store, key, &it.media, on, now, false) {
                    Ok(info) => info,
                    // Offline or slowed down: the rest waits for the next run.
                    Err(ServiceError::Offline(_) | ServiceError::RateLimited) => break,
                    Err(e) => {
                        self.log(crate::LogLevel::Warn, format!("release data for {}: {}", it.media.id, error_text(service, e)));
                        continue;
                    }
                }
            } else {
                mu::saved_all(&mu_cache, &md_cache, &store, key, on)
            };
            let t = time::now_unix();
            let Some((s, _)) = info.best(t, ListRun::from_word(&it.media.air_status), on) else { continue };
            if let Some(n) = mu::new_chapter(&s, it.progress, t)
                && store.announce_once(svc.name(), it.media.id, What::Chapter(n), t).unwrap_or(false)
                && seen.insert(it.media.id)
            {
                let label = s.latest.as_deref().map_or_else(|| format!("Ch {n}"), mu::chapter_label);
                notes.extend(chapter_note(it, n, &label, on));
            }
        }
        Ok(notes)
    }
}
