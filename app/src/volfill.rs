//! "Fill volumes from chapters" (Settings → Account, with Manga on) and the manga editor's "Vol
//! from chapters" button: volumes read from chapters read, by MangaDex's volume tables (core
//! `mangadex.rs`).
//!
//! - Only on request: the Settings row looks up each manga with chapters read and volumes short
//!   of their total (one search and one volume table per series, under 3 requests a second,
//!   saved for 30 days), then shows a preview: what goes up ("Ch 171 → Vol 0 → 19"), what
//!   MangaDex doesn't have, what is already right or higher. Volumes are never lowered.
//! - "Fill N volumes" queues the changes as an import (one transaction, mirrored with the mirror
//!   on); "Undo" takes back what hasn't been sent.
//! - The editor's button fills its Volumes field from its Chapters (a lookup first if needed);
//!   the user still saves. MangaDex not having the manga disables it, with the reason on hover.
//! - Every place the numbers show says "Volume data: MangaDex" (its acceptable use).
//!
//! Nothing runs headless (no app handle): scenes set the state.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kotora_core::mangadex::{self, FillPlan, Lookup};
use kotora_core::service::ServiceError;
use kotora_core::{Kind, ListItem, Media, paths};
use rune::{El, Tx, col, el, row, text};

use crate::manga::MEntry;
use crate::model::State;
use crate::theme::{self, PAL};
use crate::transfer::{count_pill, group, line, plural};
use crate::ui::small_button;

/// The credit MangaDex's acceptable use asks for, wherever its numbers show.
pub const CREDIT: &str = "Volume data: MangaDex";

/// The Settings row's state, and the editor's lookups.
#[derive(Default)]
pub struct VolFill {
    /// Looking series up: (done, of).
    pub busy: Option<(usize, usize)>,
    pub preview: Option<FillPlan>,
    pub error: Option<String>,
    pub applying: bool,
    /// After Fill: how many were queued and their ids (for Undo), or how many Undo took back.
    pub done: Option<(usize, Vec<i64>)>,
    pub undone: Option<usize>,
    /// Stop the lookups (Cancel while busy).
    pub cancel: Arc<AtomicBool>,
    /// The editor's lookups, by manga id of the active service: `None` while it runs.
    pub tables: HashMap<u32, Option<Lookup>>,
    /// The editor's button is under the pointer (its hover note).
    pub tip: bool,
}

fn offline_text(e: &ServiceError) -> String {
    match e {
        ServiceError::Offline(_) => "Can\u{2019}t reach MangaDex: volumes from chapters need the network. Nothing was changed.".into(),
        ServiceError::RateLimited => "MangaDex asked Kotora to slow down. Try again in a minute; what was looked up is saved.".into(),
        ServiceError::SignInAgain => "MangaDex refused the request.".into(),
        ServiceError::Other(m) => format!("MangaDex: {m}"),
    }
}

// ---------------------------------------------------------------------------------- settings

/// "Fill volumes from chapters…": look every manga up (a thread), then the preview.
/// The manga a fill looks up: chapters read, volumes short of the total.
pub fn candidates(s: &State) -> Vec<ListItem> {
    s.manga.entries.iter().map(item_of).filter(mangadex::wants_lookup).collect()
}

/// Looks `items` up on MangaDex (the cache first, unless `refresh`), calling `progress(done, of)`
/// after each; stops early when `cancel` is set. One series MangaDex can't answer for is left
/// out (`None`); no network or a rate limit stops it with the reason. Blocking: a worker thread
/// (the Settings row) or `kot`'s connection thread.
pub fn look_up(items: Vec<ListItem>, service: kotora_core::service::ServiceId, refresh: bool, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize)) -> Result<Vec<(ListItem, Option<Lookup>)>, String> {
    let mut client = mangadex::Client::online();
    let cache = mangadex::Cache::default_dir();
    let mut rows = Vec::new();
    let n = items.len();
    for (i, it) in items.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let key = mangadex::key_of(service, &it.media);
        match mangadex::find(&mut client, &cache, key, &mangadex::titles_of(&it.media), kotora_core::time::now_unix(), refresh) {
            Ok((l, _)) => rows.push((it, Some(l))),
            Err(ServiceError::Other(_)) => rows.push((it, None)),
            Err(e) => return Err(offline_text(&e)),
        }
        progress(i + 1, n);
    }
    Ok(rows)
}

