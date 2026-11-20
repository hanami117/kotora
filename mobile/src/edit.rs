//! The entry editor, the rewatch (reread) and the sync queue with its conflicts: the desktop's
//! `model.rs` editor rules and queue page over the core's [`Change`] and queue, for the phones.
//!
//! The editor state is a plain record ([`EntryEditor`]) the app keeps while the sheet is open;
//! [`editor_step`] and [`editor_set_status`] are the desktop's rules for the − / + and the status
//! cells, so both phones step the same way. [`Kotora::save_editor`] turns it into one queued
//! change, as the desktop's Save does.

use kotora_core::queue::{self, Op, OpState};
use kotora_core::{Change, Kind, Remote, Score, ScoreFormat, Values, time};

use crate::{Edit, Kotora, KotoraError, ListStatus, MediaKind, Service, other};

/// The editor of one entry: what the sheet shows and changes. `saved_*` and the totals are the
/// entry's, for the rules; the app changes the rest.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct EntryEditor {
    pub kind: MediaKind,
    pub id: u32,
    pub title: String,
    pub status: ListStatus,
    /// Episodes watched, or chapters read.
    pub progress: u32,
    pub total: Option<u32>,
    /// Manga: volumes read.
    pub volumes: u32,
    pub total_volumes: Option<u32>,
    /// On the 100-point scale; `None`: no score.
    pub score: Option<u32>,
    pub times_rewatched: u32,
    /// The saved entry's (not editable here: Rewatch / Stop rewatching act at once).
    pub rewatching: bool,
    pub saved_status: ListStatus,
}

/// A score cell: its points (100-point scale) and how the user's format shows it ("7", "7.5",
/// "★★★☆☆", ":)").
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ScoreChoice {
    pub points: u32,
    pub label: String,
}

/// The user's score format, for the editor.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ScoreScale {
    /// Low to high.
    pub choices: Vec<ScoreChoice>,
    /// The 100-point and decimal formats also step finer with − / + ([`Kotora::score_step`]).
    pub fine: bool,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueState {
    Waiting,
    Sending,
    Conflict,
    Failed,
    Sent,
}

impl From<OpState> for QueueState {
    fn from(s: OpState) -> QueueState {
        match s {
            OpState::Queued => QueueState::Waiting,
            OpState::Sending => QueueState::Sending,
            OpState::Conflict => QueueState::Conflict,
            OpState::Failed => QueueState::Failed,
            OpState::Done => QueueState::Sent,
        }
    }
}

/// One change in the sync queue, as the desktop's queue page shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct QueueItem {
    pub service: Service,
    pub id: i64,
    pub kind: MediaKind,
    pub media_id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    pub state: QueueState,
    /// "Progress 3 → 5, score 8", "Add as Watching", "Remove from list".
    pub what: String,
    /// How many edits merged into it.
    pub edits: u32,
    /// The service's words: why it failed, or why it is tried again.
    pub error: Option<String>,
    pub added: bool,
    pub removed: bool,
    pub edited_at: i64,
    pub sent_at: Option<i64>,
    /// A conflict: the change's fields as this phone has them ("Progress 7 / 24 · Score 8"), or
    /// "Remove from list".
    pub mine: String,
    /// A conflict: the same fields as the service has them; `None`: removed there.
    pub theirs: Option<String>,
    /// A conflict: the whole entry on the service ("12 / 24, Watching, score 8"); `None`: removed there.
    pub theirs_entry: Option<String>,
    /// A conflict: when the service's entry changed.
    pub theirs_changed_at: Option<i64>,
}

fn kind_of(k: MediaKind) -> Kind {
    k.into()
}

/// The fields of `op`'s change, as `v` has them: "Progress 7 / 24 · Score 8" (the desktop's
/// conflict card).
pub(crate) fn conflict_values(op: &Op, v: &Values, fmt: ScoreFormat) -> String {
    let total = op.episodes.map_or("?".to_string(), |n| n.to_string());
    let manga = op.kind == Kind::Manga;
    let mut parts = Vec::new();
    if op.change.status.is_some() {
        parts.push(v.status.label_in(op.kind).to_string());
    }
    if op.change.progress.is_some() {
        parts.push(format!("{} {} / {total}", if manga { "Chapters" } else { "Progress" }, v.progress));
    }
    if op.change.volumes.is_some() {
        parts.push(format!("Volumes {}", v.volumes));
    }
    if op.change.score.is_some() {
        parts.push(v.score.map_or("No score".to_string(), |n| format!("Score {}", fmt.words(n))));
    }
    if op.change.rewatching.is_some() || op.change.times_rewatched.is_some() {
        let re = if manga { "Reread" } else { "Rewatched" };
        parts.push(if v.rewatching { if manga { "Rereading".into() } else { "Rewatching".into() } } else { format!("{re} {}", v.times_rewatched) });
    }
    parts.join(" \u{b7} ")
}

