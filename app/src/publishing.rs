//! Publishing (Manga mode, in Airing's place in the sidebar): for each Reading manga, the latest
//! chapter out and when, the chapters left to read, and an estimate of the next release, from
//! MangaUpdates (core [`mangaupdates`]). New chapter notifications come from the same data.
//!
//! - **One worker thread** (`kotora-publishing`), started the first time it is needed (the page
//!   opens, a details page of a Reading manga, or New chapter notifications are on) and only
//!   while Manga is on; it stops when Manga goes off. Each pass shows the saved files first, then
//!   asks MangaUpdates only for what is older than [`RELEASES_TTL`](mangaupdates::RELEASES_TTL)
//!   (Refresh asks again for all). With notifications on it makes a pass every [`EVERY`];
//!   otherwise it waits without a timer. Never headless; offline it reads only the saved files.
//! - **Estimates**: the page says "estimate" wherever a date is guessed, and credits MangaUpdates.
//! - **Wrong or missing match**: a row's panel takes a mangaupdates.com/series/… link; it is
//!   kept in the store per manga (Reset goes back to the search).
//! - Drawn from [`State::publishing_rebuild`] (the data, the list or the sort changed), never
//!   per frame; rows are virtual.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use kotora_core::airnotes::What;
use kotora_core::mangaupdates::{self as mu, Cache, Confidence, Info, Key, Matched, Pace};
use kotora_core::service::ServiceError;
use kotora_core::store::Store;
use kotora_core::time::Clock;
use kotora_core::{Kind, ListItem, Scope, Status, paths};
use rune::{El, GrowthSpec, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Page, State};
use crate::notes::{Act, Note};
use crate::theme::{self, PAL};
use crate::ui::{action, alert, tag, tag_tinted};

/// Notifications on: a pass this often (only series older than 4 hours are asked again).
const EVERY: Duration = Duration::from_secs(30 * 60);
/// Notification ids remembered for clicks.
const KEEP_IDS: usize = 64;
const DAY: i64 = 86_400;

const SHORT_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The page's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PSort {
    /// Soonest estimated release first; hiatus and unknown last.
    #[default]
    Next,
    /// Most chapters to read first.
    Behind,
    /// Newest release first.
    Latest,
}

impl PSort {
    pub const ALL: [PSort; 3] = [PSort::Next, PSort::Behind, PSort::Latest];

    pub fn label(self) -> &'static str {
        match self {
            PSort::Next => "Next release",
            PSort::Behind => "Most to read",
            PSort::Latest => "Latest release",
        }
    }
}

/// How a row's series was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Not looked up yet (the worker hasn't come to it, or offline with nothing saved).
    Pending,
    /// No MangaUpdates series found.
    NotFound,
    /// Matched, no releases read yet.
    NoReleases,
    Found(Confidence),
}

/// One row, everything drawn already in words.
#[derive(Clone, Debug, PartialEq)]
pub struct PRow {
    pub id: u32,
    /// "Ch 74", "Bone 67", "Vol 9"; empty when unknown.
    pub latest: String,
    /// "1 day ago".
    pub when: String,
    /// Chapters out but not read.
    pub behind: Option<u32>,
    /// "Weekly", "On hiatus", …
    pub pace: String,
    /// "next ~Mon", "due today", "late", "about every 10 days"; empty when nothing to say.
    pub next: String,
    pub look: Look,
    /// Out in the last two days and not read (lit).
    pub fresh: bool,
    pub hiatus: bool,
    pub error: Option<String>,
    next_key: i64,
    latest_key: i64,
}

/// Kept in the state.
#[derive(Default)]
pub struct Publishing {
    pub info: HashMap<u32, Info>,
    pub rows: Vec<PRow>,
    pub sort: PSort,
    /// A pass asks MangaUpdates: done, of.
    pub busy: Option<(usize, usize)>,
    /// The last pass's failure (offline, rate limit); the saved data still shows.
    pub error: Option<String>,
    /// The row whose panel is open.
    pub open: Option<u32>,
    /// The panel's link field.
    pub link: TextEdit,
    /// What the link did: (failed, text).
    pub link_note: Option<(bool, String)>,
    pub linking: bool,
    /// The newest time releases were read.
    pub checked_at: Option<i64>,
    /// Scenes and tests: a fixed UTC offset (else the user's zone).
    pub tz: Option<i64>,
    tx: Option<Sender<Cmd>>,
    notify: Arc<AtomicBool>,
    /// The app is offline (forced or the network failed): the worker asks nothing.
    offline: Arc<AtomicBool>,
    note_ids: Vec<(String, u32)>,
}

impl Publishing {
    /// The app's offline state, for the worker (set after every message the app applies).
    pub fn note_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
    }

    /// Drops the worker (Manga off, sign out): the thread ends.
    pub fn stop(&mut self) {
        self.tx = None;
        self.busy = None;
        self.linking = false;
    }
}

/// What the worker is asked.
enum Cmd {
    /// A pass: `force` asks MangaUpdates again for all; `only` one manga.
    Pass { force: bool, only: Option<u32> },
    /// The user pasted a link for a manga.
    Pick { media_id: u32, series: u64 },
    /// Back to the search for a manga.
    Unpick { media_id: u32 },
}

/// What the worker answers.
pub enum Answer {
    /// The saved files, all at once.
    Saved(Vec<(u32, Info)>),
    /// One manga, fresh.
    One { media_id: u32, info: Info, done: usize, of: usize },
    Started(usize),
    /// The pass ended (`error`: it stopped early).
    Done { error: Option<String>, dues: Vec<(u32, u32, String)> },
    Picked { media_id: u32, result: Result<Info, String> },
}

// ------------------------------------------------------------------------------------- state

impl State {
    /// The sidebar's Publishing: the page, and a pass for what is stale.
    pub fn open_publishing(&mut self) {
        self.go(Page::Publishing);
        self.publishing_rebuild();
        self.publishing_send(Cmd::Pass { force: false, only: None });
    }

    /// `kot publishing`: a pass for what is stale (as opening the page).
    pub fn publishing_kick(&mut self) {
        self.publishing_send(Cmd::Pass { force: false, only: None });
    }

    /// `kot publishing`'s rows, in the page's order.
    #[cfg(test)]
    pub fn ctl_publishing(&self) -> Vec<kotora_ctl::PublishingItem> {
        ctl_items(&self.manga.entries, &self.publishing.rows)
    }

    /// `kot publishing`'s snapshot (UI thread): the Reading manga, what is in memory, and the
    /// rest the caller reads from the saved files ([`ctl_reply`]).
    pub fn ctl_publishing_snapshot(&self) -> CtlSnapshot {
        let entries: Vec<crate::manga::MEntry> = self.manga.entries.iter().filter(|e| e.status == Status::Watching).cloned().collect();
        let info = entries.iter().filter_map(|e| self.publishing.info.get(&e.id).map(|i| (e.id, i.clone()))).collect();
        let now = self.clock();
        CtlSnapshot { entries, info, sort: self.publishing.sort, now, today: self.publishing_today(now), offline: self.publishing_offline(), error: self.publishing.error.clone(), busy: self.publishing.busy.is_some() }
    }
}

/// See [`State::ctl_publishing_snapshot`].
pub struct CtlSnapshot {
    pub entries: Vec<crate::manga::MEntry>,
    pub info: HashMap<u32, Info>,
    pub sort: PSort,
    pub now: i64,
    pub today: i64,
    pub offline: bool,
    pub error: Option<String>,
    pub busy: bool,
}

/// `kot publishing`'s answer, off the UI thread: the saved files fill in what memory lacks
/// (never a request), then the same rows and notice as the page. Returns the saved infos read,
/// for the page.
pub fn ctl_reply(mut snap: CtlSnapshot, cache: &Cache, store: Option<&Store>, svc: kotora_core::service::ServiceId) -> (Vec<kotora_ctl::PublishingItem>, Option<String>, Vec<(u32, Info)>) {
    let mut read = Vec::new();
    for e in &snap.entries {
        if snap.info.contains_key(&e.id) {
            continue;
        }
        let key = Key { service: svc, media_id: e.id };
        let i = mu::saved(cache, key, store.and_then(|st| mu::picked(st, key)));
        if i.looked {
            snap.info.insert(e.id, i.clone());
            read.push((e.id, i));
        }
    }
    let checked = snap.info.values().filter_map(|i| i.fetched_at).max();
    let rows = build_rows(&snap.entries, &snap.info, snap.sort, snap.now, snap.today);
    let items = ctl_items(&snap.entries, &rows);
    let note = notice(snap.offline, snap.error.as_deref(), checked, snap.now).or_else(|| {
        (snap.busy || snap.info.values().all(|i| i.fetched_at.is_none())).then(|| "Kotora is checking MangaUpdates; run it again in a minute for the rest.".to_string())
    });
    (items, note.filter(|_| !snap.entries.is_empty()), read)
}

