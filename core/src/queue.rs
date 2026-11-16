//! The sync queue: local edits wait here until the service has them.
//!
//! - **Local first:** [`Store::enqueue`] changes the saved entry (the list shows it at once) and
//!   queues the change, in one transaction.
//! - **Coalescing:** while a change waits, a new edit of the same entry merges into it (progress
//!   3→4→5 is one "progress 5"); an edit back to the old value removes it.
//! - **Worker:** [`step`] sends the oldest due change. It first reads the service's entry: if that
//!   changed after our base (`updated_at`), it is a conflict and nothing is overwritten. A change
//!   that only raises the progress wins on its own. Then it sends (`PATCH my_list_status` on MAL)
//!   and saves what the service answered.
//! - **Adding:** [`Store::add_to_list`] puts an anime on the saved list and queues it with the
//!   `added` mark: the service not having it is then no conflict, and Discard takes it off again.
//! - **Errors:** no network or rate limit: try again later (30 s doubling to 15 min). The sign-in
//!   is gone: [`Step::SignInAgain`], the queue is kept. Anything else: `failed`, with the
//!   service's words, until the user edits or discards it.
//!
//! - **Kinds:** anime and manga changes share the queue (one worker, one badge, one page);
//!   each change carries its list's kind, and the worker sends it with that kind's calls.
//!
//! No threads and no clock here: the caller passes `now` and does any waiting, so tests drive it.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::service::{ListService, ServiceError};
use crate::store::{Result, Store, StoreError};
use crate::{Change, Kind, Media, Remote, Scope, Score, ScoreFormat, Status, Values};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpState {
    Queued,
    Sending,
    Conflict,
    Failed,
    /// Sent (kept a day, for the queue page's Sent list).
    Done,
}

impl OpState {
    fn name(self) -> &'static str {
        match self {
            OpState::Queued => "queued",
            OpState::Sending => "sending",
            OpState::Conflict => "conflict",
            OpState::Failed => "failed",
            OpState::Done => "done",
        }
    }

    fn from_name(s: &str) -> OpState {
        match s {
            "sending" => OpState::Sending,
            "conflict" => OpState::Conflict,
            "failed" => OpState::Failed,
            "done" => OpState::Done,
            _ => OpState::Queued,
        }
    }

    /// The word the queue page and `kot queue` show.
    pub fn label(self) -> &'static str {
        match self {
            OpState::Queued => "Waiting",
            OpState::Sending => "Sending",
            OpState::Conflict => "Conflict",
            OpState::Failed => "Failed",
            OpState::Done => "Sent",
        }
    }

    /// Still in the queue (counted in the badge).
    pub fn is_open(self) -> bool {
        self != OpState::Done
    }
}

/// One change in the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub id: i64,
    /// The list the entry is on.
    pub kind: Kind,
    pub media_id: u32,
    pub title: String,
    pub episodes: Option<u32>,
    pub change: Change,
    /// The entry before the first edit.
    pub old: Values,
    /// The service's `updated_at` the edit started from. -1: send whatever the service has
    /// ("Keep mine" on an entry removed there).
    pub base_updated_at: i64,
    pub created_at: i64,
    pub edited_at: i64,
    /// How many edits merged into this change.
    pub edits: u32,
    pub state: OpState,
    pub attempts: u32,
    pub next_try: i64,
    pub error: Option<String>,
    pub sent_at: Option<i64>,
    /// In a conflict: the service's entry; `None` when it was removed there.
    pub remote: Option<Remote>,
    /// The change puts the anime on the list (it wasn't there).
    pub added: bool,
}

impl Op {
    /// The values this change wants.
    pub fn mine(&self) -> Values {
        self.change.apply(self.old)
    }
}

/// The change in a few words: "Progress 3 → 5", "Status → Completed, score 8", "Score removed".
pub fn describe(change: &Change, old: &Values) -> String {
    describe_in(Kind::Anime, change, old)
}

/// [`describe`] for an entry of a list of `kind`: a manga's change says "chapters", "volumes",
/// "reread" and its status names ("Status → Reading").
pub fn describe_in(kind: Kind, change: &Change, old: &Values) -> String {
    describe_with(kind, ScoreFormat::Point10, change, old)
}

