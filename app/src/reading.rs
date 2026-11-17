//! Reading detection (Manga): Kotora notices a chapter read in YACReader and asks, like it does
//! for an episode played in a player.
//!
//! - A thread (`kotora-reading`) runs only while Manga is on, Settings → Manga → Reading detection
//!   is on, and the user is signed in. Every 30 s it `stat`s each YACReader library database
//!   (`kotora_core::manga::reading::Watch`); a changed one is read read-only and lock-free, and
//!   the newest comic opened is recognised (the name parser, the manga list, remembered picks,
//!   one search for a series off the list) and sent here.
//! - Here (UI thread) it becomes [`Reading`]: Now Playing shows it, and once it is read to the
//!   end the banner and a notification ask "Mark Ch N of X as read?" (Mark read / Not now /
//!   Wrong manga…). The anime rules: only forward, never past the count, volumes only from a
//!   whole-volume file; "Update automatically" records a sure match that skips nothing, with
//!   Undo. The edit goes through the queue like the row's `+1` (the mirror follows).
//!
//! Nothing runs headless; the scenes fill [`ReadingUi::current`] by hand.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use kotora_core::manga::reading::{self as core_reading, LibState, Offer, Read, Watch};
use kotora_core::recognize::Found;
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::{Change, Kind, Scope, Status};
use rune::{El, TextEdit, TextStyle, Tx, col, el, row, text, text_input};

use crate::model::{Handle, Msg, Page, State};
use crate::notes::{Act, Kind as NoteKind, Note};
use crate::theme::{self, PAL};
use crate::ui::{action, button, cover, field_hint, ghost, mix};

/// How often the libraries are looked at.
const POLL: Duration = Duration::from_secs(30);

/// The only reader so far.
pub const READER: &str = "YACReader";

/// What the thread found for the newest comic.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub read: Read,
    pub found: Found,
    /// The closest titles on the list, for "Wrong manga…".
    pub candidates: Vec<(u32, String)>,
    /// The cover of a match that isn't on the list (from the search).
    pub cover_url: Option<String>,
}

/// What the reading thread tells the UI.
#[derive(Debug)]
pub enum ReadingMsg {
    Seen(Box<Seen>),
    /// The libraries watched, and those that couldn't be read.
    Libs { watched: Vec<String>, errors: Vec<LibState> },
}

/// The manga's state at the offer, as with anime ([`crate::now::Mark`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub id: u32,
    pub chapters: u32,
    pub volumes: u32,
    pub status: Status,
}

/// A chapter (or volume) marked from here, for Undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marked {
    pub id: u32,
    pub offer: Offer,
    pub old_chapters: u32,
    pub old_volumes: u32,
    pub old_status: Status,
    pub completed: bool,
    pub auto: bool,
}

/// The comic read last, and what Kotora does about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub read: Read,
    pub found: Found,
    pub candidates: Vec<(u32, String)>,
    pub cover_url: Option<String>,
    pub dismissed: bool,
    /// Asked (notified) or marked already.
    pub acted: bool,
    pub marked: Option<Marked>,
    /// "Wrong manga…" is open.
    pub picking: bool,
    pub mark: Option<Mark>,
}

impl Reading {
    fn from_seen(s: Seen) -> Reading {
        Reading { read: s.read, found: s.found, candidates: s.candidates, cover_url: s.cover_url, dismissed: false, acted: false, marked: None, picking: false, mark: None }
    }

    pub fn media_id(&self) -> Option<u32> {
        match &self.found {
            Found::List { media_id, .. } => Some(*media_id),
            _ => None,
        }
    }

    pub fn sure(&self) -> bool {
        matches!(self.found, Found::List { sure: true, .. })
    }
}

/// Settings → Manga → Reading detection, and the running watch.
pub struct ReadingUi {
    /// `[reading] yacreader`.
    pub on: bool,
    /// `[reading] libraries`: added by the user.
    pub added: Vec<String>,
    /// What the thread watches (added and found in YACReader's settings), once it said.
    pub watched: Vec<String>,
    pub errors: Vec<LibState>,
    pub draft: TextEdit,
    pub error: Option<String>,
    pub current: Option<Reading>,
    tx: Option<Sender<In>>,
    /// Counts the threads started: a message from a stopped one is dropped.
    epoch: u64,
}

impl Default for ReadingUi {
    fn default() -> Self {
        ReadingUi { on: true, added: Vec::new(), watched: Vec::new(), errors: Vec::new(), draft: TextEdit::new(""), error: None, current: None, tx: None, epoch: 0 }
    }
}

impl ReadingUi {
    pub fn running(&self) -> bool {
        self.tx.is_some()
    }

    fn send(&self, m: In) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(m);
        }
    }
}

enum In {
    /// The user's pick for a series key (`None`: not a manga).
    Correct { key: String, media_id: Option<u32> },
    /// The manga list gained or lost entries: recognise again.
    ListChanged,
}

// ------------------------------------------------------------------------------- the thread

/// At start and after any change that matters (Manga on/off, the switch, the libraries, sign
/// in/out): starts or stops the thread. Never headless.
pub fn sync(s: &mut State) {
    let want = s.reading.on && s.manga.enabled && s.has_list() && s.handle.is_some() && !rune::is_headless();
    match (want, s.reading.running()) {
        (true, false) => start(s),
        (false, true) => stop(s),
        _ => {}
    }
}

