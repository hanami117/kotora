//! Library folders and "Play next episode" (`kotora_core::library` does the scanning and matching).
//!
//! One thread, `kotora-library`, owns its own database connection. It scans every folder at start,
//! when the folders change, on Rescan, and on demand ([`refresh`]: the tray menu opened, the window
//! shown; at most once per [`REFRESH_AFTER`]). A walk stats the files and parses only new or
//! changed ones. No timer: at rest the thread sleeps on its channel. It matches the files to the list again when the list
//! changes (a sync) or a "Wrong anime" pick is made. Its results reach the UI with `update_if`,
//! which redraws only when something visible changed. Nothing runs headless: scenes fill
//! [`Library`] with fake data ([`fake`]).
//!
//! The UI parts are here too, to keep the shared pages' files small: the Settings card, the list
//! row's Play button, the details page's block and Now Playing's "Up next" card.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use kotora_core::config::{self, Config};
use kotora_core::library::{self as lib, Index, Next};
use kotora_core::recognize::relations;
use kotora_core::store::Store;
use kotora_core::{Status, paths};
use rune::{El, TextEdit, TextStyle, Tx, col, el, icon, row, text, text_input};

use crate::icons;
use crate::model::{Entry, Handle, Page, State};
use crate::play::{self, Player};
use crate::theme::{self, PAL};
use crate::ui::{button, card, cell, field_hint, ghost};

/// An on-demand walk ([`refresh`]) is skipped when the last one is younger than this.
pub const REFRESH_AFTER: Duration = Duration::from_secs(60);

/// Most anime in the tray's Play next menu.
pub const PLAY_MENU_MAX: usize = 8;

const SERVICE: &str = "mal";

/// A folder of Settings → Library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub path: String,
    /// There and not empty at the last scan. Offline folders keep their files.
    pub online: bool,
    /// Episode files found (the saved ones while offline).
    pub files: usize,
    /// Scanned at least once since start.
    pub scanned: bool,
}

impl Folder {
    fn new(path: &str) -> Folder {
        Folder { path: path.to_string(), online: true, files: 0, scanned: false }
    }
}

enum Cmd {
    Folders(Vec<String>),
    Rescan,
    /// The list changed: match again, no walk.
    Rematch,
}

#[derive(Default)]
pub struct Library {
    pub folders: Vec<Folder>,
    pub index: Index,
    /// A scan runs (Rescan pressed, or folders changed).
    pub scanning: bool,
    /// `[library] open_with`.
    pub open_with: String,
    /// The player detection last saw (`mpv`, `iina`, `vlc`).
    pub last_player: String,
    /// The players installed (looked for once at start, off the UI thread).
    pub found: play::Found,
    /// The "add a folder" field.
    pub draft: TextEdit,
    /// Why a folder can't be added, or Choose… failed.
    pub error: Option<String>,
    /// The last Play: "Opening episode 7 in IINA" (shown on Now Playing and the details page).
    pub note: Option<String>,
    tx: Option<Sender<Cmd>>,
    /// When the last walk was asked for (for [`refresh`]).
    last_walk: Option<Instant>,
}

impl Library {
    pub fn installed(&self) -> Vec<Player> {
        self.found.installed()
    }

    /// The player Play opens now.
    pub fn player(&self) -> Player {
        play::resolve(&self.open_with, &self.last_player, &self.installed())
    }

    fn online(&self, folder: &str) -> bool {
        self.folders.iter().any(|f| f.path == folder && f.online)
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }

    /// The thread's answer. True when something shown changed.
    fn update(&mut self, folders: Vec<Folder>, index: Index) -> bool {
        // A folder added or removed while the scan ran: keep the UI's list, take the counts.
        let mut changed = self.scanning;
        self.scanning = false;
        for f in &mut self.folders {
            if let Some(n) = folders.iter().find(|n| n.path == f.path)
                && n != f
            {
                *f = n.clone();
                changed = true;
            }
        }
        if index != self.index {
            self.index = index;
            changed = true;
        }
        changed
    }
}

