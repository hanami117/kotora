//! Statistics, the phone's settings (`config.toml`, the desktop's keys) and "Fill volumes from
//! chapters": the desktop's Statistics page (`app/src/stats.rs`), Settings → Appearance /
//! Startup / Notifications and `app/src/volfill.rs`, over the same core calls.
//!
//! - **Statistics** are local (`Store::stats`): the list and Kotora's activity log, no request.
//! - **Settings** are `config.toml` in the config folder, the desktop's sections and keys
//!   (`[display] theme`, `intro`, `list_kind`; `[startup] open_on`, `mode`, `last_page`;
//!   `[notifications] new_episode`, `started_airing`, `new_chapter`). The phone's notification
//!   scheduling reads the same keys ([`Kotora::settings`]).
//! - **Fill volumes** looks the manga up on MangaDex (a [`VolumeFill`], blocking: a worker), shows
//!   the preview, then queues the changes as one import (never lowered); Undo takes back what
//!   hasn't been sent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use kotora_core::config::{self, Config, OpenOn, StartMode, Theme};
use kotora_core::mangadex::{self, FillPlan, Lookup};
use kotora_core::service::ServiceError;
use kotora_core::stats::{self, Period, Title};
use kotora_core::store::Store;
use kotora_core::{Kind, ListItem, Scope, paths, time};

use crate::{Kotora, KotoraError, MediaKind, Service, other};

// ------------------------------------------------------------------------------- statistics

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct StatsPeriod {
    pub units: u32,
    pub volumes: u32,
    pub minutes: u64,
    pub titles: u32,
}

impl From<&Period> for StatsPeriod {
    fn from(p: &Period) -> Self {
        StatsPeriod { units: p.units, volumes: p.volumes, minutes: p.minutes, titles: p.titles }
    }
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct StatsTitle {
    pub media_id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    pub progress: u32,
    pub total: Option<u32>,
    /// Episodes (chapters) in the period (Most watched lately).
    pub units: u32,
}

impl From<&Title> for StatsTitle {
    fn from(t: &Title) -> Self {
        StatsTitle { media_id: t.media_id, title: t.title.clone(), cover_url: t.cover_url.clone(), progress: t.progress, total: t.total, units: t.units }
    }
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct NamedCount {
    pub name: String,
    pub count: u32,
}

/// Everything the Statistics page shows for one list (`kotora_core::stats::Stats`).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct StatsData {
    pub kind: MediaKind,
    /// Today, as a day number (days since 1970-01-01) in the user's time zone.
    pub today: i64,
    /// The day the activity log started (local), if it has.
    pub since_day: Option<i64>,
    // The list.
    pub titles: u32,
    /// Watching, Plan to watch, Completed, On hold, Dropped (`Status::ALL`).
    pub statuses: Vec<u32>,
    /// Scores 1 to 10.
    pub scores: Vec<u32>,
    pub units: u64,
    pub volumes: u64,
    pub minutes: u64,
    pub guessed: u32,
    pub watched: u32,
    pub with_details: u32,
    pub genres: Vec<NamedCount>,
    pub makers: Vec<NamedCount>,
    pub airing: Vec<StatsTitle>,
    // The log.
    /// Episodes (chapters) per day, oldest first, the last one today.
    pub days: Vec<u32>,
    pub first_day: i64,
    pub week: StatsPeriod,
    pub month: StatsPeriod,
    pub year: StatsPeriod,
    pub all: StatsPeriod,
    pub streak: u32,
    pub best_streak: u32,
    /// Monday first.
    pub weekdays: Vec<u32>,
    pub hours: Vec<u32>,
    pub lately: Vec<StatsTitle>,
}

impl StatsData {
    fn from_core(s: &stats::Stats) -> Self {
        let named = |v: &[(String, u32)]| v.iter().map(|(n, c)| NamedCount { name: n.clone(), count: *c }).collect();
        let l = &s.list;
        let g = &s.log;
        StatsData {
            kind: s.kind.into(),
            today: s.today,
            since_day: s.since.map(|t| time::Clock::local(t).day),
            titles: l.titles,
            statuses: l.statuses.to_vec(),
            scores: l.scores.to_vec(),
            units: l.units,
            volumes: l.volumes,
            minutes: l.minutes,
            guessed: l.guessed,
            watched: l.watched,
            with_details: l.with_details,
            genres: named(&l.genres),
            makers: named(&l.makers),
            airing: l.airing.iter().map(StatsTitle::from).collect(),
            days: g.days.clone(),
            first_day: g.first_day,
            week: (&g.week).into(),
            month: (&g.month).into(),
            year: (&g.year).into(),
            all: (&g.all).into(),
            streak: g.streak,
            best_streak: g.best_streak,
            weekdays: g.weekdays.to_vec(),
            hours: g.hours.to_vec(),
            lately: g.lately.iter().map(StatsTitle::from).collect(),
        }
    }
}

/// "3 d 4 h", "5 h 20 min", "45 min" (the desktop's words).
#[uniffi::export]
pub fn stats_duration(minutes: u64) -> String {
    stats::duration(minutes)
}

/// "1,234".
#[uniffi::export]
pub fn stats_grouped(n: u64) -> String {
    stats::grouped(n)
}

/// "Nov 19, 2026" of a day number.
#[uniffi::export]
pub fn stats_date_text(day: i64) -> String {
    stats::date_text(day)
}

/// "1 day", "3 days".
#[uniffi::export]
pub fn stats_days(n: u32) -> String {
    stats::days(n)
}

/// The month (1 to 12) of a day number.
#[uniffi::export]
pub fn stats_month(day: i64) -> u32 {
    time::civil_from_days(day).1
}

/// The weekday of a day number, Monday 0.
#[uniffi::export]
pub fn stats_weekday(day: i64) -> u8 {
    time::weekday(day)
}

// --------------------------------------------------------------------------------- settings

/// The phone's settings, as `config.toml` has them (defaults when it doesn't).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct KotoraSettings {
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// The startup animation.
    pub intro: bool,
    /// `home` or `last`.
    pub open_on: String,
    /// `last`, `anime` or `manga`.
    pub start_mode: String,
    /// The page left last (`list`, `airing`, `search`, `settings`, `stats`, …), as the app wrote it.
    pub last_page: String,
    /// The Anime | Manga switch as it was left.
    pub list_kind: MediaKind,
    pub notify_new_episode: bool,
    pub notify_started_airing: bool,
    pub notify_new_chapter: bool,
}

