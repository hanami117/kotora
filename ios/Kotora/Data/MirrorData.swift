import Foundation

/// Mirror edits (Beta), the desktop's mirror.rs through `kotora-mobile` (`mobile/src/mirror.rs`).
/// Its own protocol, next to `Repository`, so the two repositories gain it in this file. The
/// words come from the core, the same on both phones and the desktop.
protocol MirrorRepository: AnyObject, Sendable {
    func mirrorState() async -> MirrorState?
    /// Network: reads the other service's lists and compares them. Nothing is saved yet.
    func mirrorCompare(mangaOnly: Bool) async throws -> MirrorPreview
    func mirrorChoose(_ preview: MirrorPreview, _ align: MirrorAlign) async throws -> String
    func mirrorStop() async throws -> String
    func mirrorQueue() async -> MirrorQueue?
    /// Network for "Add to AniList" on an AniList main list.
    func mirrorSettle(kind: MediaKind, malId: UInt32, how: MirrorSettle) async throws
    func mirrorKeepMine(_ id: Int64) async throws
    func mirrorTakeTheirs(_ id: Int64) async throws
    func mirrorDiscard(_ id: Int64) async throws
}

extension CoreRepository: MirrorRepository {
    private func work<T: Sendable>(_ f: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try f() }.value
    }

    func mirrorState() async -> MirrorState? { try? await work { self.kotora.mirrorState() } }
    func mirrorCompare(mangaOnly: Bool) async throws -> MirrorPreview {
        try await work { mangaOnly ? try self.kotora.mirrorCompareManga() : try self.kotora.mirrorCompare() }
    }
    func mirrorChoose(_ preview: MirrorPreview, _ align: MirrorAlign) async throws -> String {
        try await work { try self.kotora.mirrorChoose(preview: preview, align: align) }
    }
    func mirrorStop() async throws -> String { try await work { try self.kotora.mirrorStop() } }
    func mirrorQueue() async -> MirrorQueue? { try? await work { self.kotora.mirrorQueue() } }
    func mirrorSettle(kind: MediaKind, malId: UInt32, how: MirrorSettle) async throws {
        try await work { try self.kotora.mirrorSettle(kind: kind, malId: malId, how: how) }
    }
    func mirrorKeepMine(_ id: Int64) async throws { try await work { try self.kotora.mirrorKeepMine(id: id) } }
    func mirrorTakeTheirs(_ id: Int64) async throws { try await work { try self.kotora.mirrorTakeTheirs(id: id) } }
    func mirrorDiscard(_ id: Int64) async throws { try await work { try self.kotora.mirrorDiscard(id: id) } }
}

/// Sample data has one account: the row says to sign in to the other service, as the desktop.
extension SampleRepository: MirrorRepository {
    func mirrorState() async -> MirrorState? {
        MirrorState(main: .mal, to: .aniList, on: false, title: "Mirror edits to AniList", tag: "Beta",
                    note: "Sign in to AniList too: choose Use AniList above, sign in, then come back to MyAnimeList. Your MyAnimeList list stays the main one.",
                    canTurnOn: false, otherSignedIn: false, alignManga: nil, waiting: 0, attention: 0, badge: 0, trouble: nil,
                    accountLine: "MyAnimeList", accountTip: nil, reading: "Reading your AniList list…",
                    readingManga: "Reading your AniList manga list…", saving: "Saving…", turningOff: "Turning the mirror off…")
    }
    func mirrorCompare(mangaOnly: Bool) async throws -> MirrorPreview { throw KotoraError.Other(message: "Sample data: nothing is asked.") }
    func mirrorChoose(_ preview: MirrorPreview, _ align: MirrorAlign) async throws -> String { throw KotoraError.Other(message: "Sample data: nothing is saved.") }
    func mirrorStop() async throws -> String { "Off. Nothing more goes to the mirror." }
    func mirrorQueue() async -> MirrorQueue? { nil }
    func mirrorSettle(kind: MediaKind, malId: UInt32, how: MirrorSettle) async throws {}
    func mirrorKeepMine(_ id: Int64) async throws {}
    func mirrorTakeTheirs(_ id: Int64) async throws {}
    func mirrorDiscard(_ id: Int64) async throws {}
}