/// The rows as `kot` prints them.
fn ctl_items(entries: &[crate::manga::MEntry], rows: &[PRow]) -> Vec<kotora_ctl::PublishingItem> {
    rows.iter()
            .filter_map(|r| {
                let e = entries.iter().find(|e| e.id == r.id)?;
                let flag = match r.look {
                    Look::Pending => "looking",
                    Look::NotFound => "no match",
                    Look::NoReleases => "no releases",
                    Look::Found(Confidence::Guess) => "check match",
                    Look::Found(_) => "",
                };
                Some(kotora_ctl::PublishingItem { title: e.title.clone(), latest: r.latest.clone(), when: r.when.clone(), behind: r.behind, pace: r.pace.clone(), next: r.next.clone(), flag: flag.into() })
            })
        .collect()
}

impl State {
    /// Refresh: asks MangaUpdates again for every Reading manga.
    pub fn publishing_refresh(&mut self) {
        self.publishing.error = None;
        self.publishing_send(Cmd::Pass { force: true, only: None });
    }

    /// A details page of a Reading manga: its row's data, if not in memory or stale.
    pub fn publishing_for_details(&mut self, media_id: u32) {
        if self.manga.entries.iter().any(|e| e.id == media_id && e.status == Status::Watching) {
            self.publishing_send(Cmd::Pass { force: false, only: Some(media_id) });
        }
    }

    /// Settings: notifications on or off (the worker starts, or drops its timer).
    pub fn publishing_notify_changed(&mut self) {
        let on = self.prefs.notify.new_chapter;
        self.publishing.notify.store(on, Ordering::SeqCst);
        if on && self.manga.enabled {
            self.publishing_send(Cmd::Pass { force: false, only: None });
        }
    }

    /// At start, and when Manga is turned on: the worker, if notifications want it.
    pub fn publishing_start(&mut self) {
        if self.manga.enabled && self.prefs.notify.new_chapter {
            self.publishing_notify_changed();
        }
    }

    fn publishing_send(&mut self, cmd: Cmd) {
        if !self.manga.enabled || rune::is_headless() || !self.has_list() {
            return;
        }
        let Some(h) = self.handle.clone() else { return };
        let p = &mut self.publishing;
        p.notify.store(self.prefs.notify.new_chapter, Ordering::SeqCst);
        p.offline.store(self.offline || paths::offline_forced(), Ordering::SeqCst);
        let cmd = match &p.tx {
            Some(tx) => match tx.send(cmd) {
                Ok(()) => return,
                // The thread is gone (it can only end by a panic while the sender lives).
                Err(mpsc::SendError(cmd)) => cmd,
            },
            None => cmd,
        };
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(cmd);
        p.tx = Some(tx);
        let (notify, offline) = (p.notify.clone(), p.offline.clone());
        let _ = std::thread::Builder::new().name("kotora-publishing".into()).spawn(move || run(&h, &rx, &notify, &offline));
    }

    /// The worker's answer.
    pub fn publishing_answer(&mut self, a: Answer) -> bool {
        if !self.manga.enabled || !self.has_list() {
            return false;
        }
        let p = &mut self.publishing;
        match a {
            Answer::Saved(all) => {
                for (id, info) in all {
                    // A fresher answer already in memory stays.
                    let newer = p.info.get(&id).is_some_and(|i| i.fetched_at > info.fetched_at);
                    if !newer {
                        p.info.insert(id, info);
                    }
                }
            }
            Answer::Started(n) => {
                p.busy = Some((0, n));
                p.error = None;
            }
            Answer::One { media_id, info, done, of } => {
                p.busy = Some((done, of));
                p.info.insert(media_id, info);
            }
            Answer::Done { error, dues } => {
                p.busy = None;
                p.error = error;
                self.publishing_notes(&dues);
            }
            Answer::Picked { media_id, result } => {
                p.linking = false;
                match result {
                    Ok(info) => {
                        let title = info.matched.as_ref().map_or_else(String::new, |m| m.title.clone());
                        p.link_note = Some((false, if title.is_empty() { "Back to the search.".into() } else { format!("Now matched with \u{201c}{title}\u{201d}.") }));
                        p.link = TextEdit::default();
                        p.info.insert(media_id, info);
                    }
                    Err(e) => p.link_note = Some((true, e)),
                }
            }
        }
        self.publishing.checked_at = self.publishing.info.values().filter_map(|i| i.fetched_at).max();
        self.publishing_rebuild();
        self.page == Page::Publishing || self.page == Page::Details
    }

    /// The rows, from the Reading manga and what is known of them.
    pub fn publishing_rebuild(&mut self) {
        let now = self.clock();
        let today = self.publishing_today(now);
        self.publishing.rows = build_rows(&self.manga.entries, &self.publishing.info, self.publishing.sort, now, today);
    }

    /// Offline: forced, or the app's last request found no network.
    pub fn publishing_offline(&self) -> bool {
        self.offline || paths::offline_forced()
    }

    /// The notice over the page, the details card and `kot publishing` ([`notice`]).
    pub fn publishing_notice(&self) -> Option<String> {
        notice(self.publishing_offline(), self.publishing.error.as_deref(), self.publishing.checked_at, self.clock())
    }

    fn publishing_today(&self, now: i64) -> i64 {
        match self.publishing.tz {
            Some(o) => Clock::at(now, o).day,
            None => Clock::local(now).day,
        }
    }

    pub fn set_publishing_sort(&mut self, s: PSort) {
        self.publishing.sort = s;
        self.publishing_rebuild();
    }

    /// A row's press: its panel opens or closes.
    pub fn publishing_toggle(&mut self, id: u32) {
        let p = &mut self.publishing;
        p.open = if p.open == Some(id) { None } else { Some(id) };
        p.link = TextEdit::default();
        p.link_note = None;
    }

    /// The panel's Use: the pasted link becomes the manga's series.
    pub fn publishing_use_link(&mut self, media_id: u32) {
        let text = self.publishing.link.text.clone();
        let Some(series) = mu::series_id_from_link(&text) else {
            self.publishing.link_note = Some((true, "That isn\u{2019}t a MangaUpdates series link. Open the series on mangaupdates.com and copy the address (mangaupdates.com/series/\u{2026}).".into()));
            return;
        };
        if self.publishing_offline() {
            self.publishing.link_note = Some((true, "Offline: the link is checked with MangaUpdates first; try again when online.".into()));
            return;
        }
        self.publishing.linking = true;
        self.publishing.link_note = None;
        self.publishing_send(Cmd::Pick { media_id, series });
    }

    /// The panel's Reset: forget the picked series, search again.
    pub fn publishing_unpick(&mut self, media_id: u32) {
        if self.publishing_offline() {
            self.publishing.link_note = Some((true, "Offline: Reset searches MangaUpdates again; try again when online.".into()));
            return;
        }
        self.publishing.linking = true;
        self.publishing.link_note = None;
        self.publishing_send(Cmd::Unpick { media_id });
    }

    /// Shows the due New chapter notifications, under the switch. Never visible in the window.
    fn publishing_notes(&mut self, dues: &[(u32, u32, String)]) {
        for (id, n, label) in dues {
            let Some(e) = self.manga.entries.iter().find(|e| e.id == *id) else { continue };
            // The window may know a newer progress than the saved list.
            if !self.prefs.notify.new_chapter || e.status != Status::Watching || e.chapters >= *n {
                continue;
            }
            let behind = n - e.chapters;
            let body = if behind > 1 { format!("{behind} chapters to read. {}.", mu::CREDIT) } else { format!("{}.", mu::CREDIT) };
            let note = Note { id: format!("kotora-m{id}-ch{n}"), kind: crate::notes::Kind::Chapter, summary: format!("New chapter of {}: {label}", e.title), body, tail: None };
            let ids = &mut self.publishing.note_ids;
            ids.retain(|(i, _)| *i != note.id);
            ids.push((note.id.clone(), *id));
            if ids.len() > KEEP_IDS {
                ids.remove(0);
            }
            self.notes.show(note);
        }
    }

