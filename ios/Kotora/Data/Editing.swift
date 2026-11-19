import Foundation

/// The editor, the rewatch (reread) and the sync queue with its conflicts: the desktop's editor
/// and queue page over `kotora-mobile` (`mobile/src/edit.rs`). Kept apart from `Repository` so the
/// screens of other areas don't change; both repositories conform.
protocol EntryEditing: AnyObject, Sendable {
    func editor(kind: MediaKind, id: UInt32) async throws -> EntryEditor
    func scoreScale() async throws -> ScoreScale
    func scoreLabel(_ points: UInt32?) -> String
    func scoreStep(_ points: UInt32?, _ delta: Int32) -> UInt32?
    /// The words of the saved change; `nil`: nothing changed.
    func saveEditor(_ editor: EntryEditor) async throws -> String?
    func startRewatch(kind: MediaKind, id: UInt32) async throws -> Edit
    func stopRewatch(kind: MediaKind, id: UInt32) async throws -> Edit
    /// Remove from list (`mobile/src/remove.rs`): the question first, then the removal (Undo discards it) and −1.
    func removeQuestion(kind: MediaKind, id: UInt32) async throws -> String
    func remove(kind: MediaKind, id: UInt32) async throws -> Removal
    func undoRemoval(_ removal: Removal) async throws
    func minusOne(kind: MediaKind, id: UInt32) async throws -> Edit
    /// Move to ▸ / Score ▸ (the desktop's right-click menu); `nil`: nothing changed.
    func moveTo(kind: MediaKind, id: UInt32, status: ListStatus) async throws -> Edit?
    func setScore(kind: MediaKind, id: UInt32, points: UInt32?) async throws -> Edit?
    /// "Episode 9 of X hasn't aired yet (expected …)." before a +1; `nil`: +1 at once.
    func notAiredQuestion(kind: MediaKind, id: UInt32) async -> String?

    func queueItems() async -> [QueueItem]
    func conflictCount() async -> UInt32
    func keepMine(_ item: QueueItem) async throws
    func takeTheirs(_ item: QueueItem) async throws
    func discard(_ item: QueueItem) async throws
    func retryNow() async throws
}

private func work<T: Sendable>(_ f: @escaping @Sendable () throws -> T) async throws -> T {
    try await Task.detached(priority: .userInitiated) { try f() }.value
}

private func work<T: Sendable>(_ f: @escaping @Sendable () -> T) async -> T {
    await Task.detached(priority: .userInitiated) { f() }.value
}

extension CoreRepository: EntryEditing {
    func editor(kind: MediaKind, id: UInt32) async throws -> EntryEditor { try await work { try self.kotora.editor(kind: kind, id: id) } }
    func scoreScale() async throws -> ScoreScale { try await work { try self.kotora.scoreScale() } }
    func scoreLabel(_ points: UInt32?) -> String { kotora.scoreLabel(points: points) }
    func scoreStep(_ points: UInt32?, _ delta: Int32) -> UInt32? { kotora.scoreStep(points: points, delta: delta) }
    func saveEditor(_ editor: EntryEditor) async throws -> String? { try await work { try self.kotora.saveEditor(editor: editor) } }
    func startRewatch(kind: MediaKind, id: UInt32) async throws -> Edit { try await work { try self.kotora.startRewatch(kind: kind, id: id) } }
    func stopRewatch(kind: MediaKind, id: UInt32) async throws -> Edit { try await work { try self.kotora.stopRewatch(kind: kind, id: id) } }
    func removeQuestion(kind: MediaKind, id: UInt32) async throws -> String { try await work { try self.kotora.removeQuestion(kind: kind, id: id) } }
    func remove(kind: MediaKind, id: UInt32) async throws -> Removal { try await work { try self.kotora.remove(kind: kind, id: id) } }
    func undoRemoval(_ removal: Removal) async throws { try await work { try self.kotora.discard(service: removal.service, id: removal.opId) } }
    func minusOne(kind: MediaKind, id: UInt32) async throws -> Edit { try await work { try self.kotora.minusOne(kind: kind, id: id) } }
    func moveTo(kind: MediaKind, id: UInt32, status: ListStatus) async throws -> Edit? { try await work { try self.kotora.moveTo(kind: kind, id: id, status: status) } }
    func setScore(kind: MediaKind, id: UInt32, points: UInt32?) async throws -> Edit? { try await work { try self.kotora.setScore(kind: kind, id: id, points: points) } }
    func notAiredQuestion(kind: MediaKind, id: UInt32) async -> String? { await work { self.kotora.notAiredQuestion(kind: kind, id: id) } }
    func queueItems() async -> [QueueItem] { await work { self.kotora.queueItems() } }
    func conflictCount() async -> UInt32 { await work { self.kotora.conflictCount() } }
    func keepMine(_ item: QueueItem) async throws { try await work { try self.kotora.keepMine(service: item.service, id: item.id) } }
    func takeTheirs(_ item: QueueItem) async throws { try await work { try self.kotora.takeTheirs(service: item.service, id: item.id) } }
    func discard(_ item: QueueItem) async throws { try await work { try self.kotora.discard(service: item.service, id: item.id) } }
    func retryNow() async throws { try await work { try self.kotora.retryNow() } }
}