/// At start (never headless): reads the settings and starts the thread.
pub fn start(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    let c = Config::load(&paths::config_file()).unwrap_or_default();
    let l = &mut s.library;
    l.folders = c.library.folders.iter().map(|p| Folder::new(p)).collect();
    l.open_with = c.library.open_with.clone();
    l.last_player = c.library.last_player.clone();
    let (tx, rx) = mpsc::channel();
    l.tx = Some(tx);
    l.last_walk = Some(Instant::now());
    let folders = c.library.folders;
    let _ = std::thread::Builder::new().name("kotora-library".into()).spawn(move || run(&h, &rx, folders));
}

fn run(h: &Handle, rx: &Receiver<Cmd>, mut folders: Vec<String>) {
    let found = play::find();
    h.update_if(move |s, _| {
        let changed = s.library.found != found;
        s.library.found = found;
        changed
    });
    let mut store: Option<Store> = None;
    let mut walk = true;
    loop {
        if store.is_none() {
            store = Store::open(&paths::db_file()).ok();
        }
        if let Some(st) = store.as_mut() {
            let (shown, index) = pass(st, &folders, walk);
            h.update_if(move |s, _| {
                let changed = s.library.update(shown, index);
                if changed {
                    // The tray's Play next menu follows the files.
                    s.sync_tray();
                }
                changed
            });
        }
        // Several commands at once are one pass. No timeout: nothing runs at rest.
        let Ok(first) = rx.recv() else { return };
        walk = false;
        for c in std::iter::once(first).chain(rx.try_iter()) {
            match c {
                Cmd::Folders(f) => {
                    folders = f;
                    if let Some(st) = &store {
                        let _ = lib::keep_only(st, &folders);
                    }
                    walk = true;
                }
                Cmd::Rescan => walk = true,
                Cmd::Rematch => {}
            }
        }
    }
}

/// One pass: walk the folders (or only count the saved files), then match everything.
fn pass(st: &mut Store, folders: &[String], walk: bool) -> (Vec<Folder>, Index) {
    let mut shown = Vec::new();
    for f in folders {
        let r = if walk { lib::scan(st, f).ok() } else { None };
        let r = r.unwrap_or_else(|| lib::Scan { online: Path::new(f).is_dir(), files: lib::load_folder(st, f).map_or(0, |v| v.len()), ..Default::default() });
        shown.push(Folder { path: f.clone(), online: r.online, files: r.files, scanned: true });
    }
    let rows = lib::load_all(st).unwrap_or_default();
    let items = st.load_list(SERVICE).unwrap_or_default();
    let rel = relations::load(&paths::cache_dir());
    let cached = |key: &str| st.recognition(SERVICE, key).ok().flatten();
    (shown, Index::build(&rows, &items, &cached, &rel))
}

/// The list changed (a sync, an add) or a pick was made: match the files again.
pub fn list_changed(s: &State) {
    s.library.send(Cmd::Rematch);
}

/// Detection saw a player: remember it for "Open with: Automatic" (saved when it changes).
pub fn saw_player(s: &mut State, name: &str) {
    let Some(p) = Player::from_seen(name) else { return };
    if s.library.last_player == p.key() || s.handle.is_none() {
        return;
    }
    s.library.last_player = p.key().to_string();
    save(s, "last_player", config::quote(p.key()));
}

/// Writes `[library] key = value` off the UI thread.
fn save(s: &State, key: &'static str, value: String) {
    let Some(h) = s.handle.clone() else { return };
    let _ = std::thread::Builder::new().name("kotora-settings".into()).spawn(move || {
        if let Err(e) = config::write_setting(&paths::config_file(), "library", key, &value) {
            h.update_if(move |s, _| {
                s.library.error = Some(format!("Can't save the setting: {e}"));
                true
            });
        }
    });
}

fn save_folders(s: &mut State) {
    let list: Vec<String> = s.library.folders.iter().map(|f| f.path.clone()).collect();
    save(s, "folders", config::quote_list(&list));
    s.library.scanning = s.library.tx.is_some();
    s.library.send(Cmd::Folders(list));
}

/// A typed or picked path, tidied: `~` expanded, no trailing slash.
fn tidy(raw: &str) -> String {
    let t = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    let t = match (t.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ if t == "~" => std::env::var("HOME").unwrap_or_default(),
        _ => t.to_string(),
    };
    let trimmed = t.trim_end_matches('/');
    if trimmed.is_empty() && t.starts_with('/') { "/".into() } else { trimmed.to_string() }
}