/// [`describe_in`] with the scores in the user's format ("score 7.3 → 8.0", "score 4 stars").
pub fn describe_with(kind: Kind, fmt: ScoreFormat, change: &Change, old: &Values) -> String {
    let manga = kind == Kind::Manga;
    let re = if manga { "reread" } else { "rewatch" };
    let mut parts = Vec::new();
    if let Some(st) = change.status {
        parts.push(format!("status \u{2192} {}", st.label_in(kind)));
    }
    if let Some(p) = change.progress {
        parts.push(format!("{} {} \u{2192} {p}", if manga { "chapters" } else { "progress" }, old.progress));
    }
    if let Some(v) = change.volumes {
        parts.push(format!("volumes {} \u{2192} {v}", old.volumes));
    }
    match change.score {
        Some(Some(n)) => parts.push(match old.score {
            Some(o) => format!("score {} \u{2192} {}", fmt.words(o), fmt.words(n)),
            None => format!("score {}", fmt.words(n)),
        }),
        Some(None) => parts.push("score removed".into()),
        None => {}
    }
    match (change.rewatching, change.times_rewatched) {
        (Some(true), _) => parts.push(format!("{re} started")),
        (Some(false), Some(n)) if n > old.times_rewatched => parts.push(format!("{re} finished ({})", times(n))),
        (Some(false), _) => parts.push(format!("{re} stopped")),
        (None, Some(n)) => parts.push(format!("{re}{} {}", if manga { "" } else { "ed" }, times(n))),
        (None, None) => {}
    }
    let mut text = parts.join(", ");
    if let Some(first) = text.get(..1) {
        text = first.to_uppercase() + &text[1..];
    }
    text
}

/// "once", "twice", "3 times".
pub fn times(n: u32) -> String {
    match n {
        1 => "once".into(),
        2 => "twice".into(),
        n => format!("{n} times"),
    }
}

/// [`describe`] for a queued change: an add says so ("Add as Watching, progress 12").
pub fn describe_op(op: &Op) -> String {
    describe_op_with(op, ScoreFormat::Point10)
}

/// [`describe_op`] with the scores in the user's format.
pub fn describe_op_with(op: &Op, fmt: ScoreFormat) -> String {
    if !op.added {
        return describe_with(op.kind, fmt, &op.change, &op.old);
    }
    let status = op.change.status.unwrap_or(op.old.status);
    let mut text = format!("Add as {}", status.label_in(op.kind));
    if let Some(p) = op.change.progress.filter(|p| *p > 0) {
        text.push_str(&format!(", {} {p}", if op.kind == Kind::Manga { "chapter" } else { "progress" }));
    }
    if let Some(v) = op.change.volumes.filter(|v| *v > 0) {
        text.push_str(&format!(", volume {v}"));
    }
    if let Some(Some(n)) = op.change.score {
        text.push_str(&format!(", score {}", fmt.words(n)));
    }
    text
}

/// Sent changes stay this long on the queue page.
const KEEP_DONE: i64 = 86_400;
/// The longest wait between tries.
pub const MAX_BACKOFF: i64 = 900;

/// The wait before try number `attempts + 1`: 30 s, 1, 2, 4, 8 min, then 15 min.
pub fn backoff(attempts: u32) -> i64 {
    (30i64 << attempts.saturating_sub(1).min(5)).min(MAX_BACKOFF)
}

// ------------------------------------------------------------------------------------ encoding

/// The `score` column: NULL unchanged, 0 removes, 1-100 points.
fn score_out(s: Option<Option<Score>>) -> Option<u8> {
    s.map(Score::points_or_zero)
}

fn score_in(n: Option<i64>) -> Option<Option<Score>> {
    n.map(|n| Score::from_sql_points(Some(n)))
}

fn status_in(s: Option<String>) -> Option<Status> {
    s.as_deref().and_then(Status::from_mal)
}

const OP_SELECT: &str = "SELECT o.id, o.media_id, COALESCE(m.title, ''), m.episodes, o.status, o.progress, o.score,
        o.old_status, o.old_progress, o.old_score, o.base_updated_at, o.created_at, o.edited_at, o.edits,
        o.state, o.attempts, o.next_try, o.error, o.sent_at, o.r_status, o.r_progress, o.r_score, o.r_updated_at, o.added,
        o.rewatching, o.times_rewatched, o.old_rewatching, o.old_times_rewatched, o.r_rewatching, o.r_times_rewatched,
        o.kind, o.volumes, o.old_volumes, o.r_volumes
 FROM pending_op o LEFT JOIN media m ON m.service = o.service AND m.kind = o.kind AND m.id = o.media_id";