/// Sample data: the rules are the core's own (`editorStep`, …, pure functions); the saved values
/// and the queue stay in memory. One made-up conflict shows the queue page.
extension SampleRepository: EntryEditing {
    private static let queueLock = NSLock()
    nonisolated(unsafe) private static var queue: [QueueItem] = [
        QueueItem(service: .mal, id: 1, kind: .anime, mediaId: 23, title: "Sakamichi Cycle Club", coverUrl: nil, state: .conflict,
                  what: "Progress 14 → 15", edits: 1, error: nil, added: false, removed: false,
                  editedAt: Int64(Date().timeIntervalSince1970) - 1_800, sentAt: nil,
                  mine: "Progress 15 / 24 · Score 8", theirs: "Progress 13 / 24 · Score 7", theirsEntry: "13 / 24, Watching, score 7",
                  theirsChangedAt: Int64(Date().timeIntervalSince1970) - 600),
    ]

    private func sampleQueue<T>(_ f: (inout [QueueItem]) -> T) -> T {
        Self.queueLock.lock()
        defer { Self.queueLock.unlock() }
        return f(&Self.queue)
    }

    private static let tens: [UInt32: String] = Dictionary(uniqueKeysWithValues: (1...10).map { (UInt32($0 * 10), String($0)) })

    func editor(kind: MediaKind, id: UInt32) async throws -> EntryEditor {
        try locked {
            guard let r = rows.first(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            return EntryEditor(kind: kind, id: id, title: r.title, status: r.status, progress: r.progress, total: r.total, volumes: r.volumes,
                               totalVolumes: r.totalVolumes, score: r.score.flatMap { UInt32($0) }.map { $0 * 10 },
                               timesRewatched: r.timesRewatched, rewatching: r.rewatching, savedStatus: r.status)
        }
    }

    func scoreScale() async throws -> ScoreScale {
        ScoreScale(choices: (1...10).map { ScoreChoice(points: UInt32($0 * 10), label: String($0)) }, fine: false)
    }

    func scoreLabel(_ points: UInt32?) -> String { points.map { String(($0 + 5) / 10) } ?? "No score" }
    func scoreStep(_ points: UInt32?, _ delta: Int32) -> UInt32? { points }

    func saveEditor(_ ed: EntryEditor) async throws -> String? {
        locked {
            guard let i = rows.firstIndex(where: { $0.kind == ed.kind && $0.id == ed.id }) else { return nil }
            var n = rows[i]
            n.status = ed.status
            n.progress = ed.progress
            n.volumes = ed.volumes
            n.score = ed.score.map { String(($0 + 5) / 10) }
            n.timesRewatched = ed.timesRewatched
            if n == rows[i] { return nil }
            n.waiting = true
            rows[i] = n
            return "\(n.title): saved"
        }
    }

    func startRewatch(kind: MediaKind, id: UInt32) async throws -> Edit { try rewatch(kind, id, on: true) }
    func stopRewatch(kind: MediaKind, id: UInt32) async throws -> Edit { try rewatch(kind, id, on: false) }

    private func rewatch(_ kind: MediaKind, _ id: UInt32, on: Bool) throws -> Edit {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            let r = rows[i]
            rows[i].rewatching = on
            rows[i].progress = on ? 0 : (r.total ?? r.progress)
            rows[i].waiting = true
            let word = kind == .manga ? "reread" : "rewatch"
            return Edit(kind: kind, id: id, message: "\(r.title): \(word) \(on ? "started" : "stopped")", beforeStatus: r.status,
                        beforeProgress: r.progress, beforeRewatching: r.rewatching, beforeTimesRewatched: r.timesRewatched, beforeVolumes: r.volumes)
        }
    }