/// "Fill…": look every manga up (the saved answers first) and show the preview.
fn start(s: &mut State) {
    start_with(s, false);
}

/// "Look up again": the same, asking MangaDex again for every manga (the saved answers are
/// replaced).
fn look_again(s: &mut State) {
    s.volfill.preview = None;
    start_with(s, true);
}

fn start_with(s: &mut State, refresh: bool) {
    let v = &mut s.volfill;
    if v.busy.is_some() || v.applying {
        return;
    }
    v.error = None;
    v.preview = None;
    v.done = None;
    v.undone = None;
    if paths::offline_forced() || s.offline {
        v.error = Some(offline_text(&ServiceError::Offline(String::new())));
        return;
    }
    let Some(h) = s.handle.clone() else { return };
    let items = candidates(s);
    let service = s.service;
    let v = &mut s.volfill;
    v.busy = Some((0, items.len()));
    v.cancel = Arc::new(AtomicBool::new(false));
    let cancel = v.cancel.clone();
    let _ = std::thread::Builder::new().name("kotora-mangadex".into()).spawn(move || {
        let hp = h.clone();
        let r = look_up(items, service, refresh, &cancel, move |i, n| {
            hp.update_if(move |s, _| {
                s.volfill.busy = Some((i, n));
                true
            });
        });
        let stopped = cancel.load(Ordering::Relaxed);
        h.update_if(move |s, tx| {
            let v = &mut s.volfill;
            v.busy = None;
            match r {
                Err(e) => v.error = Some(e),
                Ok(rows) => {
                    for (it, l) in &rows {
                        if let Some(l) = l {
                            v.tables.insert(it.media.id, Some(l.clone()));
                        }
                    }
                    if !stopped {
                        v.preview = Some(mangadex::plan_fill(&rows));
                        tx.scroll_into_view("set/fill-volumes-preview");
                    }
                }
            }
            true
        });
    });
}

/// The preview as `kot fill-volumes` prints it.
pub fn plan_text(p: &FillPlan) -> String {
    let mut out = Vec::new();
    let mut part = |head: String, lines: Vec<String>| {
        if !lines.is_empty() {
            out.push(head);
            out.extend(lines.into_iter().map(|l| format!("  {l}")));
        }
    };
    part(format!("Volumes that go up ({}):", p.fill.len()), p.fill.iter().map(|r| format!("{}: ch {} \u{2192} vol {} \u{2192} {}", r.title, r.chapters, r.from, r.to)).collect());
    part(format!("Higher than MangaDex, left alone ({}):", p.higher.len()), p.higher.iter().map(|r| format!("{}: vol {} (MangaDex: {})", r.title, r.from, r.to)).collect());
    part(format!("Already right ({}):", p.same.len()), p.same.iter().map(|r| format!("{}: vol {}", r.title, r.from)).collect());
    part(format!("Not found on MangaDex ({}):", p.not_found.len()), p.not_found.clone());
    if p.fill.is_empty() {
        out.push("Nothing to change.".into());
    }
    out.push(format!("{CREDIT}."));
    out.join("\n")
}

/// A manga entry as the core sees it (its titles, ids and counts).
fn item_of(e: &MEntry) -> ListItem {
    ListItem {
        media: Media {
            kind: Kind::Manga,
            id: e.id,
            title: e.main_title.clone(),
            title_en: e.title_en.clone(),
            volumes: e.volumes_total,
            episodes: e.chapters_total,
            id_mal: e.id_mal,
            ..Default::default()
        },
        status: e.status,
        progress: e.chapters,
        score: e.score,
        rewatching: e.rereading,
        times_rewatched: e.times_reread,
        volumes: e.volumes,
        updated_at: e.updated_at,
    }
}

fn stop(s: &mut State) {
    s.volfill.cancel.store(true, Ordering::Relaxed);
}

fn close(s: &mut State) {
    let v = &mut s.volfill;
    v.preview = None;
    v.error = None;
    v.done = None;
    v.undone = None;
}

/// Fill: the preview's volumes go to the queue as one import.
fn apply(s: &mut State) {
    let Some(p) = &s.volfill.preview else { return };
    let actions = mangadex::fill_actions(p);
    if actions.is_empty() {
        close(s);
        return;
    }
    s.volfill.applying = true;
    crate::queue::send(s, crate::queue::Cmd::FillVolumes(actions));
}

