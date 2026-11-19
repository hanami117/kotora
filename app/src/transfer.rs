//! Settings → Account → Your list: export the list as a MyAnimeList XML file, and import one
//! (core's `listxml.rs` reads, writes and compares; this is the UI and its threads).
//!
//! - **Export** (MyAnimeList lists only: AniList's terms don't allow it, see `listxml.rs`): a save
//!   dialog, then a thread writes the file from the saved list. `kot export FILE` does the same
//!   through [`export_to`].
//! - **Import:** an open dialog, then a thread reads the file and compares it with the saved list.
//!   With AniList active it first looks the file's MyAnimeList ids up on AniList (50 to a request,
//!   through the app's AniList rate budget; ids already on the list need no request). The
//!   preview counts new, different, same and unknown entries, and asks about status conflicts.
//!   Applying hands the adds and updates to the queue worker as one command: one transaction,
//!   sent at the service's pace like any edit, and "Undo import" takes back what still waits.
//!
//! Nothing here runs headless: the dialogs and threads need the app handle; scenes set the state.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use kotora_core::listxml::{self, Mode, Plan, PlanRow, Verdict};
use kotora_core::queue::describe_in;
use kotora_core::service::ServiceId;
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::{Kind, Status, paths};
use rune::{El, col, el, row, text};

use crate::model::{Page, State};
use crate::theme::{self, PAL};
use crate::ui::{cell, small_button};

/// The import panel and the export note.
#[derive(Default)]
pub struct Transfer {
    /// The last export: what was written, or why not.
    pub export_note: Option<Result<String, String>>,
    pub exporting: bool,
    /// The same for the manga list.
    pub manga_export_note: Option<Result<String, String>>,
    pub manga_exporting: bool,
    /// The kind of list the import panel (busy, error, preview, done) is about.
    pub import_kind: Kind,
    /// Reading or matching a file: what is going on.
    pub busy: Option<String>,
    /// The file couldn't be read or matched.
    pub error: Option<String>,
    pub preview: Option<Preview>,
    /// Sent to the queue worker; waiting for its answer.
    pub applying: bool,
    pub done: Option<Done>,
}

/// A file compared with the list, waiting for the user's pick.
pub struct Preview {
    /// The file's name (no folders: it shows in the panel).
    pub file: String,
    pub plan: Plan,
    /// Status conflicts settled for the file's status (by target id); the rest keep the list's.
    pub use_file: HashSet<u32>,
}

/// An import handed to the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub queued: usize,
    /// The queued changes (for Undo).
    pub ids: Vec<i64>,
    /// After Undo: how many were taken back.
    pub undone: Option<usize>,
}

// ---------------------------------------------------------------------------------------- export

/// The note under Export when it can't be used.
pub fn refusal(s: &State) -> Option<&'static str> {
    listxml::export_refusal(s.service)
}

/// Writes the saved list of the active service to `path` (also `kot export`). Off the UI thread.
pub fn export_to(path: &Path, kind: Kind) -> Result<String, String> {
    let service = crate::account::active();
    if let Some(why) = listxml::export_refusal(service) {
        return Err(why.into());
    }
    let store =
        Store::open(&paths::db_file()).map_err(|e| format!("Can't open the saved list: {e}"))?;
    let ex = listxml::export_kind(&store, service, kind)?;
    if ex.written == 0 {
        return Err(if kind == Kind::Manga {
            "Your saved manga list is empty: turn manga on, sync, and try again.".into()
        } else {
            "Your saved list is empty: sign in and sync first.".into()
        });
    }
    write_file(path, ex.xml.as_bytes())?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut msg = format!(
        "Exported {} to {name}.",
        if kind == Kind::Manga {
            plural(ex.written, "manga", "manga")
        } else {
            plural(ex.written, "anime", "anime")
        }
    );
    if !ex.skipped.is_empty() {
        msg.push_str(&format!(
            " Left out (no MyAnimeList id): {}.",
            ex.skipped.join(", ")
        ));
    }
    Ok(msg)
}

/// Writes next to the target, then renames: a failed write never leaves half a file.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !dir.is_dir() {
        return Err(format!("There is no folder {}.", dir.display()));
    }
    let tmp = dir.join(format!(".kotora-export-{}.tmp", std::process::id()));
    let r = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| format!("Can't write {}: {e}", path.display()))
}

pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The export state of a kind: the note and whether it is writing.
fn export_state(t: &mut Transfer, kind: Kind) -> (&mut Option<Result<String, String>>, &mut bool) {
    if kind == Kind::Manga {
        (&mut t.manga_export_note, &mut t.manga_exporting)
    } else {
        (&mut t.export_note, &mut t.exporting)
    }
}