/// Sign out or a switch of service: the watch and the comic go (the next sync starts again).
pub fn forget(s: &mut State) {
    s.reading.tx = None;
    s.reading.watched.clear();
    s.reading.errors.clear();
    s.withdraw_reading_note();
    s.reading.current = None;
}

/// The libraries changed: a new watch.
fn restart(s: &mut State) {
    stop(s);
    sync(s);
}

fn stop(s: &mut State) {
    // Dropping the sender ends the thread at its next wake.
    s.reading.tx = None;
    s.reading.watched.clear();
    s.reading.errors.clear();
    if s.reading.current.as_ref().is_some_and(|r| r.marked.is_none()) {
        s.withdraw_reading_note();
        s.reading.current = None;
    }
}

fn start(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let (tx, rx) = mpsc::channel();
    let added = s.reading.added.clone();
    s.reading.epoch += 1;
    let h = Out { h, epoch: s.reading.epoch };
    if std::thread::Builder::new().name("kotora-reading".into()).spawn(move || run(&h, &rx, &added)).is_ok() {
        s.reading.tx = Some(tx);
    }
}

/// The thread's way to the UI: its messages count only while it is the running one.
struct Out {
    h: Handle,
    epoch: u64,
}

fn send(o: &Out, m: ReadingMsg) {
    let epoch = o.epoch;
    o.h.update_if(move |s, _| s.reading.running() && s.reading.epoch == epoch && s.apply(Msg::Reading(m)));
}