/// Adds a folder. `check`: it must be there now (a folder typed in; a picked one is).
pub fn add_folder(s: &mut State, raw: &str) -> Result<(), String> {
    let path = tidy(raw);
    if path.is_empty() {
        return Err("Type a folder's path, or press Choose\u{2026}".into());
    }
    if !path.starts_with('/') {
        return Err("Give the whole path, from / (or ~/).".into());
    }
    if let Some(f) = s.library.folders.iter().find(|f| lib::overlaps(&f.path, &path)) {
        return Err(if f.path == path { "That folder is already in the list.".into() } else { format!("That overlaps {}, already in the list.", f.path) });
    }
    if !Path::new(&path).is_dir() {
        return Err(format!("No folder at {path}."));
    }
    s.library.folders.push(Folder::new(&path));
    s.library.draft.clear();
    s.library.error = None;
    save_folders(s);
    Ok(())
}

pub fn remove_folder(s: &mut State, path: &str) {
    s.library.folders.retain(|f| f.path != path);
    save_folders(s);
}

fn add_typed(s: &mut State) {
    let raw = s.library.draft.text.clone();
    if let Err(e) = add_folder(s, &raw) {
        s.library.error = Some(e);
    }
}

/// Choose…: the system's folder picker (macOS: an `NSOpenPanel` sheet; Linux: `zenity` or
/// `kdialog` on a thread).
fn choose(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
    // macOS: AppKit's own panel, in this process, so it follows Kotora's theme (`chrome.rs`).
    #[cfg(target_os = "macos")]
    {
        let r = crate::chrome::pick_folder(PROMPT, move |picked| {
            let Some(p) = picked else { return };
            h.update_if(move |s, _| {
                if let Err(e) = add_folder(s, &p) {
                    s.library.error = Some(e);
                }
                true
            });
        });
        if let Err(e) = r {
            s.library.error = Some(e);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = std::thread::Builder::new().name("kotora-choose".into()).spawn(move || {
        let picked = pick_folder();
        h.update_if(move |s, _| {
            match picked {
                Ok(Some(p)) => {
                    if let Err(e) = add_folder(s, &p) {
                        s.library.error = Some(e);
                    }
                }
                Ok(None) => return false,
                Err(e) => s.library.error = Some(e),
            }
            true
        });
    });
}

#[cfg(target_os = "macos")]
const PROMPT: &str = "Choose a folder with anime in it";

#[cfg(not(target_os = "macos"))]
fn pick_folder() -> Result<Option<String>, String> {
    let tries: &[(&str, &[&str])] = &[("zenity", &["--file-selection", "--directory", "--title=Choose a folder with anime in it"]), ("kdialog", &["--getexistingdirectory", "--title", "Choose a folder with anime in it"])];
    for (prog, args) in tries {
        match std::process::Command::new(prog).args(*args).stdin(std::process::Stdio::null()).output() {
            Ok(out) if out.status.success() => {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return Ok((!p.is_empty()).then_some(p));
            }
            // Cancelled.
            Ok(_) => return Ok(None),
            Err(_) => continue,
        }
    }
    Err("No folder picker here (zenity or kdialog): type the folder's path and press Add.".into())
}

fn rescan(s: &mut State) {
    if s.library.tx.is_some() {
        s.library.scanning = true;
        s.library.last_walk = Some(Instant::now());
        s.library.send(Cmd::Rescan);
    }
}

/// A quiet rescan on demand (the tray menu opened, the window shown), unless one ran less than
/// [`REFRESH_AFTER`] ago or there are no folders. The walk is incremental: unchanged files cost a
/// stat each.
pub fn refresh(s: &mut State) {
    let l = &mut s.library;
    if l.tx.is_none() || l.folders.is_empty() || l.last_walk.is_some_and(|t| t.elapsed() < REFRESH_AFTER) {
        return;
    }
    l.last_walk = Some(Instant::now());
    l.send(Cmd::Rescan);
}

fn set_open_with(s: &mut State, key: &'static str) {
    s.library.open_with = key.to_string();
    save(s, "open_with", config::quote(key));
}

// ------------------------------------------------------------------------------------- playing

/// What Play would do for an entry; `None` when there are no folders (no button at all).
pub fn next_for(s: &State, e: &Entry) -> Option<Next> {
    if s.library.folders.is_empty() {
        return None;
    }
    Some(s.library.index.next(e.id, e.progress, e.episodes, &|f| s.library.online(f)))
}

/// The tray's Play next menu: the Watching (or rewatching) anime whose next episode is a file in
/// an online folder, most recently updated first, at most [`PLAY_MENU_MAX`]. Each is (media id,
/// "Title · Ep N"). Empty without folders.
pub fn play_menu(s: &State) -> Vec<(u32, String)> {
    if s.library.folders.is_empty() {
        return Vec::new();
    }
    let mut watching: Vec<&Entry> = s.entries.iter().filter(|e| e.status == Status::Watching || e.rewatching).collect();
    watching.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    watching
        .into_iter()
        .filter_map(|e| match next_for(s, e) {
            Some(Next::Ready { episode, .. }) => Some((e.id, format!("{} \u{b7} Ep {episode}", e.title))),
            _ => None,
        })
        .take(PLAY_MENU_MAX)
        .collect()
}

/// Opens the next episode's file in the player. `Ok`: what it does, in a sentence.
pub fn play_next(s: &mut State, id: u32) -> Result<String, String> {
    let e = s.entries.iter().find(|e| e.id == id).ok_or("That anime isn't on your list.")?;
    let next = next_for(s, e).ok_or("No library folders yet: add one in Settings \u{2192} Library.")?;
    let Next::Ready { episode, path } = next else { return Err(next.reason().unwrap_or_default()) };
    let player = s.library.player();
    let found = s.library.found.clone();
    let note = format!("Opening episode {episode} of {} in {}", e.title, if player == Player::System { "the default player" } else { player.label() });
    s.library.note = Some(note.clone());
    // The file is looked at off the UI thread: a sleeping drive may take seconds.
    let h = s.handle.clone();
    let _ = std::thread::Builder::new().name("kotora-play".into()).spawn(move || {
        let r = if Path::new(&path).is_file() || rune::is_headless() {
            play::open(player, &path, &found)
        } else {
            Err(format!("Episode {episode} is gone from {}: looking again.", lib::folder_name(Path::new(&path).parent().map_or("", |p| p.to_str().unwrap_or("")))))
        };
        if let (Err(m), Some(h)) = (r, h) {
            h.update_if(move |s, _| {
                s.library.note = None;
                s.error = Some(m);
                rescan(s);
                true
            });
        }
    });
    Ok(note)
}

/// A short note at the window's bottom that goes by itself ([`TOAST_SECS`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub text: String,
    /// The note is an error ("Episode 7 not in your folders").
    pub warn: bool,
    pub until: std::time::Instant,
    /// Undo: the entry and the change that puts it back (`State::toast_undo`).
    pub undo: Option<(u32, kotora_core::Change)>,
}

/// How long a toast stays.
pub const TOAST_SECS: f64 = 3.5;

impl State {
    /// Middle-click, P and Cmd/Ctrl+Return: plays the next episode of `id` as ▶ Play does, and
    /// says so in a toast, or says why not ("Episode 7 not in your folders").
    pub fn play_entry(&mut self, id: u32) {
        let (text, warn) = match play_next(self, id) {
            Ok(note) => (note, false),
            Err(m) => (m, true),
        };
        self.toast = Some(Toast { text, warn, until: std::time::Instant::now() + std::time::Duration::from_secs_f64(TOAST_SECS), undo: None });
    }

    /// The entry P plays: the grid's selection, the list row whose editor is open, or the
    /// open details page's anime when it is on the list.
    pub fn play_target(&self) -> Option<u32> {
        let on_list = |id: u32| self.entries.iter().any(|e| e.id == id).then_some(id);
        match self.page {
            Page::List(st) if self.view_of(st) != kotora_core::config::ListView::List => {
                self.grid.sel.filter(|id| self.rows.iter().any(|&k| self.entries[k].id == *id))
            }
            Page::List(_) => self.editor.map(|ed| ed.id).and_then(on_list),
            Page::Details => self.detail.as_ref().map(|d| d.id()).and_then(on_list),
            _ => None,
        }
    }
}

/// `poll`: the toast goes when its time is up (one wake, nothing ticks meanwhile).
pub fn toast_tick(s: &mut State, tx: &mut Tx) -> bool {
    let Some(t) = &s.toast else { return false };
    let now = std::time::Instant::now();
    if now >= t.until {
        s.toast = None;
        return true;
    }
    tx.wake_after(t.until - now);
    false
}

/// The toast, bottom right over the page.
pub fn toast(t: &Toast) -> El {
    let c = if t.warn { PAL.warn() } else { PAL.ok() };
    row()
        .key("toast")
        .role("status")
        .label(t.text.clone())
        .pin(None, None, Some(24.0), Some(24.0))
        .max_w(460.0)
        .px(14.0)
        .py(10.0)
        .gap(10.0)
        .center_y()
        .radius(theme::R_LG)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(if PAL.light { 0.14 } else { 0.4 }))
        .child(el().size(8.0, 8.0).radius(4.0).fill(c))
        // The text gives way to the button: it wraps (3 lines at most), and a word too long for the
        // toast is cut at the toast's edge instead of running under Undo.
        .child(row().shrink().min_w(0.0).clip_rounded(0.0).child(text(t.text.clone()).font(theme::BODY).color(PAL.text).wrap().max_lines(3).shrink().min_w(0.0)))
        .children(t.undo.is_some().then(|| crate::ui::button("toast/undo", "Undo", false, Some(State::toast_undo)).h(26.0).px(10.0)))
}