fn export_anime(s: &mut State) {
    export(s, Kind::Anime);
}

fn export_manga(s: &mut State) {
    export(s, Kind::Manga);
}

/// Export…: the save dialog, then a thread writes the file.
pub fn export(s: &mut State, kind: Kind) {
    if refusal(s).is_some() || *export_state(&mut s.transfer, kind).1 {
        return;
    }
    let Some(h) = s.handle.clone() else { return };
    let name = listxml::default_file_name_of(kind, now_unix());
    let on_path = move |picked: Option<String>| {
        let Some(path) = picked else { return };
        h.update_if(move |s, _| {
            let (note, busy) = export_state(&mut s.transfer, kind);
            *busy = true;
            *note = None;
            true
        });
        let h2 = h.clone();
        let _ = std::thread::Builder::new()
            .name("kotora-export".into())
            .spawn(move || {
                let r = export_to(Path::new(&path), kind);
                h2.update_if(move |s, _| {
                    let (note, busy) = export_state(&mut s.transfer, kind);
                    *busy = false;
                    *note = Some(r);
                    true
                });
            });
    };
    let prompt = if kind == Kind::Manga {
        EXPORT_MANGA_PROMPT
    } else {
        EXPORT_PROMPT
    };
    if let Err(e) = pick(prompt, Some(&name), on_path) {
        *export_state(&mut s.transfer, kind).0 = Some(Err(e));
    }
}

const EXPORT_PROMPT: &str = "Export your anime list as a MyAnimeList XML file";
const EXPORT_MANGA_PROMPT: &str = "Export your manga list as a MyAnimeList XML file";
const IMPORT_PROMPT: &str =
    "Import anime list: choose a MyAnimeList anime list export (.xml or .xml.gz)";
const IMPORT_MANGA_PROMPT: &str =
    "Import manga list: choose a MyAnimeList manga list export (.xml or .xml.gz)";

