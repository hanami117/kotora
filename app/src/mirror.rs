//! The account mirror's UI and worker (core's `mirror.rs` does the work).
//!
//! - Settings → Account → "Mirror edits to <other>": the switch. Turning it on reads the other
//!   service's list (and looks MyAnimeList ids up on AniList where needed, 50 to a request), then
//!   shows the preview with the three choices. Nothing is saved or sent until one is picked.
//!   Turning it off drops the mirror's changes that still wait.
//! - The mirror's worker thread (`kotora-mirror`) sends the mirror's queue at the mirror
//!   service's own pace, looks up AniList ids of new adds, and reads the mirror's list after
//!   each main sync to find edits made on its website. It runs only while there is work, like
//!   the main queue's worker, and is separate from it: a failure on one side never blocks the
//!   other.
//! - The Queue page shows the mirror's part under the main queue: website edits to settle,
//!   conflicts, and what waits or failed.
//!
//! Nothing here runs headless: without an app handle the commands do nothing; scenes set the
//! state.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kotora_core::mirror::{self, Align, Link, Preview, Settle, Wait, WebEdit};
use kotora_core::queue::{self, Op, OpState, Step};
use kotora_core::service::{ListService, ServiceError, ServiceId};
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::{Kind, ListItem, ScoreFormat, Status, Values, paths};
use rune::{El, col, el, row, text};

use crate::model::{Handle, State};
use crate::theme::{self, PAL};
use crate::transfer::{count_pill, group, line_tagged, plural};
use crate::ui::{action, cell, small_button, switch_if};

/// The mirror as the window shows it.
#[derive(Default)]
pub struct MirrorUi {
    /// The saved link (`None`: off).
    pub link: Option<Link>,
    /// The other service has a saved sign-in (read on a worker).
    pub other_signed_in: bool,
    /// Reading, comparing or saving: what is going on.
    pub busy: Option<String>,
    pub error: Option<String>,
    pub preview: Option<Prev>,
    /// After turning it on or off: what happened.
    pub note: Option<String>,
    /// The mirror's queue: open changes, then those sent in the last day.
    pub ops: Vec<Op>,
    pub edits: Vec<WebEdit>,
    pub waits: Vec<Wait>,
    pub synced_at: Option<i64>,
    /// How the mirror service shows scores (AniList: the saved profile's format; MyAnimeList:
    /// 1-10).
    pub fmt: ScoreFormat,
    /// The mirror service's last problem (offline, sign in again), in plain words.
    pub trouble: Option<String>,
    /// The manga lists were compared (turned on with Manga on, or "Align manga" since).
    pub manga_aligned: bool,
    pub worker: Arc<Worker>,
}

/// The two lists compared, waiting for the user's pick.
pub struct Prev {
    pub p: Preview,
    /// The mirror's list as read (saved only when a choice is made).
    pub fetched: Vec<ListItem>,
    /// Merge: MyAnimeList ids whose status conflict takes the mirror's status.
    pub mirror_status: HashSet<u32>,
    /// With Manga on: the manga lists, compared too (one choice covers both).
    pub manga: Option<Box<Prev>>,
    /// The mirror is on already: these lists (the manga ones) are aligned now, nothing else.
    pub extend: bool,
}

impl Prev {
    /// The part comparing the lists of `kind`.
    fn part_mut(&mut self, kind: Kind) -> Option<&mut Prev> {
        if self.p.kind == kind {
            return Some(self);
        }
        self.manga.as_deref_mut().filter(|m| m.p.kind == kind)
    }

    /// Every part, anime first.
    fn parts(&self) -> Vec<&Prev> {
        std::iter::once(self).chain(self.manga.as_deref()).collect()
    }

    /// What a choice queues, all parts together: (for the mirror, for the main list).
    fn actions(&self, align: Align) -> (Vec<kotora_core::listxml::Action>, Vec<kotora_core::listxml::Action>) {
        let (mut to, mut main) = (Vec::new(), Vec::new());
        for part in self.parts() {
            let (t, m) = mirror::align_actions(&part.p, align, &part.mirror_status);
            to.extend(t);
            main.extend(m);
        }
        (to, main)
    }
}

impl MirrorUi {
    /// What needs the user on the Queue page: website edits, conflicts and failures.
    pub fn attention(&self) -> u32 {
        self.edits.len() as u32 + self.ops.iter().filter(|o| matches!(o.state, OpState::Conflict | OpState::Failed)).count() as u32
    }

    /// The mirror's part of the Sync queue badge, counted like the main queue's (every change
    /// not sent yet: waiting, sending, in conflict or failed), plus website edits to settle and
    /// adds still looking up their AniList id. 0 while the mirror is off.
    pub fn badge(&self) -> u32 {
        if self.link.is_none() {
            return 0;
        }
        (self.edits.len() + self.waiting() + self.waits.iter().filter(|w| w.error.is_none()).count()) as u32
    }

    /// Changes still waiting for the mirror.
    pub fn waiting(&self) -> usize {
        self.ops.iter().filter(|o| o.state.is_open()).count()
    }

    fn has_queue_items(&self) -> bool {
        self.link.is_some() && (!self.ops.is_empty() || !self.edits.is_empty() || !self.waits.is_empty())
    }
}

// ------------------------------------------------------------------------------------ worker

#[derive(Clone, Debug)]
pub enum Cmd {
    /// Send what waits now; `sync`: then read the mirror's list (after a main sync).
    Kick { sync: bool },
    Settle(Kind, u32, Settle),
    KeepMine(i64),
    TakeTheirs(i64),
    Discard(i64),
}

#[derive(Default)]
pub struct Worker {
    slot: Mutex<Option<Sender<Cmd>>>,
    /// The mirror's sign-in is gone: send nothing until it is back.
    hold: AtomicBool,
}