impl KotoraSettings {
    fn from_config(c: &Config) -> Self {
        KotoraSettings {
            theme: c.display.theme.key().to_string(),
            intro: c.display.intro,
            open_on: c.startup.open_on().key().to_string(),
            start_mode: c.startup.mode().key().to_string(),
            last_page: c.startup.last_page.clone(),
            list_kind: c.display.list_kind.into(),
            notify_new_episode: c.notifications.new_episode,
            notify_started_airing: c.notifications.started_airing,
            notify_new_chapter: c.notifications.new_chapter,
        }
    }
}

/// One setting's new value.
#[derive(uniffi::Enum, Clone, Debug, PartialEq)]
pub enum Setting {
    Theme { value: String },
    Intro { on: bool },
    OpenOn { value: String },
    StartMode { value: String },
    LastPage { value: String },
    ListKind { kind: MediaKind },
    NotifyNewEpisode { on: bool },
    NotifyStartedAiring { on: bool },
    NotifyNewChapter { on: bool },
}

impl Setting {
    /// The section, key and TOML literal; an unknown choice is an error, never written.
    fn line(&self) -> Result<(&'static str, &'static str, String), KotoraError> {
        let bad = |what: &str, v: &str| other(format!("{v:?} is not a {what}"));
        let b = |on: &bool| on.to_string();
        Ok(match self {
            Setting::Theme { value } => {
                let t = Theme::ALL.into_iter().find(|t| t.key() == value).ok_or_else(|| bad("theme", value))?;
                ("display", "theme", config::quote(t.key()))
            }
            Setting::Intro { on } => ("display", "intro", b(on)),
            Setting::OpenOn { value } => ("startup", "open_on", config::quote(OpenOn::from_key(value).ok_or_else(|| bad("page", value))?.key())),
            Setting::StartMode { value } => ("startup", "mode", config::quote(StartMode::from_key(value).ok_or_else(|| bad("mode", value))?.key())),
            Setting::LastPage { value } => ("startup", "last_page", config::quote(value)),
            Setting::ListKind { kind } => ("display", "list_kind", config::quote(Kind::from(*kind).name())),
            Setting::NotifyNewEpisode { on } => ("notifications", "new_episode", b(on)),
            Setting::NotifyStartedAiring { on } => ("notifications", "started_airing", b(on)),
            Setting::NotifyNewChapter { on } => ("notifications", "new_chapter", b(on)),
        })
    }
}