fn run(h: &Out, rx: &Receiver<In>, added: &[String]) {
    let libs = core_reading::libraries(added, true);
    send(h, ReadingMsg::Libs { watched: libs.iter().map(|l| l.path.to_string_lossy().into_owned()).collect(), errors: Vec::new() });
    // No library: nothing to watch until the user adds one (that starts a new thread).
    if libs.is_empty() {
        return;
    }
    let mut watch = Watch::new(libs, now_unix());
    let mut store: Option<Store> = None;
    let mut last: Option<Read> = None;
    let mut last_errors: Vec<LibState> = Vec::new();
    loop {
        let mut errors = Vec::new();
        if let Some(r) = watch.poll(&mut errors)
            && last.as_ref() != Some(&r)
        {
            let seen = recognise(&mut store, r.clone());
            last = Some(r);
            send(h, ReadingMsg::Seen(Box::new(seen)));
        }
        if errors != last_errors {
            last_errors = errors.clone();
            send(h, ReadingMsg::Libs { watched: watch.libraries().map(|l| l.path.to_string_lossy().into_owned()).collect(), errors });
        }
        match rx.recv_timeout(POLL) {
            Ok(In::Correct { key, media_id }) => {
                if let Some(st) = open(&mut store) {
                    let (id, source) = media_id.map_or((0, "ignore"), |id| (id, "user"));
                    let _ = st.set_recognition_of(crate::account::name(), Kind::Manga, &key, id, source, "", now_unix());
                }
                if let Some(r) = last.as_ref().filter(|r| r.key() == key) {
                    send(h, ReadingMsg::Seen(Box::new(recognise(&mut store, r.clone()))));
                }
            }
            Ok(In::ListChanged) => {
                if let Some(r) = &last {
                    send(h, ReadingMsg::Seen(Box::new(recognise(&mut store, r.clone()))));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn open(store: &mut Option<Store>) -> Option<&mut Store> {
    if store.is_none() {
        *store = Store::open(&kotora_core::paths::db_file()).ok();
    }
    store.as_mut()
}

fn recognise(store: &mut Option<Store>, read: Read) -> Seen {
    let nothing = |read: Read| Seen { read, found: Found::Nothing, candidates: Vec::new(), cover_url: None };
    if read.series.is_empty() {
        return nothing(read);
    }
    let Some(st) = open(store) else { return nothing(read) };
    let svc = crate::account::name();
    let items = st.load_list(Scope::new(svc, Kind::Manga)).unwrap_or_default();
    let key = read.key();
    let cached = st.recognition_of(svc, Kind::Manga, &key).ok().flatten();
    let candidates = core_reading::candidates(&read.series, &items).into_iter().take(8).map(|c| (c.media_id, c.title)).collect();
    let mut cover_url = None;
    let found = match core_reading::on_list(&read.series, &items, &cached) {
        Some(f) => f,
        None => match cached.filter(|(_, src, _)| src == "search") {
            Some((0, _, _)) => Found::Nothing,
            Some((id, _, _)) if items.iter().any(|it| it.media.id == id) => Found::List { media_id: id, sure: false, why: format!("found by a {} search", crate::account::active().title()) },
            Some((id, _, title)) if !title.is_empty() => Found::NotOnList { media_id: id, title },
            _ => {
                let (f, url) = search(st, &read.series, &items, &key);
                cover_url = url;
                f
            }
        },
    };
    Seen { read, found, candidates, cover_url }
}

/// The one manga search for a series off the list (signed in, online), cached 30 days.
fn search(st: &Store, series: &str, items: &[kotora_core::ListItem], key: &str) -> (Found, Option<String>) {
    if kotora_core::paths::offline_forced() {
        return (Found::Nothing, None);
    }
    let Some(mut svc) = crate::account::open() else { return (Found::Nothing, None) };
    let Ok(hits) = svc.browse_manga(series, 10) else { return (Found::Nothing, None) };
    let found = core_reading::from_search(series, &hits, items, crate::account::active().title());
    let (id, title, url) = match &found {
        Found::List { media_id, .. } | Found::NotOnList { media_id, .. } => {
            let h = hits.iter().find(|h| h.id == *media_id);
            (*media_id, h.map(|h| h.title.clone()).unwrap_or_default(), h.and_then(|h| h.cover_url.clone()))
        }
        _ => (0, String::new(), None),
    };
    let _ = st.set_recognition_of(crate::account::name(), Kind::Manga, key, id, "search", &title, now_unix());
    (found, url)
}

// ------------------------------------------------------------------------------ the UI side

impl State {
    pub fn apply_reading(&mut self, m: ReadingMsg) -> bool {
        match m {
            ReadingMsg::Libs { watched, errors } => {
                let changed = self.reading.watched != watched || self.reading.errors != errors;
                self.reading.watched = watched;
                self.reading.errors = errors;
                changed
            }
            ReadingMsg::Seen(seen) => self.reading_seen(*seen),
        }
    }

    /// A comic from the thread. The same file keeps its answers (Not now, marked).
    pub fn reading_seen(&mut self, seen: Seen) -> bool {
        let new = Reading::from_seen(seen);
        match &mut self.reading.current {
            Some(old) if old.read.path == new.read.path => {
                let (dismissed, acted, marked, picking, mark) = (old.dismissed, old.acted, old.marked, old.picking, old.mark);
                // Read on to the end since: a new chance to ask.
                let acted = acted && !(new.read.finished && !old.read.finished);
                *old = Reading { dismissed, acted, marked, picking, mark, ..new };
            }
            _ => {
                if self.reading.current.as_ref().is_some_and(|r| r.marked.is_none()) {
                    self.withdraw_reading_note();
                }
                self.reading.current = Some(new);
            }
        }
        if !self.recheck_reading() {
            self.act_reading(true);
        }
        true
    }

    /// The entry and what to mark, if anything.
    pub fn reading_offer(&self) -> Option<(usize, Offer)> {
        let r = self.reading.current.as_ref()?;
        let id = r.media_id()?;
        let i = self.manga.entries.iter().position(|e| e.id == id)?;
        let e = &self.manga.entries[i];
        Some((i, core_reading::offer(e.chapters, e.chapters_total, e.volumes, e.volumes_total, &r.read)?))
    }

    /// Why it asks even with "Update automatically" on: it skips chapters (or volumes).
    pub fn reading_note(&self) -> Option<String> {
        let (i, o) = self.reading_offer()?;
        let e = &self.manga.entries[i];
        let (what, (a, b)) = match (o.chapter, o.volume) {
            (Some(c), _) => ("chapter", core_reading::skipped(e.chapters, c)?),
            (None, Some(v)) => ("volume", core_reading::skipped(e.volumes, v)?),
            _ => return None,
        };
        Some(if a == b { format!("Skips {what} {a}") } else { format!("Skips {what}s {a}\u{2013}{b}") })
    }

    /// The reading banner shows (the anime one wins when both would).
    pub fn reading_banner(&self) -> bool {
        let Some(r) = &self.reading.current else { return false };
        if r.dismissed || !self.has_list() || !self.manga.enabled {
            return false;
        }
        r.marked.is_some() || (r.read.finished && self.reading_offer().is_some())
    }

    fn reading_mark(&self) -> Option<Mark> {
        let id = self.reading.current.as_ref()?.media_id()?;
        let e = self.manga.entries.iter().find(|e| e.id == id)?;
        Some(Mark { id, chapters: e.chapters, volumes: e.volumes, status: e.status })
    }

    /// After the manga list changed: a changed entry asks again ("Not now" was about the old
    /// counts); a list that gained or lost entries is matched again.
    pub fn recheck_reading(&mut self) -> bool {
        let mark = self.reading_mark();
        let Some(r) = &mut self.reading.current else { return false };
        let old = std::mem::replace(&mut r.mark, mark);
        let (Some(old), Some(mark)) = (old, mark) else { return false };
        if old == mark || old.id != mark.id || r.marked.is_some() {
            return false;
        }
        r.dismissed = false;
        r.acted = false;
        self.act_reading(false);
        true
    }

    /// The manga list gained or lost entries (`before`: the ids then).
    pub(crate) fn reading_list_changed(&mut self, before: &[u32]) {
        let mut now: Vec<u32> = self.manga.entries.iter().map(|e| e.id).collect();
        now.sort_unstable();
        if self.reading.current.is_some() && now != before {
            self.reading.send(In::ListChanged);
        }
        self.recheck_reading();
    }

    /// Read to the end and recognised: mark it (auto, sure, nothing skipped) or ask.
    fn act_reading(&mut self, may_record: bool) {
        let Some(r) = &self.reading.current else { return };
        if !r.read.finished || r.acted || r.dismissed || !self.has_list() {
            return;
        }
        let Some((i, o)) = self.reading_offer() else { return };
        let title = self.manga.entries[i].title.clone();
        let note = self.reading_note();
        let auto = may_record && self.prefs.auto && r.sure() && note.is_none();
        if let Some(r) = &mut self.reading.current {
            r.acted = true;
        }
        if auto {
            if self.mark_reading(true).is_ok() {
                self.notify_reading(NoteKind::Recorded, format!("Marked {} as read", o.label()), title);
            }
        } else {
            let q = if self.reading.current.as_ref().is_some_and(Reading::sure) { "Mark" } else { "Not sure: mark" };
            self.notify_reading(NoteKind::ReadOffer, format!("{q} {} of {title} as read?", o.label()), note.map(|n| format!("{n}.")).unwrap_or_default());
        }
    }

    /// Mark read (the banner, `kot confirm`, the notification): the offer goes to the queue.
    pub fn mark_reading(&mut self, auto: bool) -> Result<(String, Offer), String> {
        let (i, o) = self.reading_offer().ok_or("Nothing to mark: no recognised new chapter was read.")?;
        let e = &self.manga.entries[i];
        let (id, title, old_chapters, old_volumes, old_status) = (e.id, e.title.clone(), e.chapters, e.volumes, e.status);
        let last = o.chapter.is_some_and(|c| e.chapters_total == Some(c));
        let status = (old_status == Status::PlanToWatch && !last).then_some(Status::Watching);
        if let Some(r) = &mut self.reading.current {
            r.mark = None;
        }
        let r = self.manga_edit(id, Change { progress: o.chapter, volumes: o.volume, status, ..Default::default() });
        let mark = self.reading_mark();
        if let Some(r) = &mut self.reading.current {
            r.mark = mark;
        }
        r?;
        let completed = old_status != Status::Completed && self.manga.entries.iter().any(|e| e.id == id && e.status == Status::Completed);
        if let Some(r) = &mut self.reading.current {
            r.marked = Some(Marked { id, offer: o, old_chapters, old_volumes, old_status, completed, auto });
            r.acted = true;
            r.picking = false;
        }
        Ok((title, o))
    }

    /// The banner's Mark read, and `kot confirm`: marks, and the notification goes.
    pub fn confirm_reading(&mut self) -> Result<(String, Offer), String> {
        let r = self.mark_reading(false)?;
        self.withdraw_reading_note();
        Ok(r)
    }

    /// Undo: the entry goes back (the queue drops the change if it hasn't left).
    pub fn undo_reading(&mut self) {
        let Some(m) = self.reading.current.as_ref().and_then(|r| r.marked) else { return };
        if let Some(r) = &mut self.reading.current {
            r.mark = None;
        }
        let _ = self.manga_edit(m.id, Change { progress: Some(m.old_chapters), volumes: Some(m.old_volumes), status: Some(m.old_status), ..Default::default() });
        let mark = self.reading_mark();
        self.withdraw_reading_note();
        if let Some(r) = &mut self.reading.current {
            r.mark = mark;
            r.marked = None;
            r.dismissed = true;
        }
    }

    /// Not now (the banner, `kot dismiss`, the notification).
    pub fn dismiss_reading(&mut self) -> bool {
        match &mut self.reading.current {
            Some(r) if !r.dismissed => {
                r.dismissed = true;
                r.picking = false;
            }
            _ => return false,
        }
        self.withdraw_reading_note();
        true
    }

    /// "Wrong manga…": the picker on the Now Playing page.
    pub fn wrong_manga(&mut self) {
        if let Some(r) = &mut self.reading.current {
            r.picking = true;
        }
        self.go(Page::NowPlaying);
    }

    /// The user's pick: remembered for this series, and the match is sure.
    pub fn pick_manga(&mut self, media_id: u32) {
        let Some(r) = &mut self.reading.current else { return };
        r.found = Found::List { media_id, sure: true, why: "you picked it".into() };
        r.picking = false;
        r.dismissed = false;
        r.acted = false;
        let key = r.read.key();
        self.reading.send(In::Correct { key, media_id: Some(media_id) });
        self.recheck_reading();
        self.act_reading(true);
    }

    /// "Not a manga": this series is ignored from now on.
    pub fn ignore_reading(&mut self) {
        let Some(r) = &mut self.reading.current else { return };
        r.found = Found::Ignored;
        r.picking = false;
        r.dismissed = true;
        let key = r.read.key();
        self.reading.send(In::Correct { key, media_id: None });
        self.withdraw_reading_note();
    }

    /// "Add as Reading": a series found by the search, not on the list. The offer follows once
    /// the entry is there.
    pub fn add_reading(&mut self) {
        let Some(r) = &self.reading.current else { return };
        let Found::NotOnList { media_id, title } = &r.found else { return };
        let media = kotora_core::Media { id: *media_id, kind: Kind::Manga, title: title.clone(), cover_url: r.cover_url.clone(), ..Default::default() };
        let id = *media_id;
        match self.manga_add(media, Status::Watching) {
            Ok(_) => {
                if let Some(r) = &mut self.reading.current {
                    r.found = Found::List { media_id: id, sure: true, why: "you added it".into() };
                    r.dismissed = false;
                    r.acted = false;
                }
                self.recheck_reading();
                self.act_reading(false);
            }
            Err(e) => self.toast = Some(crate::library::Toast { text: e, warn: true, until: std::time::Instant::now() + Duration::from_secs(6), undo: None }),
        }
    }

    /// The title shown for the comic: the entry's, the search hit's, else the series.
    pub fn reading_title(&self, r: &Reading) -> String {
        match &r.found {
            Found::List { media_id, .. } => self.manga.entries.iter().find(|e| e.id == *media_id).map(|e| e.title.clone()),
            Found::NotOnList { title, .. } => Some(title.clone()),
            _ => None,
        }
        .unwrap_or_else(|| if r.read.series.is_empty() { r.read.file_name.clone() } else { r.read.series.clone() })
    }

    fn reading_match_text(&self, r: &Reading) -> String {
        match &r.found {
            Found::List { media_id, sure, why } => {
                let st = self.manga.entries.iter().find(|e| e.id == *media_id).map(|e| e.status.label_in(Kind::Manga)).unwrap_or("");
                format!("In your list ({st}), {}: {why}", if *sure { "sure" } else { "not sure" })
            }
            Found::NotOnList { title, .. } => format!("Not on your list: {} knows it as {title}", self.service.title()),
            Found::Ignored => "Not a manga (you said so)".into(),
            Found::Nothing if r.read.series.is_empty() => "No series in the file name".into(),
            Found::Nothing => "Not found on your list".into(),
        }
    }

    /// `kot now`'s reading part.
    pub fn ctl_reading(&self) -> Option<kotora_ctl::ReadingInfo> {
        let r = self.reading.current.as_ref()?;
        let pos = |n: i64| u32::try_from(n).ok().filter(|n| *n > 0);
        Some(kotora_ctl::ReadingInfo {
            reader: READER.into(),
            file: r.read.file_name.clone(),
            title: self.reading_title(r),
            label: r.read.label(),
            page: pos(r.read.page),
            pages: pos(r.read.pages),
            finished: r.read.finished,
            matched: self.reading_match_text(r),
            offer: self.reading_offer().map(|(_, o)| o.label()),
            note: self.reading_note(),
            recorded: r.marked.map(|m| m.offer.label()),
            dismissed: r.dismissed,
        })
    }

    // ------------------------------------------------------------------------ notifications

    /// Every notification about this comic: `kotora-manga-<id>-<file>` (a new file replaces).
    pub fn reading_note_id(&self) -> Option<String> {
        let r = self.reading.current.as_ref()?;
        let id = r.media_id()?;
        let what = r.read.chapter.map(|c| format!("ch{c}")).or(r.read.volume.map(|v| format!("v{v}"))).unwrap_or_default();
        Some(format!("kotora-manga-{id}-{what}"))
    }

    fn notify_reading(&mut self, kind: NoteKind, summary: String, body: String) {
        let on = match kind {
            NoteKind::ReadOffer => self.prefs.notify.offer,
            NoteKind::Recorded => self.prefs.notify.recorded,
            _ => false,
        };
        let Some(id) = self.reading_note_id().filter(|_| on) else {
            if kind == NoteKind::Recorded {
                self.withdraw_reading_note();
            }
            return;
        };
        let tail = match kind {
            NoteKind::ReadOffer => Some("Open Kotora, or run kot confirm.".to_string()),
            _ => Some("Undo is in Kotora's banner.".to_string()),
        };
        self.notes.show(Note { id, kind, summary, body, tail });
    }

    pub(crate) fn withdraw_reading_note(&mut self) {
        if let Some(id) = self.reading_note_id() {
            self.notes.withdraw(&id);
        }
    }

    /// A button of a reading notification. `None`: not about the comic being read.
    pub fn reading_note_action(&mut self, id: &str, act: Act) -> Option<()> {
        if self.reading_note_id().as_deref() != Some(id) {
            return None;
        }
        let marked = self.reading.current.as_ref().is_some_and(|r| r.marked.is_some());
        let asks = !marked && self.reading_offer().is_some();
        match act {
            Act::Update if asks => match self.mark_reading(false) {
                Ok((title, o)) => self.notify_reading(NoteKind::Recorded, format!("Marked {} as read", o.label()), title),
                Err(_) => self.open_reading(),
            },
            Act::NotNow if asks => {
                self.dismiss_reading();
            }
            Act::Undo if marked => self.undo_reading(),
            _ => self.open_reading(),
        }
        Some(())
    }

    fn open_reading(&mut self) {
        self.go(Page::NowPlaying);
        if let Some(h) = &self.handle {
            crate::tray::show_window(h);
        }
    }

    // ----------------------------------------------------------------------------- settings

    pub fn set_reading_on(&mut self, on: bool) {
        self.reading.on = on;
        crate::detect::save_setting(self, "reading", "yacreader", on.to_string());
        sync(self);
    }

    fn save_reading_libraries(&mut self) {
        let v = kotora_core::config::quote_list(&self.reading.added);
        crate::detect::save_setting(self, "reading", "libraries", v);
        restart(self);
    }
}

/// From `config.toml` at start.
pub fn load(s: &mut State, c: &kotora_core::config::Config) {
    s.reading.on = c.reading.yacreader;
    s.reading.added = c.reading.libraries.clone();
}

/// Adds a library folder typed or picked: it must have YACReader's database.
pub fn add_library(s: &mut State, raw: &str) -> Result<(), String> {
    let path = crate::library::tidy(raw);
    if path.is_empty() {
        return Err("Type a library folder's path, or press Choose\u{2026}".into());
    }
    let lib = core_reading::library_at(&path);
    let shown = lib.path.to_string_lossy().into_owned();
    if s.reading.added.iter().chain(&s.reading.watched).any(|p| *p == shown) {
        return Err("That library is already watched.".into());
    }
    if !lib.db_path().is_file() {
        return Err(format!("No YACReader library at {shown} (it has no .yacreaderlibrary folder)."));
    }
    s.reading.added.push(shown);
    s.reading.draft.clear();
    s.reading.error = None;
    s.save_reading_libraries();
    Ok(())
}

pub fn remove_library(s: &mut State, path: &str) {
    s.reading.added.retain(|p| p != path);
    s.save_reading_libraries();
}

fn add_typed(s: &mut State) {
    let raw = s.reading.draft.text.clone();
    if let Err(e) = add_library(s, &raw) {
        s.reading.error = Some(e);
    }
}

fn choose(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let r = crate::library::choose_folder("Choose a YACReader library folder", move |p| {
        h.update_if(move |s, _| {
            if let Err(e) = p.and_then(|p| add_library(s, &p)) {
                s.reading.error = Some(e);
            }
            true
        });
    });
    if let Err(e) = r {
        s.reading.error = Some(e);
    }
}

// ---------------------------------------------------------------------------------- views

pub const SWITCH_NOTE: &str = "Kotora notices a chapter you read to the end in YACReader and asks before it marks it. It reads YACReader\u{2019}s library database every 30 seconds, read only.";

/// Settings → Manga → Reading detection → YACReader.
pub fn switch_row_note(s: &State) -> String {
    if !s.manga.enabled {
        return "Needs Manga on (the Anime | Manga switch).".into();
    }
    SWITCH_NOTE.into()
}

/// The libraries block: found ones, added ones (Remove), the field and Choose….
pub fn libraries_block(s: &State) -> El {
    let r = &s.reading;
    let mut c = col().key("reading-libs").w_full();
    let mut shown: Vec<(String, bool)> = r.added.iter().map(|p| (p.clone(), true)).collect();
    for p in &r.watched {
        if !shown.iter().any(|(x, _)| x == p) {
            shown.push((p.clone(), false));
        }
    }
    if shown.is_empty() {
        let why = if !r.on || !s.manga.enabled { "Libraries listed in YACReader\u{2019}s settings are found by themselves once this is on." } else { "No YACReader library found. Add your library\u{2019}s folder (the one with .yacreaderlibrary in it)." };
        c = c.child(text(why).font(theme::LABEL).color(PAL.faint).wrap().py(4.0));
    }
    for (path, added) in shown {
        let err = r.errors.iter().find(|e| e.path.to_string_lossy() == path).and_then(|e| e.error.clone());
        let there = std::path::Path::new(&path).join(".yacreaderlibrary").join("library.ydb").is_file();
        let (dot, state) = match (&err, there) {
            (Some(e), _) => (PAL.warn(), format!("Can\u{2019}t read it now: {e}")),
            (None, false) => (PAL.warn(), "Not there (drive not connected?)".to_string()),
            (None, true) if r.running() => (PAL.ok(), if added { "Watched".to_string() } else { "Watched \u{b7} found in YACReader\u{2019}s settings".to_string() }),
            (None, true) => (PAL.faint, if added { "Added".to_string() } else { "Found in YACReader\u{2019}s settings".to_string() }),
        };
        let mut line = row()
            .key(format!("set/reading/{path}"))
            .gap(12.0)
            .py(6.0)
            .center_y()
            .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
            .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(path.clone()).font(theme::BODY_MED).color(PAL.text).truncate()).child(text(state).font(theme::LABEL).color(PAL.dim).truncate()));
        if added {
            let p = path.clone();
            line = line.child(ghost(&format!("set/reading/remove/{path}"), "Remove", move |s| remove_library(s, &p)));
        }
        c = c.child(line);
    }
    let input = row()
        .key("set/reading/box")
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, if r.error.is_some() { PAL.warn() } else { PAL.border_strong })
        .child(
            text_input(&r.draft, TextStyle::new(12.0, 400.0).mono())
                .key("set/reading/input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&r.draft, if cfg!(windows) { "D:\\Comics" } else { "~/Comics" }), PAL.faint)
                .on_edit(|s: &mut State| &mut s.reading.draft)
                .on_input_key(crate::ui::field_page_key)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.reading.error = None)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    add_typed(s);
                }),
        );
    c = c.child(
        row()
            .gap(8.0)
            .mt(6.0)
            .center_y()
            .child(input)
            .child(button("set/reading/add", "Add", false, Some(add_typed)))
            .child(button("set/reading/choose", "Choose\u{2026}", false, Some(choose))),
    );
    if let Some(e) = &r.error {
        c = c.child(text(e.clone()).key("set/reading/error").font(theme::LABEL).color(PAL.warn()).wrap().mt(6.0));
    }
    c
}

