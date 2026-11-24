//! The activity log (0.4.5): what the user watched and read in Kotora, and when. One row per
//! edit that raised the progress (episodes, chapters) or the volumes of an entry on the main
//! list, written by the store calls that change it (`queue.rs`), in their transaction. The
//! Statistics page and `kot stats` count from it (`stats.rs`).
//!
//! Kotora's own record of the user's own edits, kept on this computer: not service data, so
//! signing out keeps it (as the library folders). Nothing is read from the service to fill it:
//! website edits and imports are not activity, and activity from before 0.4.5 is not known
//! (kv `activity:since` says when the log started).
//!
//! The rules:
//! - Progress or volumes up: one row, `from` to `to`. An add at episode N (Now Playing's "Add
//!   as Watching at episode N") counts that one episode. An add as Completed, and Plan to watch
//!   to Completed in one step, are a title watched before, not now: no row.
//! - Progress down (an Undo, a Discard, a correction): the entry's rows of the last two days give
//!   back what is above the new progress. A rewatch that starts (progress back to 0) gives back
//!   nothing, and a rewatch stopped before its end (an Undo of its start) logs nothing.

use rusqlite::{Connection, OptionalExtension, params};

use crate::store::Result;
use crate::{Scope, Status, Values};

/// How far back a lowered progress takes back activity (an Undo comes within minutes; a later
/// correction is the user fixing the count, not taking back a watch).
const TAKE_BACK: i64 = 2 * 86_400;

/// kv: when the log started (unix seconds), written when the table is created (`store.rs`
/// `ADDITIVE`) and kept at the earliest row.
pub const SINCE_KEY: &str = "activity:since";

/// One logged edit: the progress (and volumes) before and after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub media_id: u32,
    pub at: i64,
    pub from: u32,
    pub to: u32,
    pub vol_from: u32,
    pub vol_to: u32,
}

impl Row {
    /// Episodes or chapters this edit added.
    pub fn units(&self) -> u32 {
        self.to.saturating_sub(self.from)
    }

    /// Volumes this edit added.
    pub fn volumes(&self) -> u32 {
        self.vol_to.saturating_sub(self.vol_from)
    }
}

/// Called inside every store call that changes a main entry (`queue.rs`), after the change.
/// `before`: the entry before the call (`None`: it wasn't on the list).
pub(crate) fn follow(tx: &Connection, sc: Scope, media_id: u32, before: Option<Values>, now: i64) -> Result<()> {
    follow_with(tx, sc, media_id, before, now, true)
}

/// As [`follow`], but only a lowered progress counts (Take theirs: the website's count won; a
/// higher one there was watched elsewhere, at a time Kotora doesn't know).
pub(crate) fn follow_down(tx: &Connection, sc: Scope, media_id: u32, before: Option<Values>, now: i64) -> Result<()> {
    follow_with(tx, sc, media_id, before, now, false)
}

fn follow_with(tx: &Connection, sc: Scope, media_id: u32, before: Option<Values>, now: i64, ups: bool) -> Result<()> {
    if crate::mirror::is_mirror(tx, sc.service)? {
        return Ok(());
    }
    let Some((after, _)) = crate::queue::entry_values(tx, sc, media_id)? else { return Ok(()) };
    let (from, vol_from) = match before {
        // Added as Completed: watched before. Added at episode N: that one episode.
        None if after.status == Status::Completed => return Ok(()),
        None => (after.progress.saturating_sub(1), after.volumes.saturating_sub(1)),
        Some(b) if b.status == Status::PlanToWatch && after.status == Status::Completed => return Ok(()),
        Some(b) => (b.progress, b.volumes),
    };
    let rewatch_start = before.is_some_and(|b| (after.rewatching && !b.rewatching) || after.times_rewatched > b.times_rewatched);
    // A rewatch stopped without being finished (an Undo or Discard of its start, or turned
    // off): the progress goes back to where it was before; nothing was watched.
    let rewatch_undone = before.is_some_and(|b| b.rewatching && !after.rewatching && after.times_rewatched <= b.times_rewatched);
    if after.progress < from && !rewatch_start {
        take_back(tx, sc, media_id, after.progress, Col::Progress, now)?;
    }
    if after.volumes < vol_from && !rewatch_start {
        take_back(tx, sc, media_id, after.volumes, Col::Volumes, now)?;
    }
    let up = after.progress > from;
    let vol_up = after.volumes > vol_from;
    if !ups || rewatch_undone || (!up && !vol_up) {
        return Ok(());
    }
    let (from, to) = if up { (from, after.progress) } else { (after.progress, after.progress) };
    let (vol_from, vol_to) = if vol_up { (vol_from, after.volumes) } else { (after.volumes, after.volumes) };
    tx.execute(
        "INSERT INTO activity (service, kind, media_id, at, p_from, p_to, v_from, v_to) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![sc.service, sc.kind.name(), media_id, now, from, to, vol_from, vol_to],
    )?;
    start(tx, now)
}