    /// The manga of a New chapter notification.
    pub fn chapter_note_media(&self, id: &str) -> Option<u32> {
        self.publishing.note_ids.iter().find(|(i, _)| i == id).map(|(_, m)| *m)
    }

    /// A click on a New chapter notification: the manga's page.
    pub fn chapter_note_action(&mut self, media: u32, _act: Act) {
        if let Some(e) = self.manga.entries.iter().find(|e| e.id == media) {
            let stub = crate::manga::stub(e);
            self.open_details(stub);
        }
        if let Some(h) = &self.handle {
            crate::tray::show_window(h);
        }
    }

    /// Tests: `kot`'s snapshot of the fake manga list, offline, nothing in memory.
    #[cfg(test)]
    pub fn fake_publishing_offline_snapshot() -> CtlSnapshot {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        s.offline = true;
        s.ctl_publishing_snapshot()
    }

    /// Scenes and tests: data as if MangaUpdates had answered.
    pub fn publishing_fake(&mut self, info: Vec<(u32, Info)>) {
        self.publishing.info = info.into_iter().collect();
        self.publishing.tz = Some(0);
        self.publishing.checked_at = self.publishing.info.values().filter_map(|i| i.fetched_at).max();
        self.publishing_rebuild();
    }
}

/// The rows of the Reading manga in `entries`, sorted.
pub fn build_rows(entries: &[crate::manga::MEntry], info: &HashMap<u32, Info>, sort: PSort, now: i64, today: i64) -> Vec<PRow> {
    let mut rows: Vec<PRow> = entries.iter().filter(|e| e.status == Status::Watching).map(|e| row_of(e.id, e.chapters, info.get(&e.id), now, today)).collect();
    let title = |id: u32| entries.iter().find(|e| e.id == id).map_or_else(String::new, |e| e.title.to_lowercase());
    match sort {
        PSort::Next => rows.sort_by_cached_key(|r| (r.next_key, title(r.id))),
        PSort::Behind => rows.sort_by_cached_key(|r| (std::cmp::Reverse(r.behind.map_or(-1, i64::from)), r.next_key, title(r.id))),
        PSort::Latest => rows.sort_by_cached_key(|r| (std::cmp::Reverse(r.latest_key), title(r.id))),
    }
    rows
}

/// What the page says about its data, as the Airing page does:
/// - offline with saved data: its age, and that it can't refresh;
/// - offline with nothing saved: where the data comes from;
/// - a failed refresh: the error, and that the saved releases show (when there are some).
pub fn notice(offline: bool, error: Option<&str>, checked_at: Option<i64>, now: i64) -> Option<String> {
    if offline {
        return Some(match checked_at {
            Some(at) => format!("Offline: MangaUpdates data from {}; can\u{2019}t refresh now.", ago_long(at, now)),
            None => "Offline: release data comes from MangaUpdates.".into(),
        });
    }
    let e = error?;
    Some(if checked_at.is_some() { format!("{e} Showing the saved releases.") } else { e.to_string() })
}

/// "3 hours ago", "2 days ago", "less than an hour ago".
fn ago_long(at: i64, now: i64) -> String {
    let a = kotora_core::time::age(at, now);
    match a.split_once(' ') {
        None => "less than an hour ago".into(),
        Some((n, "h")) => format!("{n} {} ago", if n == "1" { "hour" } else { "hours" }),
        Some(_) => format!("{a} ago"),
    }
}

/// "1 day ago", "3 h ago", "just now".
fn ago(at: i64, now: i64) -> String {
    match kotora_core::time::age(at, now).as_str() {
        "now" => "just now".into(),
        a => format!("{a} ago"),
    }
}

/// A release day (a UTC date) in words: "today", "~Mon" within the week, "~Dec 3" after.
fn day_words(day: i64, today: i64) -> String {
    let d = day.div_euclid(DAY);
    match d - today {
        i64::MIN..=0 => "today".into(),
        1 => "~tomorrow".into(),
        2..=6 => format!("~{}", SHORT_DAYS[usize::from(kotora_core::time::weekday(d))]),
        _ => {
            let (_, m, date) = kotora_core::time::civil_from_days(d);
            format!("~{} {date}", MONTHS[(m as usize + 11) % 12])
        }
    }
}

fn row_of(id: u32, read: u32, info: Option<&Info>, now: i64, today: i64) -> PRow {
    let mut r = PRow { id, latest: String::new(), when: String::new(), behind: None, pace: String::new(), next: String::new(), look: Look::Pending, fresh: false, hiatus: false, error: None, next_key: i64::MAX, latest_key: i64::MIN };
    let Some(info) = info else { return r };
    r.error = info.error.clone();
    let Some(m) = &info.matched else {
        r.look = if info.looked { Look::NotFound } else { Look::Pending };
        return r;
    };
    let Some(s) = info.summary(now) else {
        r.look = Look::NoReleases;
        return r;
    };
    r.look = Look::Found(m.confidence);
    if let Some(l) = &s.latest {
        r.latest = mu::chapter_label(l);
    } else if let Some(v) = &s.volume_only {
        r.latest = format!("Vol {v}");
    }
    if let Some(at) = s.latest_at {
        r.when = ago(at, now);
        r.latest_key = at;
    }
    r.behind = mu::behind(s.latest_no, read);
    r.fresh = r.behind.is_some_and(|b| b > 0) && s.latest_at.is_some_and(|at| now - at < mu::NEW_FOR);
    let c = s.cadence;
    r.pace = c.pace.label();
    r.hiatus = c.pace == Pace::Hiatus;
    r.next = match (c.pace, c.next) {
        (Pace::Hiatus | Pace::Unknown, _) => String::new(),
        (Pace::Irregular(d), _) => format!("about every {d} days"),
        (_, Some(n)) if n.div_euclid(DAY) <= today => "due today".into(),
        (_, Some(n)) => format!("next {}", day_words(n, today)),
        (_, None) if c.overdue => "late".into(),
        _ => String::new(),
    };
    r.next_key = c.sort_key();
    r
}

// ------------------------------------------------------------------------------------ worker

/// The worker may ask MangaUpdates: not offline (forced, or the app found no network).
fn online(offline: &AtomicBool) -> bool {
    !offline.load(Ordering::SeqCst) && !paths::offline_forced()
}

/// Whether the notifications' timer makes a pass: switch on and online.
fn timer_pass(notify: bool, online: bool) -> bool {
    notify && online
}

fn run(h: &crate::model::Handle, rx: &Receiver<Cmd>, notify: &AtomicBool, offline: &AtomicBool) {
    let cache = Cache::default_dir();
    let mut client = mu::Client::online();
    loop {
        let cmd = if notify.load(Ordering::SeqCst) {
            match rx.recv_timeout(EVERY) {
                Ok(c) => c,
                // Turned off meanwhile, or offline: no check (the next timer, or the page, looks again).
                Err(RecvTimeoutError::Timeout) if !timer_pass(notify.load(Ordering::SeqCst), online(offline)) => continue,
                Err(RecvTimeoutError::Timeout) => Cmd::Pass { force: false, only: None },
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => c,
                Err(_) => return,
            }
        };
        // Several passes waiting are one (the strongest).
        let mut cmds = vec![cmd];
        while let Ok(c) = rx.try_recv() {
            cmds.push(c);
        }
        let Ok(store) = Store::open(&paths::db_file()) else { continue };
        let svc = crate::account::active();
        let mut pass: Option<(bool, Option<u32>)> = None;
        for c in cmds {
            match c {
                Cmd::Pass { force, only } => {
                    pass = Some(match pass {
                        None => (force, only),
                        Some((f, o)) => (f || force, if o.is_none() || only.is_none() { None } else { o.or(only) }),
                    });
                }
                Cmd::Pick { media_id, .. } | Cmd::Unpick { media_id } if !online(offline) => {
                    let result = Err("Offline: try again when online.".to_string());
                    h.update_if(move |s, _| s.publishing_answer(Answer::Picked { media_id, result }));
                }
                Cmd::Pick { media_id, series } => {
                    let result = pick(&mut client, &cache, &store, Key { service: svc, media_id }, Some(series));
                    h.update_if(move |s, _| s.publishing_answer(Answer::Picked { media_id, result }));
                }
                Cmd::Unpick { media_id } => {
                    let result = pick(&mut client, &cache, &store, Key { service: svc, media_id }, None);
                    h.update_if(move |s, _| s.publishing_answer(Answer::Picked { media_id, result }));
                }
            }
        }
        if let Some((force, only)) = pass {
            let send = |a: Answer| h.update_if(move |s, _| s.publishing_answer(a));
            one_pass(&mut client, &cache, &store, svc, force, only, kotora_core::time::now_unix, online(offline), &send);
        }
    }
}