fn reading_cover(s: &State, r: &Reading, w: f32, h: f32, size: f32) -> El {
    let title = s.reading_title(r);
    let (id, url) = match &r.found {
        Found::List { media_id, .. } => (crate::manga::cover_id(*media_id), s.manga.entries.iter().find(|e| e.id == *media_id).and_then(|e| e.cover_url.as_deref())),
        Found::NotOnList { media_id, .. } => (crate::manga::cover_id(*media_id), r.cover_url.as_deref()),
        _ => (title.bytes().map(u32::from).sum::<u32>() | crate::manga::COVER_BIT, None),
    };
    let rad = if w > 60.0 { theme::R_LG } else { theme::R_XS };
    let tile = cover(id, &title, w, h, size).radius(rad);
    match s.covers.get(id, url, w, h) {
        Some(img) => tile.child(el().key("rd-cover-img").absolute(0.0, 0.0).size(w, h).radius(rad).pixels(img.pixels())),
        None => tile,
    }
}

/// The banner on top of every page: "Read Ch 362 of X in YACReader · Mark as read?".
pub fn banner(s: &State) -> El {
    let Some(r) = &s.reading.current else { return el() };
    let title = s.reading_title(r);
    let line = |lead: &str, bold: String, tail: String| {
        row()
            .w_full()
            .min_w(0.0)
            .clip_fade(0.0, 0.0, 16.0, 0.0)
            .child(text(lead.to_string()).font(theme::BODY).color(PAL.text))
            .child(text(bold).font(TextStyle::new(13.0, 600.0)).color(PAL.text).truncate().min_w(0.0))
            .child(text(tail).font(theme::BODY).color(PAL.text))
    };
    let b = row()
        .key("rd-banner")
        .mx(theme::PAGE_PAD)
        .mt(14.0)
        .px(14.0)
        .py(12.0)
        .gap(14.0)
        .center_y()
        .radius(theme::R_LG)
        .linear_gradient(std::f32::consts::PI / 2.0, [(0.0, mix(PAL.bg, PAL.accent, 0.18)), (0.6, mix(PAL.bg, PAL.accent, 0.06)), (1.0, mix(PAL.bg, PAL.accent, 0.03))])
        .border(1.0, PAL.accent.alpha(0.35))
        .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.accent))
        .child(reading_cover(s, r, 36.0, 52.0, 12.0));
    if let Some(m) = r.marked {
        let how = match (m.auto, m.completed) {
            (true, true) => "Marked automatically \u{b7} Completed",
            (false, true) => "Marked \u{b7} Completed",
            (true, false) => "Marked automatically",
            (false, false) => "Marked",
        };
        let waiting = s.manga.entries.iter().find(|e| e.id == m.id).and_then(|e| e.waiting).unwrap_or(if s.service == kotora_core::service::ServiceId::AniList { "Sent to AniList" } else { "Sent to MyAnimeList" });
        return b
            .child(col().grow(1.0).min_w(0.0).gap(3.0).child(line("Read ", title, format!(" {}", m.offer.label()))).child(text(format!("{how} \u{b7} {waiting}")).font(theme::LABEL).color(PAL.dim)))
            .child(button("rd-banner/undo", "Undo", false, Some(State::undo_reading)))
            .child(ghost("rd-banner/close", "Close", |s| {
                s.dismiss_reading();
            }));
    }
    let Some((i, o)) = s.reading_offer() else { return el() };
    let e = &s.manga.entries[i];
    // "Ch 120 of 200" when the count is known, else "Ch 120".
    let of = |n: Option<u32>| n.map(|n| format!(" of {n}")).unwrap_or_default();
    let total = match (o.chapter, o.volume) {
        (Some(c), _) => format!("Ch {c}{}", of(e.chapters_total)),
        (None, Some(v)) => format!("Vol {v}{}", of(e.volumes_total)),
        _ => String::new(),
    };
    // A skip note takes the place of "Matched in your list" (as on the anime banner).
    let note = s.reading_note();
    let matched = match (r.sure(), note.is_some()) {
        (true, true) => String::new(),
        (true, false) => " \u{b7} Matched in your list".to_string(),
        (false, _) => " \u{b7} Not sure it\u{2019}s this one".to_string(),
    };
    let mut small = row().min_w(0.0).gap(6.0).child(text(format!("Read to the end in {READER}{matched}")).font(theme::LABEL).color(PAL.dim).truncate().min_w(0.0));
    if let Some(n) = note {
        small = small.child(text("\u{b7}").font(theme::LABEL).color(PAL.dim)).child(text(n).key("rd-banner/note").font(theme::LABEL).color(PAL.warn()).truncate().min_w(0.0));
    }
    b.child(col().grow(1.0).min_w(0.0).gap(3.0).child(line("Read ", title, format!(" {}", r.read.label()))).child(small))
        .child(text(format!("Mark {total} as read?")).font(TextStyle::new(14.0, 600.0).features("'tnum' 1")).color(PAL.cream))
        .child(button("rd-banner/mark", "Mark read", true, Some(|s: &mut State| {
            let _ = s.confirm_reading();
        })))
        .child(button("rd-banner/not-now", "Not now", false, Some(|s: &mut State| {
            s.dismiss_reading();
        })))
        .child(ghost("rd-banner/wrong", "Wrong manga\u{2026}", State::wrong_manga))
}

