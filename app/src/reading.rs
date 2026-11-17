//! Reading detection (Manga): Kotora notices a chapter read in YACReader and asks, like it does
//! for an episode played in a player.
//!
//! - A thread (`kotora-reading`) runs only while Manga is on, Settings → Manga → Reading detection
//!   is on, and the user is signed in. Every 10 s it reads the YACReader viewer's command line
//!   (`<library> --comicId=N`: the comic open now, which YACReader writes to its database only on
//!   close) and `stat`s each YACReader library database (`kotora_core::manga::reading::Watch`);
//!   a changed one is read read-only and lock-free. The comic is recognised (the name parser, the manga list, remembered picks,
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
const POLL: Duration = Duration::from_secs(10);

/// How the comic counts as read: "Read to the end in YACReader", "Ch 13 opened next in Simple
/// Comic", "Open for 10 minutes in Okular".
pub fn how(r: &Read) -> String {
    match &r.why {
        Some(why) => {
            let mut w = why.clone();
            if let Some(first) = w.get(..1) {
                w.replace_range(..1, &first.to_uppercase());
            }
            format!("{w} in {}", r.source)
        }
        None => format!("Read to the end in {}", r.source),
    }
}

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
    /// The comics the readers have open now (sent when it changes, before any `Seen`).
    Open(Vec<std::path::PathBuf>),
    /// Where to continue a manga (`None`: nothing to continue).
    Resume { media_id: u32, at: Option<kotora_core::manga::resume::Resume> },
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
    /// The reader has it open now (its process and command line, or the file). Closed, it
    /// stays only while it asks or offers Undo, and is no longer "Now Reading".
    pub open: bool,
}

impl Reading {
    fn from_seen(s: Seen) -> Reading {
        Reading { read: s.read, found: s.found, candidates: s.candidates, cover_url: s.cover_url, dismissed: false, acted: false, marked: None, picking: false, mark: None, open: false }
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
    /// `[reading] find_libraries`: also the libraries in YACReader's own settings.
    pub find: bool,
    /// `[reading] open_files`: stage 2, comic files open in any process (opt-in).
    pub open_files: bool,
    /// `[reading] comic_folders`.
    pub folders: Vec<String>,
    pub folder_draft: TextEdit,
    pub folder_error: Option<String>,
    /// `[reading] open_minutes`.
    pub minutes: u32,
    pub current: Option<Reading>,
    /// What the readers have open now (`ReadingMsg::Open`).
    pub open_now: Vec<std::path::PathBuf>,
    /// The manga of the comic read last (Now Reading's "Continue reading" once it closed).
    pub last_media: Option<u32>,
    /// "Continue reading": the thread's answers by manga, and the manga asked about.
    pub resume: std::collections::HashMap<u32, Option<kotora_core::manga::resume::Resume>>,
    pub resume_asked: std::collections::HashSet<u32>,
    tx: Option<Sender<In>>,
    /// Counts the threads started: a message from a stopped one is dropped.
    epoch: u64,
}

impl Default for ReadingUi {
    fn default() -> Self {
        ReadingUi {
            on: true,
            added: Vec::new(),
            watched: Vec::new(),
            errors: Vec::new(),
            draft: TextEdit::new(""),
            error: None,
            find: true,
            open_files: false,
            folders: Vec::new(),
            folder_draft: TextEdit::new(""),
            folder_error: None,
            minutes: 10,
            current: None,
            open_now: Vec::new(),
            last_media: None,
            resume: Default::default(),
            resume_asked: Default::default(),
            tx: None,
            epoch: 0,
        }
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
    /// Where to continue this manga (`resume`); answered now and again after each change.
    Resume(u32),
    /// The question about a counted comic was answered: report what is open again.
    Answered,
}

/// `kotora: reading: …` on stderr: what detection saw, matched, opened, closed and asked. Only
/// on a change; file names, never folders. `KOTORA_DEBUG_READING=1` adds every look.
pub(crate) fn log(msg: impl std::fmt::Display) {
    if !rune::is_headless() {
        eprintln!("kotora: reading: {msg}");
    }
}

fn debug_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("KOTORA_DEBUG_READING").is_some_and(|v| !v.is_empty() && v != "0"))
}

fn debug(msg: impl std::fmt::Display) {
    if debug_on() {
        log(msg);
    }
}