/// The Reading manga of the saved list.
fn reading(store: &Store, svc: kotora_core::service::ServiceId) -> Vec<ListItem> {
    store.load_list(Scope::new(svc.name(), Kind::Manga)).unwrap_or_default().into_iter().filter(|it| it.status == Status::Watching).collect()
}

/// One pass (see the module): the saved files, then MangaUpdates for what is stale, then the
/// chapters to announce (each marked announced, whatever the switch says).
#[allow(clippy::too_many_arguments)]
pub fn one_pass<T: kotora_core::http::Transport>(client: &mut mu::Client<T>, cache: &Cache, store: &Store, svc: kotora_core::service::ServiceId, force: bool, only: Option<u32>, now: fn() -> i64, online: bool, send: &dyn Fn(Answer)) {
    let items: Vec<ListItem> = reading(store, svc).into_iter().filter(|it| only.is_none_or(|id| it.media.id == id)).collect();
    let key = |it: &ListItem| Key { service: svc, media_id: it.media.id };
    let mut infos: HashMap<u32, Info> = items.iter().map(|it| (it.media.id, mu::saved(cache, key(it), mu::picked(store, key(it))))).collect();
    send(Answer::Saved(infos.iter().map(|(k, v)| (*k, v.clone())).collect()));
    let mut error = None;
    if online {
        let t = now();
        let todo: Vec<&ListItem> = items.iter().filter(|it| force || mu::stale(cache, key(it), mu::picked(store, key(it)).as_ref(), t)).collect();
        if !todo.is_empty() {
            send(Answer::Started(todo.len()));
        }
        for (i, it) in todo.iter().enumerate() {
            let id = it.media.id;
            match mu::fetch(client, cache, key(it), mu::picked(store, key(it)), &it.media, now(), force) {
                Ok(info) => {
                    infos.insert(id, info.clone());
                    send(Answer::One { media_id: id, info, done: i + 1, of: todo.len() });
                }
                Err(e @ (ServiceError::Offline(_) | ServiceError::RateLimited)) => {
                    error = Some(match e {
                        ServiceError::RateLimited => "MangaUpdates asked Kotora to slow down; the rest comes on the next refresh.".to_string(),
                        _ => "Can\u{2019}t reach MangaUpdates.".to_string(),
                    });
                    break;
                }
                Err(e) => {
                    let mut info = infos.remove(&id).unwrap_or_default();
                    info.error = Some(crate::online::error_text(e));
                    infos.insert(id, info.clone());
                    send(Answer::One { media_id: id, info, done: i + 1, of: todo.len() });
                }
            }
        }
    }
    // Offline the page says so itself ([`notice`]); no announcements from old data either.
    let t = now();
    let mut dues = Vec::new();
    for it in items.iter().filter(|_| online) {
        let Some(s) = infos.get(&it.media.id).and_then(|i| i.summary(t)) else { continue };
        if let Some(n) = mu::new_chapter(&s, it.progress, t)
            && store.announce_once(svc.name(), it.media.id, What::Chapter(n), t).unwrap_or(false)
        {
            dues.push((it.media.id, n, s.latest.as_deref().map_or_else(|| format!("Ch {n}"), mu::chapter_label)));
        }
    }
    send(Answer::Done { error, dues });
}

/// A pasted series (checked with MangaUpdates, then its releases), or (`None`) back to the
/// search. Saves the choice.
fn pick<T: kotora_core::http::Transport>(client: &mut mu::Client<T>, cache: &Cache, store: &Store, key: Key, series: Option<u64>) -> Result<Info, String> {
    let item = store.load_item(Scope::new(key.service.name(), Kind::Manga), key.media_id).ok().flatten().ok_or("That manga isn\u{2019}t on your list any more.")?;
    let now = kotora_core::time::now_unix();
    let picked = match series {
        Some(id) => {
            let (id, title) = client.series(id).map_err(|e| match e {
                ServiceError::Other(m) if m.contains("no such") => "MangaUpdates has no series with that link.".to_string(),
                e => crate::online::error_text(e),
            })?;
            Some(Matched { id, title, confidence: Confidence::Picked })
        }
        None => None,
    };
    mu::set_picked(store, key, picked.as_ref()).map_err(|e| e.to_string())?;
    mu::fetch(client, cache, key, picked, &item.media, now, true).map_err(crate::online::error_text)
}

// -------------------------------------------------------------------------------------- view

fn header() -> El {
    let h = |t: &str| text(t.to_uppercase()).font(theme::CAPTION).color(PAL.faint);
    col()
        .key("thead")
        .w_full()
        .child(
            row()
                .w_full()
                .gap(theme::COL_GAP)
                .center_y()
                .h(30.0)
                .px(theme::PAGE_PAD)
                .child(el().w(34.0))
                .child(h("Title").grow(1.0).min_w(0.0))
                .child(h("Latest").w(COL_LATEST))
                .child(h("To read").w(COL_BEHIND))
                .child(h("Next (estimate)").w(COL_NEXT))
                .child(el().w(38.0)),
        )
        .child(el().w_full().h(1.0).fill(PAL.border))
}

const COL_LATEST: f32 = 150.0;
const COL_BEHIND: f32 = 92.0;
const COL_NEXT: f32 = 170.0;

fn sort_chips(s: &State) -> El {
    let mut r = row().key("pub/sort").gap(6.0).px(theme::PAGE_PAD).pb(10.0).center_y().child(text("Sort").font(theme::LABEL).color(PAL.faint).pr(2.0));
    for o in PSort::ALL {
        let on = s.publishing.sort == o;
        let chip = row()
            .key(format!("pub/sort/{}", o.label()))
            .role("button")
            .label(format!("Sort by {}", o.label().to_lowercase()))
            .checked(on)
            .focusable(true)
            .h(26.0)
            .px(10.0)
            .center_y()
            .radius(13.0)
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.set_publishing_sort(o);
            })
            .child(text(o.label()).font(theme::LABEL).color(if on { PAL.accent_hi } else { PAL.dim }));
        r = r.child(if on { chip.fill(PAL.accent.alpha(0.14)) } else { chip.border(1.0, PAL.border) });
    }
    r
}

/// Under the rows: the credit and what the dates are.
fn footer() -> El {
    col().key("pub/footer").w_full().child(el().w_full().h(1.0).fill(PAL.border)).child(
        row()
            .px(theme::PAGE_PAD)
            .py(8.0)
            .child(text(format!("{} \u{b7} Next dates are estimates from past releases, not announcements.", mu::CREDIT)).key("pub/credit").font(theme::SMALL).color(PAL.faint).truncate()),
    )
}