/// Hands `cmd` to the mirror's worker, starting it if needed. Nothing when the mirror is off.
pub fn send(s: &mut State, cmd: Cmd) {
    let (Some(h), Some(link)) = (s.handle.clone(), s.mirror.link) else { return };
    let w = s.mirror.worker.clone();
    let mut slot = w.slot.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = match slot.as_ref() {
        Some(tx) => match tx.send(cmd) {
            Ok(()) => return,
            Err(mpsc::SendError(cmd)) => cmd,
        },
        None => cmd,
    };
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(cmd);
    *slot = Some(tx);
    drop(slot);
    let w2 = w.clone();
    if std::thread::Builder::new().name("kotora-mirror".into()).spawn(move || run(&h, &w2, link, &rx)).is_err() {
        *w.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// From another worker (the main queue, a sync): wake the mirror's worker.
pub fn kick(h: &Handle, sync: bool) {
    h.update_if(move |s, _| {
        send(s, Cmd::Kick { sync });
        false
    });
}

/// The mirror's state from the store, for the window.
struct Snapshot {
    link: Option<Link>,
    ops: Vec<Op>,
    edits: Vec<WebEdit>,
    waits: Vec<Wait>,
    synced_at: Option<i64>,
    fmt: ScoreFormat,
    manga_aligned: bool,
}

fn snapshot(store: &Store) -> Snapshot {
    let link = store.mirror_link().ok().flatten();
    let to = link.map(|l| l.to.name());
    Snapshot {
        link,
        ops: to.and_then(|t| store.queue(t, now_unix()).ok()).unwrap_or_default(),
        edits: store.web_edits().unwrap_or_default(),
        waits: store.mirror_waits().unwrap_or_default(),
        synced_at: to.and_then(|t| store.synced_at(t).ok().flatten()),
        fmt: match link.map(|l| l.to) {
            Some(ServiceId::AniList) => store.profile(ServiceId::AniList.name()).ok().flatten().map(|p| p.score_format).unwrap_or_default(),
            _ => ScoreFormat::Point10,
        },
        manga_aligned: store.mirror_manga_aligned().unwrap_or(false),
    }
}

fn report(h: &Handle, snap: Snapshot, trouble: Option<Option<String>>) {
    h.update_if(move |s, _| {
        s.mirror.link = snap.link;
        s.mirror.ops = snap.ops;
        s.mirror.edits = snap.edits;
        s.mirror.waits = snap.waits;
        s.mirror.synced_at = snap.synced_at;
        s.mirror.fmt = snap.fmt;
        s.mirror.manga_aligned = snap.manga_aligned;
        if let Some(t) = trouble {
            s.mirror.trouble = t;
        }
        true
    });
}

fn trouble_text(to: ServiceId, e: &ServiceError) -> String {
    match e {
        ServiceError::SignInAgain => format!("Sign in to {} again: the mirror waits until then.", to.title()),
        ServiceError::Offline(_) => format!("Can\u{2019}t reach {}. Its changes wait and go out when it answers.", to.title()),
        ServiceError::RateLimited => format!("{} asked Kotora to slow down. Its changes go out in a minute.", to.title()),
        ServiceError::Other(m) => format!("{}: {m}", to.title()),
    }
}

/// One command on the store. "Use the mirror's" for an anime not on an AniList main list looks
/// its AniList media up first (one request).
fn apply(store: &mut Store, link: Link, cmd: Cmd, want_sync: &mut bool) -> Result<(), String> {
    let now = now_unix();
    let to = link.to.name();
    let r = match cmd {
        Cmd::Kick { sync } => {
            *want_sync |= sync;
            store.retry_now(to)
        }
        Cmd::KeepMine(id) => store.keep_mine(id),
        Cmd::TakeTheirs(id) => store.take_theirs(to, id, now),
        Cmd::Discard(id) => store.discard(to, id, now),
        Cmd::Settle(kind, mal, how) => {
            let mut media = None;
            let missing = store.web_edits().unwrap_or_default().iter().any(|e| e.kind == kind && e.mal_id == mal && e.main_id.is_none());
            if how == Settle::UseMirror && missing && link.main == ServiceId::AniList {
                let mut svc = crate::account::open_for(link.main).ok_or_else(|| format!("{} is needed to add it: sign in, and check you are online.", link.main.title()))?;
                media = svc.media_by_mal_ids_of(kind, &[mal]).map_err(|e| trouble_text(link.main, &e))?.into_iter().next();
            }
            store.settle_web_edit(kind, mal, how, media.as_ref(), now)
        }
    };
    r.map_err(|e| e.to_string())
}

fn run(h: &Handle, w: &Worker, link: Link, rx: &Receiver<Cmd>) {
    let mut store = match Store::open(&paths::db_file()) {
        Ok(s) => s,
        Err(e) => {
            *w.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let msg = format!("Can\u{2019}t open the saved list: {e}");
            h.update_if(move |s, _| {
                s.mirror.trouble = Some(msg);
                true
            });
            return;
        }
    };
    let _ = store.queue_start(link.to.name(), now_unix());
    let mut svc: Option<crate::account::Service> = None;
    let mut want_sync = false;
    let mut next: Option<Cmd> = None;
    loop {
        let mut changed = false;
        while let Some(cmd) = next.take().or_else(|| rx.try_recv().ok()) {
            if matches!(cmd, Cmd::Kick { .. }) {
                w.hold.store(false, Ordering::Relaxed);
            }
            match apply(&mut store, link, cmd, &mut want_sync) {
                Ok(()) => changed = true,
                Err(m) => h.update_if(move |s, _| {
                    s.mirror.trouble = Some(m);
                    true
                }),
            }
        }
        // Turned off or switched meanwhile: stop.
        if store.mirror_link().ok().flatten() != Some(link) {
            report(h, snapshot(&store), None);
            break;
        }
        if changed {
            report(h, snapshot(&store), None);
        }
        if svc.is_none() && !w.hold.load(Ordering::Relaxed) {
            svc = crate::account::open_for(link.to);
        }
        let Some(service) = svc.as_deref_mut() else {
            report(h, snapshot(&store), Some(Some(format!("Not signed in to {}: its changes wait.", link.to.title()))));
            if end_if_idle(w, rx, &mut next) { break } else { continue }
        };
        // New adds whose AniList id isn't known yet.
        if store.mirror_waits().is_ok_and(|w| w.iter().any(|x| x.error.is_none())) {
            match mirror::map_waits(service, &mut store, now_unix()) {
                Ok(_) => report(h, snapshot(&store), None),
                Err(e) => {
                    let hold = e == ServiceError::SignInAgain;
                    report(h, snapshot(&store), Some(Some(trouble_text(link.to, &e))));
                    if hold {
                        w.hold.store(true, Ordering::Relaxed);
                        svc = None;
                        if end_if_idle(w, rx, &mut next) { break } else { continue }
                    }
                }
            }
        }
        let wait = match queue::step(&mut store, service, now_unix()) {
            Ok(Step::Handled { outcome, .. }) => {
                let t = match outcome {
                    queue::Outcome::Retry(m) => Some(Some(trouble_text(link.to, &ServiceError::Offline(m)))),
                    queue::Outcome::Sent | queue::Outcome::AlreadyThere => Some(None),
                    _ => None,
                };
                report(h, snapshot(&store), t);
                continue;
            }
            Ok(Step::Wait(secs)) => Some(Duration::from_secs(secs.clamp(1, queue::MAX_BACKOFF) as u64)),
            Ok(Step::Idle) if want_sync => {
                want_sync = false;
                let t = match mirror::sync_mirror(service, &mut store, now_unix()) {
                    Ok(_) => None,
                    Err(e) => Some(trouble_text(link.to, &e)),
                };
                report(h, snapshot(&store), Some(t));
                continue;
            }
            Ok(Step::Idle) => None,
            Ok(Step::SignInAgain) => {
                w.hold.store(true, Ordering::Relaxed);
                svc = None;
                report(h, snapshot(&store), Some(Some(trouble_text(link.to, &ServiceError::SignInAgain))));
                None
            }
            Err(e) => {
                report(h, snapshot(&store), Some(Some(format!("The mirror\u{2019}s queue can\u{2019}t be saved: {e}"))));
                None
            }
        };
        match wait {
            Some(d) => match rx.recv_timeout(d) {
                Ok(cmd) => next = Some(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => {
                if end_if_idle(w, rx, &mut next) {
                    break;
                }
            }
        }
    }
}

/// Nothing to do: the thread ends, unless a command slipped in (senders hold the lock).
fn end_if_idle(w: &Worker, rx: &Receiver<Cmd>, next: &mut Option<Cmd>) -> bool {
    let mut slot = w.slot.lock().unwrap_or_else(|e| e.into_inner());
    match rx.try_recv() {
        Ok(cmd) => {
            *next = Some(cmd);
            false
        }
        Err(_) => {
            *slot = None;
            true
        }
    }
}

// --------------------------------------------------------------------------- load, on, off

/// At start and after a sign-in or a switch: the saved link, the mirror's queue, and whether
/// the other service is signed in. Then the mirror's worker sends what waits.
pub fn load(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let main = s.service;
    let _ = std::thread::Builder::new().name("kotora-mirror-load".into()).spawn(move || {
        let other = Link::of(main).to;
        let signed = crate::account::has_token(other);
        let Ok(store) = Store::open(&paths::db_file()) else { return };
        // A link of the other direction (made before a switch) is not this list's.
        if store.mirror_link().ok().flatten().is_some_and(|l| l.main != main) {
            let _ = store.unlink_mirror();
        }
        let snap = snapshot(&store);
        h.update_if(move |s, _| {
            s.mirror.other_signed_in = signed;
            let on = snap.link.is_some();
            s.mirror.link = snap.link;
            s.mirror.ops = snap.ops;
            s.mirror.edits = snap.edits;
            s.mirror.waits = snap.waits;
            s.mirror.synced_at = snap.synced_at;
            s.mirror.manga_aligned = snap.manga_aligned;
            if on {
                send(s, Cmd::Kick { sync: false });
            }
            true
        });
    });
}

/// A switch of services: the link goes (the old mirror is now the active list, and its waiting
/// changes go out with it), and the window forgets the mirror.
pub fn unlink(s: &mut State) {
    let had = s.mirror.link.take().is_some();
    s.mirror = MirrorUi { worker: s.mirror.worker.clone(), ..MirrorUi::default() };
    if had && s.handle.is_some() {
        let _ = std::thread::spawn(|| {
            if let Ok(store) = Store::open(&paths::db_file()) {
                let _ = store.unlink_mirror();
            }
        });
    }
}

/// The switch.
fn set(s: &mut State, on: bool) {
    if on { begin(s) } else { stop(s) }
}

/// On: read the other list and compare (a thread). Nothing is saved yet.
fn begin(s: &mut State) {
    if s.mirror.busy.is_some() || s.mirror.link.is_some() || s.mirror.preview.is_some() {
        return;
    }
    let link = Link::of(s.service);
    s.mirror.error = None;
    s.mirror.note = None;
    s.mirror.busy = Some(format!("Reading your {} list\u{2026}", link.to.title()));
    let Some(h) = s.handle.clone() else { return };
    let _ = std::thread::Builder::new().name("kotora-mirror-compare".into()).spawn(move || {
        let r = compare(&h, link);
        h.update_if(move |s, _| {
            s.mirror.busy = None;
            match r {
                Ok(p) => s.mirror.preview = Some(p),
                Err(e) => s.mirror.error = Some(e),
            }
            true
        });
    });
}

fn busy(h: &Handle, msg: String) {
    h.update_if(move |s, _| {
        s.mirror.busy = Some(msg);
        true
    });
}

/// Reads the mirror's anime list (and its manga list, with Manga on) and compares each with
/// the saved main list (a thread).
fn compare(h: &Handle, link: Link) -> Result<Prev, String> {
    let manga = Store::open(&paths::db_file()).ok().and_then(|st| st.manga_enabled().ok()).unwrap_or(false);
    let mut p = compare_kind(h, link, Kind::Anime)?;
    if manga {
        busy(h, format!("Reading your {} manga list\u{2026}", link.to.title()));
        p.manga = Some(Box::new(compare_kind(h, link, Kind::Manga)?));
    }
    Ok(p)
}

/// Reads the mirror's list of `kind` and compares it with the saved main list of that kind.
fn compare_kind(h: &Handle, link: Link, kind: Kind) -> Result<Prev, String> {
    let open = |id: ServiceId| crate::account::open_for(id).ok_or_else(|| format!("Sign in to {} first, and check you are online.", id.title()));
    let mut to = open(link.to)?;
    let fetched = to.fetch_list_of(kind).map_err(|e| trouble_text(link.to, &e))?;
    let store = Store::open(&paths::db_file()).map_err(|e| format!("Can\u{2019}t open the saved list: {e}"))?;
    let main = store.load_list((link.main.name(), kind)).map_err(|e| format!("Can\u{2019}t read the saved list: {e}"))?;
    let lookup = |svc: &mut dyn ListService, ids: Vec<u32>| -> Result<HashMap<u32, kotora_core::Media>, String> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        busy(h, format!("Looking up {} on {}\u{2026}", plural(ids.len(), "MyAnimeList id", "MyAnimeList ids"), svc.title()));
        let found = svc.media_by_mal_ids_of(kind, &ids).map_err(|e| format!("Can\u{2019}t look the ids up on {}: {e}", svc.title()))?;
        Ok(found.into_iter().filter_map(|m| Some((m.id_mal?, m))).collect())
    };
    let mirror_media = if link.to == ServiceId::AniList { lookup(to.as_mut(), mirror::ids_only_on_main_of(link, kind, &main, &fetched))? } else { HashMap::new() };
    let main_media = if link.main == ServiceId::AniList { lookup(open(link.main)?.as_mut(), mirror::ids_only_on_mirror(link, &main, &fetched))? } else { HashMap::new() };
    let p = mirror::compare_of(link, kind, &main, &fetched, &mirror_media, &main_media);
    Ok(Prev { p, fetched, mirror_status: HashSet::new(), manga: None, extend: false })
}

/// "Align manga…": a mirror that is on, Manga turned on since: compare the manga lists now
/// (a thread), with the same three choices. Nothing is saved yet.
fn begin_manga(s: &mut State) {
    if s.mirror.busy.is_some() || s.mirror.preview.is_some() {
        return;
    }
    let Some(link) = s.mirror.link else { return };
    s.mirror.error = None;
    s.mirror.note = None;
    s.mirror.busy = Some(format!("Reading your {} manga list\u{2026}", link.to.title()));
    let Some(h) = s.handle.clone() else { return };
    let _ = std::thread::Builder::new().name("kotora-mirror-compare".into()).spawn(move || {
        let r = compare_kind(&h, link, Kind::Manga).map(|p| Prev { extend: true, ..p });
        h.update_if(move |s, _| {
            s.mirror.busy = None;
            match r {
                Ok(p) => s.mirror.preview = Some(p),
                Err(e) => s.mirror.error = Some(e),
            }
            true
        });
    });
}

/// A choice in the preview: the mirror's list is saved, the alignment queued, the link saved.
fn choose(s: &mut State, align: Align) {
    let Some(prev) = s.mirror.preview.take() else { return };
    let (to, main) = prev.actions(align);
    let link = prev.p.link;
    let extend = prev.extend;
    s.mirror.busy = Some("Saving\u{2026}".into());
    let Some(h) = s.handle.clone() else { return };
    // The mirror's lists as read, each saved under its kind.
    let fetched: Vec<(Kind, Vec<ListItem>)> = prev.parts().into_iter().map(|p| (p.p.kind, p.fetched.clone())).collect();
    let with_manga = fetched.iter().any(|(k, _)| *k == Kind::Manga);
    let _ = std::thread::Builder::new().name("kotora-mirror-start".into()).spawn(move || {
        let r = (|| {
            let mut store = Store::open(&paths::db_file()).map_err(|e| e.to_string())?;
            let now = now_unix();
            for (kind, list) in &fetched {
                store.replace_list((link.to.name(), *kind), list, now).map_err(|e| e.to_string())?;
            }
            if extend {
                return store.align_mirror(&to, &main, now).map_err(|e| e.to_string());
            }
            let r = store.start_mirror(link, &to, &main, now).map_err(|e| e.to_string())?;
            if with_manga {
                store.set_mirror_manga_aligned().map_err(|e| e.to_string())?;
            }
            Ok(r)
        })();
        h.update_if(move |s, _| {
            s.mirror.busy = None;
            match r {
                Ok((a, b)) => {
                    s.mirror.link = Some(link);
                    s.mirror.manga_aligned |= with_manga;
                    s.mirror.note = Some(match (a.len(), b.len()) {
                        (0, 0) => format!("On. Edits you make here from now on also go to {}.", link.to.title()),
                        (a, 0) => format!("On. Queued {} for {}.", plural(a, "change", "changes"), link.to.title()),
                        (a, b) => format!("On. Queued {} for {} and {} for {}.", plural(a, "change", "changes"), link.to.title(), plural(b, "change", "changes"), link.main.title()),
                    });
                    send(s, Cmd::Kick { sync: false });
                    s.queue_cmd(crate::queue::Cmd::Kick { refresh: false });
                }
                Err(e) => s.mirror.error = Some(format!("The mirror can\u{2019}t be turned on: {e}")),
            }
            true
        });
    });
}

fn cancel(s: &mut State) {
    s.mirror.preview = None;
    s.mirror.error = None;
}

/// Off: the link goes and the mirror's waiting changes are dropped (a thread).
fn stop(s: &mut State) {
    let Some(link) = s.mirror.link else { return };
    let Some(h) = s.handle.clone() else {
        s.mirror.link = None;
        return;
    };
    s.mirror.busy = Some("Turning the mirror off\u{2026}".into());
    let _ = std::thread::spawn(move || {
        let r = Store::open(&paths::db_file()).and_then(|mut st| st.stop_mirror()).map_err(|e| e.to_string());
        h.update_if(move |s, _| {
            s.mirror.busy = None;
            match r {
                Ok(n) => {
                    s.mirror.link = None;
                    s.mirror.ops.clear();
                    s.mirror.edits.clear();
                    s.mirror.waits.clear();
                    s.mirror.trouble = None;
                    s.mirror.note = Some(match n {
                        0 => "Off. Nothing more goes to the mirror.".to_string(),
                        n => format!("Off. {} for {} that hadn\u{2019}t been sent {} dropped.", plural(n, "change", "changes"), link.to.title(), if n == 1 { "was" } else { "were" }),
                    });
                }
                Err(e) => s.mirror.error = Some(format!("The mirror can\u{2019}t be turned off: {e}")),
            }
            true
        });
    });
}

fn pick_status(s: &mut State, kind: Kind, mal: u32, mirror_status: bool) {
    if let Some(p) = s.mirror.preview.as_mut().and_then(|p| p.part_mut(kind)) {
        if mirror_status {
            p.mirror_status.insert(mal);
        } else {
            p.mirror_status.remove(&mal);
        }
    }
}

fn pick_all(s: &mut State, kind: Kind, mirror_status: bool) {
    if let Some(p) = s.mirror.preview.as_mut().and_then(|p| p.part_mut(kind)) {
        p.mirror_status = if mirror_status { p.p.merge.iter().filter(|r| r.conflict.is_some()).map(|r| r.mal_id).collect() } else { HashSet::new() };
    }
}

// --------------------------------------------------------------------------------- settings

/// Settings → Account: the mirror switch, its state, and the preview while turning it on.
pub fn settings_row(s: &State) -> El {
    let m = &s.mirror;
    let link = Link::of(s.service);
    let (main, to) = (link.main.title(), link.to.title());
    let signed_in = s.account.is_some();
    let can = signed_in && m.other_signed_in && m.busy.is_none() && m.preview.is_none();
    let note = match (&m.busy, m.link) {
        (Some(b), _) => b.clone(),
        (None, Some(_)) => on_note(s),
        (None, None) if !signed_in => format!("Sign in to {main} first."),
        (None, None) if !m.other_signed_in => format!("Sign in to {to} too: choose Use {to} above, sign in, then come back to {main}. Your {main} list stays the main one."),
        (None, None) => format!("Every change you make in Kotora also goes to your {to} list, through its own queue. Turning it on first compares the two lists and shows you what would change."),
    };
    let title = format!("Mirror edits to {to}");
    let on = m.link.is_some() || m.preview.is_some();
    // In beta for 0.2.1 (Thomas's decision): it writes to two accounts.
    let beta = crate::ui::tag("Beta", PAL.accent, false).key("set/mirror-beta");
    let mut c = col().key("set/mirror").w_full().child(line_tagged("set/mirror-line", &title, Some(beta), note, switch_if("set/mirror-switch".into(), &title, on, can || m.link.is_some(), set)));
    if let Some(n) = &m.note {
        c = c.child(text(n.clone()).key("set/mirror-note").font(theme::LABEL).color(PAL.text).wrap().pb(12.0));
    }
    if let Some(e) = &m.error {
        c = c.child(text(e.clone()).key("set/mirror-error").font(theme::LABEL).color(PAL.status(Status::Dropped)).wrap().pb(12.0));
    }
    // Manga turned on after the mirror: its lists wait for one comparison.
    if m.link.is_some() && s.manga.enabled && !m.manga_aligned && m.preview.is_none() && m.busy.is_none() {
        let note = "Manga edits are mirrored too. Your two manga lists weren\u{2019}t compared yet: align them once, with the same choices.".to_string();
        c = c.child(crate::transfer::line("set/mirror-align-manga", "Align manga lists", note, small_button("set/mirror-align-manga-button", "Compare\u{2026}", false, Some(begin_manga as fn(&mut State)))));
    }
    if let Some(p) = &m.preview {
        c = c.child(preview(p, (fmt_of(s, p.p.link.main), fmt_of(s, p.p.link.to))));
    }
    c
}

/// The state line of a mirror that is on.
fn on_note(s: &State) -> String {
    let m = &s.mirror;
    let Some(link) = m.link else { return String::new() };
    let mut parts = vec![format!("On. Every change you make here also goes to {}. Changes made on its website are never overwritten: they show in the Queue.", link.to.title())];
    let waiting = m.waiting();
    if waiting > 0 {
        parts.push(format!("{} waiting for {}.", plural(waiting, "change", "changes"), link.to.title()));
    }
    if m.attention() > 0 {
        parts.push(format!("{} to look at in the Queue.", plural(m.attention() as usize, "thing", "things")));
    }
    if let Some(t) = &m.trouble {
        parts.push(t.clone());
    }
    parts.join(" ")
}

/// How `svc` shows scores: the active service's format, the mirror's saved one, MyAnimeList's 1-10.
fn fmt_of(s: &State, svc: ServiceId) -> ScoreFormat {
    match svc {
        ServiceId::Mal => ScoreFormat::Point10,
        _ if svc == s.service => s.score_format(),
        _ => s.mirror.fmt,
    }
}

/// An entry's values in words; a manga's: Reading, chapter and volume, rereading.
fn values_text_of(kind: Kind, v: &Values, fmt: ScoreFormat) -> String {
    let manga = kind == Kind::Manga;
    let mut t = v.status.label_in(kind).to_string();
    if v.rewatching {
        t.push_str(if manga { ", rereading" } else { ", rewatching" });
    }
    if v.progress > 0 {
        t.push_str(&format!(", {} {}", kind.unit(), v.progress));
    }
    if manga && v.volumes > 0 {
        t.push_str(&format!(", volume {}", v.volumes));
    }
    if let Some(sc) = v.score {
        t.push_str(&format!(", score {}", fmt.words(sc)));
    }
    t
}

/// The preview of the lists compared: one section per kind (anime, then manga with Manga on),
/// then what each choice would queue, and the choices.
fn preview(prev: &Prev, fmts: (ScoreFormat, ScoreFormat)) -> El {
    let p = &prev.p;
    let (main, to) = (p.link.main.title(), p.link.to.title());
    let parts = prev.parts();
    let mut box_ = col().key("set/mirror-preview").w_full().gap(12.0).padding(14.0).mb(12.0).radius(theme::R_MD).fill(PAL.field).border(1.0, PAL.border);
    for part in &parts {
        box_ = box_.child(preview_part(part, parts.len() > 1, fmts));
    }
    // What each choice would queue.
    let n = |a: Align| {
        let (t, m) = prev.actions(a);
        (t.len(), m.len())
    };
    let (wins_to, _) = n(Align::MainWins);
    let (merge_to, merge_main) = n(Align::Merge);
    let what = match (parts.len(), p.kind) {
        (1, Kind::Manga) => "Manga",
        (1, _) => "Anime",
        _ => "Entries",
    };
    let explain = [
        format!("{main} wins: {to} becomes a copy of your {main} list ({}). {what} only on {to} stay there.", plural(wins_to, "change", "changes")),
        format!("Merge: what is missing on each side is added; where they differ, the one further along wins and an empty score is filled ({} for {to}, {} for {main}).", plural(merge_to, "change", "changes"), plural(merge_main, "change", "changes")),
        "Don\u{2019}t align: nothing changes now; only edits you make from now on are mirrored.".to_string(),
        "Nothing is removed from either list, and every change goes through the Queue at each service\u{2019}s pace.".to_string(),
    ];
    for (i, t) in explain.into_iter().enumerate() {
        box_ = box_.child(text(t).key(format!("set/mirror-explain/{i}")).font(theme::LABEL).color(if i == 3 { PAL.faint } else { PAL.dim }).wrap());
    }
    let wins: fn(&mut State) = |s| choose(s, Align::MainWins);
    let merge: fn(&mut State) = |s| choose(s, Align::Merge);
    let none: fn(&mut State) = |s| choose(s, Align::DontAlign);
    box_.child(
        row()
            .key("set/mirror-actions")
            .gap(8.0)
            .flex_wrap()
            .child(small_button("set/mirror-main-wins", &format!("{main} wins"), false, Some(wins)))
            .child(small_button("set/mirror-merge", "Merge", true, Some(merge)))
            .child(small_button("set/mirror-dont-align", "Don\u{2019}t align", false, Some(none)))
            .child(small_button("set/mirror-cancel", "Cancel", false, Some(cancel as fn(&mut State)))),
    )
}

/// One kind's comparison: counts, the lists, the status conflicts to pick, what can't be
/// mirrored. `heading`: with both kinds shown, a heading per kind. Keys: `set/mirror-…` for
/// anime, `set/mirror-manga-…` for manga.
fn preview_part(prev: &Prev, heading: bool, (fm, ft): (ScoreFormat, ScoreFormat)) -> El {
    use kotora_core::listxml::Verdict;
    let p = &prev.p;
    let kind = p.kind;
    let k = if kind == Kind::Manga { "set/mirror-manga" } else { "set/mirror" };
    let (main, to) = (p.link.main.title(), p.link.to.title());
    let c = p.counts();
    let vt = |v: &Values, f: ScoreFormat| values_text_of(kind, v, f);
    let mut box_ = col().key(format!("{k}-part")).w_full().gap(12.0);
    if heading {
        box_ = box_.child(text(format!("{} lists", kind.label()).to_uppercase()).key(format!("{k}-heading")).font(theme::CAPTION).color(PAL.faint));
    }
    let what = if kind == Kind::Manga { "manga list" } else { "list" };
    box_ = box_.child(text(format!("Your {main} {what} and your {to} {what}, matched by MyAnimeList id")).font(theme::BODY_MED).color(PAL.text).wrap());
    box_ = box_.child(
        row()
            .key(format!("{k}-counts"))
            .gap(6.0)
            .flex_wrap()
            .child(count_pill(&format!("{k}-only-main"), c.only_main, &format!("only on {main}"), PAL.status(Status::Watching)))
            .child(count_pill(&format!("{k}-only-mirror"), c.only_mirror, &format!("only on {to}"), PAL.status(Status::PlanToWatch)))
            .child(count_pill(&format!("{k}-different"), c.different, "different", PAL.status(Status::OnHold)))
            .child(count_pill(&format!("{k}-same"), c.same, "the same", PAL.status(Status::Completed)))
            .child(count_pill(&format!("{k}-unmatched"), c.unmatched, "not mirrored", PAL.faint)),
    );
    let only_main: Vec<String> = p.plan.rows.iter().filter(|r| r.verdict == Verdict::New).map(|r| format!("{}: {}", r.title(), r.entry.values().map(|v| vt(&v, fm)).unwrap_or_default())).collect();
    if !only_main.is_empty() {
        box_ = box_.child(group(&format!("{k}-list-only-main"), format!("Only on {main}"), only_main));
    }
    let only_mirror: Vec<String> = p.only_mirror.iter().map(|o| format!("{}: {}", o.item.media.title, vt(&o.item.values(), ft))).collect();
    if !only_mirror.is_empty() {
        box_ = box_.child(group(&format!("{k}-list-only-mirror"), format!("Only on {to}"), only_mirror));
    }
    let diff: Vec<String> = p.merge.iter().map(|r| format!("{}: {main} {} \u{b7} {to} {}", r.title, vt(&r.main, fm), vt(&r.mirror, ft))).collect();
    if !diff.is_empty() {
        box_ = box_.child(group(&format!("{k}-list-different"), "On both, different".into(), diff));
    }
    let conflicts: Vec<_> = p.merge.iter().filter(|r| r.conflict.is_some()).collect();
    if !conflicts.is_empty() {
        let mut g = col().key(format!("{k}-conflicts")).gap(6.0).child(text("STATUS TO PICK (WITH \u{201c}MERGE\u{201d})").font(theme::CAPTION).color(PAL.faint).pb(2.0));
        let n = conflicts.len();
        for r in conflicts.into_iter().take(CONFLICTS_SHOWN) {
            let Some(c) = r.conflict else { continue };
            let mal = r.mal_id;
            let theirs = prev.mirror_status.contains(&mal);
            let (a, b) = (format!("{main}: {}", c.mine.label_in(kind)), format!("{to}: {}", c.file.label_in(kind)));
            g = g.child(
                row()
                    .key(format!("{k}-conflict/{mal}"))
                    .gap(8.0)
                    .center_y()
                    .child(text(r.title.clone()).font(theme::LABEL).color(PAL.text).truncate().grow(1.0).min_w(0.0))
                    .child(cell(format!("{k}-conflict/{mal}/main"), a.clone(), !theirs, move |s: &mut State| pick_status(s, kind, mal, false)).child(text(a).font(theme::LABEL).color(PAL.text)))
                    .child(cell(format!("{k}-conflict/{mal}/mirror"), b.clone(), theirs, move |s: &mut State| pick_status(s, kind, mal, true)).child(text(b).font(theme::LABEL).color(PAL.text))),
            );
        }
        if n > 1 {
            let all_theirs = p.merge.iter().filter(|r| r.conflict.is_some()).all(|r| prev.mirror_status.contains(&r.mal_id));
            let more = if n > CONFLICTS_SHOWN { format!("and {} more. ", n - CONFLICTS_SHOWN) } else { String::new() };
            let (a, b) = (format!("All {main}\u{2019}s"), format!("All {to}\u{2019}s"));
            g = g.child(
                row()
                    .key(format!("{k}-conflicts-all"))
                    .gap(8.0)
                    .center_y()
                    .child(text(format!("{more}All of them:")).font(theme::LABEL).color(PAL.faint).grow(1.0).min_w(0.0))
                    .child(cell(format!("{k}-conflicts-all/main"), a.clone(), prev.mirror_status.is_empty(), move |s: &mut State| pick_all(s, kind, false)).child(text(a).font(theme::LABEL).color(PAL.text)))
                    .child(cell(format!("{k}-conflicts-all/mirror"), b.clone(), all_theirs, move |s: &mut State| pick_all(s, kind, true)).child(text(b).font(theme::LABEL).color(PAL.text))),
            );
        }
        box_ = box_.child(g);
    }
    let mut unmatched: Vec<String> = p
        .plan
        .rows
        .iter()
        .filter_map(|r| match &r.verdict {
            Verdict::Unknown(why) => Some(format!("{}: {why}", r.entry.title)),
            _ => None,
        })
        .collect();
    unmatched.extend(p.unmatched_mirror.iter().map(|t| format!("{t}: on {to}, no MyAnimeList id")));
    unmatched.extend(p.only_mirror.iter().filter(|o| o.main_media.is_none()).map(|o| format!("{}: not on {main}, can\u{2019}t be added there", o.item.media.title)));
    if !unmatched.is_empty() {
        box_ = box_.child(group(&format!("{k}-list-unmatched"), "Not mirrored".into(), unmatched));
    }
    box_
}

/// Status conflicts listed one by one; the rest go by "All …".
const CONFLICTS_SHOWN: usize = 8;

// ------------------------------------------------------------------------------------ queue

/// Whether the Queue page has a mirror part.
pub fn in_queue(s: &State) -> bool {
    s.mirror.has_queue_items()
}

fn saved_text(kind: Kind, v: Option<&mirror::Saved>, removed: &str, fmt: ScoreFormat) -> String {
    v.map_or_else(|| removed.to_string(), |v| values_text_of(kind, &v.values(), fmt))
}

/// The Queue page's mirror part: website edits, conflicts, then waiting, failed and sent.
pub fn queue_section(s: &State) -> El {
    let m = &s.mirror;
    let Some(link) = m.link else { return el() };
    let (main, to) = (link.main.title(), link.to.title());
    let fmts = (fmt_of(s, link.main), fmt_of(s, link.to));
    let now = s.clock();
    let mut c = col().key("queue-mirror").gap(14.0).mx(theme::PAGE_PAD);
    let mut head = row().gap(8.0).center_y().child(text(format!("{to} mirror")).font(theme::SECTION).color(PAL.text));
    if let Some(at) = m.synced_at {
        head = head.child(text(format!("read {}", crate::ui::sync_age(now - at))).font(theme::LABEL).color(PAL.faint));
    }
    c = c.child(head);
    if let Some(t) = &m.trouble {
        c = c.child(crate::ui::alert("queue-mirror/trouble", PAL.warn(), t.clone(), None).mb(0.0));
    }
    for e in &m.edits {
        c = c.child(web_edit_card(e, main, to, now, fmts));
    }
    for op in m.ops.iter().filter(|o| o.state == OpState::Conflict) {
        c = c.child(conflict_card(op, main, to, fmts));
    }
    let mut list = col().key("queue-mirror-list").px(16.0).py(6.0).radius(theme::R_LG).fill(PAL.surface).border(1.0, PAL.border);
    let mut any = false;
    type Group = (&'static str, &'static str, fn(OpState) -> bool);
    let groups: [Group; 3] = [
        ("waiting", "Waiting", |st| matches!(st, OpState::Queued | OpState::Sending)),
        ("failed", "Failed", |st| st == OpState::Failed),
        ("sent", "Sent today", |st| st == OpState::Done),
    ];
    for (key, label, keep) in groups {
        let ops: Vec<&Op> = m.ops.iter().filter(|o| keep(o.state)).collect();
        if ops.is_empty() {
            continue;
        }
        any = true;
        list = list.child(text(label.to_uppercase()).key(format!("qmh/{key}")).font(theme::CAPTION).color(PAL.faint).pt(12.0).pb(4.0));
        for op in ops {
            list = list.child(op_row(op, now, fmts.1));
        }
    }
    if !m.waits.is_empty() {
        any = true;
        list = list.child(text(format!("NOT ON {} YET", to.to_uppercase())).key("qmh/waits").font(theme::CAPTION).color(PAL.faint).pt(12.0).pb(4.0));
        for w in &m.waits {
            let why = w.error.clone().unwrap_or_else(|| format!("Looking up its {to} id"));
            let wk = if w.kind == Kind::Manga { format!("qm/wait/m{}", w.mal_id) } else { format!("qm/wait/{}", w.mal_id) };
            list = list.child(
                row()
                    .key(wk)
                    .h(40.0)
                    .gap(14.0)
                    .center_y()
                    .child(text(w.title.clone()).font(theme::BODY_MED).color(PAL.text).truncate().w(240.0))
                    .child(text(why).font(theme::LABEL).color(PAL.dim).truncate().grow(1.0).min_w(0.0)),
            );
        }
    }
    if any {
        c = c.child(list);
    }
    c
}

fn op_row(op: &Op, now: i64, fmt: ScoreFormat) -> El {
    let id = op.id;
    let mut what = col().gap(2.0).grow(1.0).min_w(0.0).child(text(kotora_core::queue::describe_op_with(op, fmt)).font(theme::LABEL_NUM).color(PAL.text));
    match (&op.error, op.state) {
        (Some(e), OpState::Failed) => what = what.child(text(e.clone()).font(theme::LABEL).color(PAL.status(Status::Dropped)).truncate()),
        (Some(e), OpState::Queued) => what = what.child(text(format!("{e}; Kotora tries again by itself")).font(theme::LABEL).color(PAL.faint).truncate()),
        _ => {}
    }
    let end = match op.state {
        OpState::Failed => row().child(action(format!("qm/{id}/discard"), "Discard", false, move |s, _| send(s, Cmd::Discard(id)))),
        OpState::Done => row().child(text("Sent").font(theme::CAPTION.tracking(0.0)).color(PAL.ok())),
        st => crate::ui::pill(st.label(), if st == OpState::Sending { PAL.dim } else { PAL.warn() }),
    };
    let r = row()
        .key(format!("qm/{id}"))
        .h(48.0)
        .gap(14.0)
        .center_y()
        .child(text(op.title.clone()).font(theme::BODY_MED).color(PAL.text).truncate().w(240.0))
        .child(what)
        .child(end)
        .child(text(kotora_core::time::age(op.sent_at.unwrap_or(op.edited_at), now)).font(theme::LABEL_NUM).color(PAL.faint).w(52.0).justify_end());
    if op.state == OpState::Done { r.opacity(0.6) } else { r }
}

fn side(key: String, head: String, body: String, act: Option<El>, on: bool) -> El {
    let mut c = col()
        .key(key)
        .grow(1.0)
        .min_w(0.0)
        .gap(6.0)
        .padding(12.0)
        .radius(theme::R_MD)
        .fill(PAL.field)
        .border(1.0, if on { PAL.accent.alpha(0.5) } else { PAL.border })
        .child(text(head).font(theme::LABEL).color(PAL.faint))
        .child(text(body).font(theme::BODY_MED).color(PAL.text).wrap().pb(4.0));
    if let Some(a) = act {
        c = c.child(row().child(a));
    }
    c
}

fn card(key: String, title: String, why: String, sides: El, foot: Option<El>) -> El {
    let mut c = col()
        .key(key)
        .padding(16.0)
        .gap(10.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.accent.alpha(0.45))
        .child(row().gap(8.0).center_y().child(rune::icon(&crate::icons::ALERT, 16.0).color(PAL.accent)).child(text(title).font(theme::SECTION).color(PAL.text).truncate().min_w(0.0)))
        .child(text(why).font(theme::LABEL).color(PAL.dim).wrap())
        .child(sides);
    if let Some(f) = foot {
        c = c.child(f);
    }
    c
}

/// An entry changed, added or removed on the mirror's website.
fn web_edit_card(e: &WebEdit, main: &str, to: &str, now: i64, (fm, ft): (ScoreFormat, ScoreFormat)) -> El {
    let (mal, kind) = (e.mal_id, e.kind);
    let k = if kind == Kind::Manga { format!("mirror-edit/m{mal}") } else { format!("mirror-edit/{mal}") };
    let settle = |how: Settle| move |s: &mut State, _: &mut rune::Tx| send(s, Cmd::Settle(kind, mal, how));
    let (why, use_main, use_mirror) = match (&e.main, &e.mirror) {
        (Some(_), Some(_)) => (format!("Changed on {to} {}. Kotora didn\u{2019}t overwrite it. Pick one, or leave both as they are.", crate::ui::sync_age(now - e.at)), Some(format!("Use {main}\u{2019}s")), Some(format!("Use {to}\u{2019}s"))),
        (None, Some(_)) => (format!("Added on {to}; it isn\u{2019}t on your {main} list."), None, Some(format!("Add to {main}"))),
        (Some(_), None) => (format!("Removed on {to}; it is still on your {main} list."), Some(format!("Add back to {to}")), None),
        (None, None) => (String::new(), None, None),
    };
    let sides = row()
        .gap(12.0)
        .child(side(format!("{k}/main"), main.to_string(), saved_text(kind, e.main.as_ref(), "Not on the list", fm), use_main.map(|l| action(format!("{k}/use-main"), &l, true, settle(Settle::UseMain))), true))
        .child(side(format!("{k}/mirror"), to.to_string(), saved_text(kind, e.mirror.as_ref(), "Removed", ft), use_mirror.map(|l| action(format!("{k}/use-mirror"), &l, false, settle(Settle::UseMirror))), false));
    let foot = row().child(action(format!("{k}/leave"), "Leave both as they are", false, settle(Settle::Leave)));
    card(k, format!("{to}: {}", e.title), why, sides, Some(foot))
}

/// A mirrored change whose entry changed on the mirror's website after Kotora's last look.
fn conflict_card(op: &Op, main: &str, to: &str, (fm, ft): (ScoreFormat, ScoreFormat)) -> El {
    let id = op.id;
    let values_text = |v: &Values, f: ScoreFormat| values_text_of(op.kind, v, f);
    let k = format!("mirror-conflict/{id}");
    if op.removed {
        // A mirrored Remove from list, and the entry changed on the mirror's website since.
        let theirs = op.remote.as_ref().map_or_else(|| "Removed".to_string(), |r| values_text(&r.values, ft));
        let sides = row()
            .gap(12.0)
            .child(side(format!("{k}/mine"), format!("Removed from your {main} list"), format!("Was {}", values_text(&op.old, ft)), Some(action(format!("{k}/keep"), "Remove anyway", true, move |s, _| send(s, Cmd::KeepMine(id)))), true))
            .child(side(format!("{k}/theirs"), to.to_string(), theirs, Some(action(format!("{k}/take"), &format!("Keep it on {to}"), false, move |s, _| send(s, Cmd::TakeTheirs(id)))), false));
        return card(k, format!("{to}: {}", op.title), format!("{to} changed after you removed it. Kotora didn\u{2019}t remove it there. Pick one."), sides, None);
    }
    let mine = values_text(&op.mine(), fm);
    let (theirs, take) = match &op.remote {
        Some(r) => (values_text(&r.values, ft), format!("Keep {to}\u{2019}s")),
        None => ("Removed".to_string(), "Leave it removed".to_string()),
    };
    let sides = row()
        .gap(12.0)
        .child(side(format!("{k}/mine"), format!("From your {main} list"), mine, Some(action(format!("{k}/keep"), &format!("Send {main}\u{2019}s"), true, move |s, _| send(s, Cmd::KeepMine(id)))), true))
        .child(side(format!("{k}/theirs"), to.to_string(), theirs, Some(action(format!("{k}/take"), &take, false, move |s, _| send(s, Cmd::TakeTheirs(id)))), false));
    card(k, format!("{to}: {}", op.title), format!("{to} changed after this edit. Kotora didn\u{2019}t overwrite it. Pick one."), sides, None)
}

// ----------------------------------------------------------------------------------- scenes

/// The fake mirror of the scenes: MyAnimeList main, AniList mirror, compared with a made-up
/// AniList list built on the fake list's ids.
pub fn fake_preview(s: &mut State) {
    let link = Link::of(s.service);
    s.mirror.other_signed_in = true;
    let (main, other) = fake_lists(s, link);
    let mirror_media: HashMap<u32, kotora_core::Media> = if link.to == ServiceId::AniList {
        mirror::ids_only_on_main(link, &main, &other).into_iter().filter(|id| id % 7 != 0).map(|id| (id, kotora_core::Media { id: id + 100_000, id_mal: Some(id), title: format!("Show {id}"), ..Default::default() })).collect()
    } else {
        HashMap::new()
    };
    let p = mirror::compare(link, &main, &other, &mirror_media, &HashMap::new());
    s.mirror.preview = Some(Prev { p, fetched: other, mirror_status: HashSet::new(), manga: None, extend: false });
}

/// The fake preview with the manga lists too (Manga on): the fake manga list against a
/// made-up mirror manga list.
pub fn fake_preview_with_manga(s: &mut State) {
    fake_preview(s);
    let link = Link::of(s.service);
    let (main, other) = fake_manga_lists(s, link);
    let mirror_media: HashMap<u32, kotora_core::Media> = if link.to == ServiceId::AniList {
        mirror::ids_only_on_main_of(link, Kind::Manga, &main, &other).into_iter().map(|id| (id, kotora_core::Media { kind: Kind::Manga, id: id + 300_000, id_mal: Some(id), title: format!("Manga {id}"), ..Default::default() })).collect()
    } else {
        HashMap::new()
    };
    let p = mirror::compare_of(link, Kind::Manga, &main, &other, &mirror_media, &HashMap::new());
    if let Some(prev) = &mut s.mirror.preview {
        prev.manga = Some(Box::new(Prev { p, fetched: other, mirror_status: HashSet::new(), manga: None, extend: false }));
    }
}

/// The fake manga list as the main manga list, and a made-up mirror manga list: some further
/// along (chapters or volumes), one dropped there, one only there.
fn fake_manga_lists(s: &State, link: Link) -> (Vec<ListItem>, Vec<ListItem>) {
    let item = |e: &crate::manga::MEntry, svc: ServiceId| ListItem {
        media: kotora_core::Media {
            kind: Kind::Manga,
            id: if svc == ServiceId::AniList { e.id + 300_000 } else { e.id },
            title: e.main_title.clone(),
            episodes: e.chapters_total,
            volumes: e.volumes_total,
            id_mal: (svc == ServiceId::AniList).then_some(e.id),
            ..Default::default()
        },
        status: e.status,
        progress: e.chapters,
        score: e.score,
        rewatching: e.rereading,
        times_rewatched: e.times_reread,
        volumes: e.volumes,
        updated_at: e.updated_at,
    };
    let main: Vec<ListItem> = s.manga.entries.iter().map(|e| item(e, link.main)).collect();
    let mut other = Vec::new();
    for (i, e) in s.manga.entries.iter().enumerate() {
        let mut it = item(e, link.to);
        match i % 4 {
            0 => it.progress += 3,
            1 => it.volumes += 1,
            2 if it.status != Status::Completed => it.status = Status::Dropped,
            3 => continue,
            _ => {}
        }
        other.push(it);
    }
    other.push(ListItem {
        media: kotora_core::Media { kind: Kind::Manga, id: if link.to == ServiceId::AniList { 309_301 } else { 9301 }, title: "Lantern Post Office".into(), episodes: Some(30), id_mal: (link.to == ServiceId::AniList).then_some(9301), ..Default::default() },
        status: Status::PlanToWatch,
        progress: 0,
        score: None,
        rewatching: false,
        times_rewatched: 0,
        volumes: 0,
        updated_at: 1,
    });
    (main, other)
}

fn fake_item(e: &crate::model::Entry, svc: ServiceId) -> ListItem {
    let id = if svc == ServiceId::AniList { e.id + 100_000 } else { e.id };
    ListItem {
        media: kotora_core::Media { id, title: e.main_title.clone(), episodes: e.episodes, id_mal: (svc == ServiceId::AniList).then_some(e.id), ..Default::default() },
        status: e.status,
        progress: e.progress,
        score: e.score,
        rewatching: e.rewatching,
        times_rewatched: e.times_rewatched,
        volumes: 0,
        updated_at: e.updated_at,
    }
}

/// The fake list as the main list, and a made-up mirror list: most the same, some further
/// along, some with another status, a few only on one side, one with no MyAnimeList id.
fn fake_lists(s: &State, link: Link) -> (Vec<ListItem>, Vec<ListItem>) {
    let main: Vec<ListItem> = s.entries.iter().filter(|e| e.id > 0).map(|e| fake_item(e, link.main)).collect();
    let mut other = Vec::new();
    for (i, e) in s.entries.iter().filter(|e| e.id > 0).enumerate().take(36) {
        let mut it = fake_item(e, link.to);
        match i % 6 {
            0 => it.progress += 1,
            1 if it.status != Status::Dropped => it.status = Status::Dropped,
            2 => continue,
            3 => it.score = None,
            _ => {}
        }
        other.push(it);
    }
    for (n, (title, st)) in [("Moonlit Harbor", Status::PlanToWatch), ("Paper Kite Diaries", Status::Completed)].into_iter().enumerate() {
        let mal = 9101 + n as u32;
        let id = if link.to == ServiceId::AniList { mal + 100_000 } else { mal };
        other.push(ListItem {
            media: kotora_core::Media { id, title: title.into(), episodes: Some(12), id_mal: (link.to == ServiceId::AniList).then_some(mal), ..Default::default() },
            status: st,
            progress: if st == Status::Completed { 12 } else { 0 },
            score: None,
            rewatching: false,
            times_rewatched: 0,
            volumes: 0,
            updated_at: 1,
        });
    }
    if link.to == ServiceId::AniList {
        other.push(ListItem {
            media: kotora_core::Media { id: 555_555, title: "Web Short Specials".into(), ..Default::default() },
            status: Status::Watching,
            progress: 2,
            score: None,
            rewatching: false,
            times_rewatched: 0,
            volumes: 0,
            updated_at: 1,
        });
    }
    (main, other)
}

/// The fake mirror, on: a few changes waiting and sent, a conflict, three website edits, an
/// anime AniList doesn't know.
pub fn fake_on(s: &mut State) {
    let link = Link::of(s.service);
    let now = s.clock();
    s.mirror.other_signed_in = true;
    s.mirror.link = Some(link);
    s.mirror.synced_at = Some(now - 240);
    let pick = |i: usize| s.entries.iter().filter(|e| e.id > 0).nth(i).cloned();
    let mut ops = Vec::new();
    for (n, (i, state, change)) in [
        (0, OpState::Conflict, kotora_core::Change { progress: Some(4), ..Default::default() }),
        (1, OpState::Queued, kotora_core::Change { progress: Some(7), ..Default::default() }),
        (2, OpState::Failed, kotora_core::Change { score: Some(kotora_core::score::ten(8)), ..Default::default() }),
        (3, OpState::Done, kotora_core::Change { status: Some(Status::Completed), ..Default::default() }),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(e) = pick(i) else { continue };
        let old = Values { status: e.status, progress: e.progress, score: e.score, rewatching: false, times_rewatched: 0, volumes: 0 };
        ops.push(Op {
            id: 900 + n as i64,
            kind: kotora_core::Kind::Anime,
            media_id: e.id + 100_000,
            title: e.main_title.clone(),
            episodes: e.episodes,
            change,
            old,
            base_updated_at: now - 4000,
            created_at: now - 600,
            edited_at: now - 600,
            edits: 1,
            state,
            attempts: 0,
            next_try: 0,
            error: (state == OpState::Failed).then(|| "Validation error: score".to_string()),
            sent_at: (state == OpState::Done).then_some(now - 300),
            remote: (state == OpState::Conflict).then_some(kotora_core::Remote { values: Values { score: kotora_core::score::ten(3), ..old }, updated_at: now - 900 }),
            added: false,
            removed: false,
        });
    }
    s.mirror.ops = ops;
    let saved = |e: &crate::model::Entry, progress: u32| Some(mirror::Saved { status: e.status.mal_name().into(), progress, score: e.score, rewatching: false, times: 0, volumes: 0 });
    let mut edits = Vec::new();
    if let Some(e) = pick(4) {
        edits.push(WebEdit { kind: Kind::Anime, mal_id: e.id, title: e.main_title.clone(), main_id: Some(e.id), main: saved(&e, e.progress), mirror_id: Some(e.id + 100_000), mirror: saved(&e, e.progress + 2), at: now - 3600 });
    }
    edits.push(WebEdit { kind: Kind::Anime, mal_id: 9101, title: "Moonlit Harbor".into(), main_id: None, main: None, mirror_id: Some(109_101), mirror: Some(mirror::Saved { status: "plan_to_watch".into(), progress: 0, score: None, rewatching: false, times: 0, volumes: 0 }), at: now - 7200 });
    if let Some(e) = pick(5) {
        edits.push(WebEdit { kind: Kind::Anime, mal_id: e.id, title: e.main_title.clone(), main_id: Some(e.id), main: saved(&e, e.progress), mirror_id: None, mirror: None, at: now - 200 });
    }
    s.mirror.edits = edits;
    s.mirror.waits = vec![Wait { kind: Kind::Anime, mal_id: 9200, title: "Rust & Rain".into(), at: now - 100, error: Some(format!("Not on {}", link.to.title())) }];
    s.mirror.note = None;
}

/// Manga in the fake mirror's queue: a website edit of a manga and a mirrored manga change in
/// conflict.
pub fn fake_manga_on(s: &mut State) {
    let now = s.clock();
    let Some(e) = s.manga.entries.iter().find(|e| e.id == 2).cloned() else { return };
    let v = |ch: u32, vol: u32| mirror::Saved { status: "watching".into(), progress: ch, score: e.score, rewatching: false, times: 0, volumes: vol };
    s.mirror.edits.insert(0, WebEdit { kind: Kind::Manga, mal_id: 2, title: e.main_title.clone(), main_id: Some(2), main: Some(v(e.chapters, e.volumes)), mirror_id: Some(300_002), mirror: Some(v(e.chapters + 4, e.volumes + 1)), at: now - 1800 });
    s.mirror.ops.insert(
        0,
        Op {
            id: 950,
            kind: Kind::Manga,
            media_id: 300_001,
            title: "Inkwell Cartographers".into(),
            episodes: None,
            change: kotora_core::Change { progress: Some(120), ..Default::default() },
            old: Values { volumes: 11, ..Values::new(Status::Watching, 119, kotora_core::score::ten(9)) },
            base_updated_at: now - 4000,
            created_at: now - 600,
            edited_at: now - 600,
            edits: 1,
            state: OpState::Conflict,
            attempts: 0,
            next_try: 0,
            error: None,
            sent_at: None,
            remote: Some(kotora_core::Remote { values: Values { volumes: 12, ..Values::new(Status::Watching, 125, kotora_core::score::ten(9)) }, updated_at: now - 900 }),
            added: false,
            removed: false,
        },
    );
    s.mirror.manga_aligned = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_preview_counts_every_kind_of_row() {
        let mut s = State::fake();
        fake_preview(&mut s);
        let p = s.mirror.preview.as_ref().unwrap();
        let c = p.p.counts();
        assert!(c.only_main > 0 && c.only_mirror == 2 && c.different > 0 && c.same > 0 && c.conflicts > 0, "{c:?}");
        assert!(c.unmatched >= 1, "the AniList entry with no MyAnimeList id");
    }

    #[test]
    fn picking_a_status_changes_what_merge_sends() {
        let mut s = State::fake();
        fake_preview(&mut s);
        let mal = s.mirror.preview.as_ref().unwrap().p.merge.iter().find(|r| r.conflict.is_some()).unwrap().mal_id;
        let before = mirror::align_actions(&s.mirror.preview.as_ref().unwrap().p, Align::Merge, &HashSet::new());
        pick_status(&mut s, Kind::Anime, mal, true);
        let p = s.mirror.preview.as_ref().unwrap();
        assert!(p.mirror_status.contains(&mal));
        let after = mirror::align_actions(&p.p, Align::Merge, &p.mirror_status);
        assert_ne!(before, after);
        pick_all(&mut s, Kind::Anime, false);
        assert!(s.mirror.preview.as_ref().unwrap().mirror_status.is_empty());
    }

    #[test]
    fn headless_choices_and_switches_touch_nothing() {
        // No app handle: no thread, no store.
        let mut s = State::fake();
        fake_preview(&mut s);
        choose(&mut s, Align::Merge);
        assert!(s.mirror.preview.is_none() && s.mirror.link.is_none());
        fake_on(&mut s);
        send(&mut s, Cmd::Kick { sync: true });
        stop(&mut s);
        assert!(s.mirror.link.is_none());
    }

    /// With Manga on, the preview compares both kinds; a status picked in the manga part
    /// changes only the manga alignment; one choice queues both.
    #[test]
    fn the_preview_covers_manga_too() {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        fake_preview_with_manga(&mut s);
        let prev = s.mirror.preview.as_ref().unwrap();
        let m = prev.manga.as_deref().unwrap();
        assert_eq!((prev.p.kind, m.p.kind), (Kind::Anime, Kind::Manga));
        let c = m.p.counts();
        assert!(c.only_main > 0 && c.only_mirror == 1 && c.different > 0, "{c:?}");
        let (to, _) = prev.actions(Align::MainWins);
        let kinds: HashSet<Kind> = to
            .iter()
            .map(|a| match a {
                kotora_core::listxml::Action::Add { media, .. } => media.kind,
                kotora_core::listxml::Action::Update { kind, .. } => *kind,
            })
            .collect();
        assert_eq!(kinds, HashSet::from([Kind::Anime, Kind::Manga]), "one choice, both lists");
        if let Some(mal) = m.p.merge.iter().find(|r| r.conflict.is_some()).map(|r| r.mal_id) {
            pick_status(&mut s, Kind::Manga, mal, true);
            let prev = s.mirror.preview.as_ref().unwrap();
            assert!(prev.manga.as_ref().unwrap().mirror_status.contains(&mal));
            assert!(prev.mirror_status.is_empty(), "the anime part untouched");
        }
        // The view shows both parts, the manga one under its own keys.
        let mut rt = rune::Runtime::new(s, crate::ui::view, (1280.0, 2400.0), rune::Env { system_dark: true, scale: 1.0 });
        crate::settings::open(&mut rt.state, crate::settings::Section::Account);
        rt.frame(1.0 / 60.0);
        for k in ["set/mirror-part", "set/mirror-manga-part", "set/mirror-manga-counts", "set/mirror-manga-heading", "set/mirror-actions"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
    }

    /// Manga turned on with the mirror already on: one "Compare…" for the manga lists, until
    /// they are aligned. Manga in the mirror's queue read as manga.
    #[test]
    fn align_manga_later_and_manga_in_the_mirror_queue() {
        let render = |f: fn(&mut State)| {
            let mut s = State::fake();
            f(&mut s);
            let mut rt = rune::Runtime::new(s, crate::ui::view, (1280.0, 1600.0), rune::Env { system_dark: true, scale: 1.0 });
            rt.frame(1.0 / 60.0);
            rt
        };
        let rt = render(|s| {
            crate::manga::fake(s);
            fake_on(s);
            crate::settings::open(s, crate::settings::Section::Account);
        });
        assert!(rt.find("set/mirror-align-manga-button").is_some());
        let rt = render(|s| {
            crate::manga::fake(s);
            fake_on(s);
            s.mirror.manga_aligned = true;
            crate::settings::open(s, crate::settings::Section::Account);
        });
        assert!(rt.find("set/mirror-align-manga-button").is_none(), "aligned: nothing to ask");
        let rt = render(|s| {
            fake_on(s);
            crate::settings::open(s, crate::settings::Section::Account);
        });
        assert!(rt.find("set/mirror-align-manga-button").is_none(), "Manga off: nothing to ask");
        let rt = render(|s| {
            crate::manga::fake(s);
            fake_on(s);
            fake_manga_on(s);
            s.go(crate::model::Page::Queue);
        });
        for k in ["mirror-edit/m2/use-mirror", "mirror-edit/m2/leave", "mirror-conflict/950/keep"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        let edit = fake_edit_text();
        assert!(edit.contains("chapter") && edit.contains("volume") && edit.starts_with("Reading"), "{edit}");
    }

    fn fake_edit_text() -> String {
        let v = Values { volumes: 4, ..Values::new(Status::Watching, 68, None) };
        values_text_of(Kind::Manga, &v, ScoreFormat::Point10)
    }

    #[test]
    fn attention_counts_edits_conflicts_and_failures() {
        let mut s = State::fake();
        fake_on(&mut s);
        assert_eq!(s.mirror.attention(), 3 + 1 + 1);
        assert_eq!(s.mirror.waiting(), 3, "conflict, waiting, failed");
        unlink(&mut s);
        assert_eq!((s.mirror.attention(), s.mirror.link), (0, None));
    }
}
