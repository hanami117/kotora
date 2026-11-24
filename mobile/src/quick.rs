//! The desktop's quick edits on a row (`app/src/ctxmenu.rs`): Move to ▸ and Score ▸, each with
//! an Undo toast; the "hasn't aired yet" question before a +1 ("Mark anyway?", `ui.rs`); the
//! +1's unit for a manga (a chapter, or a volume when only volumes show, `mgrid.rs`); and the
//! row's sync tag ("Conflict in queue", "Sync failed", the desktop's `waiting_label`).

use kotora_core::airing::{self, DAY_SHORT, Timetable};
use kotora_core::queue::OpState;
use kotora_core::time::{self, Clock};
use kotora_core::{Change, Kind, Scope, Score, Status};

use crate::{Edit, Kotora, KotoraError, ListStatus, MediaKind, Row};

/// What the row's tag says for an entry with an open change (the desktop's `waiting_label`).
pub(crate) fn sync_label(state: OpState) -> &'static str {
    match state {
        OpState::Conflict => "Conflict in queue",
        OpState::Failed => "Sync failed",
        _ => "Waiting to sync",
    }
}

/// Manga: `+1` steps volumes when only volumes show (no chapter read, some volumes), as the
/// desktop's `plus_unit`.
pub(crate) fn plus_volumes(kind: Kind, chapters: u32, volumes: u32) -> bool {
    kind == Kind::Manga && chapters == 0 && volumes > 0
}

/// The +1 button's words: "One more episode", "One more chapter", "One more volume".
#[uniffi::export]
pub fn plus_one_label(row: Row) -> String {
    match row.kind {
        MediaKind::Anime => "One more episode".into(),
        MediaKind::Manga if plus_volumes(Kind::Manga, row.progress, row.volumes) => "One more volume".into(),
        MediaKind::Manga => "One more chapter".into(),
    }
}

/// " (expected Sat Nov 21, 15:30)" in the phone's zone, or nothing (the desktop's `expected_text`).
fn expected_text(at: Option<i64>) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    match at {
        Some(t) => {
            let c = Clock::local(t);
            let day = DAY_SHORT[usize::from(c.weekday % 7)];
            format!(" (expected {day} {} {}, {})", MONTHS[(c.month as usize).clamp(1, 12) - 1], c.date, c.hhmm())
        }
        None => String::new(),
    }
}

impl Kotora {
    /// Queues `change` (made against the saved values) and returns the Undo record, the score too.
    fn quick_edit(&self, kind: MediaKind, id: u32, change: impl FnOnce(&kotora_core::ListItem) -> Change, message: impl FnOnce(&kotora_core::ListItem) -> String) -> Result<Option<Edit>, KotoraError> {
        let service = self.active_or_err()?;
        let mut store = self.store();
        let scope = Scope::new(service.name(), kind.into());
        let item = store.load_item(scope, id)?.ok_or(KotoraError::NotFound)?;
        let before = item.values();
        let change = change(&item).without_same(&before);
        if change.is_empty() {
            return Ok(None);
        }
        store.enqueue(scope, id, change, time::now_unix())?;
        Ok(Some(Edit {
            kind,
            id,
            message: message(&item),
            before_status: before.status.into(),
            before_progress: before.progress,
            before_rewatching: before.rewatching,
            before_times_rewatched: before.times_rewatched,
            before_volumes: before.volumes,
            before_score: before.score.map(|s| u32::from(s.points())),
            restore_score: true,
        }))
    }
}

#[uniffi::export]
impl Kotora {
    /// Move to ▸ a status: "Moved X to Completed"; Completed also sets the last episode (and the
    /// last volume), as the editor and MAL do. `None`: it has that status already.
    pub fn move_to(&self, kind: MediaKind, id: u32, status: ListStatus) -> Result<Option<Edit>, KotoraError> {
        let st: Status = status.into();
        let k: Kind = kind.into();
        self.quick_edit(kind, id, |it| Change::to_status_of(st, &it.media), |it| format!("Moved {} to {}", it.media.title, st.label_in(k)))
    }

    /// Score ▸ a score (100-point scale), or `None` ("No score"): "Scored X 8", "Removed the score
    /// of X". A pick that already shows as the entry's keeps the precise score.
    pub fn set_score(&self, kind: MediaKind, id: u32, points: Option<u32>) -> Result<Option<Edit>, KotoraError> {
        let service = self.active_or_err()?;
        let fmt = self.score_format(&self.store(), service);
        let pick = points.and_then(Score::from_points);
        self.quick_edit(
            kind,
            id,
            |it| Change { score: Some(fmt.pick(it.score, pick)), ..Default::default() },
            |it| match fmt.pick(it.score, pick) {
                Some(n) => format!("Scored {} {}", it.media.title, fmt.words(n)),
                None => format!("Removed the score of {}", it.media.title),
            },
        )
    }

    /// Before a +1 of an anime: "Episode 9 of X hasn't aired yet (expected Sat Nov 21, 15:30)."
    /// when the next episode hasn't aired by the estimate; the app then asks "Mark anyway?".
    /// `None`: it has aired (or there is no telling): +1 at once.
    pub fn not_aired_question(&self, kind: MediaKind, id: u32) -> Option<String> {
        if kind != MediaKind::Anime {
            return None;
        }
        let service = self.active_or_err().ok()?;
        let item = self.store().load_item(Scope::new(service.name(), Kind::Anime), id).ok()??;
        let m = &item.media;
        let next = item.progress + 1;
        if m.episodes.is_some_and(|n| n > 0 && next > n) {
            return None;
        }
        let na = airing::not_aired(&m.air_status, Timetable::of_media(m), item.progress, next, time::now_unix())?;
        Some(format!("Episode {} of {} hasn't aired yet{}.", na.episode, m.display_title(crate::prefs::titles()), expected_text(na.expected)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_unit_and_the_queue_state() {
        assert_eq!(sync_label(OpState::Conflict), "Conflict in queue");
        assert_eq!(sync_label(OpState::Failed), "Sync failed");
        assert_eq!(sync_label(OpState::Queued), "Waiting to sync");
        assert!(plus_volumes(Kind::Manga, 0, 3));
        assert!(!plus_volumes(Kind::Manga, 12, 3));
        assert!(!plus_volumes(Kind::Manga, 0, 0));
        assert!(!plus_volumes(Kind::Anime, 0, 3));
        assert_eq!(expected_text(None), "");
    }
}
