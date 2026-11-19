import Foundation
import SwiftUI

/// Statistics and "Fill volumes from chapters" for the screens: the core (`CoreRepository`) or
/// made-up numbers (`SampleRepository`). Kept apart from `Repository` so the list's interface
/// stays as it is on Android.
protocol InsightsSource: AnyObject, Sendable {
    /// Local (the list and Kotora's activity log): no request.
    func stats(kind: MediaKind) async throws -> StatsData
    /// `nil`: not available (sample data).
    func volumeFill() throws -> VolumeFill?
}

extension CoreRepository: InsightsSource {
    func stats(kind: MediaKind) async throws -> StatsData {
        try await Task.detached(priority: .userInitiated) { try self.kotora.stats(kind: kind) }.value
    }

    func volumeFill() throws -> VolumeFill? { try kotora.volumeFill() }
}

extension SampleRepository: InsightsSource {
    func stats(kind: MediaKind) async throws -> StatsData { Self.fakeStats(kind: kind) }
    func volumeFill() throws -> VolumeFill? { nil }

    /// The desktop's made-up numbers for its scenes (`app/src/stats.rs`, `fake`): a year of evenings.
    static func fakeStats(kind: MediaKind, now: Date = Date()) -> StatsData {
        let n = 371
        let today = Int64(floor((now.timeIntervalSince1970 + Double(TimeZone.current.secondsFromGMT(for: now))) / 86_400))
        var days = [UInt32](repeating: 0, count: n)
        let start = n - 120
        for i in start..<n {
            let day = today - Int64(n - 1 - i)
            let wd = statsWeekday(day: day)
            let seed = (UInt32(i) &* 2_654_435_761) >> 28
            days[i] = switch (wd, seed) {
            case (5, let s), (6, let s): 1 + s % 5
            case (_, let s) where s < 6: 0
            case (_, let s): s % 3
            }
            if (60..<70).contains(i - start) { days[i] = 0 }
        }
        for i in (n - 6)..<n { days[i] = max(days[i], 1) }
        let sum = { (k: Int) in days.suffix(k).reduce(0, +) }
        let per: UInt64 = kind == .anime ? 24 : 0
        let period = { (k: Int, titles: UInt32) in
            StatsPeriod(units: sum(k), volumes: kind == .manga ? sum(k) / 8 : 0, minutes: UInt64(sum(k)) * per, titles: titles)
        }
        let names = kind == .anime ? ["Hoshikuzu Relay", "Kagerou Bakery", "Tsukimi Station 2nd Season", "Neko Tantei Rin", "Sakamichi Cycle Club"]
            : ["Paper Lanterns", "Umibe no Atelier", "Hoshi no Tegami", "Kaminari Bento", "Kaze no Shiori"]
        let ids: [UInt32] = kind == .anime ? [7, 3, 11, 5, 23] : [101, 102, 104, 103, 105]
        let titles = { (k: Int) in
            (0..<k).map { i in StatsTitle(mediaId: ids[i], title: names[i], coverUrl: nil, progress: UInt32(6 + i), total: 12, units: UInt32(max(0, 14 - i * 3))) }
        }
        let named = { (v: [(String, UInt32)]) in v.map { NamedCount(name: $0.0, count: $0.1) } }
        return StatsData(
            kind: kind, today: today, sinceDay: today - 118,
            titles: 304, statuses: [11, 48, 236, 7, 2], scores: [0, 1, 2, 4, 9, 22, 48, 71, 40, 15],
            units: kind == .anime ? 3790 : 6120, volumes: kind == .manga ? 512 : 0, minutes: kind == .anime ? 90_870 : 0,
            guessed: 12, watched: 256, withDetails: 61,
            genres: named([("Drama", 34), ("Comedy", 29), ("Action", 25), ("Slice of Life", 19), ("Romance", 16), ("Fantasy", 12), ("Mystery", 9), ("Sci-Fi", 7)]),
            makers: kind == .anime ? named([("Studio Lantern", 9), ("Harbour Works", 7), ("Iron Garden Pictures", 5), ("Studio Kite", 4), ("Paper Moon", 3), ("Ninefold", 2)])
                : named([("Aoi Kurata", 4), ("Ren Mizuno", 3), ("Sora Ikeda", 3), ("Mei Tachibana", 2)]),
            airing: kind == .anime ? titles(4).map { StatsTitle(mediaId: $0.mediaId, title: $0.title, coverUrl: nil, progress: $0.progress, total: $0.total, units: 0) } : [],
            days: days, firstDay: today - Int64(n - 1),
            week: period(4, 3), month: period(19, 6), year: period(120, 14), all: period(n, 14),
            streak: 6, bestStreak: 11,
            weekdays: [9, 7, 11, 8, 13, 31, 27],
            hours: [6, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 1, 2, 4, 6, 9, 14, 17, 15, 10],
            lately: titles(5))
    }
}

// ------------------------------------------------------------------------------------ settings

/// The settings in `config.toml` (the desktop's keys; `kotora-mobile`'s `settings`), app-wide: the
/// theme and the intro are read before anything shows. Settings belong to the phone, not to an
/// account, so the sample lists use the same ones. The Airing session's notifications read the same
/// `[notifications]` keys from the core.
@MainActor
final class Prefs: ObservableObject {
    static let shared = Prefs()

    @Published private(set) var settings: KotoraSettings

    private init() {
        settings = Core.repository?.kotora.settings()
            ?? KotoraSettings(theme: "system", intro: true, openOn: "home", startMode: "last", lastPage: "", listKind: .anime,
                        notifyNewEpisode: true, notifyStartedAiring: true, notifyNewChapter: false)
    }

    /// Saves one setting (its line in `config.toml`); the screen shows what was saved.
    func set(_ s: Setting) {
        guard let k = Core.repository?.kotora else { return }
        if let new = try? k.setSetting(setting: s) { settings = new }
    }

    /// Settings → Appearance → Theme.
    var colorScheme: ColorScheme? {
        switch settings.theme {
        case "light": .light
        case "dark": .dark
        default: nil
        }
    }

    // ---- launch behaviour (`[startup]`), as the desktop's `startup.rs` plans it ----

    /// The tab to open on: the one left last (Open on: Last page), else the first.
    var startTab: String? { settings.openOn == "last" && !settings.lastPage.isEmpty ? settings.lastPage : nil }

    /// The Anime | Manga switch at start: last used, Anime or Manga (Manga only when Manga is on).
    func startKind(mangaOn: Bool) -> MediaKind {
        let k: MediaKind = switch settings.startMode {
        case "anime": .anime
        case "manga": .manga
        default: settings.listKind
        }
        return k == .manga && !mangaOn ? .anime : k
    }

    /// Played this launch already (it never replays coming back from the background).
    var introPlayed = false
}