/// A path's file name, for the log.
fn name_of(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn found_text(f: &Found) -> String {
    match f {
        Found::List { media_id, sure, why } => format!("list entry {media_id} ({}: {why})", if *sure { "sure" } else { "unsure" }),
        Found::NotOnList { media_id, title } => format!("not on the list: {title} ({media_id})"),
        Found::Ignored => "ignored (not a manga)".into(),
        Found::Nothing => "no match".into(),
    }
}

// ------------------------------------------------------------------------------- the thread

/// At start and after any change that matters (Manga on/off, the switch, the libraries, sign
/// in/out): starts or stops the thread. Never headless.
pub fn sync(s: &mut State) {
    let want = (s.reading.on || (s.reading.open_files && OPEN_FILES_HERE)) && s.manga.enabled && s.has_list() && s.handle.is_some() && !rune::is_headless();
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
    s.reading.open_now.clear();
    s.reading.resume.clear();
    s.reading.resume_asked.clear();
    s.withdraw_reading_note();
    s.reading.current = None;
}

/// Stage 2 runs here (macOS, Linux). Windows: not yet.
pub const OPEN_FILES_HERE: bool = cfg!(unix);

/// What the thread is started with.
struct Cfg {
    yacreader: bool,
    find: bool,
    added: Vec<String>,
    open_files: bool,
    folders: Vec<String>,
    minutes: u32,
}

/// A setting changed: a new watch.
fn restart(s: &mut State) {
    stop(s);
    sync(s);
}

fn stop(s: &mut State) {
    // Dropping the sender ends the thread at its next wake.
    s.reading.tx = None;
    s.reading.watched.clear();
    s.reading.errors.clear();
    s.reading.open_now.clear();
    s.reading.resume.clear();
    s.reading.resume_asked.clear();
    if s.reading.current.as_ref().is_some_and(|r| r.marked.is_none()) {
        s.withdraw_reading_note();
        s.reading.current = None;
    }
}

fn start(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let (tx, rx) = mpsc::channel();
    let r = &s.reading;
    let cfg = Cfg { yacreader: r.on, find: r.find, added: r.added.clone(), open_files: r.open_files && OPEN_FILES_HERE, folders: r.folders.clone(), minutes: r.minutes };
    s.reading.epoch += 1;
    let h = Out { h, epoch: s.reading.epoch };
    if std::thread::Builder::new().name("kotora-reading".into()).spawn(move || run(&h, &rx, &cfg)).is_ok() {
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

/// A path with its symlinks resolved (a library or comic folder behind a link: the log and
/// `/proc` name the real place). Kept as given when it can't be resolved.
fn real(p: &str) -> String {
    std::fs::canonicalize(p).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string())
}

fn run(h: &Out, rx: &Receiver<In>, cfg: &Cfg) {
    // Comic folders as the process table names them (symlinks resolved), and as given.
    let mut folders: Vec<String> = cfg.folders.iter().map(|f| real(f)).collect();
    for f in &cfg.folders {
        if !folders.contains(f) {
            folders.push(f.clone());
        }
    }
    let libs = if cfg.yacreader { core_reading::libraries(&cfg.added, cfg.find) } else { Vec::new() };
    send(h, ReadingMsg::Libs { watched: libs.iter().map(|l| l.path.to_string_lossy().into_owned()).collect(), errors: Vec::new() });
    // The process table (macOS, Linux; `None` elsewhere): the YACReader viewer's command line
    // (it keeps no file open) and, with stage 2 on, open comic files.
    let mut scanner = if cfg.open_files || cfg.yacreader { kotora_detect::comics::system() } else { None };
    let mut tick: u32 = 0;
    // Nothing to watch until a setting changes (that starts a new thread).
    if libs.is_empty() && scanner.is_none() {
        return;
    }
    let mut tracker = kotora_core::manga::openfile::Tracker::new(cfg.minutes);
    log(format_args!(
        "watching {} YACReader librar{}{}; open files {}",
        watch_len(&cfg.added, cfg.find, cfg.yacreader),
        if watch_len(&cfg.added, cfg.find, cfg.yacreader) == 1 { "y" } else { "ies" },
        if cfg.yacreader { "" } else { " (YACReader off)" },
        if cfg.open_files { "on" } else { "off" }
    ));
    // The viewer's own files (a comic opened on its own, not from the library).
    let viewer_dir = if cfg.yacreader { kotora_core::manga::viewer::find_dir() } else { None };
    let mut viewer_log = viewer_dir.as_deref().map(kotora_core::manga::viewer::LogTail::new);
    let mut viewer_file: Option<String> = None;
    debug(format_args!("viewer log: {}", if viewer_dir.is_some() { "found" } else { "none" }));
    let mut viewer_since: Option<std::time::Instant> = None;
    // The bookmarks' time now: only a write after this counts.
    let mut bookmarks_seen = viewer_dir.as_ref().and_then(|d| std::fs::metadata(d.join("bookmarks.yacr")).and_then(|m| m.modified()).ok());
    let mut files_kept: Vec<kotora_core::manga::openfile::Open> = Vec::new();
    let mut open_sent: Vec<std::path::PathBuf> = Vec::new();
    // "Continue reading": the manga asked about, answered again after each change.
    let mut resume_ids: Vec<u32> = Vec::new();
    let known = kotora_core::manga::yacreader::find_ini().and_then(|i| kotora_core::manga::yacreader::libraries(&i).ok()).unwrap_or_default();
    let mut yac_cache: Option<(String, i64, String)> = None;
    let mut watch = Watch::new(libs, now_unix());
    let mut store: Option<Store> = None;
    let mut last: Option<Read> = None;
    let mut last_errors: Vec<LibState> = Vec::new();
    loop {
        let mut errors = Vec::new();
        let mut changed = false;
        // The library's path for the file, links resolved: the same name the viewer's log and
        // the process table give it.
        if let Some(r) = watch.poll(&mut errors).map(|mut r| {
            r.path = std::path::PathBuf::from(real(&r.path.to_string_lossy()));
            r
        })
            && last.as_ref() != Some(&r)
        {
            changed = true;
            let seen = recognise(&mut store, r.clone());
            last = Some(r);
            send(h, ReadingMsg::Seen(Box::new(seen)));
        }
        if let Some(sc) = &mut scanner {
            // Command lines every wake (a few names, cached); open files every third (they
            // read every process's descriptors inside comic folders).
            // While a file is open, its closing is looked for at every wake.
            let files = cfg.open_files && (tick.is_multiple_of(3) || !files_kept.is_empty());
            let opts = kotora_detect::comics::ScanOpts { folders: folders.clone(), files, yac_args: cfg.yacreader, reader_args: cfg.open_files };
            tick = tick.wrapping_add(1);
            let found = sc.scan_with(&opts);
            let mut open: Vec<kotora_core::manga::openfile::Open> = Vec::new();
            // The same file by one name: links resolved (the library's own path, the log's, argv's).
            let found: Vec<_> = found.into_iter().map(|mut o| {
                if o.yac.is_none() {
                    o.path = real(&o.path);
                }
                o
            }).collect();
            for o in found {
                match o.yac {
                    // The viewer: its comic from the library's database (read only).
                    Some(id) => {
                        if let Some(path) = yac_comic(&mut yac_cache, &o.path, id) {
                            open.push(kotora_core::manga::openfile::Open { app: o.app, path: real(&path) });
                        }
                    }
                    None => open.push(kotora_core::manga::openfile::Open { app: o.app, path: o.path }),
                }
            }
            // A YACReader viewer runs: its log names the comic opened last. Newer than the viewer's
            // start, it beats the command line (the next volume opened inside the viewer keeps the
            // old --comicId); a viewer opened from the Finder has only the log.
            if sc.viewers == 0 {
                viewer_since = None;
            } else {
                let since = *viewer_since.get_or_insert_with(std::time::Instant::now);
                if let Some(t) = viewer_log.as_mut() {
                    t.poll();
                    let fresh = t.fresh_since(since).map(real);
                    if let Some(p) = viewer_from_log(&mut open, fresh, t.last.as_deref().map(real), sc.bare_viewers > 0, |p| std::path::Path::new(p).is_file())
                        && sc.bare_viewers > 0
                    {
                        viewer_file = Some(p);
                    }
                }
            }
            // Closed: the page it was left on, from the viewer's bookmarks (written on close).
            if let (Some(dir), Some(p)) = (&viewer_dir, &viewer_file)
                && sc.bare_viewers == 0
            {
                let m = std::fs::metadata(dir.join("bookmarks.yacr")).and_then(|m| m.modified()).ok();
                if m.is_some() && m != bookmarks_seen {
                    bookmarks_seen = m;
                    if let Some(r) = viewer_read(dir, p)
                        && last.as_ref() != Some(&r)
                    {
                        let seen = recognise(&mut store, r.clone());
                        last = Some(r);
                        changed = true;
                        send(h, ReadingMsg::Seen(Box::new(seen)));
                    }
                    viewer_file = None;
                }
            }
            // Between file scans, the files seen last stand (the tracker counts from them).
            if opts.files {
                files_kept = open.clone();
            } else if cfg.open_files {
                for k in &files_kept {
                    if !open.contains(k) {
                        open.push(k.clone());
                    }
                }
            }
            // Open or closed, said first: a `Seen` that follows knows.
            let mut paths: Vec<std::path::PathBuf> = open.iter().map(|o| std::path::PathBuf::from(&o.path)).collect();
            paths.sort();
            if paths != open_sent {
                changed = true;
                for p in paths.iter().filter(|p| !open_sent.contains(p)) {
                    let app = open.iter().find(|o| std::path::Path::new(&o.path) == p.as_path()).map(|o| o.app.as_str()).unwrap_or("");
                    log(format_args!("open in {app}: {}", name_of(p)));
                }
                for p in open_sent.iter().filter(|p| !paths.contains(p)) {
                    log(format_args!("closed: {}", name_of(p)));
                }
                open_sent = paths.clone();
                send(h, ReadingMsg::Open(paths));
            }
            if let Some(r) = tracker.update(&open, now_unix())
                && last.as_ref() != Some(&r)
            {
                let seen = recognise(&mut store, r.clone());
                last = Some(r);
                send(h, ReadingMsg::Seen(Box::new(seen)));
            }
        }
        if changed {
            let libs = kotora_core::manga::resume::with_ids(watch.libraries().cloned().collect(), &known);
            for id in &resume_ids {
                send(h, ReadingMsg::Resume { media_id: *id, at: resume_for(&mut store, &libs, *id) });
            }
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
            Ok(In::Resume(id)) => {
                if !resume_ids.contains(&id) {
                    resume_ids.push(id);
                }
                let libs = kotora_core::manga::resume::with_ids(watch.libraries().cloned().collect(), &known);
                send(h, ReadingMsg::Resume { media_id: id, at: resume_for(&mut store, &libs, id) });
            }
            Ok(In::Answered) => tracker.release(),
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

/// A comic the viewer had open on its own, after it was closed: its page from `bookmarks.yacr`
/// (from 0), the count from the `.cbz`. A strong signal: read to the end counts.
fn viewer_read(dir: &std::path::Path, path: &str) -> Option<Read> {
    use kotora_core::manga::viewer;
    let p = std::path::Path::new(path);
    let bm = *viewer::read_bookmarks(dir).get(&viewer::bookmark_key(p)?)?;
    let mut r = kotora_core::manga::openfile::read_of(&kotora_core::manga::openfile::Open { app: "YACReader".into(), path: path.into() }, now_unix());
    r.page = bm.last_page + 1;
    r.pages = viewer::zip_pages(p).unwrap_or(0);
    r.finished = kotora_core::manga::chapter_read(false, r.page, r.pages);
    r.weak = false;
    Some(r)
}

/// The viewer's comic from its log: a line newer than the viewer's start (`fresh`) replaces the
/// comic its command line names (the next volume opened inside it keeps the old --comicId); a
/// viewer with no comic in its command line (opened from the Finder) takes the log's last line.
/// Returns the path used.
fn viewer_from_log(open: &mut Vec<kotora_core::manga::openfile::Open>, fresh: Option<String>, last: Option<String>, bare: bool, exists: impl Fn(&str) -> bool) -> Option<String> {
    let p = fresh.or(if bare { last } else { None }).filter(|p| exists(p))?;
    if !open.iter().any(|o| o.path == p) {
        debug(format_args!("viewer log: {}", name_of(std::path::Path::new(&p))));
        open.retain(|o| o.app != "YACReader");
        open.push(kotora_core::manga::openfile::Open { app: "YACReader".into(), path: p.clone() });
    }
    Some(p)
}

/// How many libraries the watch will have (for the start line).
fn watch_len(added: &[String], find: bool, on: bool) -> usize {
    if on { core_reading::libraries(added, find).len() } else { 0 }
}

/// Where to continue manga `id` of the list (its entry and the user's picks from the store).
fn resume_for(store: &mut Option<Store>, libs: &[kotora_core::manga::yacreader::Library], id: u32) -> Option<kotora_core::manga::resume::Resume> {
    let st = open(store)?;
    let svc = crate::account::name();
    let item = st.load_item(Scope::new(svc, Kind::Manga), id).ok()??;
    let picked = st.picked_keys(svc, Kind::Manga, id).unwrap_or_default();
    kotora_core::manga::resume::resume(libs, &item, &picked)
}

/// How YACReaderLibrary opens a comic (`library_comic_opener.cpp`): the viewer with the library,
/// `--comicId` and `--libraryId`, so the page is saved back to the library. Without a library id
/// (a library YACReader's settings don't list) the file alone.
pub fn launch_args(r: &kotora_core::manga::resume::Resume) -> Vec<String> {
    match r.library_id {
        Some(lib) => vec![r.library.to_string_lossy().into_owned(), format!("--comicId={}", r.comic_id), format!("--libraryId={lib}")],
        None => vec![r.path.to_string_lossy().into_owned()],
    }
}

/// Starts YACReader on the comic (never waits on it). macOS: `open -n <YACReader.app> --args …`
/// as the library does; Linux: `YACReader …`; else, or when YACReader isn't there, the file with
/// the system's opener.
fn launch(r: &kotora_core::manga::resume::Resume) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let file = r.path.to_string_lossy().into_owned();
    let spawn = |prog: &str, args: &[String]| Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ());
    #[cfg(target_os = "macos")]
    {
        let home = kotora_core::paths::home_dir().unwrap_or_default();
        let app = [std::path::PathBuf::from("/Applications/YACReader.app"), home.join("Applications/YACReader.app")].into_iter().find(|p| p.is_dir());
        if let Some(app) = app {
            let mut a = vec!["-n".to_string(), app.to_string_lossy().into_owned(), "--args".into()];
            a.extend(launch_args(r));
            return spawn("open", &a).map_err(|e| format!("Can't start YACReader: {e}"));
        }
        spawn("open", &[file]).map_err(|e| format!("Can't open the comic: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if spawn("YACReader", &launch_args(r)).is_ok() {
            return Ok(());
        }
        spawn("xdg-open", &[file]).map_err(|e| format!("Can't open the comic (no YACReader, no xdg-open): {e}"))
    }
    #[cfg(windows)]
    {
        // YACReader's installer puts it in Program Files.
        let exe = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .flat_map(|d| [std::path::PathBuf::from(&d).join("YACReader").join("YACReader.exe"), std::path::PathBuf::from(&d).join("Programs").join("YACReader").join("YACReader.exe")])
            .find(|p| p.is_file());
        if let Some(exe) = exe
            && spawn(&exe.to_string_lossy(), &launch_args(r)).is_ok()
        {
            return Ok(());
        }
        crate::win::open(&file)
    }
}

/// The file of comic `id` in the YACReader library at `library` (its database, read only),
/// remembered while the viewer shows the same comic.
fn yac_comic(cache: &mut Option<(String, i64, String)>, library: &str, id: i64) -> Option<String> {
    if let Some((l, i, p)) = cache.as_ref()
        && l == library
        && *i == id
    {
        return Some(p.clone());
    }
    let path = core_reading::viewer_comic(library, id)?.to_string_lossy().into_owned();
    *cache = Some((library.to_string(), id, path.clone()));
    Some(path)
}

fn open(store: &mut Option<Store>) -> Option<&mut Store> {
    if store.is_none() {
        *store = Store::open(&kotora_core::paths::db_file()).ok();
    }
    store.as_mut()
}

/// What one match line says, to log it once per open: the file, the match and "read to the end";
/// the page counts only once it is known on both lines (the open-file signal comes first, without
/// a page; YACReader's database follows with one).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogKey {
    pub path: std::path::PathBuf,
    pub found: String,
    pub finished: bool,
    pub page: Option<(i64, i64)>,
}

/// Whether `new` says something the last line didn't.
pub(crate) fn log_due(prev: Option<&LogKey>, new: &LogKey) -> bool {
    let Some(p) = prev else { return true };
    p.path != new.path || p.found != new.found || p.finished != new.finished || matches!((p.page, new.page), (Some(a), Some(b)) if a != b)
}

thread_local! {
    static LAST_LOG: std::cell::RefCell<Option<LogKey>> = const { std::cell::RefCell::new(None) };
}

fn recognise(store: &mut Option<Store>, read: Read) -> Seen {
    let seen = recognise_quiet(store, read);
    let r = &seen.read;
    let key = LogKey { path: r.path.clone(), found: found_text(&seen.found), finished: r.finished, page: (r.pages > 0 && !r.weak).then_some((r.page, r.pages)) };
    let due = LAST_LOG.with(|l| {
        let mut l = l.borrow_mut();
        let due = log_due(l.as_ref(), &key);
        // Keep the page once known, so a later line without one doesn't hide a real change.
        let page = key.page.or(l.as_ref().filter(|p| p.path == key.path).and_then(|p| p.page));
        *l = Some(LogKey { page, ..key.clone() });
        due
    });
    if !due {
        return seen;
    }
    let state = if r.finished { format!(", read to the end{}", r.why.as_ref().map(|w| format!(" ({w})")).unwrap_or_default()) } else if r.pages > 0 && !r.weak { format!(", page {} of {}", r.page, r.pages) } else { String::new() };
    let renum = r.renumbered.as_ref().map(|n| format!(" [{n}]")).unwrap_or_default();
    log(format_args!("{} in {}: \"{}\" {}{renum}{state} → {}", name_of(&r.path), r.source, r.series, r.label(), found_text(&seen.found)));
    seen
}

fn recognise_quiet(store: &mut Option<Store>, read: Read) -> Seen {
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
            // An answer cached before today's rules ("Dragon Ball Z: Fukkatsu no F" for "Dragon
            // Ball Z") is searched again.
            Some((id, _, title)) if !title.is_empty() && core_reading::title_fits(&read.series, &title) => Found::NotOnList { media_id: id, title },
            _ => {
                let (f, url) = search(st, &read.series, &items, &key);
                cover_url = url;
                f
            }
        },
    };
    // A known release that numbers on its own: the entry's numbers ("DBZ v11 = Dragon Ball vol 27").
    let mut read = read;
    let main = match &found {
        Found::List { media_id, .. } => items.iter().find(|it| it.media.id == *media_id).map(|it| it.media.title.clone()),
        Found::NotOnList { title, .. } => Some(title.clone()),
        _ => None,
    };
    if let Some(m) = main {
        core_reading::renumber(&mut read, &m);
    }
    // The cover of a manga off the list: the search's, else its saved details, else one fetch.
    if cover_url.is_none()
        && let Found::NotOnList { media_id, .. } = &found
    {
        cover_url = manga_cover(st, *media_id);
    }
    Seen { read, found, candidates, cover_url }
}

/// A manga's cover from its saved details page, or one details fetch (signed in, online), saved
/// like the details page's.
fn manga_cover(st: &Store, id: u32) -> Option<String> {
    let svc = crate::account::name();
    if let Some((d, _)) = st.details(Scope::new(svc, Kind::Manga), id).ok().flatten() {
        return d.cover_url;
    }
    if kotora_core::paths::offline_forced() {
        return None;
    }
    let d = crate::account::open()?.details_of(Kind::Manga, id).ok()?;
    let _ = st.save_details(svc, &d, now_unix());
    d.cover_url
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
            ReadingMsg::Open(paths) => self.reading_open(paths),
            ReadingMsg::Resume { media_id, at } => {
                let changed = self.reading.resume.get(&media_id) != Some(&at);
                self.reading.resume.insert(media_id, at);
                changed
            }
        }
    }

    /// Asks the reading thread where to continue `media_id` (once; it answers again on changes).
    pub fn want_resume(&mut self, media_id: u32) {
        if self.reading.running() && self.reading.resume_asked.insert(media_id) {
            self.reading.send(In::Resume(media_id));
        }
    }

    /// Where to continue `media_id`, when known.
    pub fn resume_of(&self, media_id: u32) -> Option<&kotora_core::manga::resume::Resume> {
        self.reading.resume.get(&media_id)?.as_ref()
    }

    /// "Continue reading": reopens the comic in YACReader at the saved page.
    pub fn continue_reading(&mut self, media_id: u32) {
        let Some(r) = self.resume_of(media_id).cloned() else { return };
        self.ctx = None;
        if self.handle.is_none() {
            return;
        }
        let h = self.handle.clone();
        let _ = std::thread::Builder::new().name("kotora-continue".into()).spawn(move || {
            if let Err(e) = launch(&r)
                && let Some(h) = h
            {
                h.update_if(move |s, _| {
                    s.toast = Some(crate::library::Toast { text: e, warn: true, until: std::time::Instant::now() + Duration::from_secs(6), undo: None });
                    true
                });
            }
        });
    }

    /// The comic followed now: open in its reader. `None` when it was closed (it may still ask
    /// in the banner). The sidebar's Now Reading card and Discord show only this.
    pub fn now_reading(&self) -> Option<&Reading> {
        self.reading.current.as_ref().filter(|r| r.open)
    }

    /// Whether a closed comic still has something to say: a question or an Undo.
    fn reading_asks(&self) -> bool {
        self.reading.current.as_ref().is_some_and(|r| r.marked.is_some() || (r.read.finished && !r.dismissed && self.reading_offer().is_some()))
    }

    /// The comics open now. The followed one closed: it is no longer "Now Reading" at once, and
    /// it goes unless it asks (read to the end) or offers Undo.
    pub fn reading_open(&mut self, paths: Vec<std::path::PathBuf>) -> bool {
        self.reading.open_now = paths;
        let Some(r) = &mut self.reading.current else { return true };
        let open = self.reading.open_now.contains(&r.read.path);
        if r.open == open {
            return true;
        }
        r.open = open;
        if !open && !self.reading_asks() {
            self.withdraw_reading_note();
            self.reading.current = None;
            debug("closed: Now Reading cleared");
        }
        true
    }

    /// A comic from the thread. The same file keeps its answers (Not now, marked).
    pub fn reading_seen(&mut self, seen: Seen) -> bool {
        let mut new = Reading::from_seen(seen);
        new.open = self.reading.open_now.contains(&new.read.path);
        if let Some(id) = new.media_id() {
            self.reading.last_media = Some(id);
            self.want_resume(id);
        }
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
        // Closed with nothing to ask (YACReader's write on close of a half-read comic): gone.
        if self.reading.current.as_ref().is_some_and(|r| !r.open) && !self.reading_asks() {
            self.reading.current = None;
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
        // An open file only (no page) is a guess: always ask, as for a player seen by its file.
        let auto = may_record && self.prefs.auto && r.sure() && !r.read.weak && note.is_none();
        if let Some(r) = &mut self.reading.current {
            r.acted = true;
        }
        log(format_args!("{}: {} of {title}", if auto { "marking" } else { "asks" }, o.label()));
        if auto {
            if self.mark_reading(true).is_ok() {
                self.notify_reading(NoteKind::ReadRecorded, format!("Marked {} as read", o.label()), title);
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
        log(format_args!("marked {} of {title}", o.label()));
        self.reading.send(In::Answered);
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
        log("undone");
        if let Some(r) = &mut self.reading.current {
            r.mark = mark;
            r.marked = None;
            r.dismissed = true;
        }
        if self.reading.current.as_ref().is_some_and(|r| !r.open) {
            self.reading.current = None;
        }
    }

    /// Not now (the banner, `kot dismiss`, the notification).
    pub fn dismiss_reading(&mut self) -> bool {
        let open = match &mut self.reading.current {
            Some(r) if !r.dismissed => {
                r.dismissed = true;
                r.picking = false;
                log(format_args!("not now: {}", r.read.file_name));
                r.open
            }
            _ => return false,
        };
        self.withdraw_reading_note();
        self.reading.send(In::Answered);
        // Closed and answered: nothing left to show (live test: a closed comic said "not now"
        // stayed as Now Reading).
        if !open {
            self.reading.current = None;
        }
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
        self.reading.send(In::Answered);
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
            reader: r.read.source.clone(),
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
            open: r.open,
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
            NoteKind::ReadOffer => self.prefs.notify.read_offer,
            NoteKind::ReadRecorded => self.prefs.notify.read_recorded,
            _ => false,
        };
        let Some(id) = self.reading_note_id().filter(|_| on) else {
            if kind == NoteKind::ReadRecorded {
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
                Ok((title, o)) => self.notify_reading(NoteKind::ReadRecorded, format!("Marked {} as read", o.label()), title),
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
        restart(self);
    }

    /// Settings: also the libraries in YACReader's own settings.
    pub fn set_reading_find(&mut self, on: bool) {
        self.reading.find = on;
        crate::detect::save_setting(self, "reading", "find_libraries", on.to_string());
        restart(self);
    }

    /// Settings: comic files open in any reader (stage 2, opt-in).
    pub fn set_open_files(&mut self, on: bool) {
        self.reading.open_files = on;
        crate::detect::save_setting(self, "reading", "open_files", on.to_string());
        restart(self);
    }

    /// Settings: the minutes open that count a comic as read (1 to 60).
    pub fn step_open_minutes(&mut self, up: bool) {
        let m = self.reading.minutes;
        let m = if up { (m + if m >= 10 { 5 } else { 1 }).min(60) } else { m.saturating_sub(if m > 10 { 5 } else { 1 }).max(1) };
        if m == self.reading.minutes {
            return;
        }
        self.reading.minutes = m;
        crate::detect::save_setting(self, "reading", "open_minutes", m.to_string());
        restart(self);
    }

    fn save_comic_folders(&mut self) {
        let v = kotora_core::config::quote_list(&self.reading.folders);
        crate::detect::save_setting(self, "reading", "comic_folders", v);
        restart(self);
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
    s.reading.find = c.reading.find_libraries;
    s.reading.open_files = c.reading.open_files;
    s.reading.folders = c.reading.comic_folders.clone();
    s.reading.minutes = c.reading.open_minutes.clamp(1, 60);
}

/// Adds a comic folder (stage 2): other processes' files count only inside these.
pub fn add_comic_folder(s: &mut State, raw: &str) -> Result<(), String> {
    let path = crate::library::tidy(raw);
    if path.is_empty() {
        return Err("Type a folder's path, or press Choose\u{2026}".into());
    }
    if !std::path::Path::new(&path).is_absolute() {
        return Err("Give the whole path, from / (or ~/).".into());
    }
    if s.reading.folders.contains(&path) {
        return Err("That folder is already in the list.".into());
    }
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("No folder at {path}."));
    }
    s.reading.folders.push(path);
    s.reading.folder_draft.clear();
    s.reading.folder_error = None;
    s.save_comic_folders();
    Ok(())
}

pub fn remove_comic_folder(s: &mut State, path: &str) {
    s.reading.folders.retain(|p| p != path);
    s.save_comic_folders();
}

fn add_typed_folder(s: &mut State) {
    let raw = s.reading.folder_draft.text.clone();
    if let Err(e) = add_comic_folder(s, &raw) {
        s.reading.folder_error = Some(e);
    }
}

fn choose_comic_folder(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let r = crate::library::choose_folder("Choose a folder with comics in it", move |p| {
        h.update_if(move |s, _| {
            if let Err(e) = p.and_then(|p| add_comic_folder(s, &p)) {
                s.reading.folder_error = Some(e);
            }
            true
        });
    });
    if let Err(e) = r {
        s.reading.folder_error = Some(e);
    }
}

/// Settings → Manga → Reading detection → Comic folders (stage 2).
pub fn folders_block(s: &State) -> El {
    let r = &s.reading;
    let mut c = col().key("reading-folders").w_full();
    if r.folders.is_empty() {
        c = c.child(text("None: only known readers (Simple Comic, MComix, Okular, \u{2026}) are looked at, wherever their files are.").font(theme::LABEL).color(PAL.faint).wrap().py(4.0));
    }
    for path in &r.folders {
        let there = std::path::Path::new(path).is_dir();
        let p = path.clone();
        c = c.child(
            row()
                .key(format!("set/comic-folder/{path}"))
                .gap(12.0)
                .py(6.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(if there { PAL.ok() } else { PAL.warn() }))
                .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(path.clone()).font(theme::BODY_MED).color(PAL.text).truncate()).child(text(if there { "Any app\u{2019}s comic files in here count" } else { "Not there (drive not connected?)" }).font(theme::LABEL).color(PAL.dim)))
                .child(ghost(&format!("set/comic-folder/remove/{path}"), "Remove", move |s| remove_comic_folder(s, &p))),
        );
    }
    let input = row()
        .key("set/comic-folder/box")
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, if r.folder_error.is_some() { PAL.warn() } else { PAL.border_strong })
        .child(
            text_input(&r.folder_draft, TextStyle::new(12.0, 400.0).mono())
                .key("set/comic-folder/input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&r.folder_draft, "~/Manga"), PAL.faint)
                .on_edit(|s: &mut State| &mut s.reading.folder_draft)
                .on_input_key(crate::ui::field_page_key)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.reading.folder_error = None)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    add_typed_folder(s);
                }),
        );
    c = c.child(
        row()
            .gap(8.0)
            .mt(6.0)
            .center_y()
            .child(input)
            .child(button("set/comic-folder/add", "Add", false, Some(add_typed_folder)))
            .child(button("set/comic-folder/choose", "Choose\u{2026}", false, Some(choose_comic_folder))),
    );
    if let Some(e) = &r.folder_error {
        c = c.child(text(e.clone()).key("set/comic-folder/error").font(theme::LABEL).color(PAL.warn()).wrap().mt(6.0));
    }
    c
}