// ------------------------------------------------------------------------------ volume fill

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FillRow {
    pub media_id: u32,
    pub title: String,
    pub chapters: u32,
    pub from: u32,
    pub to: u32,
}

impl From<&mangadex::FillRow> for FillRow {
    fn from(r: &mangadex::FillRow) -> Self {
        FillRow { media_id: r.media_id, title: r.title.clone(), chapters: r.chapters, from: r.from, to: r.to }
    }
}

/// What "Fill volumes from chapters" would do (`mangadex::FillPlan`).
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct FillPreview {
    pub fill: Vec<FillRow>,
    pub not_found: Vec<String>,
    pub same: Vec<FillRow>,
    pub higher: Vec<FillRow>,
    /// The look-up was stopped (Cancel): nothing to fill.
    pub stopped: bool,
}

impl FillPreview {
    fn of(p: &FillPlan) -> Self {
        let rows = |v: &[mangadex::FillRow]| v.iter().map(FillRow::from).collect();
        FillPreview { fill: rows(&p.fill), not_found: p.not_found.clone(), same: rows(&p.same), higher: rows(&p.higher), stopped: false }
    }
}

#[derive(uniffi::Record, Clone, Copy, Debug, Default, PartialEq)]
pub struct FillProgress {
    pub done: u32,
    pub of: u32,
}

/// One "Fill volumes from chapters": the look-up (with its progress and Cancel), the plan, the
/// changes it queued (for Undo). Made by [`Kotora::volume_fill`] for the active service.
#[derive(uniffi::Object)]
pub struct VolumeFill {
    db: PathBuf,
    service: Service,
    done: AtomicU32,
    of: AtomicU32,
    cancel: AtomicBool,
    plan: Mutex<Option<FillPlan>>,
    queued: Mutex<Vec<i64>>,
}

/// The desktop's words for MangaDex not answering.
fn mangadex_text(e: &ServiceError) -> String {
    match e {
        ServiceError::Offline(_) => "Can\u{2019}t reach MangaDex: volumes from chapters need the network. Nothing was changed.".into(),
        ServiceError::RateLimited => "MangaDex asked Kotora to slow down. Try again in a minute; what was looked up is saved.".into(),
        ServiceError::SignInAgain => "MangaDex refused the request.".into(),
        ServiceError::Other(m) => format!("MangaDex: {m}"),
    }
}

#[uniffi::export]
impl VolumeFill {
    /// The manga it would look up: chapters read, volumes short of their total.
    pub fn candidates(&self) -> Result<u32, KotoraError> {
        Ok(self.items()?.len() as u32)
    }

    pub fn progress(&self) -> FillProgress {
        FillProgress { done: self.done.load(Ordering::Relaxed), of: self.of.load(Ordering::Relaxed) }
    }

