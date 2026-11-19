//! Android's additions next to `edit.rs` (kept apart so `edit.rs` stays the same on both
//! branches): Remove from list (the desktop's `remove.rs`: off the list at once, the removal
//! queued, Undo discards it) and −1 ("One less").

use kotora_core::{Change, Kind, Scope, Status, time};

use crate::{Edit, Kotora, KotoraError, MediaKind, Service, other};

/// A queued removal: Undo is `discard(service, op_id)`.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Removal {
    pub service: Service,
    pub op_id: i64,
    /// "Removed Hoshikuzu Relay from your list".
    pub message: String,
}

#[uniffi::export]
impl Kotora {
    /// The question asked first, in the desktop's words.
    pub fn remove_question(&self, kind: MediaKind, id: u32) -> Result<String, KotoraError> {
        let service = self.active_or_err()?;
        let item = self.store().load_item(Scope::new(service.name(), kind.into()), id)?.ok_or(KotoraError::NotFound)?;
        let mut q = format!("Remove {} from your list? It goes from your {} list too, with its progress and score.", item.media.title, service.id().title());
        // With the mirror on, the other service loses it too (desktop `remove_question`).
        if let Some(other) = self.mirror_target() {
            q.push_str(&format!(" Also removes it from {other}."));
        }
        Ok(q)
    }

    /// Remove from list: off the saved list now, the removal waits in the sync queue.
    pub fn remove(&self, kind: MediaKind, id: u32) -> Result<Removal, KotoraError> {
        let service = self.active_or_err()?;
        let mut store = self.store();
        let scope = Scope::new(service.name(), kind.into());
        let item = store.load_item(scope, id)?.ok_or(KotoraError::NotFound)?;
        let op_id = store.remove_from_list(scope, id, time::now_unix())?;
        Ok(Removal { service, op_id, message: format!("Removed {} from your list", item.media.title) })
    }

    /// −1 episode (chapter), queued; Undo as for +1. A completed entry goes back to Watching
    /// (Reading) like the desktop's editor step.
    pub fn minus_one(&self, kind: MediaKind, id: u32) -> Result<Edit, KotoraError> {
        let service = self.active_or_err()?;
        let k: Kind = kind.into();
        let mut store = self.store();
        let scope = Scope::new(service.name(), k);
        let item = store.load_item(scope, id)?.ok_or(KotoraError::NotFound)?;
        if item.progress == 0 {
            return Err(other(format!("{} is at {} 0.", item.media.title, k.unit())));
        }
        let before = item.values();
        let mut change = Change { progress: Some(item.progress - 1), ..Default::default() };
        if before.status == Status::Completed && !before.rewatching {
            change.status = Some(Status::Watching);
        }
        store.enqueue(scope, id, change, time::now_unix())?;
        Ok(Edit {
            kind,
            id,
            message: format!("{}: {} {} saved", item.media.title, k.unit(), item.progress - 1),
            before_status: before.status.into(),
            before_progress: before.progress,
            before_rewatching: before.rewatching,
            before_times_rewatched: before.times_rewatched,
            before_volumes: before.volumes,
            before_score: None,
            restore_score: false,
        })
    }
}