pub const OPEN_FILES_NOTE: &str = "Kotora sees which comic file (.cbz, .cbr, .cb7, .zip, .pdf, or a folder of pages) a reader has open, not the page. It asks when the next chapter of the series opens, or after the minutes below. Checks every 30 seconds. Off by default.";

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

pub const SWITCH_NOTE: &str = "Kotora notices a chapter you read to the end in YACReader and asks before it marks it. It sees the comic the YACReader viewer shows (its command line) and reads YACReader\u{2019}s library database when it changes, read only, every 10 seconds.";

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

/// The cover id and address of the comic's manga (a tile id from the title when unknown).
fn cover_of<'a>(s: &'a State, r: &'a Reading, title: &str) -> (u32, Option<&'a str>) {
    match &r.found {
        Found::List { media_id, .. } => (crate::manga::cover_id(*media_id), s.manga.entries.iter().find(|e| e.id == *media_id).and_then(|e| e.cover_url.as_deref())),
        Found::NotOnList { media_id, .. } => (crate::manga::cover_id(*media_id), r.cover_url.as_deref()),
        _ => (title.bytes().map(u32::from).sum::<u32>() | crate::manga::COVER_BIT, None),
    }
}

/// Manga mode with reading detection on: the sidebar says "Now Reading" and shows the comic.
pub fn now_reading_mode(s: &State) -> bool {
    s.manga.mode && s.manga.enabled && (s.reading.on || (s.reading.open_files && OPEN_FILES_HERE))
}