/// The service's whole entry: "12 / 24, Watching, score 8" (the desktop's `remote_text_of`).
pub(crate) fn remote_text(kind: Kind, r: &Remote, episodes: Option<u32>, fmt: ScoreFormat) -> String {
    let total = episodes.map_or("?".to_string(), |n| n.to_string());
    let score = r.values.score.map_or("no score".to_string(), |n| format!("score {}", fmt.words(n)));
    if kind == Kind::Manga {
        return format!("Ch {} / {total}, vol {}, {}, {score}", r.values.progress, r.values.volumes, r.values.status.label_in(kind));
    }
    format!("{} / {total}, {}, {score}", r.values.progress, r.values.status.label())
}

/// The desktop's `editor_step`: − / + on the progress, kept within the count; the last episode
/// completes the entry, and stepping back off it undoes that.
#[uniffi::export]
pub fn editor_step(editor: EntryEditor, delta: i32) -> EntryEditor {
    let mut ed = editor;
    let max = ed.total.filter(|n| *n > 0);
    let p = ed.progress.saturating_add_signed(delta);
    ed.progress = max.map_or(p, |n| p.min(n));
    if delta > 0 && max.is_some_and(|n| ed.progress == n) {
        ed.status = ListStatus::Completed;
    } else if delta < 0 && ed.status == ListStatus::Completed && max.is_some_and(|n| ed.progress < n) && ed.saved_status != ListStatus::Completed {
        ed.status = ed.saved_status;
    }
    ed
}

/// − / + on a manga's volumes, kept within the count.
#[uniffi::export]
pub fn editor_step_volumes(editor: EntryEditor, delta: i32) -> EntryEditor {
    let mut ed = editor;
    let v = ed.volumes.saturating_add_signed(delta);
    ed.volumes = ed.total_volumes.filter(|n| *n > 0).map_or(v, |n| v.min(n));
    ed
}

/// The desktop's `editor_status`: Completed fills the progress (and a manga's volumes) when the
/// count is known.
#[uniffi::export]
pub fn editor_set_status(editor: EntryEditor, status: ListStatus) -> EntryEditor {
    let mut ed = editor;
    ed.status = status;
    if status == ListStatus::Completed {
        if let Some(n) = ed.total.filter(|n| *n > 0) {
            ed.progress = n;
        }
        if ed.kind == MediaKind::Manga
            && let Some(n) = ed.total_volumes.filter(|n| *n > 0)
        {
            ed.volumes = n;
        }
    }
    ed
}

impl Kotora {
    fn fmt_active(&self) -> Result<(Service, ScoreFormat), KotoraError> {
        let service = self.active_or_err()?;
        let fmt = self.score_format(&self.store(), service);
        Ok((service, fmt))
    }

    /// Queues `change` for the entry; returns the undo record with `message`.
    fn queue_edit(&self, kind: MediaKind, id: u32, change: Change, message: String) -> Result<Edit, KotoraError> {
        let service = self.active_or_err()?;
        let mut store = self.store();
        let scope = kotora_core::Scope::new(service.name(), kind.into());
        let item = store.load_item(scope, id)?.ok_or(KotoraError::NotFound)?;
        let before = item.values();
        store.enqueue(scope, id, change, time::now_unix())?;
        Ok(Edit {
            kind,
            id,
            message,
            before_status: before.status.into(),
            before_progress: before.progress,
            before_rewatching: before.rewatching,
            before_times_rewatched: before.times_rewatched,
            before_volumes: before.volumes,
            before_score: None,
            restore_score: false,
        })
    }

    fn service_fmt(&self, service: Service) -> ScoreFormat {
        self.score_format(&self.store(), service)
    }
}

