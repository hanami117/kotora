import Foundation

/// Home's Airing this week, the manga details' Publishing card, the title language and "Keep the
/// higher progress": `mobile/src/home.rs` and `prefs.rs`. A protocol of its own, so the screens ask
/// `model.repo as? ExtrasSource`; the sample data has none of it.
protocol ExtrasSource: AnyObject, Sendable {
    func homeAiring() async throws -> [HomeAiring]
    func publishingCard(id: UInt32) async throws -> PublishingCard?
    func titleLanguage() async -> String
    func setTitleLanguage(_ value: String) async throws
    func keepHigherProgress() async -> Bool
    func setKeepHigherProgress(_ on: Bool) async throws
}

extension CoreRepository: ExtrasSource {
    private func run<T: Sendable>(_ f: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try f() }.value
    }

    func homeAiring() async throws -> [HomeAiring] { try await run { try self.kotora.homeAiring() } }
    func publishingCard(id: UInt32) async throws -> PublishingCard? { try await run { try self.kotora.publishingCard(mediaId: id) } }
    func titleLanguage() async -> String { (try? await run { self.kotora.titleLanguage() }) ?? "romaji" }
    func setTitleLanguage(_ value: String) async throws { try await run { try self.kotora.setTitleLanguage(value: value) } }
    func keepHigherProgress() async -> Bool { (try? await run { self.kotora.keepHigherProgress() }) ?? true }
    func setKeepHigherProgress(_ on: Bool) async throws { try await run { try self.kotora.setKeepHigherProgress(on: on) } }
}

/// A notification tapped before the main screen exists (a cold start): kept until the tabs
/// appear and open it.
@MainActor
enum PendingOpen {
    static var ref: RowRef?

    static func take() -> RowRef? {
        defer { ref = nil }
        return ref
    }
}