/// The queue worker's answer to Fill.
pub fn applied(s: &mut State, r: Result<Vec<i64>, String>) -> bool {
    let v = &mut s.volfill;
    v.applying = false;
    match r {
        Ok(ids) => {
            v.preview = None;
            v.done = Some((ids.len(), ids));
        }
        Err(e) => v.error = Some(e),
    }
    true
}

fn undo(s: &mut State) {
    let Some((_, ids)) = s.volfill.done.clone() else { return };
    crate::queue::send(s, crate::queue::Cmd::UndoFillVolumes(ids));
}

/// The queue worker's answer to Undo.
pub fn undone(s: &mut State, r: Result<usize, String>) -> bool {
    match r {
        Ok(n) => s.volfill.undone = Some(n),
        Err(e) => s.volfill.error = Some(e),
    }
    true
}

/// Settings → Account → Fill volumes from chapters… (with Manga on).
pub fn settings_row(s: &State) -> El {
    let v = &s.volfill;
    let signed_in = s.account.is_some();
    let idle = v.busy.is_none() && !v.applying && v.preview.is_none();
    let note = match v.busy {
        Some((i, n)) => format!("Looking up {} on MangaDex\u{2026} ({i} of {n})", plural(n, "manga", "manga")),
        None if !signed_in => "Sign in first.".into(),
        None => "Sets volumes read from chapters read, with MangaDex\u{2019}s volume tables. You see every change first; volumes are never lowered.".into(),
    };
    let button = if v.busy.is_some() {
        small_button("set/fill-volumes-stop", "Stop", false, Some(stop as fn(&mut State)))
    } else {
        small_button("set/fill-volumes-button", "Fill\u{2026}", false, (idle && signed_in).then_some(start as fn(&mut State)))
    };
    let mut c = col().key("set/fill-volumes").w_full().child(line("set/fill-volumes-line", "Fill volumes from chapters\u{2026}", note, button));
    if let Some(e) = &v.error {
        c = c.child(text(e.clone()).key("set/fill-volumes-error").font(theme::LABEL).color(PAL.status(kotora_core::Status::Dropped)).wrap().pb(12.0));
    }
    if let Some(p) = &v.preview {
        c = c.child(preview(s, p));
    }
    if let Some((n, _)) = &v.done {
        let (msg, can_undo) = match v.undone {
            Some(u) => (format!("Took back {} that hadn\u{2019}t been sent yet.", plural(u, "change", "changes")), false),
            None => (s.queued_text(*n, "volume change", "volume changes"), true),
        };
        let mut r = row().key("set/fill-volumes-done").w_full().gap(8.0).pb(12.0).center_y().flex_wrap().child(text(msg).font(theme::LABEL).color(PAL.text).wrap().grow(1.0).min_w(0.0));
        if can_undo {
            r = r.child(small_button("set/fill-volumes-undo", "Undo", false, Some(undo as fn(&mut State))));
        }
        c = c.child(r.child(small_button("set/fill-volumes-close", "Close", false, Some(close as fn(&mut State)))));
    }
    c
}

