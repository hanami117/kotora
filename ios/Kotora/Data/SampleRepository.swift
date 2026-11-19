import Foundation

/// Made-up lists in memory (the mockup's titles; nothing real), the same rows as the Android
/// app's `SampleRepository`. Edits change the rows here only; nothing goes anywhere. Used by the
/// screenshots and by "Look around" in debug builds.
final class SampleRepository: Repository, @unchecked Sendable {
    let isSample = true
    let now: Int64
    private let lock = NSLock()
    var rows: [Row]

    init(now: Int64 = Int64(Date().timeIntervalSince1970)) {
        self.now = now
        rows = Self.sampleRows(now)
    }

    func locked<T>(_ f: () throws -> T) rethrows -> T {
        lock.lock()
        defer { lock.unlock() }
        return try f()
    }

    func hasClientId(_ service: Service) -> Bool { true }

    func accounts() async throws -> [Account] { [Account(service: .mal, userName: "sample-user", pictureUrl: nil, active: true)] }

    func list(kind: MediaKind, status: ListStatus?, sort: Sort) async throws -> [Row] {
        let l = locked { rows.filter { $0.kind == kind && (status == nil || Self.showsIn($0, status!)) } }
        switch sort {
        case .updated: return l.sorted { $0.updatedAt > $1.updatedAt }
        case .title: return l.sorted { $0.title.lowercased() < $1.title.lowercased() }
        case .score: return l.sorted { (Float($0.score ?? "") ?? -1) > (Float($1.score ?? "") ?? -1) }
        }
    }

    func counts(kind: MediaKind) async throws -> [StatusCount] {
        locked { ListStatus.allCases.map { st in StatusCount(status: st, count: UInt32(rows.filter { $0.kind == kind && Self.showsIn($0, st) }.count)) } }
    }

    func entry(kind: MediaKind, id: UInt32) async throws -> Row? { locked { rows.first { $0.kind == kind && $0.id == id } } }
    func syncedAt(kind: MediaKind) async -> Int64? { now - 240 }
    func waitingCount() async -> UInt32 { locked { UInt32(rows.filter(\.waiting).count) } }
    func mangaEnabled() async -> Bool { true }
    func setMangaEnabled(_ on: Bool) async throws {}

    func plusOne(kind: MediaKind, id: UInt32) async throws -> Edit {
        try locked {
            guard let i = rows.firstIndex(where: { $0.kind == kind && $0.id == id }) else {
                throw KotoraError.NotFound(message: "That isn't on your list.")
            }
            let r = rows[i]
            if let total = r.total, r.progress >= total {
                throw KotoraError.Other(message: "\(r.title) is at its last \(kind.unit).")
            }
            let next = r.progress + 1
            let completes = r.total == next
            var n = r
            n.progress = next
            if completes { n.status = .completed }
            n.waiting = true
            n.updatedAt = now
            rows[i] = n
            return Edit(kind: kind, id: id, message: "\(r.title): \(kind.unit) \(next) saved" + (completes ? ", completed" : ""),
                        beforeStatus: r.status, beforeProgress: r.progress, beforeRewatching: r.rewatching,
                        beforeTimesRewatched: r.timesRewatched, beforeVolumes: r.volumes)
        }
    }

    func undo(_ edit: Edit) async throws {
        locked {
            guard let i = rows.firstIndex(where: { $0.kind == edit.kind && $0.id == edit.id }) else { return }
            rows[i].status = edit.beforeStatus
            rows[i].progress = edit.beforeProgress
            rows[i].rewatching = edit.beforeRewatching
            rows[i].waiting = false
        }
    }

    func sync() async throws -> SyncReport {
        locked { SyncReport(service: .mal, anime: UInt32(rows.filter { $0.kind == .anime }.count), manga: UInt32(rows.filter { $0.kind == .manga }.count), syncedAt: now) }
    }

    func flush() async throws -> FlushReport {
        FlushReport(sent: 0, conflicts: 0, failed: 0, waiting: 0, signInAgain: false, retryIn: nil)
    }

    func signInStart(_ service: Service) async throws -> PendingSignIn { throw KotoraError.NoClientId(message: "Sample data: no sign-in.") }
    func signInFinish(_ pending: PendingSignIn, redirect: String) async throws -> Account {
        throw KotoraError.NoClientId(message: "Sample data: no sign-in.")
    }
    func signOut(_ service: Service) async throws {}
    func setActive(_ service: Service) async throws {}