pub(crate) fn press_play(s: &mut State, id: u32) {
    if let Err(m) = play_next(s, id) {
        s.error = Some(m);
    }
}

/// The list row's ▶, before `+1`: only when the next episode's file is there. `None` without
/// folders, so rows look as before.
pub fn row_button(s: &State, e: &Entry) -> Option<El> {
    let next = next_for(s, e)?;
    if !(e.status == Status::Watching || e.rewatching) {
        return Some(el().size(26.0, 26.0));
    }
    let Next::Ready { episode, .. } = next else {
        return Some(el().size(26.0, 26.0));
    };
    let id = e.id;
    Some(
        el().key("play")
            .role("button")
            .label(format!("Play episode {episode}"))
            .focusable(true)
            .size(26.0, 26.0)
            .radius(13.0)
            .center()
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                press_play(s, id);
            })
            .child(icon(&icons::PLAY, 18.0).color(PAL.accent_hi)),
    )
}

/// The Play button with its reason when it can't (details page, Up next).
fn play_button(key: String, next: &Next, id: u32) -> El {
    match next {
        Next::Ready { episode, .. } => {
            let label = format!("Play episode {episode}");
            row()
                .key(key)
                .role("button")
                .label(label.clone())
                .focusable(true)
                .h(32.0)
                .px(12.0)
                .gap(6.0)
                .center_y()
                .radius(theme::R_SM)
                .fill(PAL.accent)
                .press(PAL.press())
                .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    press_play(s, id);
                })
                .child(icon(&icons::PLAY, 15.0).color(PAL.on_accent))
                .child(text(label).font(theme::LABEL_BTN).color(PAL.on_accent))
        }
        other => button(&key, &other.reason().unwrap_or_default(), false, None),
    }
}