/// The preview: counts, what goes up, what MangaDex doesn't have, what stays; the credit.
fn preview(s: &State, p: &FillPlan) -> El {
    let mut b = col().key("set/fill-volumes-preview").w_full().gap(12.0).padding(14.0).mb(12.0).radius(theme::R_MD).fill(PAL.field).border(1.0, PAL.border);
    // "go up" always; the other chips only when they count something.
    let chip = |key: &str, n: usize, word: &str, c: rune::Color| (n > 0).then(|| count_pill(key, n, word, c));
    b = b.child(
        row()
            .key("set/fill-volumes-counts")
            .gap(6.0)
            .flex_wrap()
            .child(count_pill("set/fill-volumes-up", p.fill.len(), "volumes go up", PAL.status(kotora_core::Status::Watching)))
            .children(chip("set/fill-volumes-same", p.same.len(), "already right", PAL.status(kotora_core::Status::Completed)))
            .children(chip("set/fill-volumes-higher", p.higher.len(), "higher than MangaDex (left alone)", PAL.status(kotora_core::Status::OnHold)))
            .children(chip("set/fill-volumes-missing", p.not_found.len(), "not found on MangaDex", PAL.faint)),
    );
    let line = |r: &mangadex::FillRow| format!("{}: Ch {} \u{2192} Vol {} \u{2192} {}", r.title, r.chapters, r.from, r.to);
    if !p.fill.is_empty() {
        b = b.child(group("set/fill-volumes-list-up", "Volumes that go up".into(), p.fill.iter().map(line).collect()));
    }
    if !p.not_found.is_empty() {
        b = b.child(group("set/fill-volumes-list-missing", "Not found on MangaDex".into(), p.not_found.clone()));
    }
    if !p.higher.is_empty() {
        let higher = p.higher.iter().map(|r| format!("{}: Vol {} (MangaDex: {})", r.title, r.from, r.to)).collect();
        b = b.child(group("set/fill-volumes-list-higher", "Higher than MangaDex (left alone)".into(), higher));
    }
    if !p.same.is_empty() {
        let same = p.same.iter().map(|r| format!("{}: Vol {}", r.title, r.from)).collect();
        b = b.child(group("set/fill-volumes-list-same", "Already right".into(), same));
    }
    let n = p.fill.len();
    if n == 0 {
        // Nothing goes up: say so, and only Close.
        b = b.child(text("Nothing to change.").key("set/fill-volumes-nothing").font(theme::BODY_MED).color(PAL.text));
        b = b.child(text(CREDIT).key("set/fill-volumes-credit").font(theme::LABEL).color(PAL.faint));
        return b.child(
            row()
                .key("set/fill-volumes-actions")
                .gap(8.0)
                .child(small_button("set/fill-volumes-close-preview", "Close", false, Some(close as fn(&mut State))))
                .child(small_button("set/fill-volumes-refresh", "Look up again", false, Some(look_again as fn(&mut State)))),
        );
    }
    let mirror = s.mirror.link.map(|l| format!(" With the mirror on, {} gets them too.", l.to.title())).unwrap_or_default();
    b = b.child(text(format!("Only volumes change, through the sync queue to {} at its pace; Undo takes back what hasn\u{2019}t been sent.{mirror}", s.service.title())).font(theme::LABEL).color(PAL.dim).wrap());
    b = b.child(text(format!("{CREDIT}. Saved answers are used for 30 days; \u{201c}Look up again\u{201d} asks MangaDex anew.")).key("set/fill-volumes-credit").font(theme::LABEL).color(PAL.faint));
    let fill: Option<fn(&mut State)> = (!s.volfill.applying).then_some(apply);
    b.child(
        row()
            .key("set/fill-volumes-actions")
            .gap(8.0)
            .child(small_button("set/fill-volumes-apply", &format!("Fill {}", plural(n, "volume count", "volume counts")), true, fill))
            .child(small_button("set/fill-volumes-cancel", "Cancel", false, Some(close as fn(&mut State))))
            .child(small_button("set/fill-volumes-refresh", "Look up again", false, Some(look_again as fn(&mut State)))),
    )
}

// ------------------------------------------------------------------------------------ editor

/// The editor's "Vol from chapters": fills the Volumes field from the Chapters field (never
/// lower than it is), looking the manga up first when needed. The user still saves.
pub fn fill_editor(s: &mut State) {
    let Some(ed) = s.manga.editor else { return };
    let Some(e) = s.manga.entries.iter().find(|e| e.id == ed.id) else { return };
    match s.volfill.tables.get(&ed.id) {
        Some(Some(Lookup::Found(t))) => {
            let mut to = mangadex::volumes_for(t, ed.chapters);
            if let Some(n) = e.volumes_total {
                to = to.min(n);
            }
            let text = if to > ed.volumes {
                format!("Chapter {} ends volume {to}. {CREDIT}.", ed.chapters)
            } else {
                format!("MangaDex: chapter {} ends volume {to}; volumes stay {}. {CREDIT}.", ed.chapters, ed.volumes)
            };
            if let Some(ed) = &mut s.manga.editor {
                ed.volumes = ed.volumes.max(to);
            }
            s.toast = Some(crate::library::Toast { text, warn: false, until: std::time::Instant::now() + std::time::Duration::from_secs_f64(4.0), undo: None });
        }
        Some(_) => {}
        None => {
            let item = item_of(e);
            let Some(h) = s.handle.clone() else { return };
            if paths::offline_forced() || s.offline {
                s.error = Some(offline_text(&ServiceError::Offline(String::new())));
                return;
            }
            let id = ed.id;
            let service = s.service;
            s.volfill.tables.insert(id, None);
            let _ = std::thread::Builder::new().name("kotora-mangadex".into()).spawn(move || {
                let mut client = mangadex::Client::online();
                let cache = mangadex::Cache::default_dir();
                let key = mangadex::key_of(service, &item.media);
                let r = mangadex::find(&mut client, &cache, key, &mangadex::titles_of(&item.media), kotora_core::time::now_unix(), false);
                h.update_if(move |s, _| {
                    match r {
                        Ok((l, _)) => {
                            s.volfill.tables.insert(id, Some(l));
                            // Still the same editor: fill it now.
                            if s.manga.editor.is_some_and(|ed| ed.id == id) {
                                fill_editor(s);
                            }
                        }
                        Err(e) => {
                            s.volfill.tables.remove(&id);
                            s.error = Some(offline_text(&e));
                        }
                    }
                    true
                });
            });
        }
    }
}

