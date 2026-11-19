import Foundation
import SwiftUI

/// The app's side of kotora-mobile's `Platform`: the time zone, a log, and the token vault.
///
/// Spike: demo mode only. The vault hands out a made-up MyAnimeList token so the core treats the
/// bundled sample list as signed in; nothing is ever saved. The build has no Client IDs, so the
/// core can't reach a service even if asked (it answers "no Client ID").
final class DemoPlatform: Platform {
    func utcOffset(atUnix: Int64) -> Int32 {
        Int32(TimeZone.current.secondsFromGMT(for: Date(timeIntervalSince1970: TimeInterval(atUnix))))
    }

    func log(level: LogLevel, message: String) {
        NSLog("kotora core [%@]: %@", String(describing: level), message)
    }

    func tokenLoad(service: String) -> Data? {
        guard service == "mal" else { return nil }
        return Data(#"{"access_token":"sample","refresh_token":"sample","expires_at":4102444800}"#.utf8)
    }

    func tokenSave(service: String, bytes: Data) -> Bool { false }
    func tokenDelete(service: String) {}
}

/// One `Kotora` per process, over the sample database in Application Support.
@MainActor
final class CoreModel: ObservableObject {
    let core: Kotora?
    let startError: String?

    @Published var kind: MediaKind = .anime
    @Published var status: ListStatus = .watching
    @Published var sort: Sort = .updated
    @Published private(set) var rows: [Row] = []
    @Published private(set) var counts: [StatusCount] = []
    @Published private(set) var waiting: UInt32 = 0
    @Published var snack: Snack?
    /// Bumped on every edit, so open pages re-read.
    @Published private(set) var version = 0

    struct Snack: Identifiable, Equatable {
        let id = UUID()
        let text: String
        let edit: Edit?
    }

    init() {
        let fm = FileManager.default
        let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let data = support.appendingPathComponent("data", isDirectory: true)
        let config = support.appendingPathComponent("config", isDirectory: true)
        let cache = fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("kotora", isDirectory: true)
        try? fm.createDirectory(at: data, withIntermediateDirectories: true)
        try? fm.createDirectory(at: config, withIntermediateDirectories: true)
        let db = data.appendingPathComponent("kotora.db")
        if ProcessInfo.processInfo.arguments.contains("-resetSample") {
            for suffix in ["", "-wal", "-shm"] { try? fm.removeItem(atPath: db.path + suffix) }
        }
        if !fm.fileExists(atPath: db.path), let sample = Bundle.main.url(forResource: "demo", withExtension: "db") {
            try? fm.copyItem(at: sample, to: db)
        }
        do {
            core = try Kotora(
                paths: Paths(configDir: config.path, dataDir: data.path, cacheDir: cache.path),
                ids: ClientIds(mal: "", anilist: ""),
                platform: DemoPlatform())
            startError = nil
        } catch {
            core = nil
            startError = "\(error)"
        }
        reload()
        // Screenshots: `-plusOne <id>` does a +1 at launch, so the Undo bar shows.
        let plus = UserDefaults.standard.integer(forKey: "plusOne")
        if plus > 0, let row = entry(.anime, UInt32(plus)) { plusOne(row) }
    }

    var unit: String { kind == .manga ? "chapter" : "episode" }

    func reload() {
        guard let core else { return }
        rows = (try? core.list(kind: kind, status: status, sort: sort)) ?? []
        counts = (try? core.counts(kind: kind)) ?? []
        waiting = core.waitingCount()
        version += 1
    }

    func count(_ s: ListStatus) -> UInt32 {
        counts.first { $0.status == s }?.count ?? 0
    }

    func entry(_ kind: MediaKind, _ id: UInt32) -> Row? {
        try? core?.entry(kind: kind, id: id)
    }

    func allRows(_ kind: MediaKind) -> [Row] {
        (try? core?.list(kind: kind, status: nil, sort: .updated)) ?? []
    }

    var accounts: [Account] { core?.accounts() ?? [] }

    var syncedAt: Date? {
        core?.syncedAt(kind: kind).map { Date(timeIntervalSince1970: TimeInterval($0)) }
    }

    var mangaEnabled: Bool { core?.mangaEnabled() ?? false }

    func setMangaEnabled(_ on: Bool) {
        try? core?.setMangaEnabled(on: on)
        if !on { kind = .anime }
        reload()
    }

    func plusOne(_ row: Row) {
        guard let core else { return }
        do {
            let edit = try core.plusOne(kind: row.kind, id: row.id)
            snack = Snack(text: edit.message, edit: edit)
        } catch {
            snack = Snack(text: "\(error)", edit: nil)
        }
        reload()
    }

    func undo(_ edit: Edit) {
        do {
            try core?.undo(edit: edit)
            snack = Snack(text: "Undone", edit: nil)
        } catch {
            snack = Snack(text: "\(error)", edit: nil)
        }
        reload()
    }

    /// Sample data: nothing is fetched or sent.
    func pretendSync() {
        snack = Snack(text: "Sample data: nothing is fetched or sent", edit: nil)
    }
}

extension ListStatus: CaseIterable {
    public static var allCases: [ListStatus] { [.watching, .planToWatch, .completed, .onHold, .dropped] }

    func label(_ kind: MediaKind) -> String {
        switch self {
        case .watching: return kind == .manga ? "Reading" : "Watching"
        case .planToWatch: return kind == .manga ? "Plan to read" : "Plan to watch"
        case .completed: return "Completed"
        case .onHold: return "On hold"
        case .dropped: return "Dropped"
        }
    }
}

extension Service {
    var label: String { self == .mal ? "MyAnimeList" : "AniList" }
}

extension Row {
    var key: String { "\(kind == .manga ? "m" : "a")\(id)" }

    var initials: String {
        let words = title.split(separator: " ").filter { $0.first?.isLetter == true }
        return String(words.prefix(2).compactMap { $0.first }).uppercased()
    }

    var formatLabel: String {
        switch format {
        case "tv": return "TV"
        case "ona": return "ONA"
        case "ova": return "OVA"
        case "movie": return "Movie"
        case "special": return "Special"
        case "manga": return "Manga"
        default: return format.capitalized
        }
    }

    var airing: Bool { airStatus == "currently_airing" || airStatus == "currently_publishing" }
}
