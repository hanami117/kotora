//! Library folders and "Play next episode" (`kotora_core::library` does the scanning and matching).
//!
//! One thread, `kotora-library`, owns its own database connection. It scans every folder at start,
//! when the folders change, on Rescan, and every [`RESCAN_EVERY`] (a walk that stats the files and
//! parses only new or changed ones), and matches the files to the list again when the list
//! changes (a sync) or a "Wrong anime" pick is made. Its results reach the UI with `update_if`,
//! which redraws only when something visible changed. Nothing runs headless: scenes fill
//! [`Library`] with fake data ([`fake`]).
//!
//! The UI parts are here too, to keep the shared pages' files small: the Settings card, the list
//! row's Play button, the details page's block and Now Playing's "Up next" card.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use kotora_core::config::{self, Config};
use kotora_core::library::{self as lib, Index, Next};
use kotora_core::recognize::relations;
use kotora_core::store::Store;
use kotora_core::{Status, paths};
use rune::{El, TextEdit, TextStyle, Tx, col, el, icon, row, text, text_input};

use crate::icons;
use crate::model::{Entry, Handle, State};
use crate::play::{self, Player};
use crate::theme::{self, PAL};
use crate::ui::{button, card, cell, ghost, option};

/// How often the folders are walked again for changes.
pub const RESCAN_EVERY: Duration = Duration::from_secs(5 * 60);

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
            h.update_if(move |s, _| s.library.update(shown, index));
        }
        // Several commands at once are one pass.
        let first = match rx.recv_timeout(RESCAN_EVERY) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        walk = first.is_none();
        for c in first.into_iter().chain(rx.try_iter()) {
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

/// Choose…: the system's folder picker, on a thread (`osascript` on macOS, `zenity` or `kdialog`
/// on Linux).
fn choose(s: &mut State) {
    let Some(h) = s.handle.clone() else { return };
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

fn pick_folder() -> Result<Option<String>, String> {
    let tries: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("osascript", &["-e", "POSIX path of (choose folder with prompt \"Choose a folder with anime in it\")"])]
    } else {
        &[("zenity", &["--file-selection", "--directory", "--title=Choose a folder with anime in it"]), ("kdialog", &["--getexistingdirectory", "--title", "Choose a folder with anime in it"])]
    };
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
        s.library.send(Cmd::Rescan);
    }
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

/// Settings → Library.
pub fn settings_card(s: &State) -> El {
    let l = &s.library;
    let mut c = card("library-card", "Library").child(
        text("Folders where your anime are. Kotora finds the episodes in them (samples, extras and creditless openings left out) and links them to your list, so Play opens the next one.")
            .font(theme::LABEL)
            .color(PAL.dim)
            .wrap(),
    );
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
                .placeholder(if cfg!(target_os = "macos") { "/Volumes/Disk/Anime" } else { "~/Videos/Anime" }, PAL.faint)
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
    c.child(option("Open with", "Automatic: the player Kotora last saw playing, else your system's default. Only installed players show.".into(), el()))
        .child(choice)
}

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
        assert!(play_next(&mut s, 0).unwrap().contains("episode 6 of Lanterns of Kirigaoka in IINA"));
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