/// The details page: Play next, and which episodes are in the folders. `None` without folders.
pub fn details_block(s: &State, id: u32) -> Option<El> {
    if s.library.folders.is_empty() {
        return None;
    }
    let eps = s.library.index.episodes(id);
    let have = if eps.is_empty() { "No episodes in your folders.".to_string() } else { format!("Episodes {} in your folders", lib::ranges(&eps)) };
    let mut c = card("det-library", "On your disk").child(text(have).key("det/local-episodes").font(theme::LABEL).color(PAL.dim).wrap().pt(4.0));
    if let Some(e) = s.entries.iter().find(|e| e.id == id)
        && let Some(next) = next_for(s, e)
        && next != Next::Done
    {
        c = c.child(row().pt(10.0).child(play_button("det/play".into(), &next, id)));
        if let Some(n) = s.library.note.as_ref().filter(|n| n.contains(&e.title)) {
            c = c.child(text(n.clone()).font(theme::LABEL).color(PAL.faint).pt(6.0));
        }
    }
    Some(c)
}

/// Now Playing while nothing plays: the Watching entries whose next episode is in the folders
/// (the most recently updated first, three at most). `None` when there are none.
pub fn up_next_card(s: &State) -> Option<El> {
    let mut ready: Vec<(&Entry, Next)> = s
        .entries
        .iter()
        .filter(|e| e.status == Status::Watching || e.rewatching)
        .filter_map(|e| next_for(s, e).filter(|n| matches!(n, Next::Ready { .. })).map(|n| (e, n)))
        .collect();
    if ready.is_empty() {
        return None;
    }
    ready.sort_by_key(|(e, _)| std::cmp::Reverse(e.updated_at));
    let mut c = card("np-up-next", "Up next");
    for (e, n) in ready.into_iter().take(3) {
        c = c.child(
            row()
                .key(format!("np/up-next/{}", e.id))
                .gap(12.0)
                .py(6.0)
                .center_y()
                .child(text(e.title.clone()).font(theme::BODY_MED).color(PAL.text).truncate().grow(1.0).min_w(0.0))
                .child(play_button(format!("np/play/{}", e.id), &n, e.id)),
        );
    }
    if let Some(n) = &s.library.note {
        c = c.child(text(n.clone()).font(theme::LABEL).color(PAL.faint).pt(4.0));
    }
    let who = match s.library.player() {
        Player::System => "your default player".to_string(),
        p => p.label().to_string(),
    };
    Some(c.child(text(format!("Opens in {who} (Settings \u{2192} Library).")).font(theme::LABEL).color(PAL.faint).pt(4.0)))
}