    /// Stops a running look-up (it returns a `stopped` preview).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Looks every candidate up on MangaDex (saved answers first, unless `refresh`), under its
    /// rate limit. Network: a worker thread. No network or a rate limit is an error with the
    /// desktop's words; nothing is changed until [`VolumeFill::apply`].
    pub fn look_up(&self, refresh: bool) -> Result<FillPreview, KotoraError> {
        let items = self.items()?;
        self.cancel.store(false, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.of.store(items.len() as u32, Ordering::Relaxed);
        let mut client = mangadex::Client::online();
        let cache = mangadex::Cache::default_dir();
        let mut rows: Vec<(ListItem, Option<Lookup>)> = Vec::new();
        for (i, it) in items.into_iter().enumerate() {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(FillPreview { stopped: true, ..Default::default() });
            }
            let key = mangadex::key_of(self.service.id(), &it.media);
            match mangadex::find(&mut client, &cache, key, &mangadex::titles_of(&it.media), time::now_unix(), refresh) {
                Ok((l, _)) => rows.push((it, Some(l))),
                Err(ServiceError::Other(_)) => rows.push((it, None)),
                Err(e) => return Err(other(mangadex_text(&e))),
            }
            self.done.store(i as u32 + 1, Ordering::Relaxed);
        }
        let plan = mangadex::plan_fill(&rows);
        let preview = FillPreview::of(&plan);
        *self.plan.lock().unwrap_or_else(|e| e.into_inner()) = Some(plan);
        Ok(preview)
    }

    /// Fill: the preview's volumes go to the queue as one import. Returns how many.
    pub fn apply(&self) -> Result<u32, KotoraError> {
        let plan = self.plan.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or_else(|| other("Look the volumes up first."))?;
        let actions = mangadex::fill_actions(&plan);
        if actions.is_empty() {
            return Ok(0);
        }
        let mut store = Store::open(&self.db)?;
        let ids = store.import(self.service.name(), &actions, time::now_unix()).map_err(|e| other(format!("The volumes can't be saved: {e}")))?;
        let n = ids.len() as u32;
        *self.queued.lock().unwrap_or_else(|e| e.into_inner()) = ids;
        Ok(n)
    }

    /// Undo: takes back what Fill queued and hasn't been sent. Returns how many.
    pub fn undo(&self) -> Result<u32, KotoraError> {
        let ids = std::mem::take(&mut *self.queued.lock().unwrap_or_else(|e| e.into_inner()));
        let mut store = Store::open(&self.db)?;
        let n = store.undo_import(self.service.name(), &ids, time::now_unix()).map_err(|e| other(format!("The volumes can't be taken back: {e}")))?;
        Ok(n as u32)
    }
}

impl VolumeFill {
    fn items(&self) -> Result<Vec<ListItem>, KotoraError> {
        let store = Store::open(&self.db)?;
        Ok(store.load_list(Scope::new(self.service.name(), Kind::Manga))?.into_iter().filter(mangadex::wants_lookup).collect())
    }
}

/// The credit MangaDex's acceptable use asks for, wherever its numbers show.
#[uniffi::export]
pub fn mangadex_credit() -> String {
    "Volume data: MangaDex".into()
}

// ------------------------------------------------------------------------------------- api

#[uniffi::export]
impl Kotora {
    /// The Statistics page's numbers for the active account's `kind` list. Local: no request.
    pub fn stats(&self, kind: MediaKind) -> Result<StatsData, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let s = store.stats(Scope::new(service.name(), kind.into()), time::now_unix(), &time::local_offset)?;
        Ok(StatsData::from_core(&s))
    }

    /// The settings in `config.toml` (defaults for what it doesn't say).
    pub fn settings(&self) -> KotoraSettings {
        let c = Config::load(&paths::config_file()).unwrap_or_default();
        KotoraSettings::from_config(&c)
    }

    /// Saves one setting (only its line in `config.toml` changes) and returns them all.
    pub fn set_setting(&self, setting: Setting) -> Result<KotoraSettings, KotoraError> {
        let (section, key, value) = setting.line()?;
        config::write_setting(&paths::config_file(), section, key, &value).map_err(other)?;
        Ok(self.settings())
    }

    /// A "Fill volumes from chapters" for the active account's manga list.
    pub fn volume_fill(&self) -> Result<Arc<VolumeFill>, KotoraError> {
        let service = self.active_or_err()?;
        Ok(Arc::new(VolumeFill {
            db: self.db.clone(),
            service,
            done: AtomicU32::new(0),
            of: AtomicU32::new(0),
            cancel: AtomicBool::new(false),
            plan: Mutex::new(None),
            queued: Mutex::new(Vec::new()),
        }))
    }
}

#[cfg(test)]
#[path = "insights_tests.rs"]
mod tests;