/// The system's file dialog; `done` gets the path (on the UI thread on macOS, a thread on Linux
/// and Windows).
fn pick(
    prompt: &str,
    save: Option<&str>,
    done: impl Fn(Option<String>) + Send + 'static,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::chrome::pick_file(prompt, save, done)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let prompt = prompt.to_string();
        let save = save.map(str::to_string);
        std::thread::Builder::new()
            .name("kotora-pick".into())
            .spawn(move || match pick_linux(&prompt, save.as_deref()) {
                Ok(p) => done(p),
                Err(e) => eprintln!("kotora: {e}"),
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Windows: its own Save and Open dialogs (`win::pick_file`).
#[cfg(windows)]
fn pick_linux(prompt: &str, save: Option<&str>) -> Result<Option<String>, String> {
    crate::win::pick_file(prompt, save)
}

/// `zenity` or `kdialog`, as the library folder picker.
#[cfg(not(any(target_os = "macos", windows)))]
fn pick_linux(prompt: &str, save: Option<&str>) -> Result<Option<String>, String> {
    let title = format!("--title={prompt}");
    let tries: Vec<(&str, Vec<String>)> = match save {
        Some(name) => vec![
            (
                "zenity",
                vec![
                    "--file-selection".into(),
                    "--save".into(),
                    "--confirm-overwrite".into(),
                    format!("--filename={name}"),
                    title.clone(),
                ],
            ),
            (
                "kdialog",
                vec![
                    "--getsavefilename".into(),
                    name.into(),
                    "*.xml|XML files".into(),
                    "--title".into(),
                    prompt.into(),
                ],
            ),
        ],
        None => vec![
            (
                "zenity",
                vec![
                    "--file-selection".into(),
                    title.clone(),
                    "--file-filter=MyAnimeList export | *.xml *.gz".into(),
                    "--file-filter=All files | *".into(),
                ],
            ),
            (
                "kdialog",
                vec![
                    "--getopenfilename".into(),
                    ".".into(),
                    "*.xml *.gz|MyAnimeList export".into(),
                    "--title".into(),
                    prompt.into(),
                ],
            ),
        ],
    };
    for (prog, args) in tries {
        match std::process::Command::new(prog)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(out) if out.status.success() => {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return Ok((!p.is_empty()).then_some(p));
            }
            Ok(_) => return Ok(None),
            Err(_) => continue,
        }
    }
    Err("No file dialog here (zenity or kdialog). Use kot export FILE to export.".into())
}

// ---------------------------------------------------------------------------------------- import

fn import_anime(s: &mut State) {
    import(s, Kind::Anime);
}

fn import_manga(s: &mut State) {
    import(s, Kind::Manga);
}

/// Import…: the open dialog, then a thread reads and compares the file.
pub fn import(s: &mut State, kind: Kind) {
    if s.transfer.busy.is_some() || s.transfer.applying {
        return;
    }
    s.transfer.import_kind = kind;
    let Some(h) = s.handle.clone() else { return };
    let on_path = move |picked: Option<String>| {
        let Some(path) = picked else { return };
        h.update_if(move |s, _| {
            let t = &mut s.transfer;
            s.transfer = Transfer {
                export_note: t.export_note.take(),
                manga_export_note: t.manga_export_note.take(),
                exporting: t.exporting,
                manga_exporting: t.manga_exporting,
                import_kind: kind,
                busy: Some("Reading the file\u{2026}".into()),
                ..Transfer::default()
            };
            true
        });
        let h2 = h.clone();
        let _ = std::thread::Builder::new()
            .name("kotora-import".into())
            .spawn(move || {
                let r = read_and_plan(&path, kind, &h2);
                h2.update_if(move |s, tx| {
                    s.transfer.busy = None;
                    match r {
                        Ok(p) => {
                            s.transfer.preview = Some(p);
                            tx.scroll_into_view("set/import-preview");
                        }
                        Err(e) => s.transfer.error = Some(e),
                    }
                    true
                });
            });
    };
    let prompt = if kind == Kind::Manga {
        IMPORT_MANGA_PROMPT
    } else {
        IMPORT_PROMPT
    };
    if let Err(e) = pick(prompt, None, on_path) {
        s.transfer.error = Some(e);
    }
}

/// Reads `path` and compares it with the saved list of the active service (a thread).
fn read_and_plan(path: &str, kind: Kind, h: &crate::model::Handle) -> Result<Preview, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read the file: {e}"))?;
    let file = listxml::parse_kind(&bytes, kind)?;
    let service = crate::account::active();
    let store =
        Store::open(&paths::db_file()).map_err(|e| format!("Can't open the saved list: {e}"))?;
    let list = store
        .load_list((service.name(), kind))
        .map_err(|e| format!("Can't read the saved list: {e}"))?;
    let mut mapped = HashMap::new();
    if service != ServiceId::Mal {
        let ids = Plan::ids_to_map(&file, &list, service.name());
        if !ids.is_empty() {
            let n = ids.len();
            h.update_if(move |s, _| {
                s.transfer.busy = Some(format!(
                    "Looking up {} on {}\u{2026}",
                    plural(n, "MyAnimeList id", "MyAnimeList ids"),
                    service.title()
                ));
                true
            });
            let mut svc = crate::account::open_for(service).ok_or_else(|| {
                format!(
                    "{} is needed to match MyAnimeList ids: sign in, and check you are online.",
                    service.title()
                )
            })?;
            let media = svc
                .media_by_mal_ids_of(kind, &ids)
                .map_err(|e| format!("Can't look the ids up on {}: {e}", service.title()))?;
            mapped = media
                .into_iter()
                .filter_map(|m| Some((m.id_mal?, m)))
                .collect();
        }
    }
    let plan = listxml::plan(&file, service.name(), &list, &mapped);
    let name = Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    Ok(Preview {
        file: name,
        plan,
        use_file: HashSet::new(),
    })
}

fn apply(s: &mut State, mode: Mode) {
    let Some(p) = &s.transfer.preview else { return };
    let actions = listxml::actions(&p.plan, mode, &p.use_file);
    if actions.is_empty() {
        s.transfer.preview = None;
        s.transfer.done = Some(Done {
            queued: 0,
            ids: Vec::new(),
            undone: None,
        });
        return;
    }
    s.transfer.applying = true;
    crate::queue::send(s, crate::queue::Cmd::Import(actions));
}

fn add_new(s: &mut State) {
    apply(s, Mode::NewOnly);
}

fn add_and_update(s: &mut State) {
    apply(s, Mode::NewAndDifferent);
}

fn cancel(s: &mut State) {
    s.transfer.preview = None;
    s.transfer.error = None;
    s.transfer.done = None;
}

fn undo(s: &mut State) {
    let Some(d) = &s.transfer.done else { return };
    if d.ids.is_empty() || d.undone.is_some() {
        return;
    }
    crate::queue::send(s, crate::queue::Cmd::UndoImport(d.ids.clone()));
}

fn open_queue(s: &mut State) {
    s.go(Page::Queue);
}

/// The queue worker saved the import (or couldn't).
pub fn imported(s: &mut State, r: Result<Vec<i64>, String>) -> bool {
    s.transfer.applying = false;
    match r {
        Ok(ids) => {
            s.transfer.preview = None;
            s.transfer.done = Some(Done {
                queued: ids.len(),
                ids,
                undone: None,
            });
        }
        Err(e) => s.transfer.error = Some(e),
    }
    true
}

/// The queue worker took an import back.
pub fn undone(s: &mut State, r: Result<usize, String>) -> bool {
    match (r, s.transfer.done.as_mut()) {
        (Ok(n), Some(d)) => d.undone = Some(n),
        (Err(e), _) => s.transfer.error = Some(e),
        _ => {}
    }
    true
}

// ------------------------------------------------------------------------------------------ view

/// Title and note on the left, the control on the right (as the other settings rows).
pub(crate) fn line(key: &str, title: &str, note: String, control: El) -> El {
    let head = text(title.to_string())
        .font(theme::BODY_MED)
        .color(PAL.text)
        .wrap();
    row()
        .key(key.to_string())
        .w_full()
        .gap(16.0)
        .py(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(head)
                .child(text(note).font(theme::LABEL).color(PAL.dim).wrap()),
        )
        .child(control)
}

/// Settings → Account: Export anime list… (and Export manga list…).
pub fn export_row(s: &State, kind: Kind) -> El {
    let t = &s.transfer;
    let manga = kind == Kind::Manga;
    let (export_note, exporting) = if manga {
        (&t.manga_export_note, t.manga_exporting)
    } else {
        (&t.export_note, t.exporting)
    };
    let go: fn(&mut State) = if manga { export_manga } else { export_anime };
    let (note, on): (String, Option<fn(&mut State)>) = match (refusal(s), export_note) {
        (Some(why), _) => (why.to_string(), None),
        (None, _) if exporting => ("Writing the file\u{2026}".into(), None),
        (None, Some(Ok(m))) => (m.clone(), Some(go)),
        (None, Some(Err(e))) => (e.clone(), Some(go)),
        (None, None) if s.account.is_none() => ("Sign in first: the export is written from your saved list.".into(), None),
        (None, None) if manga => ("Your manga list as a MyAnimeList XML file, which MyAnimeList, AniList, Kitsu and Taiga can import. Status, chapters, volumes, score, rereads and the dates Kotora has.".into(), Some(go)),
        (None, None) => ("Your anime list as a MyAnimeList XML file, which MyAnimeList, AniList, Kitsu and Taiga can import. Status, episodes, score, rewatches and the dates Kotora has.".into(), Some(go)),
    };
    let (key, title, button) = if manga {
        (
            "set/export-manga",
            "Export manga list\u{2026}",
            "set/export-manga-button",
        )
    } else {
        (
            "set/export",
            "Export anime list\u{2026}",
            "set/export-button",
        )
    };
    let r = line(
        key,
        title,
        note,
        small_button(button, "Export\u{2026}", false, on),
    );
    if matches!(export_note, Some(Err(_))) && refusal(s).is_none() {
        r.key(if manga {
            "set/export-manga-error"
        } else {
            "set/export-error"
        })
    } else {
        r
    }
}

/// Settings → Account: Import anime list… (and Import manga list…) and, once a file is read, its
/// preview under the row of the kind it is about.
pub fn import_row(s: &State, kind: Kind) -> El {
    let t = &s.transfer;
    let manga = kind == Kind::Manga;
    let service = s.service.title();
    let signed_in = s.account.is_some();
    let idle = t.busy.is_none() && !t.applying;
    // The panel belongs to the kind the last import was started for.
    let mine = t.import_kind == kind;
    let note = match (&t.busy, signed_in) {
        (Some(b), _) if mine => b.clone(),
        (_, false) => "Sign in first: the file is compared with your list.".into(),
        _ if manga => format!(
            "Read a MyAnimeList manga XML export (.xml or .xml.gz) and add it to your {service} manga list. You see what changes before anything is sent."
        ),
        _ => format!(
            "Read a MyAnimeList anime XML export (.xml or .xml.gz) and add it to your {service} anime list. You see what changes before anything is sent."
        ),
    };
    let go: fn(&mut State) = if manga { import_manga } else { import_anime };
    let on: Option<fn(&mut State)> = if idle && signed_in && t.preview.is_none() {
        Some(go)
    } else {
        None
    };
    let (key, line_key, title, button) = if manga {
        (
            "set/import-manga",
            "set/import-manga-line",
            "Import manga list\u{2026}",
            "set/import-manga-button",
        )
    } else {
        (
            "set/import",
            "set/import-line",
            "Import anime list\u{2026}",
            "set/import-button",
        )
    };
    let mut c = col().key(key).w_full().child(line(
        line_key,
        title,
        note,
        small_button(button, "Import\u{2026}", false, on),
    ));
    if !mine {
        return c;
    }
    if let Some(e) = &t.error {
        c = c.child(
            text(e.clone())
                .key("set/import-error")
                .font(theme::LABEL)
                .color(PAL.status(Status::Dropped))
                .wrap()
                .pb(12.0),
        );
    }
    if let Some(p) = &t.preview {
        c = c.child(preview(s, p));
    }
    if let Some(d) = &t.done {
        c = c.child(done_view(s, d));
    }
    c
}

pub(crate) fn count_pill(key: &str, n: usize, word: &str, color: rune::Color) -> El {
    row()
        .key(key.to_string())
        .px(8.0)
        .h(22.0)
        .gap(6.0)
        .center_y()
        .radius(11.0)
        .fill(PAL.tint(color, 0.14))
        .child(el().size(6.0, 6.0).radius(3.0).fill(color))
        .child(
            text(format!("{n} {word}"))
                .font(theme::LABEL_NUM)
                .color(PAL.text),
        )
}

/// The most rows listed per group; the rest is "and N more".
const SHOWN: usize = 6;

pub(crate) fn group(key: &str, head: String, rows: Vec<String>) -> El {
    let mut c = col().key(key.to_string()).gap(3.0).child(
        text(head.to_uppercase())
            .font(theme::CAPTION)
            .color(PAL.faint)
            .pb(2.0),
    );
    let n = rows.len();
    for r in rows.into_iter().take(SHOWN) {
        c = c.child(text(r).font(theme::LABEL).color(PAL.dim).truncate());
    }
    if n > SHOWN {
        c = c.child(
            text(format!("and {} more", n - SHOWN))
                .font(theme::LABEL)
                .color(PAL.faint),
        );
    }
    c
}

fn values_text(r: &PlanRow) -> String {
    let Some(v) = r.entry.values() else {
        return String::new();
    };
    let manga = r.entry.kind == Kind::Manga;
    let mut t = v.status.label_in(r.entry.kind).to_string();
    if v.rewatching {
        t.push_str(if manga { ", rereading" } else { ", rewatching" });
    }
    if v.progress > 0 {
        t.push_str(&format!(
            ", {} {}",
            if manga { "chapter" } else { "episode" },
            v.progress
        ));
    }
    if v.volumes > 0 {
        t.push_str(&format!(", volume {}", v.volumes));
    }
    if let Some(sc) = v.score {
        t.push_str(&format!(", score {}", sc.to_ten()));
    }
    t
}

fn preview(s: &State, p: &Preview) -> El {
    let c = p.plan.counts();
    let service = s.service.title();
    let kind = p.plan.kind;
    let manga = kind == Kind::Manga;
    let mut box_ = col()
        .key("set/import-preview")
        .w_full()
        .gap(12.0)
        .padding(14.0)
        .mb(12.0)
        .radius(theme::R_MD)
        .fill(PAL.field)
        .border(1.0, PAL.border);
    box_ = box_.child(
        row()
            .gap(8.0)
            .center_y()
            .child(
                text(p.file.clone())
                    .font(theme::BODY_MED)
                    .color(PAL.text)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            )
            .child(
                text(if manga {
                    plural(p.plan.rows.len(), "manga", "manga")
                } else {
                    plural(p.plan.rows.len(), "anime", "anime")
                })
                .font(theme::LABEL_NUM)
                .color(PAL.faint),
            ),
    );
    box_ = box_.child(
        row()
            .key("set/import-counts")
            .gap(6.0)
            .flex_wrap()
            .child(count_pill(
                "set/import-new",
                c.new,
                "new",
                PAL.status(Status::Watching),
            ))
            .child(count_pill(
                "set/import-different",
                c.different,
                "different",
                PAL.status(Status::OnHold),
            ))
            .child(count_pill(
                "set/import-same",
                c.same,
                "same",
                PAL.status(Status::Completed),
            ))
            .child(count_pill(
                "set/import-unknown",
                c.unknown,
                "unknown",
                PAL.faint,
            )),
    );
    let new: Vec<String> = p
        .plan
        .rows
        .iter()
        .filter(|r| r.verdict == Verdict::New)
        .map(|r| format!("{}: {}", r.title(), values_text(r)))
        .collect();
    if !new.is_empty() {
        box_ = box_.child(group(
            "set/import-list-new",
            format!("New on your {service} list"),
            new,
        ));
    }
    let diff: Vec<String> = p
        .plan
        .rows
        .iter()
        .filter(|r| r.verdict == Verdict::Different && r.conflict.is_none())
        .map(|r| match r.current {
            _ if !r.entry.update_on_import => {
                format!("{}: kept (the file asks not to update it)", r.title())
            }
            Some(cur) if !r.update.is_empty() => {
                format!("{}: {}", r.title(), describe_in(kind, &r.update, &cur))
            }
            _ => format!("{}: yours is further along, kept", r.title()),
        })
        .collect();
    if !diff.is_empty() {
        box_ = box_.child(group(
            "set/import-list-different",
            "Different (with \u{201c}update\u{201d})".into(),
            diff,
        ));
    }
    let conflicts: Vec<&PlanRow> = p.plan.conflicts().collect();
    if !conflicts.is_empty() {
        let mut g = col().key("set/import-conflicts").gap(6.0).child(
            text("STATUS TO PICK (WITH \u{201c}UPDATE\u{201d})")
                .font(theme::CAPTION)
                .color(PAL.faint)
                .pb(2.0),
        );
        let n = conflicts.len();
        for r in conflicts.into_iter().take(CONFLICTS_SHOWN) {
            let (Some(id), Some(k)) = (r.target, r.conflict) else {
                continue;
            };
            let file_on = p.use_file.contains(&id);
            g = g.child(
                row()
                    .key(format!("set/import-conflict/{id}"))
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(r.title().to_string())
                            .font(theme::LABEL)
                            .color(PAL.text)
                            .truncate()
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(
                        cell(
                            format!("set/import-conflict/{id}/mine"),
                            format!("Keep {}", k.mine.label()),
                            !file_on,
                            move |s: &mut State| pick_status(s, id, false),
                        )
                        .child(
                            text(format!("Keep {}", k.mine.label()))
                                .font(theme::LABEL)
                                .color(PAL.text),
                        ),
                    )
                    .child(
                        cell(
                            format!("set/import-conflict/{id}/file"),
                            format!("Use {}", k.file.label()),
                            file_on,
                            move |s: &mut State| pick_status(s, id, true),
                        )
                        .child(
                            text(format!("Use {}", k.file.label()))
                                .font(theme::LABEL)
                                .color(PAL.text),
                        ),
                    ),
            );
        }
        if n > 1 {
            let all_file = p
                .plan
                .conflicts()
                .filter_map(|r| r.target)
                .all(|id| p.use_file.contains(&id));
            let more = if n > CONFLICTS_SHOWN {
                format!("and {} more. ", n - CONFLICTS_SHOWN)
            } else {
                String::new()
            };
            g = g.child(
                row()
                    .key("set/import-conflicts-all")
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(format!("{more}All of them:"))
                            .font(theme::LABEL)
                            .color(PAL.faint)
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(
                        cell(
                            "set/import-conflicts-all/mine".into(),
                            "Keep all yours".into(),
                            p.use_file.is_empty(),
                            |s: &mut State| pick_all(s, false),
                        )
                        .child(text("Keep all yours").font(theme::LABEL).color(PAL.text)),
                    )
                    .child(
                        cell(
                            "set/import-conflicts-all/file".into(),
                            "Use all from the file".into(),
                            all_file,
                            |s: &mut State| pick_all(s, true),
                        )
                        .child(
                            text("Use all from the file")
                                .font(theme::LABEL)
                                .color(PAL.text),
                        ),
                    ),
            );
        }
        box_ = box_.child(g);
    }
    let unknown: Vec<String> = p
        .plan
        .rows
        .iter()
        .filter_map(|r| match &r.verdict {
            Verdict::Unknown(why) => Some(format!(
                "{}: {why}",
                if r.entry.title.is_empty() {
                    format!("MyAnimeList {}", r.entry.mal_id)
                } else {
                    r.entry.title.clone()
                }
            )),
            _ => None,
        })
        .collect();
    if !unknown.is_empty() {
        box_ = box_.child(group("set/import-list-unknown", "Left out".into(), unknown));
    }
    let count_rule = if manga {
        "A higher chapter or volume count wins"
    } else {
        "A higher episode count wins"
    };
    let to = match s.mirror_target() {
        Some(m) => format!("{service}, and to {m} (the mirror), at each service\u{2019}s pace"),
        None => format!("{service} at its pace"),
    };
    let mut notes = vec![format!(
        "Changes go through the sync queue to {to}, and wait there while you are offline. {count_rule}; a score fills an empty one."
    )];
    if p.plan.other > 0 {
        let verb = if p.plan.other == 1 { "is" } else { "are" };
        let (what, hint) = if manga {
            (
                plural(p.plan.other, "anime entry", "anime entries"),
                "Import anime list\u{2026}",
            )
        } else {
            (
                plural(p.plan.other, "manga entry", "manga entries"),
                "Import manga list\u{2026}",
            )
        };
        notes.push(format!(
            "{what} in the file {verb} left out: use \u{201c}{hint}\u{201d} for those."
        ));
    }
    notes.push("Dates and notes in the file are not imported.".into());
    box_ = box_.child(
        text(notes.join(" "))
            .font(theme::LABEL)
            .color(PAL.faint)
            .wrap(),
    );
    let busy = s.transfer.applying;
    let new_on: Option<fn(&mut State)> = (c.new > 0 && !busy).then_some(add_new);
    let both_on: Option<fn(&mut State)> =
        (c.new + c.to_update > 0 && !busy).then_some(add_and_update);
    box_.child(
        row()
            .key("set/import-actions")
            .gap(8.0)
            .flex_wrap()
            .child(small_button(
                "set/import-add-new",
                &format!("Add new only ({})", c.new),
                false,
                new_on,
            ))
            .child(small_button(
                "set/import-add-update",
                &format!("Add new and update ({})", c.new + c.to_update),
                true,
                both_on,
            ))
            .child(small_button(
                "set/import-cancel",
                "Cancel",
                false,
                (!busy).then_some(cancel as fn(&mut State)),
            )),
    )
}

/// Status conflicts listed one by one; the rest go by "Keep all yours" / "Use all".
const CONFLICTS_SHOWN: usize = 8;

fn pick_all(s: &mut State, file: bool) {
    if let Some(p) = &mut s.transfer.preview {
        p.use_file = if file {
            p.plan.conflicts().filter_map(|r| r.target).collect()
        } else {
            HashSet::new()
        };
    }
}

fn pick_status(s: &mut State, id: u32, file: bool) {
    if let Some(p) = &mut s.transfer.preview {
        if file {
            p.use_file.insert(id);
        } else {
            p.use_file.remove(&id);
        }
    }
}

fn done_view(s: &State, d: &Done) -> El {
    let (msg, can_undo) = match d.undone {
        Some(n) => (
            format!(
                "Took back {} that hadn\u{2019}t been sent yet.",
                plural(n, "change", "changes")
            ),
            false,
        ),
        None if d.queued == 0 => (
            "Nothing to change: your list already has everything in the file.".to_string(),
            false,
        ),
        None => (s.queued_text(d.queued, "change", "changes"), true),
    };
    let mut r = row()
        .key("set/import-done")
        .w_full()
        .gap(8.0)
        .pb(12.0)
        .center_y()
        .flex_wrap()
        .child(
            text(msg)
                .font(theme::LABEL)
                .color(PAL.text)
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        );
    if can_undo {
        r = r
            .child(small_button(
                "set/import-queue",
                "Open Queue",
                false,
                Some(open_queue),
            ))
            .child(small_button(
                "set/import-undo",
                "Undo import",
                false,
                Some(undo),
            ));
    }
    r.child(small_button(
        "set/import-close",
        "Close",
        false,
        Some(cancel),
    ))
}

// ---------------------------------------------------------------------------------------- scenes

/// The fake preview of the scenes: a made-up file compared with the fake list.
pub fn fake_preview(s: &mut State) {
    let file = listxml::parse(fake_file(s).as_bytes()).expect("the fake file reads");
    let anilist = s.service != ServiceId::Mal;
    let list: Vec<kotora_core::ListItem> = s.entries.iter().map(|e| item(e, anilist)).collect();
    let mut mapped = HashMap::new();
    if s.service != ServiceId::Mal {
        // AniList: the fake list's ids are taken as MyAnimeList ids too; 9104 is new there.
        mapped.insert(
            9104,
            kotora_core::Media {
                id: 179104,
                id_mal: Some(9104),
                title: "Lantern Festival Nights".into(),
                episodes: Some(12),
                format: "tv".into(),
                ..Default::default()
            },
        );
    }
    let plan = listxml::plan(&file, s.service.name(), &list, &mapped);
    s.transfer = Transfer {
        preview: Some(Preview {
            file: "animelist_demo.xml.gz".into(),
            plan,
            use_file: HashSet::new(),
        }),
        ..Transfer::default()
    };
}

/// A fake entry as the saved list holds it (AniList: its id doubles as the MyAnimeList id).
fn item(e: &crate::model::Entry, anilist: bool) -> kotora_core::ListItem {
    kotora_core::ListItem {
        media: kotora_core::Media {
            id: e.id,
            title: e.main_title.clone(),
            episodes: e.episodes,
            id_mal: anilist.then_some(e.id),
            ..Default::default()
        },
        status: e.status,
        progress: e.progress,
        score: e.score,
        rewatching: e.rewatching,
        times_rewatched: e.times_rewatched,
        volumes: 0,
        updated_at: e.updated_at,
    }
}

/// A made-up MyAnimeList export for the scenes, built on the fake list's ids.
fn fake_file(s: &State) -> String {
    let mut x = String::from("<myanimelist><myinfo><user_name>demo_user</user_name></myinfo>");
    let mut add = |id: u32, title: &str, status: &str, ep: u32, score: u8| {
        x.push_str(&format!(
            "<anime><series_animedb_id>{id}</series_animedb_id><series_title><![CDATA[{title}]]></series_title><series_type>TV</series_type><series_episodes>12</series_episodes><my_watched_episodes>{ep}</my_watched_episodes><my_score>{score}</my_score><my_status>{status}</my_status></anime>"
        ));
    };
    // The fake list counts from id 0, which no real anime has: left out.
    for (i, e) in s
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.id > 0)
        .take(40)
    {
        let it = item(e, false);
        let st = listxml::status_to_xml(it.status);
        match i % 5 {
            // Further along in the file.
            0 => add(
                it.media.id,
                &it.media.title,
                st,
                it.progress + 1,
                it.score.map_or(0, kotora_core::Score::to_ten),
            ),
            // A status the plan can't settle.
            1 if it.status != Status::Dropped => add(
                it.media.id,
                &it.media.title,
                "Dropped",
                it.progress,
                it.score.map_or(0, kotora_core::Score::to_ten),
            ),
            _ => add(
                it.media.id,
                &it.media.title,
                st,
                it.progress,
                it.score.map_or(0, kotora_core::Score::to_ten),
            ),
        }
    }
    add(9101, "Moonlit Harbor", "Plan to Watch", 0, 0);
    add(9102, "Paper Kite Diaries", "Completed", 12, 8);
    add(9103, "Rust & Rain", "Watching", 3, 0);
    add(9104, "Lantern Festival Nights", "On-Hold", 5, 7);
    add(0, "No Id In The File", "Watching", 1, 0);
    add(9105, "Odd One", "Rewatching later", 1, 0);
    x.push_str("<manga><manga_mangadb_id>2</manga_mangadb_id></manga></myanimelist>");
    x
}