// ------------------------------------------------------------------------------------- settings

/// Settings → Library → Folders: what the folders are for.
pub const FOLDERS_NOTE: &str = "Folders where your anime are. Kotora finds the episodes in them (samples, extras and creditless openings left out) and links them to your list, so Play opens the next one.";

/// Settings → Library → Folders: the folders, the field to add one, and the summary.
pub fn folders_block(s: &State) -> El {
    let l = &s.library;
    let mut c = col().key("library-card").w_full();
    for f in &l.folders {
        let (dot, state) = match (f.scanned, f.online) {
            (false, _) => (PAL.faint, "Looking\u{2026}".to_string()),
            (true, true) => (PAL.ok(), format!("{} episode file{}", f.files, if f.files == 1 { "" } else { "s" })),
            (true, false) => (PAL.warn(), format!("Offline (drive not connected?) \u{b7} {} file{} remembered", f.files, if f.files == 1 { "" } else { "s" })),
        };
        let path = f.path.clone();
        c = c.child(
            row()
                .key(format!("set/library/{}", f.path))
                .gap(12.0)
                .py(6.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(f.path.clone()).font(theme::BODY_MED).color(PAL.text).truncate())
                        .child(text(state).font(theme::LABEL).color(PAL.dim)),
                )
                .child(ghost(&format!("set/library/remove/{}", f.path), "Remove", move |s| remove_folder(s, &path))),
        );
    }
    let input = row()
        .key("set/library/box")
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, if l.error.is_some() { PAL.warn() } else { PAL.border_strong })
        .child(
            text_input(&l.draft, TextStyle::new(12.0, 400.0).mono())
                .key("set/library/input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&l.draft, if cfg!(target_os = "macos") { "/Volumes/Disk/Anime" } else { "~/Videos/Anime" }), PAL.faint)
                .on_edit(|s: &mut State| &mut s.library.draft)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.library.error = None)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    add_typed(s);
                }),
        );
    c = c.child(
        row()
            .key("set/library/add-row")
            .gap(8.0)
            .pt(8.0)
            .center_y()
            .child(input)
            .child(button("set/library/add", "Add", false, Some(add_typed)))
            .child(button("set/library/choose", "Choose\u{2026}", false, Some(choose))),
    );
    if let Some(e) = &l.error {
        c = c.child(text(e.clone()).key("set/library/error").font(theme::LABEL).color(PAL.warn()).wrap().pt(4.0));
    }
    if !l.folders.is_empty() {
        let anime = l.index.by_media.len();
        let files: usize = l.index.by_media.values().flat_map(|m| m.values()).map(Vec::len).sum();
        let mut line = format!("{files} file{} of {anime} anime on your list", if files == 1 { "" } else { "s" });
        if l.index.unmatched > 0 {
            line.push_str(&format!(" \u{b7} {} not matched (not on your list, or unsure)", l.index.unmatched));
        }
        c = c.child(
            row()
                .key("set/library/summary")
                .gap(12.0)
                .pt(8.0)
                .center_y()
                .child(text(line).font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0))
                .child(if l.scanning { button("set/library/rescan", "Looking\u{2026}", false, None) } else { button("set/library/rescan", "Rescan", false, Some(rescan)) }),
        );
    }
    c
}