/// Now Reading with nothing read yet: how Kotora sees reading.
pub fn empty(s: &State) -> El {
    let mut how = Vec::new();
    if s.reading.on {
        how.push("YACReader: open a comic, from its library or the Finder; it shows here within seconds, and Kotora asks once it is read to the end.".to_string());
    }
    if s.reading.open_files && OPEN_FILES_HERE {
        how.push(format!("Any reader: open a comic file; Kotora asks when the next chapter opens or after {} minutes.", s.reading.minutes));
    }
    let mut c = col()
        .key("rd-empty")
        .gap(6.0)
        .padding(16.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(text("Nothing is being read").font(theme::SECTION).color(PAL.text));
    for h in how {
        c = c.child(text(h).font(theme::LABEL).color(PAL.dim).wrap());
    }
    // The manga read last: where it stopped.
    if let Some(id) = s.reading.last_media
        && let Some(line) = resume_line(s, id)
    {
        let title = s.manga.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).unwrap_or_default();
        c = c.child(text(title).key("rd-empty/last").font(theme::BODY_MED).color(PAL.text).pt(8.0)).child(line);
    }
    c
}

/// "Continue reading" for `media_id`: where the user stopped and the button (none when unknown).
pub fn resume_line(s: &State, media_id: u32) -> Option<El> {
    let r = s.resume_of(media_id)?;
    let what = if r.next { format!("Next: {}", r.label()) } else { format!("Stopped at {}", r.label()) };
    Some(
        col()
            .key("rd-resume")
            .gap(6.0)
            .pt(10.0)
            .child(text(what).key("rd-resume/where").font(theme::LABEL).color(PAL.dim).wrap())
            .child(row().child(action("rd-resume/go".into(), "Continue reading", false, move |s, _| s.continue_reading(media_id)).h(30.0).px(12.0))),
    )
}