/// The button beside the editor's Volumes: "Vol from chapters", "Looking up…", or off when
/// MangaDex doesn't have the manga (hover says so).
pub fn editor_button(s: &State, id: u32) -> El {
    let (label, on, missing) = match s.volfill.tables.get(&id) {
        Some(None) => ("Looking up\u{2026}", false, false),
        Some(Some(Lookup::NotFound)) => ("Vol from chapters", false, true),
        _ => ("Vol from chapters", true, false),
    };
    let b = small_button("med/vol-fill", label, false, on.then_some(fill_editor as fn(&mut State)));
    let b = b.label(if missing { "Vol from chapters: not found on MangaDex".to_string() } else { format!("Vol from chapters ({CREDIT})") });
    el().key("med/vol-fill-wrap").child(b).on_hover_change(|s: &mut State, inside: bool, _tx: &mut Tx| s.volfill.tip = inside)
}

/// The hover note of the editor's button, under it (mounted at the window's root).
pub fn editor_tip(s: &State) -> Option<El> {
    let ed = s.manga.editor.filter(|_| s.volfill.tip)?;
    let note = match s.volfill.tables.get(&ed.id) {
        Some(Some(Lookup::NotFound)) => "Not found on MangaDex: no series there links to this manga, or it has no volume numbers.".to_string(),
        Some(None) => "Looking it up on MangaDex\u{2026}".to_string(),
        _ => format!("Sets Volumes from Chapters with MangaDex\u{2019}s volume table; you still save. {CREDIT}."),
    };
    Some(
        text(note)
            .key("med/vol-fill-tip")
            .font(theme::LABEL)
            .color(PAL.text)
            .px(10.0)
            .py(6.0)
            .max_w(280.0)
            .wrap()
            .radius(theme::R_SM)
            .fill(PAL.raised)
            .border(1.0, PAL.border_strong)
            .shadow(4.0, 12.0, rune::Color::BLACK.alpha(if PAL.light { 0.12 } else { 0.35 }))
            .anchored("med/vol-fill-wrap", rune::Side::Bottom, rune::Align::Start, 6.0),
    )
}

// ------------------------------------------------------------------------------------ scenes

/// A made-up volume table: 10 chapters a volume, for 20 volumes.
pub fn fake_table() -> mangadex::Table {
    mangadex::Table { md_id: "00000000-0000-4000-8000-000000000000".into(), title: "Made up".into(), volumes: (1..=20).map(|v| mangadex::Vol { volume: f64::from(v), last_chapter: f64::from(v * 10) }).collect() }
}