/// Settings → Library → Open with: Automatic, then each installed player.
pub fn open_with_choice(s: &State) -> El {
    let l = &s.library;
    let mut choice = row().key("set/open-with").flex_wrap().gap(6.0);
    let last = Player::from_key(&l.last_player).filter(|p| *p != Player::System && l.installed().contains(p));
    let auto_label = match last {
        Some(p) => format!("Automatic ({})", p.label()),
        None => "Automatic".to_string(),
    };
    let on_auto = Player::from_key(&l.open_with).is_none_or(|p| !l.installed().contains(&p));
    choice = choice.child(cell("set/open-with/auto".into(), auto_label.clone(), on_auto, |s| set_open_with(s, "auto")).child(text(auto_label).font(theme::LABEL).color(PAL.text)));
    for p in l.installed() {
        let on = !on_auto && l.open_with == p.key();
        choice = choice.child(cell(format!("set/open-with/{}", p.key()), format!("Open with {}", p.label()), on, move |s| set_open_with(s, p.key())).child(text(p.label()).font(theme::LABEL).color(PAL.text)));
    }
    choice
}

/// Settings → Library → Open with: what Automatic means.
pub const OPEN_WITH_NOTE: &str = "Automatic: the player Kotora last saw playing, else your system's default. Only installed players show.";

// ------------------------------------------------------------------------------------- scenes