#[uniffi::export]
impl Kotora {
    /// The editor for one entry of the active list, from its saved values.
    pub fn editor(&self, kind: MediaKind, id: u32) -> Result<EntryEditor, KotoraError> {
        let service = self.active_or_err()?;
        let store = self.store();
        let it = store.load_item(kotora_core::Scope::new(service.name(), kind.into()), id)?.ok_or(KotoraError::NotFound)?;
        Ok(EntryEditor {
            kind,
            id,
            title: it.media.title.clone(),
            status: it.status.into(),
            progress: it.progress,
            total: it.media.episodes.filter(|n| *n > 0),
            volumes: it.volumes,
            total_volumes: it.media.volumes.filter(|n| *n > 0),
            score: it.score.map(|s| u32::from(s.points())),
            times_rewatched: it.times_rewatched,
            rewatching: it.rewatching,
            saved_status: it.status.into(),
        })
    }

    /// The active account's score format: the editor's cells.
    pub fn score_scale(&self) -> Result<ScoreScale, KotoraError> {
        let (_, fmt) = self.fmt_active()?;
        Ok(ScoreScale {
            choices: fmt.choices().into_iter().map(|s| ScoreChoice { points: u32::from(s.points()), label: fmt.show(s) }).collect(),
            fine: fmt.fine_step().is_some(),
        })
    }

    /// A score in the active account's format ("7.3", "★★★★☆"); `None` (no score): "No score".
    pub fn score_label(&self, points: Option<u32>) -> String {
        let fmt = self.fmt_active().map(|f| f.1).unwrap_or_default();
        points.and_then(Score::from_points).map_or("No score".into(), |s| fmt.show(s))
    }

    /// The fine − / + of the 100-point and decimal formats (from no score: the middle).
    pub fn score_step(&self, points: Option<u32>, delta: i32) -> Option<u32> {
        let fmt = self.fmt_active().map(|f| f.1).unwrap_or_default();
        fmt.step(points.and_then(Score::from_points), delta).map(|s| u32::from(s.points()))
    }

    /// Save: the changed fields become one queued change, as the desktop's Save. A picked score
    /// that already shows as the entry's keeps the precise one (an AniList 73 stays 73). The last
    /// episode during a rewatch ends it, counted (the core's rule). `None`: nothing changed.
    pub fn save_editor(&self, editor: EntryEditor) -> Result<Option<String>, KotoraError> {
        let (service, fmt) = self.fmt_active()?;
        let kind: Kind = editor.kind.into();
        let mut store = self.store();
        let scope = kotora_core::Scope::new(service.name(), kind);
        let item = store.load_item(scope, editor.id)?.ok_or(KotoraError::NotFound)?;
        let cur = item.values();
        let score = fmt.pick(cur.score, editor.score.and_then(Score::from_points));
        let mut change = Change {
            status: Some(editor.status.into()),
            progress: Some(editor.progress),
            score: Some(score),
            times_rewatched: Some(editor.times_rewatched),
            volumes: (kind == Kind::Manga).then_some(editor.volumes),
            ..Default::default()
        }
        .without_same(&cur);
        if change.times_rewatched.is_none() {
            change = change.completes(&cur, item.media.episodes);
        }
        if change.is_empty() {
            return Ok(None);
        }
        let what = queue::describe_with(kind, fmt, &change, &cur);
        store.enqueue(scope, editor.id, change, time::now_unix())?;
        Ok(Some(format!("{}: {what}", item.media.title)))
    }

    /// "Rewatch" ("Reread"): a completed entry is watched again from episode 1; it stays
    /// Completed, and the last episode ends the rewatch, counted. Undo puts it back.
    pub fn start_rewatch(&self, kind: MediaKind, id: u32) -> Result<Edit, KotoraError> {
        let row = self.entry(kind, id)?.ok_or(KotoraError::NotFound)?;
        let k = kind_of(kind);
        let manga = k == Kind::Manga;
        if row.rewatching {
            return Err(other(format!("{} is being {} already.", row.title, if manga { "reread" } else { "rewatched" })));
        }
        let service = self.active_or_err()?;
        let cur = self.store().load_item(kotora_core::Scope::new(service.name(), k), id)?.ok_or(KotoraError::NotFound)?.values();
        let change = Change::start_rewatch(&cur).ok_or_else(|| {
            other(if manga { format!("{} is not completed: only a completed manga can be reread.", row.title) } else { format!("{} is not completed: only a completed anime can be rewatched.", row.title) })
        })?;
        let msg = if manga { format!("{}: reread started", row.title) } else { format!("{}: rewatch started", row.title) };
        self.queue_edit(kind, id, change, msg)
    }