    func removeQuestion(kind: MediaKind, id: UInt32) async throws -> String {
        let title = try locked { rows.first { $0.kind == kind && $0.id == id }?.title ?? "This" }
        return "Remove \(title) from your list? It goes from your MyAnimeList list too, with its progress and score."
    }

    func remove(kind: MediaKind, id: UInt32) async throws -> Removal {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            let r = rows.remove(at: i)
            Self.removedRows[Int64(id)] = r
            return Removal(service: .mal, opId: Int64(id), message: "Removed \(r.title) from your list")
        }
    }

    func undoRemoval(_ removal: Removal) async throws {
        try locked { if let r = Self.removedRows.removeValue(forKey: removal.opId) { rows.append(r) } }
    }

    func minusOne(kind: MediaKind, id: UInt32) async throws -> Edit {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            let r = rows[i]
            guard r.progress > 0 else { throw KotoraError.Other(message: "\(r.title) is at \(kind.unit) 0.") }
            rows[i].progress = r.progress - 1
            if r.status == .completed && !r.rewatching { rows[i].status = .watching }
            rows[i].waiting = true
            return Edit(kind: kind, id: id, message: "\(r.title): \(kind.unit) \(r.progress - 1) saved", beforeStatus: r.status,
                        beforeProgress: r.progress, beforeRewatching: r.rewatching, beforeTimesRewatched: r.timesRewatched, beforeVolumes: r.volumes)
        }
    }

    nonisolated(unsafe) private static var removedRows: [Int64: Row] = [:]

    func moveTo(kind: MediaKind, id: UInt32, status: ListStatus) async throws -> Edit? {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            let r = rows[i]
            guard r.status != status else { return nil }
            rows[i].status = status
            if status == .completed, let t = r.total { rows[i].progress = t }
            rows[i].waiting = true
            return Edit(kind: kind, id: id, message: "Moved \(r.title) to \(status.label(kind))", beforeStatus: r.status, beforeProgress: r.progress,
                        beforeRewatching: r.rewatching, beforeTimesRewatched: r.timesRewatched, beforeVolumes: r.volumes,
                        beforeScore: r.score.flatMap { UInt32($0) }.map { $0 * 10 }, restoreScore: true)
        }
    }

    func setScore(kind: MediaKind, id: UInt32, points: UInt32?) async throws -> Edit? {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else { throw KotoraError.NotFound(message: "That isn't on your list.") }
            let r = rows[i]
            let score = points.map { String(($0 + 5) / 10) }
            guard r.score != score else { return nil }
            rows[i].score = score
            rows[i].waiting = true
            return Edit(kind: kind, id: id, message: score.map { "Scored \(r.title) \($0)" } ?? "Removed the score of \(r.title)", beforeStatus: r.status,
                        beforeProgress: r.progress, beforeRewatching: r.rewatching, beforeTimesRewatched: r.timesRewatched, beforeVolumes: r.volumes,
                        beforeScore: r.score.flatMap { UInt32($0) }.map { $0 * 10 }, restoreScore: true)
        }
    }

    func notAiredQuestion(kind: MediaKind, id: UInt32) async -> String? {
        guard let r = locked({ rows.first { $0.kind == kind && $0.id == id } }), r.airStatus == "not_yet_aired" else { return nil }
        return "Episode \(r.progress + 1) of \(r.title) hasn't aired yet."
    }

    func queueItems() async -> [QueueItem] { sampleQueue { $0 } }
    func conflictCount() async -> UInt32 { sampleQueue { UInt32($0.filter { $0.state == .conflict }.count) } }
    func keepMine(_ item: QueueItem) async throws { sampleQueue { q in q.removeAll { $0.id == item.id } } }
    func takeTheirs(_ item: QueueItem) async throws { sampleQueue { q in q.removeAll { $0.id == item.id } } }
    func discard(_ item: QueueItem) async throws { sampleQueue { q in q.removeAll { $0.id == item.id } } }
    func retryNow() async throws {}
}