/// The fake preview of the manga scenes: a made-up manga file compared with the fake manga list.
pub fn fake_manga_preview(s: &mut State) {
    let list = crate::manga::fake_items();
    let mut x = String::from(
        "<myanimelist><myinfo><user_name>demo_user</user_name><user_export_type>2</user_export_type></myinfo>",
    );
    let mut add = |id: u32,
                   title: &str,
                   status: &str,
                   vols: u32,
                   chs: u32,
                   rv: u32,
                   rc: u32,
                   score: u8| {
        x.push_str(&format!(
            "<manga><manga_mangadb_id>{id}</manga_mangadb_id><manga_title><![CDATA[{title}]]></manga_title><manga_volumes>{vols}</manga_volumes><manga_chapters>{chs}</manga_chapters><my_read_volumes>{rv}</my_read_volumes><my_read_chapters>{rc}</my_read_chapters><my_score>{score}</my_score><my_status>{status}</my_status></manga>"
        ));
    };
    for (i, it) in list.iter().enumerate() {
        let st = listxml::status_to_xml_of(Kind::Manga, it.status);
        let (vols, chs) = (
            it.media.volumes.unwrap_or(0),
            it.media.episodes.unwrap_or(0),
        );
        let score = it.score.map_or(0, kotora_core::Score::to_ten);
        match i % 4 {
            // Further along in the file.
            0 => add(
                it.media.id,
                &it.media.title,
                st,
                vols,
                chs,
                it.volumes + 1,
                it.progress + 3,
                score,
            ),
            // A status the plan can't settle.
            1 if it.status != Status::Dropped => add(
                it.media.id,
                &it.media.title,
                "Dropped",
                vols,
                chs,
                it.volumes,
                it.progress,
                score,
            ),
            _ => add(
                it.media.id,
                &it.media.title,
                st,
                vols,
                chs,
                it.volumes,
                it.progress,
                score,
            ),
        }
    }
    add(9201, "Starlit Ferry", "Plan to Read", 0, 0, 0, 0, 0);
    add(9202, "Paper Kite Atlas", "Completed", 6, 48, 6, 48, 8);
    add(9203, "Copper Lanterns", "Reading", 0, 0, 2, 17, 0);
    add(0, "No Id In The File", "Reading", 0, 0, 0, 1, 0);
    add(9204, "Odd One", "Reading later", 0, 0, 0, 1, 0);
    x.push_str("<anime><series_animedb_id>2</series_animedb_id></anime></myanimelist>");
    let file = listxml::parse_kind(x.as_bytes(), Kind::Manga).expect("the fake file reads");
    let plan = listxml::plan(&file, s.service.name(), &list, &HashMap::new());
    s.transfer = Transfer {
        import_kind: Kind::Manga,
        preview: Some(Preview {
            file: "mangalist_demo.xml.gz".into(),
            plan,
            use_file: HashSet::new(),
        }),
        ..Transfer::default()
    };
}