fn op_row(r: &Row) -> rusqlite::Result<Op> {
    let state = OpState::from_name(&r.get::<_, String>(14)?);
    let r_updated: Option<i64> = r.get(22)?;
    let remote = match (status_in(r.get(19)?), r_updated) {
        (Some(status), Some(updated_at)) => Some(Remote {
            values: Values {
                status,
                progress: r.get(20)?,
                score: score_in(r.get(21)?).flatten(),
                rewatching: r.get::<_, Option<bool>>(28)?.unwrap_or(false),
                times_rewatched: r.get::<_, Option<u32>>(29)?.unwrap_or(0),
                volumes: r.get::<_, Option<u32>>(33)?.unwrap_or(0),
            },
            updated_at,
        }),
        _ => None,
    };
    let media_id: u32 = r.get(1)?;
    let title: String = r.get(2)?;
    let kind = Kind::from_name(&r.get::<_, String>(30)?);
    Ok(Op {
        id: r.get(0)?,
        kind,
        media_id,
        title: if title.is_empty() { format!("{} {media_id}", kind.label()) } else { title },
        episodes: r.get(3)?,
        change: Change { status: status_in(r.get(4)?), progress: r.get(5)?, score: score_in(r.get(6)?), rewatching: r.get(24)?, times_rewatched: r.get(25)?, volumes: r.get(31)? },
        old: Values {
            status: status_in(r.get(7)?).unwrap_or(Status::PlanToWatch),
            progress: r.get(8)?,
            score: score_in(r.get(9)?).flatten(),
            rewatching: r.get(26)?,
            times_rewatched: r.get(27)?,
            volumes: r.get(32)?,
        },
        base_updated_at: r.get(10)?,
        created_at: r.get(11)?,
        edited_at: r.get(12)?,
        edits: r.get(13)?,
        state,
        attempts: r.get(15)?,
        next_try: r.get(16)?,
        error: r.get(17)?,
        sent_at: r.get(18)?,
        remote,
        added: r.get(23)?,
    })
}

fn entry_values(c: &Connection, sc: Scope, media_id: u32) -> Result<Option<(Values, i64)>> {
    Ok(c.query_row(
        "SELECT status, progress, score, updated_at, rewatching, times_rewatched, volumes FROM entry WHERE service = ?1 AND kind = ?3 AND media_id = ?2",
        params![sc.service, media_id, sc.kind.name()],
        |r| {
            let st: String = r.get(0)?;
            let v = Values {
                status: Status::from_mal(&st).unwrap_or(Status::PlanToWatch),
                progress: r.get(1)?,
                score: r.get(2)?,
                rewatching: r.get(4)?,
                times_rewatched: r.get(5)?,
                volumes: r.get(6)?,
            };
            Ok((v, r.get(3)?))
        },
    )
    .optional()?)
}

fn set_entry(c: &Connection, sc: Scope, media_id: u32, v: &Values, updated_at: Option<i64>) -> Result<()> {
    c.execute(
        "UPDATE entry SET status = ?3, progress = ?4, score = ?5, rewatching = ?7, times_rewatched = ?8, volumes = ?9, updated_at = COALESCE(?6, updated_at)
         WHERE service = ?1 AND kind = ?10 AND media_id = ?2",
        params![sc.service, media_id, v.status.mal_name(), v.progress, v.score, updated_at, v.rewatching, v.times_rewatched, v.volumes, sc.kind.name()],
    )?;
    Ok(())
}

fn get_op(c: &Connection, id: i64) -> Result<Op> {
    c.query_row(&format!("{OP_SELECT} WHERE o.id = ?1"), [id], op_row).optional()?.ok_or_else(|| StoreError("that change is no longer in the queue".into()))
}

