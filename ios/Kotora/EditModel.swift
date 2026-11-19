import Foundation

/// The editor's, the rewatch's and the queue page's actions (the desktop's `save_editor`,
/// `start_rewatch`, `stop_rewatch`, `queue_cmd`): through `EntryEditing`, then the list reads
/// again and the queue is sent.
extension AppModel {
    var editing: EntryEditing? { repo as? EntryEditing }

    /// Save: one queued change. Returns false when it couldn't be saved (the sheet stays).
    func saveEditor(_ ed: EntryEditor) async -> Bool {
        guard let e = editing else { return false }
        do {
            if let text = try await e.saveEditor(ed) {
                await reload()
                requestFlush()
                snack = Snack(text: text)
            }
            return true
        } catch {
            snack = Snack(text: message(of: error))
            return false
        }
    }

    /// "Rewatch" / "Reread" (start) or "Stop rewatching" / "Stop rereading": at once, with Undo.
    func rewatch(_ row: Row, start: Bool) {
        guard let e = editing else { return }
        Task {
            do {
                let edit = start ? try await e.startRewatch(kind: row.kind, id: row.id) : try await e.stopRewatch(kind: row.kind, id: row.id)
                await reload()
                requestFlush()
                snack = Snack(text: edit.message, undo: edit)
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    /// "Remove from list…" after the confirm: off the list at once, Undo discards the queued removal.
    func removeFromList(_ row: Row) {
        guard let e = editing else { return }
        Task {
            do {
                let removal = try await e.remove(kind: row.kind, id: row.id)
                await reload()
                requestFlush()
                snack = Snack(text: removal.message, removal: removal)
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    func undoRemoval(_ removal: Removal) {
        guard let e = editing else { return }
        snack = nil
        Task {
            do {
                try await e.undoRemoval(removal)
                await reload()
                requestFlush()
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    /// −1 episode (chapter), with Undo as for +1.
    func minusOne(_ row: Row) {
        guard let e = editing else { return }
        Task {
            do {
                let edit = try await e.minusOne(kind: row.kind, id: row.id)
                await reload()
                requestFlush()
                snack = Snack(text: edit.message, undo: edit)
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    enum QueueAction { case keepMine, takeTheirs, discard }

    func queue(_ action: QueueAction, _ item: QueueItem) async {
        guard let e = editing else { return }
        do {
            switch action {
            case .keepMine: try await e.keepMine(item)
            case .takeTheirs: try await e.takeTheirs(item)
            case .discard: try await e.discard(item)
            }
            await reload()
            requestFlush()
        } catch {
            snack = Snack(text: message(of: error))
        }
    }

    /// "Sync now" on the queue page: what waits may go at once, then the list is read again.
    func syncNow() {
        Task {
            try? await editing?.retryNow()
            sync()
        }
    }

    /// Move to ▸ (the desktop's right-click menu): at once, with Undo.
    func moveTo(_ row: Row, _ status: ListStatus) {
        quick { try await $0.moveTo(kind: row.kind, id: row.id, status: status) }
    }

    /// Score ▸ a score or "No score": at once, with Undo.
    func setScore(_ row: Row, _ points: UInt32?) {
        quick { try await $0.setScore(kind: row.kind, id: row.id, points: points) }
    }

    /// The Score ▸ choices, once per account.
    func loadScoreChoices() {
        guard let e = editing else { return }
        Task { scoreChoices = ((try? await e.scoreScale())?.choices ?? []).reversed() }
    }

    private func quick(_ f: @escaping (EntryEditing) async throws -> Edit?) {
        guard let e = editing else { return }
        Task {
            do {
                if let edit = try await f(e) {
                    await reload()
                    requestFlush()
                    snack = Snack(text: edit.message, undo: edit)
                }
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }
}