/// The sidebar's card for the comic being read (Manga mode), like the playing card for anime.
pub fn mini(s: &State, r: &Reading, tall: bool) -> El {
    use crate::look;
    let title = s.reading_title(r);
    let (id, url) = cover_of(s, r, &title);
    let what = r.read.label();
    let tail = if r.marked.is_some() {
        "Marked as read".to_string()
    } else if r.read.finished {
        if r.read.weak { "Counted as read".into() } else { "Read to the end".into() }
    } else if r.read.pages > 0 && !r.read.weak {
        format!("Page {} of {}", r.read.page.max(1), r.read.pages)
    } else {
        format!("Open in {}", r.read.source)
    };
    el().key("rd-mini")
        .role("button")
        .label(format!("Now reading: {title}"))
        .w_full()
        .h(if tall { 124.0 } else { 76.0 })
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .press(PAL.press())
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.go(Page::NowPlaying);
        })
        .child(look::backdrop(s, id, url, look::card_scrim()))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(if tall { 10.0 } else { 8.0 })
                .gap(10.0)
                .child(if tall { look::poster(s, id, &title, url, 68.0, 102.0, theme::R_SM) } else { look::poster(s, id, &title, url, 40.0, 60.0, theme::R_XS) }.shadow(4.0, 10.0, rune::Color::BLACK.alpha(0.35)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(3.0)
                        .child(row().gap(6.0).center_y().child(look::live_dot(!r.read.finished && r.marked.is_none(), 6.0)).child(text(r.read.source.to_uppercase()).font(theme::CAPTION).color(PAL.dim).truncate()))
                        .child(text(title).font(TextStyle::new(13.0, 600.0)).color(PAL.text).wrap().max_lines(if tall { 2 } else { 1 }))
                        .child(if tall { text(what).font(theme::SMALL).color(PAL.dim).truncate() } else { el() })
                        .child(rune::spacer())
                        .child(text(tail).key("rd-mini/state").font(theme::SMALL).color(PAL.dim).truncate()),
                ),
        )
}