/// The page.
pub fn page(s: &State) -> El {
    let p = &s.publishing;
    let n = p.rows.len();
    let mut head = crate::ui::top(s, "Publishing", Some(n), false);
    if let Some((done, of)) = p.busy {
        head = head.child(text(format!("Checking MangaUpdates\u{2026} {done} of {of}")).key("pub/updating").font(theme::LABEL).color(PAL.faint));
    } else if let Some(at) = p.checked_at {
        head = head.child(text(format!("Checked {}", ago(at, s.clock()))).key("pub/checked").font(theme::LABEL).color(PAL.faint));
    }
    head = head.child(spacer());
    if s.auth == crate::model::Auth::SignedIn {
        head = head.child(
            el().key("pub/refresh")
                .role("button")
                .label("Check MangaUpdates again")
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
                    s.publishing_refresh();
                })
                .child(icon(&icons::SYNC, 16.0).color(if p.busy.is_some() { PAL.faint } else { PAL.dim })),
        );
    }
    let mut page = col().key("page/publishing").w_full().h_full().child(head);
    let offline = s.publishing_offline();
    if let Some(n) = s.publishing_notice() {
        page = page.child(if offline {
            alert("pub/offline", PAL.warn(), n, None)
        } else {
            let retry = action("pub/retry".into(), "Retry", false, |s: &mut State, _| s.publishing_refresh());
            alert("pub/error", PAL.status(Status::Dropped), n, Some(retry))
        });
    }
    if n == 0 {
        return page.child(
            col()
                .key("empty")
                .grow(1.0)
                .min_h(0.0)
                .center()
                .gap(4.0)
                .pb(60.0)
                .child(text("No manga in Reading").font(theme::SECTION).color(PAL.text))
                .child(text("Manga you\u{2019}re reading show their latest chapters here.").font(theme::LABEL).color(PAL.dim)),
        );
    }
    page.child(sort_chips(s)).child(header()).child(
        col().key("pub/rows-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
            col().key("pub/rows").growth(GrowthSpec::default()).virtual_rows(
                n,
                theme::ROW_H,
                |s: &State, i| s.publishing.rows.get(i).map_or_else(|| format!("gone-{i}").into(), |r| format!("p{}", r.id).into()),
                |s: &State, i| s.publishing.rows.get(i).map_or_else(el, |r| list_row(s, r, s.publishing_offline())),
            ),
        ),
    )
    .child(footer())
}

fn latest_cell(r: &PRow, offline: bool) -> El {
    if r.latest.is_empty() {
        let t = match r.look {
            // Offline nothing is looked up: no promise of it.
            Look::Pending if !offline => "Looking\u{2026}",
            _ => "\u{2013}",
        };
        return text(t).font(theme::LABEL).color(PAL.faint);
    }
    col()
        .gap(1.0)
        .child(text(r.latest.clone()).font(theme::NUM_MED).color(if r.fresh { PAL.accent_hi } else { PAL.text }).truncate())
        .child(text(r.when.clone()).font(theme::SMALL).color(PAL.dim))
}

fn behind_cell(r: &PRow) -> El {
    match r.behind {
        Some(0) => text("Caught up").font(theme::LABEL).color(PAL.dim),
        Some(n) => row().gap(4.0).items_end().child(text(n.to_string()).font(theme::NUM_MED).color(PAL.text)).child(text("to read").font(theme::LABEL).color(PAL.dim)),
        None => text("\u{2013}").font(theme::LABEL).color(PAL.faint),
    }
}

fn next_cell(r: &PRow) -> El {
    if r.pace.is_empty() {
        return el();
    }
    let pace_c = if r.hiatus { PAL.st_dropped } else { PAL.text };
    let mut c = col().gap(1.0).child(text(r.pace.clone()).font(theme::LABEL).color(pace_c));
    if !r.next.is_empty() {
        let fg = if r.next == "late" { PAL.warn() } else if r.next == "due today" { PAL.accent_hi } else { PAL.dim };
        c = c.child(text(r.next.clone()).font(theme::SMALL).color(fg));
    }
    c
}

fn list_row(s: &State, r: &PRow, offline: bool) -> El {
    let id = r.id;
    let Some(e) = s.manga.entries.iter().find(|e| e.id == id) else { return el() };
    let mut sub = row().w_full().min_w(0.0).gap(8.0).center_y().clip_fade(0.0, 0.0, 16.0, 0.0);
    let read = match e.shows() {
        (false, true) => format!("Read vol {}", e.volumes),
        _ => format!("Read {}", e.chapters),
    };
    sub = sub.child(text(read).font(theme::LABEL_NUM).color(PAL.faint));
    match r.look {
        Look::Found(Confidence::Guess) => sub = sub.child(tag_tinted("Check match", PAL.warn(), PAL.warn().alpha(0.12))),
        Look::NotFound => sub = sub.child(tag_tinted("No match", PAL.dim, PAL.raised)),
        _ => {}
    }
    if r.fresh {
        sub = sub.child(tag("New chapter", PAL.accent_hi, true));
    }
    if r.error.is_some() {
        sub = sub.child(tag_tinted("Couldn\u{2019}t check", PAL.err(), PAL.err().alpha(0.12)));
    }
    let name = text(e.title.clone()).font(theme::ROW_TITLE).color(PAL.text).group_color(PAL.accent_hi).truncate();
    let title = col().gap(2.0).grow(1.0).min_w(0.0).child(crate::details::title_link(name, crate::manga::stub(e))).child(sub);
    let plus = s.has_list() && !e.at_end(e.plus_unit());
    let open = s.publishing.open == Some(id);
    let line = row()
        .key("line")
        .w_full()
        .h(theme::ROW_H)
        .px(theme::PAGE_PAD)
        .gap(theme::COL_GAP)
        .center_y()
        .role("button")
        .label(format!("Release details of {}", e.title))
        .hover(PAL.hover())
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_manga_ctx(id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.publishing_toggle(id);
        })
        .child(crate::manga::row_cover(s, e))
        .child(title)
        .child(latest_cell(r, offline).w(COL_LATEST))
        .child(behind_cell(r).w(COL_BEHIND))
        .child(next_cell(r).w(COL_NEXT))
        .child(if plus { plus_one(id, e.plus_unit()) } else { el().size(38.0, 26.0) });
    let line = if r.fresh { line.fill(PAL.accent.alpha(0.06)).child(el().absolute(0.0, 8.0).size(3.0, theme::ROW_H - 16.0).radius(1.5).fill(PAL.accent)) } else { line };
    let mut c = col().w_full().child(if open { line.fill(PAL.surface) } else { line });
    if open {
        c = c.child(panel(s, id));
    }
    c.child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
}

fn plus_one(id: u32, u: crate::manga::Unit) -> El {
    el().key("plus")
        .role("button")
        .label(if u == crate::manga::Unit::Chapters { "One more chapter" } else { "One more volume" })
        .focusable(true)
        .size(38.0, 26.0)
        .radius(theme::R_SM)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            match s.manga_plus_one(id) {
                Ok(_) => s.publishing_rebuild(),
                Err(m) => s.error = Some(m),
            }
        })
        .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent))
}

/// A row's panel (and the details page's card body): the match, recent releases, the fix.
fn panel(s: &State, id: u32) -> El {
    col().key("pub/panel").w_full().px(theme::PAGE_PAD).pl(theme::PAGE_PAD + 48.0).pt(4.0).pb(14.0).gap(10.0).fill(PAL.surface).child(match_block(s, id, true))
}