    /// As the core: a rewatch (status Completed) shows under Watching too.
    static func showsIn(_ r: Row, _ page: ListStatus) -> Bool { r.status == page || (page == .watching && r.rewatching) }

    private static func anime(_ id: UInt32, _ title: String, _ status: ListStatus, _ progress: UInt32, _ total: UInt32?, _ score: String?, _ updated: Int64,
                              format: String = "tv", air: String = "currently_airing", rewatching: Bool = false, waiting: Bool = false,
                              en: String? = nil, start: String? = "2026-10-03") -> Row {
        Row(kind: .anime, id: id, title: title, titleEn: en, status: status, progress: progress, total: total, volumes: 0, totalVolumes: nil,
            score: score, format: format, airStatus: air, startDate: start, coverUrl: nil, rewatching: rewatching, timesRewatched: 0,
            waiting: waiting, updatedAt: updated)
    }

    private static func manga(_ id: UInt32, _ title: String, _ status: ListStatus, _ chapters: UInt32, _ total: UInt32?, _ vols: UInt32, _ score: String?, _ updated: Int64) -> Row {
        Row(kind: .manga, id: id, title: title, titleEn: nil, status: status, progress: chapters, total: total, volumes: vols, totalVolumes: nil,
            score: score, format: "manga", airStatus: total == nil ? "currently_publishing" : "finished", startDate: "2021-04-01", coverUrl: nil,
            rewatching: false, timesRewatched: 0, waiting: false, updatedAt: updated)
    }

    /// The mockup's list, and enough filler for the counts. All made up.
    static func sampleRows(_ now: Int64) -> [Row] {
        let w = ListStatus.watching
        var out = [
            anime(7, "Hoshikuzu Relay", w, 6, 12, "8", now - 60, en: "Stardust Relay"),
            anime(3, "Kagerou Bakery", w, 5, 13, "7", now - 300, waiting: true),
            anime(11, "Tsukimi Station 2nd Season", w, 3, 12, nil, now - 3_600),
            anime(19, "Ashita no Lantern", .completed, 20, 25, "9", now - 7_200, air: "finished_airing", rewatching: true, start: "2019-04-06"),
            anime(5, "Neko Tantei Rin", w, 1, 8, nil, now - 86_400, format: "ona"),
            anime(23, "Sakamichi Cycle Club", w, 14, 24, "8", now - 2 * 86_400, air: "finished_airing", start: "2025-01-10"),
            anime(29, "Kumo no Ue no Toshokan", w, 2, nil, nil, now - 3 * 86_400),
            anime(31, "Shiosai Diner", w, 9, 12, "7", now - 4 * 86_400, air: "finished_airing"),
        ]
        for (i, t) in ["Garasu no Umi", "Tenbin Academy", "Hanabi Courier", "Yoru no Kissaten", "Mizuiro Signal"].enumerated() {
            out.append(anime(40 + UInt32(i), t, .planToWatch, 0, 12, nil, now - Int64(10 + i) * 86_400, air: "not_yet_aired"))
        }
        for (i, t) in ["Kitsune Postman", "Aozora Workshop", "Ginga Ramen", "Sora Tobu Shoten", "Kaze no Orchestra", "Momiji Line"].enumerated() {
            out.append(anime(60 + UInt32(i), t, .completed, 12, 12, String(9 - i % 3), now - Int64(30 + i) * 86_400, air: "finished_airing"))
        }
        out.append(anime(80, "Fuyu no Robot", .onHold, 4, 11, nil, now - 60 * 86_400, air: "finished_airing"))
        out.append(anime(81, "Hikari Tantei Office", .dropped, 2, 12, "4", now - 90 * 86_400, air: "finished_airing"))
        out.append(manga(101, "Paper Lanterns", .watching, 21, nil, 3, "8", now - 600))
        out.append(manga(102, "Umibe no Atelier", .watching, 44, 52, 5, "9", now - 86_400))
        out.append(manga(103, "Kaminari Bento", .planToWatch, 0, nil, 0, nil, now - 5 * 86_400))
        out.append(manga(104, "Hoshi no Tegami", .completed, 60, 60, 7, "8", now - 40 * 86_400))
        return out
    }
}