    /// "Stop rewatching" ("Stop rereading"): the rewatch ends without counting; the progress goes
    /// back to the last episode (the entry is still Completed).
    pub fn stop_rewatch(&self, kind: MediaKind, id: u32) -> Result<Edit, KotoraError> {
        let row = self.entry(kind, id)?.ok_or(KotoraError::NotFound)?;
        let manga = kind == MediaKind::Manga;
        if !row.rewatching {
            return Err(other(format!("{} is not being {}.", row.title, if manga { "reread" } else { "rewatched" })));
        }
        let progress = row.total.unwrap_or(row.progress);
        let msg = if manga { format!("{}: reread stopped", row.title) } else { format!("{}: rewatch stopped", row.title) };
        self.queue_edit(kind, id, Change { rewatching: Some(false), progress: Some(progress), ..Default::default() }, msg)
    }

    /// The sync queue of every signed-in account: conflicts, waiting, failed, sent today.
    pub fn queue_items(&self) -> Vec<QueueItem> {
        let now = time::now_unix();
        let mut out = Vec::new();
        // The mirror's changes have their own part of the page (`mirror.rs`), as on the desktop.
        let mirror_to = self.store().mirror_link().ok().flatten().map(|l| l.to.name());
        for service in Service::ALL.into_iter().filter(|s| self.signed_in(*s) && Some(s.name()) != mirror_to) {
            let fmt = self.service_fmt(service);
            let store = self.store();
            for op in store.queue(service.name(), now).unwrap_or_default() {
                let cover_url = store.load_item(kotora_core::Scope::new(service.name(), op.kind), op.media_id).ok().flatten().and_then(|i| i.media.cover_url);
                let conflict = op.state == OpState::Conflict;
                out.push(QueueItem {
                    service,
                    id: op.id,
                    kind: op.kind.into(),
                    media_id: op.media_id,
                    title: op.title.clone(),
                    cover_url,
                    state: op.state.into(),
                    what: queue::describe_op_with(&op, fmt),
                    edits: op.edits,
                    error: op.error.clone(),
                    added: op.added,
                    removed: op.removed,
                    edited_at: op.edited_at,
                    sent_at: op.sent_at,
                    mine: if op.removed { "Remove from list".into() } else if conflict { conflict_values(&op, &op.mine(), fmt) } else { String::new() },
                    theirs: op.remote.as_ref().filter(|_| conflict).map(|r| if op.removed { remote_text(op.kind, r, op.episodes, fmt) } else { conflict_values(&op, &r.values, fmt) }),
                    theirs_entry: op.remote.as_ref().filter(|_| conflict).map(|r| remote_text(op.kind, r, op.episodes, fmt)),
                    theirs_changed_at: op.remote.as_ref().filter(|_| conflict).map(|r| r.updated_at),
                });
            }
        }
        out
    }

    /// Conflicts waiting for a pick, all accounts.
    pub fn conflict_count(&self) -> u32 {
        let now = time::now_unix();
        let store = self.store();
        let mirror_to = store.mirror_link().ok().flatten().map(|l| l.to.name());
        Service::ALL.into_iter().filter(|s| self.signed_in(*s) && Some(s.name()) != mirror_to).map(|s| store.queue(s.name(), now).unwrap_or_default().iter().filter(|o| o.state == OpState::Conflict).count() as u32).sum()
    }

    /// A conflict, "Keep mine" ("Remove anyway"): ours goes over the service's version.
    /// `service` is the change's account (the ids are one sequence; kept for symmetry).
    pub fn keep_mine(&self, service: Service, id: i64) -> Result<(), KotoraError> {
        let _ = service;
        Ok(self.store().keep_mine(id)?)
    }

    /// A conflict, "Take theirs" ("Keep it", "Remove here too"): the entry takes the service's
    /// values.
    pub fn take_theirs(&self, service: Service, id: i64) -> Result<(), KotoraError> {
        Ok(self.store().take_theirs(service.name(), id, time::now_unix())?)
    }

    /// Discard a waiting or failed change (Undo of a removal): the entry goes back to where it was.
    pub fn discard(&self, service: Service, id: i64) -> Result<(), KotoraError> {
        Ok(self.store().discard(service.name(), id, time::now_unix())?)
    }

    /// "Try now": waiting changes may go at once (the app then flushes).
    pub fn retry_now(&self) -> Result<(), KotoraError> {
        let store = self.store();
        for s in Service::ALL.into_iter().filter(|s| self.signed_in(*s)) {
            store.retry_now(s.name())?;
        }
        Ok(())
    }
}