/// The series matched, its releases, and the link field. `wide`: releases in one line.
fn match_block(s: &State, id: u32, wide: bool) -> El {
    let p = &s.publishing;
    let info = p.info.get(&id);
    let now = s.clock();
    let mut c = col().gap(8.0).w_full();
    let matched = info.and_then(|i| i.matched.as_ref());
    let what = match (matched, info.is_some_and(|i| i.looked)) {
        (Some(m), _) => {
            let how = match m.confidence {
                Confidence::Sure => "Matched by title",
                Confidence::Guess => "Best guess by title: please check",
                Confidence::Picked => "Your pick",
            };
            let url = mu::series_url(m.id);
            let how = text(how).font(theme::LABEL).color(if m.confidence == Confidence::Guess { PAL.warn() } else { PAL.dim });
            let name = text(m.title.clone()).font(theme::BODY_MED).color(PAL.text);
            let open = text("Open on MangaUpdates \u{2197}")
                        .key("pub/open-mu")
                        .role("link")
                        .label("Open on MangaUpdates")
                        .focusable(true)
                        .font(theme::LABEL_BTN)
                        .color(PAL.accent_hi)
                        .cursor(rune::Cursor::Pointer)
                        .on_press(move |s: &mut State| crate::online::open_url(s, url.clone()));
            if wide {
                row().gap(8.0).center_y().child(text("MangaUpdates:").font(theme::LABEL).color(PAL.faint)).child(name).child(text("\u{b7}").font(theme::LABEL).color(PAL.faint)).child(how).child(el().w(6.0)).child(open)
            } else {
                row().child(col().gap(3.0).child(text("ON MANGAUPDATES").font(theme::CAPTION).color(PAL.faint)).child(name.wrap()).child(how).child(open))
            }
        }
        (None, true) => row().child(text("No MangaUpdates series has this title. Paste its link below if it\u{2019}s there under another name.").font(theme::LABEL).color(PAL.dim).wrap()),
        (None, false) => row().child(text("Not looked up yet.").font(theme::LABEL).color(PAL.dim)),
    };
    c = c.child(what);
    if let Some(sum) = info.and_then(|i| i.summary(now)) {
        let mut rel = if wide { row().gap(14.0).flex_wrap() } else { col().gap(3.0) };
        for (label, day) in sum.recent.iter().take(if wide { 6 } else { 4 }) {
            let (_, m, d) = kotora_core::time::civil_from_days(day.div_euclid(DAY));
            rel = rel.child(row().gap(6.0).child(text(label.clone()).font(theme::LABEL_NUM).color(PAL.text)).child(text(format!("{} {d}", MONTHS[(m as usize + 11) % 12])).font(theme::LABEL_NUM).color(PAL.dim)));
        }
        if !sum.recent.is_empty() {
            c = c.child(col().gap(4.0).child(text("RECENT RELEASES").font(theme::CAPTION).color(PAL.faint)).child(rel));
        }
    }
    if let Some(e) = info.and_then(|i| i.error.as_ref()) {
        c = c.child(text(e.clone()).font(theme::LABEL).color(PAL.err()).wrap());
    }
    // The fix: paste a link (and Reset when picked).
    let field = row()
        .key("pub/link")
        .grow(1.0)
        .min_w(0.0)
        .h(30.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .child(
            text_input(&p.link, theme::LABEL)
                .key("pub/link-input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(crate::ui::field_hint(&p.link, "mangaupdates.com/series/\u{2026}"), PAL.faint)
                .on_edit(|s: &mut State| &mut s.publishing.link)
                .on_submit(move |s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    s.publishing_use_link(id);
                }),
        );
    let mut fix = row().gap(8.0).center_y().w_full().max_w(560.0).child(field);
    if p.linking {
        fix = fix.child(text("Checking\u{2026}").font(theme::LABEL).color(PAL.faint));
    } else {
        fix = fix.child(action(format!("pub/use/{id}"), "Use", true, move |s: &mut State, _| s.publishing_use_link(id)));
        if matched.is_some_and(|m| m.confidence == Confidence::Picked) {
            fix = fix.child(action(format!("pub/reset/{id}"), "Reset", false, move |s: &mut State, _| s.publishing_unpick(id)));
        }
    }
    let label = if matched.is_some() { "Wrong series? Paste the right MangaUpdates link:" } else { "Paste a MangaUpdates link:" };
    c = c.child(col().gap(4.0).child(text(label).font(theme::LABEL).color(PAL.dim)).child(fix));
    if let Some((failed, t)) = &p.link_note {
        c = c.child(text(t.clone()).key("pub/link-note").font(theme::LABEL).color(if *failed { PAL.err() } else { PAL.ok() }).wrap());
    }
    c
}

/// The details page's Publishing card, for a Reading manga (or one with data).
pub fn details_card(s: &State, id: u32) -> Option<El> {
    let e = s.manga.entries.iter().find(|e| e.id == id)?;
    let info = s.publishing.info.get(&id);
    if e.status != Status::Watching && info.is_none() {
        return None;
    }
    let now = s.clock();
    let r = row_of(id, e.chapters, info, now, s.publishing_today(now));
    let mut c = crate::ui::card("det-publishing", "Publishing");
    let kv = |k: &str, v: El| crate::ui::kv(k, v);
    let looking = r.look == Look::Pending && !s.publishing_offline();
    let latest = if r.latest.is_empty() { text(if looking { "Looking\u{2026}" } else { "\u{2013}" }).font(theme::LABEL_NUM).color(PAL.dim) } else { text(format!("{} \u{b7} {}", r.latest, r.when)).font(theme::LABEL_NUM).color(PAL.text) };
    let behind = match r.behind {
        Some(0) => "Caught up".to_string(),
        Some(n) => format!("{n} to read"),
        None => "\u{2013}".into(),
    };
    let next = if r.pace.is_empty() { "\u{2013}".to_string() } else if r.next.is_empty() { r.pace.clone() } else { format!("{}, {}", r.pace, r.next) };
    if let Some(n) = s.publishing_notice() {
        let fg = if s.publishing_offline() { PAL.warn() } else { PAL.err() };
        c = c.child(text(n).key("det-publishing/notice").font(theme::LABEL).color(fg).wrap().pb(4.0));
    }
    c = c
        .child(col().pt(4.0).child(kv("Latest", latest)).child(kv("To read", text(behind).font(theme::LABEL_NUM).color(PAL.text))).child(kv("Next (estimate)", text(next).font(theme::LABEL_NUM).color(PAL.text))))
        .child(el().h(6.0))
        .child(match_block(s, id, false))
        .child(text(format!("{}. Next dates are estimates from past releases.", mu::CREDIT)).font(theme::SMALL).color(PAL.faint).wrap().pt(8.0));
    Some(c)
}

// -------------------------------------------------------------------------------------- fakes

/// Made-up release data for the fake manga list (scenes and tests), at [`crate::model::FAKE_NOW`].
pub fn fake_info() -> Vec<(u32, Info)> {
    let now = crate::model::FAKE_NOW;
    let today = now.div_euclid(DAY) * DAY;
    let rel = |ch: &str, days_ago: i64| mu::Release { chapter: ch.into(), volume: None, day: today - days_ago * DAY, added: today - days_ago * DAY + 9 * 3600 };
    let weekly = |last: u32, ago: i64, n: u32| (0..n).map(|k| rel(&(last - k).to_string(), ago + 7 * i64::from(k))).collect::<Vec<_>>();
    let m = |id: u64, title: &str, c: Confidence| Some(Matched { id, title: title.into(), confidence: c });
    let info = |matched, releases| Info { matched, looked: true, releases, fetched_at: Some(now - 40 * 60), error: None };
    vec![
        // Inkwell Cartographers: read 119, weekly, 121 out yesterday.
        (1, info(m(81_000_000_001, "Inkwell Cartographers", Confidence::Sure), weekly(121, 1, 12))),
        // Salt & Gears: read 64, every two weeks, 74 out; a guessed match.
        (2, info(m(81_000_000_002, "Salt & Gears", Confidence::Guess), (0..10u32).map(|k| rel(&(74 - k).to_string(), 5 + 14 * i64::from(k))).collect())),
        // Moth Orchestra: "Night 259", monthly, quiet since spring: hiatus.
        (4, info(m(81_000_000_004, "Moth Orchestra", Confidence::Picked), (0..6).map(|k| rel(&format!("Night {}", 259 - k), 150 + 30 * i64::from(k))).collect())),
        // Paper Wolves of Osaka: "Bone 14", irregular.
        (5, info(m(81_000_000_005, "Paper Wolves of Osaka", Confidence::Sure), [3, 9, 30, 34, 60, 75, 77].iter().enumerate().map(|(k, d)| rel(&format!("Bone {}", 14 - k as u32), *d)).collect())),
        // Night Shift Alchemist (a light novel): volumes only.
        (6, info(m(81_000_000_006, "Night Shift Alchemist", Confidence::Sure), [(9, 20), (8, 140), (7, 262)].iter().map(|(v, d)| mu::Release { chapter: String::new(), volume: Some(v.to_string()), day: today - d * DAY, added: 0 }).collect())),
        // Kumo Atelier: no series found.
        (7, Info { matched: None, looked: true, releases: Vec::new(), fetched_at: None, error: None }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> State {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        s.page = Page::Publishing;
        s.publishing_fake(fake_info());
        s
    }

    fn row(s: &State, id: u32) -> PRow {
        s.publishing.rows.iter().find(|r| r.id == id).cloned().unwrap()
    }

    #[test]
    fn only_reading_manga_with_their_numbers_in_words() {
        let s = page();
        let reading: Vec<u32> = s.manga.entries.iter().filter(|e| e.status == Status::Watching).map(|e| e.id).collect();
        let mut ids: Vec<u32> = s.publishing.rows.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        let mut want = reading.clone();
        want.sort_unstable();
        assert_eq!(ids, want);
        let r = row(&s, 1);
        assert_eq!((r.latest.as_str(), r.when.as_str(), r.behind), ("Ch 121", "23 h ago", Some(2)));
        // Out Thursday (FAKE_NOW is a Friday): next ~Thursday.
        assert_eq!((r.pace.as_str(), r.next.as_str(), r.fresh), ("Weekly", "next ~Thu", true));
        let r = row(&s, 2);
        assert_eq!((r.latest.as_str(), r.behind, r.pace.as_str(), r.look), ("Ch 74", Some(10), "Every 2 weeks", Look::Found(Confidence::Guess)));
        let r = row(&s, 4);
        assert_eq!((r.latest.as_str(), r.pace.as_str(), r.next.as_str(), r.hiatus), ("Night 259", "On hiatus", "", true));
        let r = row(&s, 5);
        assert_eq!((r.latest.as_str(), r.pace.as_str()), ("Bone 14", "Irregular"));
        assert!(r.next.starts_with("about every"), "{}", r.next);
        assert_eq!(row(&s, 6).latest, "Vol 9");
        assert_eq!(row(&s, 6).behind, None, "volumes only: no chapter count");
        assert_eq!(row(&s, 7).look, Look::NotFound);
    }

    #[test]
    fn sorts() {
        let mut s = page();
        let order = |s: &State| s.publishing.rows.iter().map(|r| r.id).collect::<Vec<_>>();
        // Next release: soonest first, hiatus and unknown last.
        let next = order(&s);
        assert_eq!(next[0], 1, "{next:?}");
        assert!(next.iter().position(|&i| i == 4) > next.iter().position(|&i| i == 2));
        s.set_publishing_sort(PSort::Behind);
        assert_eq!(order(&s)[0], 4, "Night 259 with 201 read: the most to read");
        s.set_publishing_sort(PSort::Latest);
        assert_eq!(order(&s)[0], 1);
    }

    #[test]
    fn plus_one_updates_the_count_to_read() {
        let mut s = page();
        assert_eq!(row(&s, 1).behind, Some(2));
        s.manga_plus_one(1).unwrap();
        s.publishing_rebuild();
        assert_eq!(row(&s, 1).behind, Some(1));
    }

    #[test]
    fn a_bad_link_is_said_and_nothing_is_asked() {
        let mut s = page();
        s.publishing_toggle(2);
        s.publishing.link = TextEdit::new("https://example.com/series/abc");
        s.publishing_use_link(2);
        assert!(s.publishing.link_note.as_ref().is_some_and(|(f, t)| *f && t.contains("isn\u{2019}t a MangaUpdates series link")));
        assert!(!s.publishing.linking);
        assert!(s.publishing.tx.is_none(), "headless: no worker");
    }

    #[test]
    fn notifications_need_the_switch_and_an_unread_chapter() {
        let mut s = page();
        let shown = |s: &State| s.notes.log.iter().filter(|x| matches!(x, crate::notes::Sent::Show(_))).count();
        s.publishing_notes(&[(1, 121, "Ch 121".into())]);
        assert_eq!(shown(&s), 0, "off by default");
        s.prefs.notify.new_chapter = true;
        s.publishing_notes(&[(1, 121, "Ch 121".into()), (1, 119, "Ch 119".into()), (9, 97, "Ch 97".into())]);
        let n: Vec<Note> = s.notes.log.iter().filter_map(|x| if let crate::notes::Sent::Show(n) = x { Some(n.clone()) } else { None }).collect();
        assert_eq!(n.len(), 1, "read already, and not Reading: nothing");
        assert_eq!(n[0].summary, "New chapter of Inkwell Cartographers: Ch 121");
        assert_eq!(n[0].body, "2 chapters to read. Release data: MangaUpdates.");
        assert_eq!(n[0].kind, crate::notes::Kind::Chapter);
        // A click opens the manga's page.
        assert!(s.note_action(&n[0].id, Act::Open));
        assert_eq!(s.page, Page::Details);
        assert_eq!(s.detail.as_ref().map(|d| d.stub.id), Some(1));
    }

    #[test]
    fn kot_publishing_lists_the_rows_in_order() {
        let s = page();
        let items = s.ctl_publishing();
        assert_eq!(items.len(), s.publishing.rows.len());
        assert_eq!((items[0].title.as_str(), items[0].latest.as_str(), items[0].behind), ("Inkwell Cartographers", "Ch 121", Some(2)));
        assert!(items.iter().any(|i| i.flag == "check match"));
        assert!(items.iter().any(|i| i.flag == "no match"));
    }

    #[test]
    fn the_offline_notice_matches_the_airing_page() {
        let now = 1_800_000_000;
        assert_eq!(notice(true, None, Some(now - 3 * 3600), now).as_deref(), Some("Offline: MangaUpdates data from 3 hours ago; can\u{2019}t refresh now."));
        assert_eq!(notice(true, None, Some(now - 3600), now).as_deref(), Some("Offline: MangaUpdates data from 1 hour ago; can\u{2019}t refresh now."));
        assert_eq!(notice(true, None, Some(now - 600), now).as_deref(), Some("Offline: MangaUpdates data from less than an hour ago; can\u{2019}t refresh now."));
        assert_eq!(notice(true, None, Some(now - 2 * DAY), now).as_deref(), Some("Offline: MangaUpdates data from 2 days ago; can\u{2019}t refresh now."));
        // Offline wins over a refresh's error.
        assert_eq!(notice(true, Some("Can\u{2019}t reach MangaUpdates."), None, now).as_deref(), Some("Offline: release data comes from MangaUpdates."));
        assert_eq!(notice(false, Some("Can\u{2019}t reach MangaUpdates."), Some(now), now).as_deref(), Some("Can\u{2019}t reach MangaUpdates. Showing the saved releases."));
        assert_eq!(notice(false, Some("Can\u{2019}t reach MangaUpdates."), None, now).as_deref(), Some("Can\u{2019}t reach MangaUpdates."));
        assert_eq!(notice(false, None, Some(now), now), None);
    }

    #[test]
    fn offline_the_page_and_card_say_so_and_keep_the_saved_rows() {
        // KOTORA_OFFLINE=1 (the Docker check) forces offline: the online parts can't hold there.
        let forced = paths::offline_forced();
        let mut s = page();
        if !forced {
            assert_eq!(s.publishing_notice(), None);
        }
        s.offline = true;
        s.publishing.checked_at = Some(s.clock() - 3 * 3600);
        assert_eq!(s.publishing_notice().as_deref(), Some("Offline: MangaUpdates data from 3 hours ago; can\u{2019}t refresh now."));
        assert_eq!(row(&s, 1).latest, "Ch 121", "the saved data shows");
        // No link check offline.
        s.publishing_toggle(2);
        s.publishing.link = TextEdit::new("https://www.mangaupdates.com/series/pb8uwds/x");
        s.publishing_use_link(2);
        assert!(s.publishing.link_note.as_ref().is_some_and(|(f, t)| *f && t.starts_with("Offline")));
        assert!(!s.publishing.linking);
        s.publishing_unpick(2);
        assert!(!s.publishing.linking);
        // Nothing saved: where the data comes from.
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        s.offline = true;
        s.publishing_fake(Vec::new());
        assert_eq!(s.publishing_notice().as_deref(), Some("Offline: release data comes from MangaUpdates."));
        assert!(!s.publishing.rows.is_empty(), "the Reading manga still list");
        // The details card has the same words.
        let mut s = page();
        s.offline = true;
        assert!(details_card(&s, 1).is_some());
        // A failed refresh online: the error and the saved releases.
        s.offline = false;
        s.publishing.error = Some("Can\u{2019}t reach MangaUpdates.".into());
        if !forced {
            assert_eq!(s.publishing_notice().as_deref(), Some("Can\u{2019}t reach MangaUpdates. Showing the saved releases."));
        }
    }

    /// A transport that counts requests and has no answers (any request fails).
    struct Never(std::rc::Rc<std::cell::Cell<usize>>);
    impl kotora_core::http::Transport for Never {
        fn send(&self, _: &kotora_core::http::Request) -> Result<kotora_core::http::Response, String> {
            self.0.set(self.0.get() + 1);
            Err("no network in tests".into())
        }
    }

    fn label(a: Answer) -> String {
        match a {
            Answer::Saved(v) => format!("saved {}", v.len()),
            Answer::Started(n) => format!("started {n}"),
            Answer::One { media_id, .. } => format!("one {media_id}"),
            Answer::Done { error, dues } => format!("done {error:?} {dues:?}"),
            Answer::Picked { .. } => "picked".into(),
        }
    }

    /// Saved data for Salt & Gears (id 2, read 64): chapter 65 added a minute before `now`.
    fn saved_scratch(name: &str, now: i64) -> (Cache, Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kotora-pub-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::new(dir.clone());
        let mut store = Store::open_memory().unwrap();
        let mut items = crate::manga::fake_items();
        items.retain(|it| it.media.id == 2);
        store.replace_list(Scope::new("mal", Kind::Manga), &items, 1).unwrap();
        let key = Key { service: kotora_core::service::ServiceId::Mal, media_id: 2 };
        cache.put_match(key, Some(&Matched { id: 5, title: "Salt & Gears".into(), confidence: Confidence::Sure }), now - 60);
        let rel = |ch: u32, d: i64| mu::Release { chapter: ch.to_string(), volume: None, day: (now - d * DAY).div_euclid(DAY) * DAY, added: now - d * DAY - 60 };
        cache.put_releases(5, &[rel(65, 0), rel(64, 7), rel(63, 14), rel(62, 21)], now - 60);
        (cache, store, dir)
    }

    #[test]
    fn offline_a_pass_asks_nothing_and_announces_nothing() {
        fn at() -> i64 {
            1_794_916_800
        }
        let (cache, store, dir) = saved_scratch("offline", at());
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut client = mu::Client::without_sleep(Never(asked.clone()));
        let got = std::cell::RefCell::new(Vec::new());
        let send = |a: Answer| got.borrow_mut().push(label(a));
        let svc = kotora_core::service::ServiceId::Mal;
        // Offline, even forced: the saved data at once, no request, no notification.
        one_pass(&mut client, &cache, &store, svc, true, None, at, false, &send);
        assert_eq!(*got.borrow(), ["saved 1", "done None []"]);
        assert_eq!(asked.get(), 0);
        // Back online (the data is fresh): still no request, and now the chapter is announced, once.
        got.borrow_mut().clear();
        one_pass(&mut client, &cache, &store, svc, false, None, at, true, &send);
        assert_eq!(*got.borrow(), ["saved 1", "done None [(2, 65, \"Ch 65\")]"]);
        assert_eq!(asked.get(), 0);
        // A refresh online that fails: the error, the saved data kept.
        got.borrow_mut().clear();
        one_pass(&mut client, &cache, &store, svc, true, None, at, true, &send);
        assert_eq!(*got.borrow(), ["saved 1", "started 1", "done Some(\"Can\u{2019}t reach MangaUpdates.\") []"]);
        assert_eq!(mu::saved(&cache, Key { service: svc, media_id: 2 }, None).releases.len(), 4, "the saved releases stay");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kot_reads_the_saved_files_and_says_offline() {
        let now = 1_794_916_800;
        let (cache, store, dir) = saved_scratch("kot", now);
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        s.fixed_now = Some(now);
        s.offline = true;
        // Nothing in memory yet: kot fills in from the saved files (no request is possible here).
        let snap = s.ctl_publishing_snapshot();
        let (items, note, read) = ctl_reply(snap, &cache, Some(&store), kotora_core::service::ServiceId::Mal);
        assert_eq!(read.len(), 1, "Salt & Gears was saved");
        let salt = items.iter().find(|i| i.title == "Salt & Gears").unwrap();
        assert_eq!((salt.latest.as_str(), salt.behind), ("Ch 65", Some(1)));
        assert_eq!(note.as_deref(), Some("Offline: MangaUpdates data from less than an hour ago; can\u{2019}t refresh now."));
        // The page gets what kot read.
        s.publishing_answer(Answer::Saved(read));
        assert!(s.publishing.info.contains_key(&2));
        // Nothing saved at all, offline.
        let empty = Cache::new(dir.join("none"));
        let (items, note, read) = ctl_reply(State::fake_publishing_offline_snapshot(), &empty, None, kotora_core::service::ServiceId::Mal);
        assert!(read.is_empty());
        assert!(!items.is_empty(), "the Reading manga still list");
        assert_eq!(note.as_deref(), Some("Offline: release data comes from MangaUpdates."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_timer_checks_nothing_offline() {
        assert!(timer_pass(true, true));
        assert!(!timer_pass(true, false), "offline: no check");
        assert!(!timer_pass(false, true), "switch off: no check");
        // The flag the worker reads follows the app's offline state.
        let mut s = page();
        s.offline = true;
        s.publishing.note_offline(s.publishing_offline());
        assert!(!online(&s.publishing.offline));
        s.offline = false;
        s.publishing.note_offline(s.publishing_offline());
        assert_eq!(online(&s.publishing.offline), !paths::offline_forced());
    }

    #[test]
    fn days_in_words() {
        let today = 20_000;
        assert_eq!(day_words(today * DAY, today), "today");
        assert_eq!(day_words((today + 1) * DAY, today), "~tomorrow");
        assert_eq!(day_words((today + 3) * DAY, today), format!("~{}", SHORT_DAYS[usize::from(kotora_core::time::weekday(today + 3))]));
        assert!(day_words((today + 20) * DAY, today).starts_with('~'));
        assert_eq!(ago(100, 100), "just now");
        assert_eq!(ago(0, 2 * DAY), "2 days ago");
    }

    /// The worker's pass against made-up answers: saved first, then MangaUpdates, then a
    /// chapter announced once.
    #[test]
    fn a_pass_asks_only_for_stale_series_and_announces_once() {
        use kotora_core::http::{Request, Response, Transport};
        use std::cell::RefCell;
        struct Rec(RefCell<Vec<String>>, RefCell<std::collections::VecDeque<String>>);
        impl Transport for Rec {
            fn send(&self, req: &Request) -> Result<Response, String> {
                self.0.borrow_mut().push(req.url.clone());
                let body = self.1.borrow_mut().pop_front().ok_or("no more")?;
                Ok(Response { status: 200, retry_after: None, body })
            }
        }
        let dir = std::env::temp_dir().join(format!("kotora-pub-pass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::new(dir.clone());
        let mut store = Store::open_memory().unwrap();
        let svc = kotora_core::service::ServiceId::Mal;
        let mut items = crate::manga::fake_items();
        items.retain(|it| it.media.id == 2);
        store.replace_list(Scope::new("mal", Kind::Manga), &items, 1).unwrap();
        // Salt & Gears, read 64: a search, then releases with chapter 65 just added.
        let now = 1_794_916_800_i64;
        let search = r#"{"results":[{"record":{"series_id":5,"title":"Salt & Gears","type":"Manhwa","year":""},"hit_title":"Salt & Gears"}]}"#;
        let rel = |ch: u32, d: i64| format!(r#"{{"record":{{"chapter":"{ch}","release_date":"{}","time_added":{{"timestamp":{}}}}}}}"#, date(now - d * DAY), now - d * DAY);
        let releases = format!(r#"{{"results":[{},{},{},{}]}}"#, rel(65, 0), rel(64, 7), rel(63, 14), rel(62, 21));
        let mut client = mu::Client::without_sleep(Rec(RefCell::new(Vec::new()), RefCell::new([search.to_string(), releases].into())));
        let got = RefCell::new(Vec::new());
        let send = |a: Answer| {
            got.borrow_mut().push(match a {
                Answer::Saved(v) => format!("saved {}", v.len()),
                Answer::Started(n) => format!("started {n}"),
                Answer::One { media_id, .. } => format!("one {media_id}"),
                Answer::Done { error, dues } => format!("done {error:?} {dues:?}"),
                Answer::Picked { .. } => "picked".into(),
            })
        };
        fn at() -> i64 {
            1_794_916_800 + 60
        }
        one_pass(&mut client, &cache, &store, svc, false, None, at, true, &send);
        assert_eq!(*got.borrow(), ["saved 1", "started 1", "one 2", "done None [(2, 65, \"Ch 65\")]"]);
        // Again: nothing asked (fresh), nothing announced twice.
        got.borrow_mut().clear();
        one_pass(&mut client, &cache, &store, svc, false, None, at, true, &send);
        assert_eq!(*got.borrow(), ["saved 1", "done None []"]);
        // Offline: the saved files only.
        got.borrow_mut().clear();
        one_pass(&mut client, &cache, &store, svc, true, None, at, false, &send);
        assert_eq!(*got.borrow(), ["saved 1", "done None []"]);
        let _ = std::fs::remove_dir_all(&dir);

        fn date(t: i64) -> String {
            let (y, m, d) = kotora_core::time::civil_from_days(t.div_euclid(DAY));
            format!("{y}-{m:02}-{d:02}")
        }
    }
}