/// Fake folders and files for the scenes: the first fake entries have their next episode, one is
/// missing, one is on an unplugged drive.
pub fn fake(s: &mut State) {
    use kotora_core::library::Local;
    let l = &mut s.library;
    l.folders = vec![
        Folder { path: "/Volumes/Media/Anime".into(), online: true, files: 148, scanned: true },
        Folder { path: "/Volumes/Archive/Anime".into(), online: false, files: 312, scanned: true },
    ];
    l.found = play::Found { mpv_bin: Some("/opt/homebrew/bin/mpv".into()), iina: true, vlc_app: true, ..Default::default() };
    l.last_player = "iina".into();
    l.open_with = "auto".into();
    l.index = Index::default();
    l.index.unmatched = 9;
    let local = |folder: &str, ep: u32| Local { path: format!("{folder}/Show/[Hoshi] Show - {ep:02} [1080p].mkv"), folder: folder.into(), group: Some("Hoshi".into()), mtime: 0 };
    let on = "/Volumes/Media/Anime";
    let off = "/Volumes/Archive/Anime";
    // id: (episodes in the folders, folder)
    for (id, eps, folder) in [(0u32, 1..=6, on), (1, 1..=9, on), (3, 1..=4, on), (4, 20..=26, off), (6, 1..=3, on), (8, 1..=2, on), (10, 1..=12, on)] {
        let m = l.index.by_media.entry(id).or_default();
        for ep in eps {
            m.entry(ep).or_default().push(local(folder, ep));
        }
    }
    // "Ghost of the Library Stacks" (11 of 12): episodes 1-10 only, so 12 is missing.
    let m = l.index.by_media.entry(7).or_default();
    for ep in 1..=10 {
        m.entry(ep).or_default().push(local(on, ep));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_play_menu_lists_ready_watching_anime_newest_first() {
        let mut s = State::fake();
        assert!(play_menu(&s).is_empty(), "no folders: no menu");
        fake(&mut s);
        let menu = play_menu(&s);
        assert!(!menu.is_empty() && menu.len() <= PLAY_MENU_MAX);
        let mut last = i64::MAX;
        for (id, label) in &menu {
            let e = s.entries.iter().find(|e| e.id == *id).expect("on the list");
            assert!(e.status == Status::Watching || e.rewatching, "{label}");
            assert!(matches!(next_for(&s, e), Some(Next::Ready { .. })), "{label}");
            assert_eq!(*label, format!("{} \u{b7} Ep {}", e.title, e.progress + 1));
            assert!(e.updated_at <= last, "newest first");
            last = e.updated_at;
        }
        // Watching with a file missing or offline: not in the menu.
        for e in s.entries.iter().filter(|e| e.status == Status::Watching) {
            if !matches!(next_for(&s, e), Some(Next::Ready { .. })) {
                assert!(menu.iter().all(|(id, _)| *id != e.id), "{}", e.title);
            }
        }
    }

    #[test]
    fn refresh_is_throttled_and_needs_folders() {
        let mut s = State::fake();
        let (tx, rx) = mpsc::channel();
        s.library.tx = Some(tx);
        refresh(&mut s);
        assert!(rx.try_recv().is_err(), "no folders: nothing to walk");
        fake(&mut s);
        refresh(&mut s);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Rescan)));
        refresh(&mut s);
        assert!(rx.try_recv().is_err(), "a second ask right after is skipped");
        s.library.last_walk = Instant::now().checked_sub(REFRESH_AFTER + Duration::from_secs(1));
        refresh(&mut s);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Rescan)));
    }

    #[test]
    fn typed_paths_are_tidied() {
        assert_eq!(tidy("  /m/anime/ "), "/m/anime");
        assert_eq!(tidy("\"/m/my anime\""), "/m/my anime");
        assert_eq!(tidy("/"), "/");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(tidy("~/Anime"), format!("{home}/Anime"));
        }
    }

    #[test]
    fn adding_checks_the_folder() {
        let mut s = State::fake();
        assert!(add_folder(&mut s, "relative/path").is_err());
        assert!(add_folder(&mut s, "/nonexistent/kotora-anime").unwrap_err().starts_with("No folder at"));
        let d = std::env::temp_dir().join(format!("kot-addlib-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Show")).unwrap();
        let p = d.to_string_lossy().into_owned();
        add_folder(&mut s, &p).unwrap();
        assert_eq!(s.library.folders.len(), 1);
        assert_eq!(add_folder(&mut s, &p).unwrap_err(), "That folder is already in the list.");
        assert!(add_folder(&mut s, &format!("{p}/Show")).unwrap_err().starts_with("That overlaps"));
        remove_folder(&mut s, &p);
        assert!(s.library.folders.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn play_says_why_it_cant() {
        let mut s = State::fake();
        assert!(next_for(&s, &s.entries[0]).is_none(), "no folders, no button");
        fake(&mut s);
        let e = |s: &State, id: u32| s.entries.iter().find(|e| e.id == id).unwrap().clone();
        // Lanterns: progress 5, episode 6 is there.
        assert!(matches!(next_for(&s, &e(&s, 0)), Some(Next::Ready { episode: 6, .. })));
        // The last player seen was IINA, which is macOS only (elsewhere Automatic falls back to the system's).
        let said = play_next(&mut s, 0).unwrap();
        assert!(said.contains("episode 6 of Lanterns of Kirigaoka in "), "{said}");
        assert_eq!(said.ends_with("in IINA"), cfg!(target_os = "macos"), "{said}");
        // Neon Ronin: progress 22, on the unplugged drive.
        assert_eq!(play_next(&mut s, 4).unwrap_err(), "Episode 23 is on Archive, which is offline");
        // Ghost of the Library Stacks: progress 11, files up to 10.
        assert_eq!(play_next(&mut s, 7).unwrap_err(), "Episode 12 not in your folders");
        // The pick wins over Automatic.
        s.library.open_with = "vlc".into();
        assert!(play_next(&mut s, 1).unwrap().ends_with("in VLC"));
    }

    #[test]
    fn a_scan_answer_redraws_only_on_change() {
        let mut s = State::fake();
        s.library.folders = vec![Folder::new("/m/a")];
        let done = vec![Folder { path: "/m/a".into(), online: true, files: 3, scanned: true }];
        assert!(s.library.update(done.clone(), Index::default()));
        assert!(!s.library.update(done, Index::default()), "the same again: no redraw");
        // A folder removed while the scan ran stays removed.
        s.library.folders.clear();
        assert!(!s.library.update(vec![Folder::new("/m/a")], Index::default()));
        assert!(s.library.folders.is_empty());
    }
}