/// Now Playing's reading card: the comic, the page, the match, and what to do.
pub fn card_for(s: &State, r: &Reading) -> El {
    let title = s.reading_title(r);
    let progress = if r.read.finished {
        "Read to the end".to_string()
    } else if r.read.pages > 0 {
        format!("Page {} of {}", r.read.page.max(1), r.read.pages)
    } else {
        "Opened".to_string()
    };
    let frac = if r.read.finished { 1.0 } else if r.read.pages > 0 { (r.read.page as f32 / r.read.pages as f32).clamp(0.0, 1.0) } else { 0.0 };
    let bar = row().h(4.0).radius(2.0).clip_rounded(2.0).fill(PAL.border).child(el().grow(frac).h_full().radius(2.0).fill(PAL.accent)).child(el().grow(1.0 - frac));
    let mut info = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text(format!("READING IN {}", READER.to_uppercase())).font(theme::LABEL).color(PAL.accent))
        .child(text(title).font(theme::SECTION).color(PAL.text).truncate())
        .child(text(r.read.label()).font(theme::BODY_MED).color(PAL.text))
        .child(text(r.read.file_name.clone()).font(TextStyle::new(12.0, 400.0).mono()).color(PAL.dim).truncate())
        .child(row().gap(10.0).mt(4.0).center_y().child(bar.grow(1.0)).child(text(progress).key("rd/progress").font(theme::LABEL).color(PAL.dim)))
        .child(text(s.reading_match_text(r)).key("rd/match").font(theme::LABEL).color(if r.sure() { PAL.dim } else { PAL.warn() }).wrap().mt(4.0));
    let mut acts = row().gap(8.0).mt(8.0);
    if let Some(m) = r.marked {
        acts = acts.child(text(format!("Marked {} as read.", m.offer.label())).font(theme::LABEL).color(PAL.ok())).child(button("rd/undo", "Undo", false, Some(State::undo_reading)));
    } else if let Some((_, o)) = s.reading_offer() {
        let label = format!("Mark {} as read", o.label());
        acts = acts.child(action("rd/mark".into(), &label, r.read.finished, |s, _| {
            let _ = s.confirm_reading();
        }));
        if !r.read.finished {
            info = info.child(text("Kotora asks once the last page is reached (or YACReader marks it read).").font(theme::LABEL).color(PAL.faint).wrap());
        }
    } else if matches!(r.found, Found::List { .. }) {
        info = info.child(text("Nothing to mark: your list is at or past this already.").font(theme::LABEL).color(PAL.faint).wrap());
    }
    if let Found::NotOnList { .. } = r.found {
        acts = acts.child(action("rd/add".into(), "Add as Reading", true, |s, _| s.add_reading()));
    }
    if !matches!(r.found, Found::Ignored) && r.marked.is_none() {
        acts = acts.child(ghost("rd/wrong", "Wrong manga\u{2026}", State::wrong_manga).h(26.0));
    }
    info = info.child(acts);
    let mut c = col()
        .key("np-reading")
        .gap(12.0)
        .padding(16.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(row().gap(16.0).items_start().child(reading_cover(s, r, 84.0, 120.0, 22.0)).child(info));
    if r.picking {
        c = c.child(picker(r));
    }
    c
}