fn reading_cover(s: &State, r: &Reading, w: f32, h: f32, size: f32) -> El {
    let title = s.reading_title(r);
    let (id, url) = cover_of(s, r, &title);
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
    let mut small = row().min_w(0.0).gap(6.0).child(text(format!("{}{matched}", how(&r.read))).font(theme::LABEL).color(PAL.dim).truncate().min_w(0.0));
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
    let progress = if r.read.finished && r.read.weak {
        "Counted as read".to_string()
    } else if r.read.finished {
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
        .child(text(format!("{} IN {}", if !r.open { "CLOSED" } else if r.read.weak { "OPEN" } else { "READING" }, r.read.source.to_uppercase())).key("rd/state").font(theme::LABEL).color(if r.open { PAL.accent } else { PAL.dim }))
        .child(text(title).font(theme::SECTION).color(PAL.text).truncate())
        .child(text(r.read.label()).font(theme::BODY_MED).color(PAL.text))
        .child(text(r.read.file_name.clone()).font(TextStyle::new(12.0, 400.0).mono()).color(PAL.dim).truncate())
        .child(row().gap(10.0).mt(4.0).center_y().child(bar.grow(1.0)).child(text(progress).key("rd/progress").font(theme::LABEL).color(PAL.dim)))
        .child(text(s.reading_match_text(r)).key("rd/match").font(theme::LABEL).color(if r.sure() { PAL.dim } else { PAL.warn() }).wrap().mt(4.0));
    // A release numbered on its own: what the file's numbers are on the list.
    if let Some(n) = &r.read.renumbered {
        info = info.child(text(n.clone()).key("rd/renumbered").font(theme::LABEL).color(PAL.dim).wrap());
    }
    // Unsure ("Dragon Ball Z" is another title of Dragon Ball): ask, one press answers.
    if let (Found::List { media_id, sure: false, .. }, None) = (&r.found, r.marked) {
        let id = *media_id;
        let title = s.manga.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).unwrap_or_default();
        info = info.child(
            row()
                .key("rd/ask")
                .gap(10.0)
                .mt(6.0)
                .center_y()
                .child(text(format!("Is this {title}?")).font(theme::BODY_MED).color(PAL.text))
                .child(action("rd/yes".into(), "Yes, that one", false, move |s, _| s.pick_manga(id)).h(28.0).px(10.0)),
        );
    }
    let mut acts = row().gap(8.0).mt(8.0);
    if let Some(m) = r.marked {
        acts = acts.child(text(format!("Marked {} as read.", m.offer.label())).font(theme::LABEL).color(PAL.ok())).child(button("rd/undo", "Undo", false, Some(State::undo_reading)));
    } else if let Some((_, o)) = s.reading_offer() {
        let label = format!("Mark {} as read", o.label());
        acts = acts.child(action("rd/mark".into(), &label, r.read.finished, |s, _| {
            let _ = s.confirm_reading();
        }));
        if !r.read.finished {
            let when = if r.read.weak && r.read.source == "YACReader" {
                format!("YACReader saves the page when you close the comic: Kotora asks then if it was read to the end, or when the next chapter opens, or after {} minutes open.", s.reading.minutes)
            } else if r.read.weak {
                format!("Kotora sees the file is open, not the page: it asks when the next chapter opens, or after {} minutes open.", s.reading.minutes)
            } else {
                "Kotora asks once the last page is reached (or YACReader marks it read).".to_string()
            };
            info = info.child(text(when).font(theme::LABEL).color(PAL.faint).wrap());
        } else if r.read.weak {
            info = info.child(text(how(&r.read)).key("rd/why").font(theme::LABEL).color(PAL.faint).wrap());
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
    // Closed: where it stopped, to open it again.
    if !r.open
        && let Some(line) = r.media_id().and_then(|id| resume_line(s, id))
    {
        info = info.child(line);
    }
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
        id: 1,
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
    // Open in its reader (the scenes' comics are being read).
    if !s.reading.open_now.contains(&read.path) {
        s.reading.open_now.push(read.path.clone());
    }
    s.reading_seen(Seen { read, found, candidates, cover_url: None });
}

/// A made-up comic YACReader wrote on close (`file` at `page` of 40), not open any more.
#[cfg(test)]
pub fn fake_closed(s: &mut State, file: &str, page: i64) {
    let o = kotora_core::manga::yacreader::Opened {
        id: 1,
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
    s.reading_seen(Seen { read, found, candidates: Vec::new(), cover_url: None });
}

/// A made-up "Continue reading" answer for manga `id` (`page` of 190 in volume 11).
pub fn fake_resume(s: &mut State, id: u32, page: i64, next: bool) {
    let r = kotora_core::manga::resume::Resume {
        library: "/media/comics".into(),
        library_id: Some(1),
        comic_id: 7,
        path: "/media/comics/Inkwell Cartographers v11.cbz".into(),
        chapter: None,
        volume: Some(11),
        page,
        pages: 190,
        next,
    };
    s.apply_reading(ReadingMsg::Resume { media_id: id, at: Some(r) });
}

/// Dragon Ball on the made-up list ("Dragon Ball Z" among its titles, volume 26 of 42), and
/// VIZ's "Dragon Ball Z v11" open in YACReader, as in the live test.
pub fn fake_dbz(s: &mut State) {
    let mut items = crate::manga::fake_items();
    let media = kotora_core::Media {
        id: 42,
        kind: Kind::Manga,
        title: "Dragon Ball".into(),
        synonyms: vec!["Dragon Ball Z".into()],
        episodes: Some(520),
        volumes: Some(42),
        format: "manga".into(),
        air_status: "finished".into(),
        cover_url: Some("https://cdn.example.invalid/m42.jpg".into()),
        ..Default::default()
    };
    items.push(kotora_core::ListItem { media, status: Status::Watching, progress: 300, score: None, rewatching: false, times_rewatched: 0, volumes: 26, updated_at: crate::model::FAKE_NOW - 60 });
    s.set_manga_items(&items);
    let path = "/media/comics/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz";
    let mut read = kotora_core::manga::openfile::read_of(&kotora_core::manga::openfile::Open { app: "YACReader".into(), path: path.into() }, 0);
    let found = core_reading::on_list(&read.series, &items, &None).unwrap_or(Found::Nothing);
    if let Found::List { media_id, .. } = &found
        && let Some(it) = items.iter().find(|it| it.media.id == *media_id)
    {
        core_reading::renumber(&mut read, &it.media.title);
    }
    let candidates = core_reading::candidates(&read.series, &items).into_iter().take(8).map(|c| (c.media_id, c.title)).collect();
    if !s.reading.open_now.contains(&read.path) {
        s.reading.open_now.push(read.path.clone());
    }
    s.reading_seen(Seen { read, found, candidates, cover_url: None });
}

/// A made-up comic open in `app` (stage 2), counted as read when `why` is given.
pub fn fake_open(s: &mut State, app: &str, path: &str, why: Option<&str>) {
    let open = kotora_core::manga::openfile::Open { app: app.into(), path: path.into() };
    let mut read = kotora_core::manga::openfile::read_of(&open, 0);
    read.finished = why.is_some();
    read.why = why.map(String::from);
    let items: Vec<kotora_core::ListItem> = crate::manga::fake_items();
    let found = core_reading::on_list(&read.series, &items, &None).unwrap_or(Found::Nothing);
    let candidates = core_reading::candidates(&read.series, &items).into_iter().take(8).map(|c| (c.media_id, c.title)).collect();
    // Open in its reader (the scenes' comics are being read).
    if !s.reading.open_now.contains(&read.path) {
        s.reading.open_now.push(read.path.clone());
    }
    s.reading_seen(Seen { read, found, candidates, cover_url: None });
}

#[cfg(test)]
mod tests;