#[derive(Clone, Copy)]
enum Col {
    Progress,
    Volumes,
}

/// The entry's recent rows give back what is above `to`: a row starting at or above it goes, a
/// row across it ends at it.
fn take_back(tx: &Connection, sc: Scope, media_id: u32, to: u32, col: Col, now: i64) -> Result<()> {
    let (f, t) = match col {
        Col::Progress => ("p_from", "p_to"),
        Col::Volumes => ("v_from", "v_to"),
    };
    let base = "service = ?1 AND kind = ?2 AND media_id = ?3 AND at >= ?4";
    let since = now - TAKE_BACK;
    let args = params![sc.service, sc.kind.name(), media_id, since, to];
    // A row that started at or above `to` gives it all back; one across it ends at it.
    tx.execute(&format!("UPDATE activity SET {f} = ?5, {t} = ?5 WHERE {base} AND {f} >= ?5"), args)?;
    tx.execute(&format!("UPDATE activity SET {t} = ?5 WHERE {base} AND {t} > ?5"), args)?;
    // Rows with nothing left in either column.
    tx.execute(&format!("DELETE FROM activity WHERE {base} AND p_to <= p_from AND v_to <= v_from"), params![sc.service, sc.kind.name(), media_id, since])?;
    Ok(())
}

/// Writes when the log started: the earliest time seen.
pub(crate) fn start(c: &Connection, now: i64) -> Result<()> {
    c.execute(
        "INSERT INTO kv (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = CAST(MIN(CAST(value AS INTEGER), CAST(excluded.value AS INTEGER)) AS TEXT)",
        params![SINCE_KEY, now.to_string()],
    )?;
    Ok(())
}

/// When the log started (unix seconds), if it has.
pub fn since(c: &Connection) -> Result<Option<i64>> {
    let v: Option<String> = c.query_row("SELECT value FROM kv WHERE key = ?1", [SINCE_KEY], |r| r.get(0)).optional()?;
    Ok(v.and_then(|v| v.parse().ok()))
}

/// The log of one list, oldest first.
pub fn rows(c: &Connection, sc: Scope) -> Result<Vec<Row>> {
    let mut st = c.prepare(
        "SELECT media_id, at, p_from, p_to, v_from, v_to FROM activity WHERE service = ?1 AND kind = ?2 ORDER BY at, id",
    )?;
    let rows = st.query_map(params![sc.service, sc.kind.name()], |r| {
        Ok(Row { media_id: r.get(0)?, at: r.get(1)?, from: r.get(2)?, to: r.get(3)?, vol_from: r.get(4)?, vol_to: r.get(5)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

impl crate::store::Store {
    /// The activity log of one list, oldest first.
    pub fn activity<'a>(&self, scope: impl Into<Scope<'a>>) -> Result<Vec<Row>> {
        rows(&self.conn, scope.into())
    }

    /// When the activity log started, if it has.
    pub fn activity_since(&self) -> Result<Option<i64>> {
        since(&self.conn)
    }

    /// Writes one row as the store calls do (tests and the scenes' fake data).
    pub fn log_activity<'a>(&self, scope: impl Into<Scope<'a>>, row: Row) -> Result<()> {
        let sc = scope.into();
        self.conn.execute(
            "INSERT INTO activity (service, kind, media_id, at, p_from, p_to, v_from, v_to) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![sc.service, sc.kind.name(), row.media_id, row.at, row.from, row.to, row.vol_from, row.vol_to],
        )?;
        start(&self.conn, row.at)
    }
}

#[cfg(test)]
pub(crate) mod tests;