/// "Wrong manga…": the closest titles on the manga list, and "not a manga".
fn picker(r: &Reading) -> El {
    let mut c = col()
        .key("rd-picker")
        .gap(2.0)
        .pt(8.0)
        .child(text("Which manga is it?").font(theme::BODY_MED).color(PAL.text))
        .child(text("The closest titles on your manga list. Kotora remembers your pick for this series.").font(theme::LABEL).color(PAL.dim).wrap().pb(6.0));
    if r.candidates.is_empty() {
        c = c.child(text("Nothing on your manga list comes close.").font(theme::LABEL).color(PAL.faint).py(4.0));
    }
    for (id, title) in &r.candidates {
        let id = *id;
        c = c.child(
            row()
                .key(format!("rd-pick/{id}"))
                .role("button")
                .label(title.clone())
                .focusable(true)
                .gap(10.0)
                .py(6.0)
                .px(6.0)
                .center_y()
                .radius(theme::R_SM)
                .hover(PAL.hover())
                .press(PAL.press())
                .on_press(move |s: &mut State| s.pick_manga(id))
                .child(cover(crate::manga::cover_id(id), title, 22.0, 31.0, 8.0))
                .child(text(title.clone()).font(theme::BODY).color(PAL.text).truncate().grow(1.0).min_w(0.0)),
        );
    }
    c.child(
        row()
            .gap(8.0)
            .mt(10.0)
            .child(button("rd-pick/none", "Not a manga", false, Some(State::ignore_reading)).h(26.0).px(10.0))
            .child(ghost("rd-pick/cancel", "Cancel", |s| {
                if let Some(r) = &mut s.reading.current {
                    r.picking = false;
                }
            })
            .h(26.0)),
    )
}

// ---------------------------------------------------------------------------------- scenes

/// A made-up comic for the scenes: `file` in a made-up library, on page `page` of 40.
pub fn fake(s: &mut State, file: &str, page: i64) {
    let o = kotora_core::manga::yacreader::Opened {
        rel_path: format!("/{file}"),
        path: std::path::PathBuf::from("/comics").join(file),
        parsed: kotora_core::manga::name::parse_path(file),
        meta_series: None,
        current_page: page,
        num_pages: 40,
        read_flag: false,
        last_opened: 0,
    };
    let read = Read::from_opened(&o);
    let items: Vec<kotora_core::ListItem> = crate::manga::fake_items();
    let found = core_reading::on_list(&read.series, &items, &None).unwrap_or(Found::Nothing);
    let candidates = core_reading::candidates(&read.series, &items).into_iter().take(8).map(|c| (c.media_id, c.title)).collect();
    s.reading_seen(Seen { read, found, candidates, cover_url: None });
}

#[cfg(test)]
mod tests;