fn has_other_open(c: &Connection, sc: Scope, media_id: u32, not: i64) -> Result<bool> {
    let n: i64 = c.query_row(
        "SELECT COUNT(*) FROM pending_op WHERE service = ?1 AND kind = ?4 AND media_id = ?2 AND id != ?3 AND state != 'done'",
        params![sc.service, media_id, not, sc.kind.name()],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Lays the open changes of one list (or of one entry) over the saved entries: after a full
/// sync, and after one change is settled while others for the same entry still wait.
pub(crate) fn lay_over(c: &Connection, sc: Scope) -> Result<()> {
    lay_over_where(c, sc, None)
}

fn lay_over_where(c: &Connection, sc: Scope, media_id: Option<u32>) -> Result<()> {
    // An anime added here and not yet on the service is not in a fetched list: put it back.
    c.execute(
        "INSERT INTO entry (service, kind, media_id, status, progress, score, rewatching, updated_at)
         SELECT o.service, o.kind, o.media_id, o.old_status, 0, NULL, 0, o.created_at FROM pending_op o
         WHERE o.service = ?1 AND o.kind = ?3 AND (?2 IS NULL OR o.media_id = ?2) AND o.added = 1 AND o.state != 'done'
           AND o.media_id IN (SELECT id FROM media WHERE service = ?1 AND kind = ?3)
           AND NOT EXISTS (SELECT 1 FROM entry e WHERE e.service = o.service AND e.kind = o.kind AND e.media_id = o.media_id)
         GROUP BY o.media_id",
        params![sc.service, media_id, sc.kind.name()],
    )?;
    c.execute(
        "UPDATE entry SET
            status = COALESCE((SELECT o.status FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.status IS NOT NULL ORDER BY o.id DESC LIMIT 1), status),
            progress = COALESCE((SELECT o.progress FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.progress IS NOT NULL ORDER BY o.id DESC LIMIT 1), progress),
            score = COALESCE((SELECT NULLIF(o.score, 0) FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.score IS NOT NULL ORDER BY o.id DESC LIMIT 1),
                             CASE WHEN (SELECT o.score FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.score IS NOT NULL ORDER BY o.id DESC LIMIT 1) = 0 THEN NULL ELSE score END),
            rewatching = COALESCE((SELECT o.rewatching FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.rewatching IS NOT NULL ORDER BY o.id DESC LIMIT 1), rewatching),
            times_rewatched = COALESCE((SELECT o.times_rewatched FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.times_rewatched IS NOT NULL ORDER BY o.id DESC LIMIT 1), times_rewatched),
            volumes = COALESCE((SELECT o.volumes FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id
                               AND o.state != 'done' AND o.volumes IS NOT NULL ORDER BY o.id DESC LIMIT 1), volumes),
            updated_at = MAX(updated_at, COALESCE((SELECT MAX(o.edited_at) FROM pending_op o WHERE o.service = entry.service
                               AND o.kind = entry.kind AND o.media_id = entry.media_id AND o.state != 'done'), 0))
         WHERE service = ?1 AND kind = ?3 AND (?2 IS NULL OR media_id = ?2)
           AND EXISTS (SELECT 1 FROM pending_op o WHERE o.service = entry.service AND o.kind = entry.kind AND o.media_id = entry.media_id AND o.state != 'done')",
        params![sc.service, media_id, sc.kind.name()],
    )?;
    Ok(())
}

// --------------------------------------------------------------------------------------- store

impl Store {
    /// An edit of an entry of `scope` (a service name: its anime list): the saved entry changes
    /// at once and the change is queued, in one transaction. Merges into a change that still
    /// waits for the same entry. Returns the queued change's id, or `None` when nothing is left
    /// to send (the edit put every field back).
    pub fn enqueue<'a>(&mut self, scope: impl Into<Scope<'a>>, media_id: u32, change: Change, now: i64) -> Result<Option<i64>> {
        let sc = scope.into();
        let tx = self.conn.transaction()?;
        let id = enqueue_tx(&tx, sc, media_id, change, now)?;
        tx.commit()?;
        Ok(id)
    }

    /// Open changes of both kinds first (oldest first), then what was sent in the last day
    /// (newest first).
    pub fn queue(&self, service: &str, now: i64) -> Result<Vec<Op>> {
        let mut st = self.conn.prepare(&format!(
            "{OP_SELECT} WHERE o.service = ?1 AND (o.state != 'done' OR o.sent_at >= ?2)
             ORDER BY o.state = 'done', CASE WHEN o.state = 'done' THEN -o.sent_at ELSE o.id END"
        ))?;
        let rows = st.query_map(params![service, now - KEEP_DONE], op_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Changes still in the queue, both kinds (the badge).
    pub fn open_count(&self, service: &str) -> Result<u32> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM pending_op WHERE service = ?1 AND state != 'done'", [service], |r| r.get(0))?)
    }

    /// The oldest waiting change that may go now, of either kind.
    pub fn next_due(&self, service: &str, now: i64) -> Result<Option<Op>> {
        Ok(self
            .conn
            .query_row(&format!("{OP_SELECT} WHERE o.service = ?1 AND o.state = 'queued' AND o.next_try <= ?2 ORDER BY o.id LIMIT 1"), params![service, now], op_row)
            .optional()?)
    }

    /// Seconds until the next waiting change may go; `None`: nothing waits.
    pub fn next_wait(&self, service: &str, now: i64) -> Result<Option<i64>> {
        let t: Option<i64> = self.conn.query_row("SELECT MIN(next_try) FROM pending_op WHERE service = ?1 AND state = 'queued'", [service], |r| r.get(0))?;
        Ok(t.map(|t| (t - now).max(0)))
    }

    /// At start: a change caught mid-send by a crash or a quit waits again (sending it twice is
    /// safe: the second try finds the service already holds it).
    /// Also forgets sent changes older than a day.
    pub fn queue_start(&self, service: &str, now: i64) -> Result<()> {
        self.conn.execute("UPDATE pending_op SET state = 'queued' WHERE service = ?1 AND state = 'sending'", [service])?;
        self.conn.execute("DELETE FROM pending_op WHERE service = ?1 AND state = 'done' AND sent_at < ?2", params![service, now - KEEP_DONE])?;
        Ok(())
    }

    /// "Sync now" / `kot sync`: waiting changes may go at once.
    pub fn retry_now(&self, service: &str) -> Result<()> {
        self.conn.execute("UPDATE pending_op SET next_try = 0 WHERE service = ?1 AND state = 'queued'", [service])?;
        Ok(())
    }

    fn set_state(&self, id: i64, state: OpState) -> Result<()> {
        self.conn.execute("UPDATE pending_op SET state = ?2 WHERE id = ?1", params![id, state.name()])?;
        Ok(())
    }

    fn mark_retry(&self, id: i64, attempts: u32, next_try: i64, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pending_op SET state = 'queued', attempts = ?2, next_try = ?3, error = ?4 WHERE id = ?1",
            params![id, attempts, next_try, error],
        )?;
        Ok(())
    }

    fn mark_failed(&self, id: i64, error: &str) -> Result<()> {
        self.conn.execute("UPDATE pending_op SET state = 'failed', error = ?2 WHERE id = ?1", params![id, error])?;
        Ok(())
    }

    fn mark_conflict(&self, id: i64, remote: Option<&Remote>) -> Result<()> {
        self.conn.execute(
            "UPDATE pending_op SET state = 'conflict', error = NULL, r_status = ?2, r_progress = ?3, r_score = ?4, r_updated_at = ?5,
                r_rewatching = ?6, r_times_rewatched = ?7, r_volumes = ?8 WHERE id = ?1",
            params![
                id,
                remote.map(|r| r.values.status.mal_name()),
                remote.map(|r| r.values.progress),
                remote.map(|r| Score::points_or_zero(r.values.score)),
                remote.map(|r| r.updated_at),
                remote.map(|r| r.values.rewatching),
                remote.map(|r| r.values.times_rewatched),
                remote.map(|r| r.values.volumes)
            ],
        )?;
        Ok(())
    }

    /// The service holds the change now: the entry takes the service's answer, and changes that
    /// wait behind it for the same entry start from the new `updated_at`.
    fn mark_sent(&mut self, service: &str, op: &Op, remote: &Remote, now: i64) -> Result<()> {
        let sc = Scope::new(service, op.kind);
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE pending_op SET state = 'done', sent_at = ?2, error = NULL WHERE id = ?1", params![op.id, now])?;
        tx.execute(
            "UPDATE pending_op SET base_updated_at = ?3 WHERE service = ?1 AND kind = ?5 AND media_id = ?2 AND state != 'done' AND id > ?4",
            params![service, op.media_id, remote.updated_at, op.id, op.kind.name()],
        )?;
        set_entry(&tx, sc, op.media_id, &remote.values, Some(remote.updated_at))?;
        lay_over_where(&tx, sc, Some(op.media_id))?;
        tx.commit()?;
        Ok(())
    }

    /// A conflict, "Keep mine": send ours over the service's version.
    pub fn keep_mine(&self, id: i64) -> Result<()> {
        let op = get_op(&self.conn, id)?;
        let base = op.remote.map_or(-1, |r| r.updated_at);
        self.conn.execute(
            "UPDATE pending_op SET state = 'queued', base_updated_at = ?2, attempts = 0, next_try = 0, error = NULL WHERE id = ?1",
            params![id, base],
        )?;
        Ok(())
    }

    /// A conflict, "Take theirs": the change goes and the entry takes the service's values (or
    /// leaves the list, if it was removed there).
    pub fn take_theirs(&mut self, service: &str, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let op = get_op(&tx, id)?;
        let sc = Scope::new(service, op.kind);
        tx.execute("DELETE FROM pending_op WHERE id = ?1", [id])?;
        match op.remote {
            Some(r) => set_entry(&tx, sc, op.media_id, &r.values, Some(r.updated_at))?,
            None if !has_other_open(&tx, sc, op.media_id, id)? => {
                tx.execute("DELETE FROM entry WHERE service = ?1 AND kind = ?3 AND media_id = ?2", params![service, op.media_id, op.kind.name()])?;
            }
            None => {}
        }
        lay_over_where(&tx, sc, Some(op.media_id))?;
        tx.commit()?;
        Ok(())
    }

    /// Discard a change (a failed one, or any open one): the entry goes back to where it was.
    pub fn discard(&mut self, service: &str, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let op = get_op(&tx, id)?;
        if op.state == OpState::Sending {
            return Err(StoreError("that change is being sent right now".into()));
        }
        let sc = Scope::new(service, op.kind);
        tx.execute("DELETE FROM pending_op WHERE id = ?1", [id])?;
        let alone = !has_other_open(&tx, sc, op.media_id, id)?;
        if op.added && alone {
            // It was never on the list: it leaves again.
            tx.execute("DELETE FROM entry WHERE service = ?1 AND kind = ?3 AND media_id = ?2", params![service, op.media_id, op.kind.name()])?;
        } else {
            set_entry(&tx, sc, op.media_id, &op.old, alone.then_some(op.base_updated_at).filter(|b| *b >= 0))?;
        }
        lay_over_where(&tx, sc, Some(op.media_id))?;
        tx.commit()?;
        Ok(())
    }

    /// Search and add: puts `media` on the saved list with `status` and queues the add, in one
    /// transaction. Completed starts at the last episode (when the count is known), anything else
    /// at `progress` (Now Playing's "Add as Watching at episode N": add and record in one change)
    /// or 0. An anime already on the list gets a status (and progress) change instead. Returns the queued
    /// change's id (`None`: nothing to send). The list is `media.kind`'s; a manga added as
    /// Completed also starts at the last volume (when known).
    pub fn add_to_list(&mut self, service: &str, media: &Media, status: Status, progress: Option<u32>, now: i64) -> Result<Option<i64>> {
        let sc = Scope::new(service, media.kind);
        if entry_values(&self.conn, sc, media.id)?.is_some() {
            return self.enqueue(sc, media.id, Change { status: Some(status), progress, ..Default::default() }, now);
        }
        let volumes = match (media.kind, status, media.volumes) {
            (Kind::Manga, Status::Completed, Some(n)) => n,
            _ => 0,
        };
        let progress = match (status, media.episodes, progress) {
            (Status::Completed, Some(n), _) => n,
            (_, Some(n), Some(p)) => p.min(n),
            (_, None, Some(p)) => p,
            _ => 0,
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO media (service, id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at,
               kind, volumes, authors, start_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (service, kind, id) DO UPDATE SET title = excluded.title, title_en = excluded.title_en,
               title_ja = excluded.title_ja, synonyms = excluded.synonyms, episodes = excluded.episodes,
               format = excluded.format, air_status = excluded.air_status,
               cover_url = COALESCE(excluded.cover_url, media.cover_url), fetched_at = excluded.fetched_at,
               volumes = excluded.volumes, authors = excluded.authors, start_date = COALESCE(excluded.start_date, media.start_date)",
            params![
                service,
                media.id,
                media.title,
                media.title_en,
                media.title_ja,
                serde_json::to_string(&media.synonyms).unwrap_or_else(|_| "[]".into()),
                media.episodes,
                media.format,
                media.air_status,
                media.cover_url,
                now,
                sc.kind.name(),
                media.volumes,
                serde_json::to_string(&media.authors).unwrap_or_else(|_| "[]".into()),
                if media.kind == Kind::Manga { media.start_date.clone() } else { None }
            ],
        )?;
        tx.execute(
            "INSERT INTO entry (service, kind, media_id, status, progress, score, rewatching, updated_at, volumes) VALUES (?1, ?6, ?2, ?3, ?4, NULL, 0, ?5, ?7)",
            params![service, media.id, status.mal_name(), progress, now, sc.kind.name(), volumes],
        )?;
        // `old_*` hold the added status and 0: the change always sends the status.
        tx.execute(
            "INSERT INTO pending_op (service, kind, media_id, status, progress, score, old_status, old_progress, old_score,
                base_updated_at, created_at, edited_at, added, volumes)
             VALUES (?1, ?6, ?2, ?3, ?4, NULL, ?3, 0, 0, 0, ?5, ?5, 1, ?7)",
            params![service, media.id, status.mal_name(), (progress > 0).then_some(progress), now, sc.kind.name(), (volumes > 0).then_some(volumes)],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Some(id))
    }
}

fn enqueue_tx(tx: &Connection, sc: Scope, media_id: u32, change: Change, now: i64) -> Result<Option<i64>> {
    let service = sc.service;
    let k = sc.kind.name();
    let (cur, cur_updated) = entry_values(tx, sc, media_id)?
        .ok_or_else(|| StoreError(format!("that {} is not on your saved list", if sc.kind == Kind::Manga { "manga" } else { "anime" })))?;
    // A change that waits (not one being sent) takes the edit.
    let waiting: Option<Op> = tx
        .query_row(
            &format!(
                "{OP_SELECT} WHERE o.service = ?1 AND o.kind = ?3 AND o.media_id = ?2 AND o.state IN ('queued', 'conflict', 'failed') ORDER BY o.id DESC LIMIT 1"
            ),
            params![service, media_id, k],
            op_row,
        )
        .optional()?;
    let id = match waiting {
        Some(op) => {
            let mut merged = op.change.merge(change).without_same(&op.old);
            if op.added {
                // An add always sends a status (the service has no entry to keep one from).
                merged.status = Some(op.change.merge(change).status.unwrap_or(op.old.status));
            }
            if merged.is_empty() {
                // Back where it started: nothing to send.
                tx.execute("DELETE FROM pending_op WHERE id = ?1", [op.id])?;
                let alone = !has_other_open(tx, sc, media_id, op.id)?;
                set_entry(tx, sc, media_id, &op.old, alone.then_some(op.base_updated_at).filter(|b| *b >= 0))?;
                return Ok(None);
            }
            // A failed change edited again gets a fresh start; a conflict stays a conflict.
            tx.execute(
                "UPDATE pending_op SET status = ?2, progress = ?3, score = ?4, rewatching = ?6, times_rewatched = ?7, volumes = ?8, edits = edits + 1, edited_at = ?5,
                    attempts = CASE WHEN state = 'failed' THEN 0 ELSE attempts END,
                    next_try = CASE WHEN state = 'failed' THEN 0 ELSE next_try END,
                    error = CASE WHEN state = 'conflict' THEN error ELSE NULL END,
                    state = CASE WHEN state = 'failed' THEN 'queued' ELSE state END
                 WHERE id = ?1",
                params![
                    op.id,
                    merged.status.map(Status::mal_name),
                    merged.progress,
                    score_out(merged.score),
                    now,
                    merged.rewatching,
                    merged.times_rewatched,
                    merged.volumes
                ],
            )?;
            op.id
        }
        None => {
            let c = change.without_same(&cur);
            if c.is_empty() {
                return Ok(None);
            }
            // A change for this entry is on its way: start from the same service state.
            let sending: Option<i64> = tx
                .query_row(
                    "SELECT base_updated_at FROM pending_op WHERE service = ?1 AND kind = ?3 AND media_id = ?2 AND state = 'sending' ORDER BY id DESC LIMIT 1",
                    params![service, media_id, k],
                    |r| r.get(0),
                )
                .optional()?;
            tx.execute(
                "INSERT INTO pending_op (service, media_id, status, progress, score, old_status, old_progress, old_score,
                    base_updated_at, created_at, edited_at, rewatching, times_rewatched, old_rewatching, old_times_rewatched,
                    kind, volumes, old_volumes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    service,
                    media_id,
                    c.status.map(Status::mal_name),
                    c.progress,
                    score_out(c.score),
                    cur.status.mal_name(),
                    cur.progress,
                    Score::points_or_zero(cur.score),
                    sending.unwrap_or(cur_updated),
                    now,
                    c.rewatching,
                    c.times_rewatched,
                    cur.rewatching,
                    cur.times_rewatched,
                    k,
                    c.volumes,
                    cur.volumes
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    set_entry(tx, sc, media_id, &change.apply(cur), Some(now))?;
    Ok(Some(id))
}

// -------------------------------------------------------------------------------------- worker

/// What one [`step`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing waits.
    Idle,
    /// The next change may go in this many seconds (a retry after an error).
    Wait(i64),
    /// One change was handled; its entry (of list `kind`) may have changed.
    Handled { kind: Kind, media_id: u32, outcome: Outcome },
    /// The service wants a new sign-in. The change waits again; the queue is kept.
    SignInAgain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The service already had these values (from another device, or a send we never heard back
    /// from).
    AlreadyThere,
    Conflict,
    Failed(String),
    /// No network or rate limited: tried again later.
    Retry(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Send,
    AlreadyThere(Remote),
    Conflict,
}

/// What to do with `op`, given the service's entry now. `keep_higher`: a conflict on the counts
/// only (progress, and a manga's volumes) where ours are higher sends ours (Settings → Updates,
/// on by default). Higher means: no count below the service's, at least one above.
pub fn decide(op: &Op, remote: Option<&Remote>, keep_higher: bool) -> Decision {
    match remote {
        None if op.base_updated_at < 0 || op.added => Decision::Send,
        // Removed on the service after our edit: ask.
        None => Decision::Conflict,
        Some(r) if op.base_updated_at < 0 || r.updated_at <= op.base_updated_at => Decision::Send,
        Some(r) if op.change.is_in(&r.values) => Decision::AlreadyThere(*r),
        Some(r) => {
            // Only the counts, and ours are higher: the higher numbers win.
            let c = &op.change;
            let not_lower = c.progress.is_none_or(|p| p >= r.values.progress) && c.volumes.is_none_or(|v| v >= r.values.volumes);
            let above = c.progress.is_some_and(|p| p > r.values.progress) || c.volumes.is_some_and(|v| v > r.values.volumes);
            if keep_higher && c.is_count_only() && not_lower && above { Decision::Send } else { Decision::Conflict }
        }
    }
}

/// Sends the oldest due change of `service`, if any. One service request or two (read, then
/// write); the service's own rate limit spaces them. A higher progress wins a progress-only
/// conflict (the default; [`step_with`] can turn that off).
pub fn step(store: &mut Store, service: &mut dyn ListService, now: i64) -> Result<Step> {
    step_with(store, service, now, true)
}

/// [`step`] with the "keep the higher progress" setting.
pub fn step_with(store: &mut Store, service: &mut dyn ListService, now: i64, keep_higher: bool) -> Result<Step> {
    let name = service.name();
    let Some(op) = store.next_due(name, now)? else {
        return Ok(match store.next_wait(name, now)? {
            Some(w) => Step::Wait(w.max(1)),
            None => Step::Idle,
        });
    };
    store.set_state(op.id, OpState::Sending)?;
    let remote = match service.get_entry_of(op.kind, op.media_id) {
        Ok(r) => r,
        Err(e) => return give_back(store, &op, e, now),
    };
    let outcome = match decide(&op, remote.as_ref(), keep_higher) {
        Decision::Send => match service.update_entry_of(op.kind, op.media_id, &op.change) {
            Ok(r) => {
                store.mark_sent(name, &op, &r, now)?;
                Outcome::Sent
            }
            Err(e) => return give_back(store, &op, e, now),
        },
        Decision::AlreadyThere(r) => {
            store.mark_sent(name, &op, &r, now)?;
            Outcome::AlreadyThere
        }
        Decision::Conflict => {
            store.mark_conflict(op.id, remote.as_ref())?;
            Outcome::Conflict
        }
    };
    Ok(Step::Handled { kind: op.kind, media_id: op.media_id, outcome })
}

fn give_back(store: &Store, op: &Op, e: ServiceError, now: i64) -> Result<Step> {
    let outcome = match e {
        ServiceError::SignInAgain => {
            store.set_state(op.id, OpState::Queued)?;
            return Ok(Step::SignInAgain);
        }
        ServiceError::Offline(_) | ServiceError::RateLimited => {
            let attempts = op.attempts + 1;
            let msg = e.to_string();
            store.mark_retry(op.id, attempts, now + backoff(attempts), &msg)?;
            Outcome::Retry(msg)
        }
        ServiceError::Other(m) => {
            store.mark_failed(op.id, &m)?;
            Outcome::Failed(m)
        }
    };
    Ok(Step::Handled { kind: op.kind, media_id: op.media_id, outcome })
}

#[cfg(test)]
mod tests;