/// The scenes' preview: the fake manga list against made-up tables (Moth Orchestra and Kumo
/// Atelier not found).
pub fn fake_preview(s: &mut State) {
    let rows: Vec<(ListItem, Option<Lookup>)> = s
        .manga
        .entries
        .iter()
        .map(item_of)
        .filter(mangadex::wants_lookup)
        .map(|it| {
            let l = if matches!(it.media.id, 4 | 7) { Lookup::NotFound } else { Lookup::Found(fake_table()) };
            (it, Some(l))
        })
        .collect();
    s.volfill.preview = Some(mangadex::plan_fill(&rows));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_fills_volumes_from_chapters_and_never_lowers() {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        // Salt & Gears: chapter 64, volume 0, no total.
        s.manga_toggle_editor(2);
        s.volfill.tables.insert(2, Some(Lookup::Found(fake_table())));
        fill_editor(&mut s);
        assert_eq!(s.manga.editor.map(|e| e.volumes), Some(6), "chapter 64 ends volume 6");
        assert!(s.toast.as_ref().is_some_and(|t| t.text.contains(CREDIT)));
        assert_eq!(s.manga.entries.iter().find(|e| e.id == 2).map(|e| e.volumes), Some(0), "nothing saved yet");
        s.manga_editor_step(crate::manga::Unit::Volumes, 5);
        fill_editor(&mut s);
        assert_eq!(s.manga.editor.map(|e| e.volumes), Some(11), "never lowered");
        // Not found: the button is off.
        s.volfill.tables.insert(2, Some(Lookup::NotFound));
        let before = s.manga.editor.map(|e| e.volumes);
        fill_editor(&mut s);
        assert_eq!(s.manga.editor.map(|e| e.volumes), before);
    }

    fn keys(name: &str) -> rune::Runtime<State> {
        let mut s = State::fake();
        (crate::scenes::SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = rune::Runtime::new(s, crate::ui::view, (1360.0, 1400.0), rune::Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    /// "Already right" and "Higher than MangaDex" apart; chips at 0 hidden but "go up"; with
    /// nothing going up, "Nothing to change." and Close only.
    #[test]
    fn the_preview_splits_right_and_higher_and_says_when_nothing_changes() {
        let rt = keys("settings-fill-volumes-preview");
        for k in ["set/fill-volumes-up", "set/fill-volumes-same", "set/fill-volumes-higher", "set/fill-volumes-missing", "set/fill-volumes-list-same", "set/fill-volumes-list-higher", "set/fill-volumes-apply", "set/fill-volumes-cancel"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        assert!(rt.find("set/fill-volumes-nothing").is_none());
        let rt = keys("settings-fill-volumes-nothing");
        for k in ["set/fill-volumes-up", "set/fill-volumes-same", "set/fill-volumes-higher", "set/fill-volumes-nothing", "set/fill-volumes-close-preview"] {
            assert!(rt.find(k).is_some(), "{k}");
        }
        for k in ["set/fill-volumes-missing", "set/fill-volumes-apply", "set/fill-volumes-cancel"] {
            assert!(rt.find(k).is_none(), "{k}: hidden");
        }
    }

    /// What `kot fill-volumes` prints: every group with its count, the credit.
    #[test]
    fn the_kot_preview_reads_plainly() {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        fake_preview(&mut s);
        let t = plan_text(s.volfill.preview.as_ref().unwrap());
        assert!(t.starts_with("Volumes that go up (3):\n  Paper Wolves of Osaka: ch 12 \u{2192} vol 0 \u{2192} 1"), "{t}");
        assert!(t.contains("Higher than MangaDex, left alone (2):") && t.contains("Already right (1):") && t.contains("Not found on MangaDex (1):\n  Moth Orchestra"));
        assert!(t.ends_with("Volume data: MangaDex.") && !t.contains("Nothing to change"));
        let empty = plan_text(&FillPlan::default());
        assert_eq!(empty, "Nothing to change.\nVolume data: MangaDex.");
        // The preview offers "Look up again" (asks MangaDex anew).
        let rt = keys("settings-fill-volumes-preview");
        assert!(rt.find("set/fill-volumes-refresh").is_some());
        assert!(keys("settings-fill-volumes-nothing").find("set/fill-volumes-refresh").is_some());
    }

    /// The done line names the mirror when it is on (a fill is mirrored like any import).
    #[test]
    fn the_done_line_says_where_the_changes_go() {
        let mut s = State::fake();
        assert_eq!(s.queued_text(3, "volume change", "volume changes"), "Queued 3 volume changes for MyAnimeList. They go out at its pace; the Queue page shows them.");
        crate::mirror::fake_on(&mut s);
        assert_eq!(
            s.queued_text(3, "volume change", "volume changes"),
            "Queued 3 volume changes for MyAnimeList, mirrored to AniList. They go out at each service\u{2019}s pace; the Queue page shows them."
        );
        assert!(s.queued_text(1, "change", "changes").starts_with("Queued 1 change for MyAnimeList, mirrored to AniList."));
    }

    #[test]
    fn the_preview_lists_what_goes_up_and_headless_does_nothing() {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        fake_preview(&mut s);
        let p = s.volfill.preview.as_ref().unwrap();
        assert!(p.fill.iter().any(|r| r.title == "Salt & Gears" && r.from == 0 && r.to == 6));
        assert!(p.not_found.contains(&"Moth Orchestra".to_string()));
        assert!(p.fill.iter().chain(&p.same).chain(&p.higher).all(|r| r.to <= 20));
        // No app handle: nothing is looked up or queued.
        close(&mut s);
        start(&mut s);
        assert!(s.volfill.busy.is_none() || s.handle.is_none());
    }
}
