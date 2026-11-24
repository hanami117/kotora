import Foundation

/// Airing, Publishing and their notifications: the pages' data, in the desktop's words, from
/// `kotora-mobile` (`mobile/src/pages.rs`). A protocol of its own beside `Repository`, so the
/// screens ask `model.repo as? PagesSource`; the sample data has none of it.
protocol PagesSource: AnyObject, Sendable {
    func airingPage(type: AiringType, mine: AiringMine, order: AiringOrder) async throws -> AiringPage
    /// Network.
    func airingRefresh(force: Bool) async throws
    func publishingPage(sort: PublishingSort) async throws -> PublishingPage
    /// Network (MangaUpdates, MangaDex): only what is older than four hours, unless `force`.
    func publishingRefresh(force: Bool) async throws -> PublishingReport
    /// Network.
    func publishingLink(id: UInt32, link: String) async throws
    func publishingReset(id: UInt32) async throws
    func noticesDue() async throws -> [Notice]
    func upcomingEpisodes(within: Int64) async throws -> [Notice]
    func notifySettings() async -> NotifySettings
    func setNotify(_ kind: NoticeKind, _ on: Bool) async throws
}

extension CoreRepository: PagesSource {
    private func work<T: Sendable>(_ f: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try f() }.value
    }

    func airingPage(type: AiringType, mine: AiringMine, order: AiringOrder) async throws -> AiringPage {
        try await work { try self.kotora.airingPage(kind: type, mine: mine, order: order) }
    }
    func airingRefresh(force: Bool) async throws { try await work { try self.kotora.airingRefresh(force: force) } }
    func publishingPage(sort: PublishingSort) async throws -> PublishingPage {
        try await work { try self.kotora.publishingPage(sort: sort) }
    }
    func publishingRefresh(force: Bool) async throws -> PublishingReport {
        try await work { try self.kotora.publishingRefresh(force: force) }
    }
    func publishingLink(id: UInt32, link: String) async throws {
        try await work { try self.kotora.publishingLink(mediaId: id, link: link) }
    }
    func publishingReset(id: UInt32) async throws { try await work { try self.kotora.publishingReset(mediaId: id) } }
    func noticesDue() async throws -> [Notice] { try await work { try self.kotora.noticesDue() } }
    func upcomingEpisodes(within: Int64) async throws -> [Notice] {
        try await work { try self.kotora.upcomingEpisodes(withinSecs: within) }
    }
    func notifySettings() async -> NotifySettings { (try? await work { self.kotora.notifySettings() }) ?? NotifySettings(startedAiring: true, newEpisode: true, newChapter: false) }
    func setNotify(_ kind: NoticeKind, _ on: Bool) async throws { try await work { try self.kotora.setNotify(kind: kind, on: on) } }
}

/// Debug "Look around": a made-up week from the sample list (Watching anime, one slot a day) and
/// made-up releases for the Reading manga. Nothing is asked, nothing is notified.
extension SampleRepository: PagesSource {
    func airingPage(type: AiringType, mine: AiringMine, order: AiringOrder) async throws -> AiringPage {
        let rows = (try await list(kind: .anime, status: .watching, sort: .updated)).filter { $0.airStatus == "currently_airing" }
        let now = Date()
        let cal = Calendar.current
        let names = ["Today", "Tomorrow"]
        let f = DateFormatter()
        f.dateFormat = "EEEE MMM d"
        f.locale = Locale(identifier: "en_US_POSIX")
        var days: [AiringDay] = []
        for d in 0..<7 {
            let date = cal.date(byAdding: .day, value: d, to: now)!
            let label = d < 2 ? "\(names[d]) · \(f.string(from: date))" : f.string(from: date)
            let slots: [AiringSlot] = mine == .off ? [] : rows.enumerated().filter { $0.offset % 7 == d }.map { _, r in
                let at = cal.date(bySettingHour: 17 + d % 5, minute: 30, second: 0, of: date)!
                let secs = Int64(at.timeIntervalSince(now))
                let left = secs / 86_400 > 0 ? "\(secs / 86_400) d " : ""
                return AiringSlot(id: r.id, title: r.title, titleEn: r.titleEn, coverUrl: r.coverUrl, kindLabel: "TV", episode: r.progress + 1,
                                  at: Int64(at.timeIntervalSince1970), time: String(format: "%02d:30", 17 + d % 5), status: r.status,
                                  note: secs <= 0 ? "aired" : "in \(left)\(secs % 86_400 / 3600) h",
                                  toWatch: secs <= 0 ? "~1 to watch" : "Caught up")
            }
            days.append(AiringDay(label: label, today: d == 0, onList: slots.isEmpty ? "" : "\(slots.count) on your list", slots: slots))
        }
        let cards = rows.map { r in
            AiringCard(id: r.id, title: r.title, titleEn: r.titleEn, coverUrl: r.coverUrl, meta: "TV · \(r.total.map { "\($0) eps" } ?? "? eps")",
                       when: "Sat 17:30 · ep \(r.progress + 1)", status: r.status, lit: false, toWatch: "Caught up")
        }
        return AiringPage(season: "Fall 2026", serviceTitle: "MyAnimeList", fetchedAt: Int64(now.timeIntervalSince1970) - 3600, stale: false,
                          hasShows: true, days: days, untimed: [], cards: mine == .off ? [] : cards)
    }

    func airingRefresh(force: Bool) async throws {}

    func publishingPage(sort: PublishingSort) async throws -> PublishingPage {
        let rows = try await list(kind: .manga, status: .watching, sort: .updated)
        let paces = ["Weekly", "Every 2 weeks", "Monthly", "On hiatus"]
        let out = rows.enumerated().map { i, r in
            let latest = r.progress + UInt32(i % 3)
            return PublishingRow(id: r.id, title: r.title, titleEn: r.titleEn, coverUrl: r.coverUrl, progress: r.progress, total: r.total,
                                 latest: "Ch \(latest)", when: "\(i + 1) days ago", behind: latest - r.progress, pace: paces[i % 4],
                                 next: i % 4 == 3 ? "" : "next ~\(["Mon", "Thu", "Sat"][i % 3])", look: i == 1 ? .guess : .sure,
                                 fresh: i == 0 && latest > r.progress, hiatus: i % 4 == 3, finished: false, ahead: false, source: "MangaUpdates",
                                 seriesTitle: r.title, seriesUrl: "https://www.mangaupdates.com/series/", recent: [PublishingRelease(label: "Ch \(latest)", when: "\(i + 1) days ago")], error: nil)
        }
        return PublishingPage(rows: out, checkedAt: Int64(Date().timeIntervalSince1970) - 1800, credit: "Release data: MangaUpdates, MangaDex", stale: false)
    }

    func publishingRefresh(force: Bool) async throws -> PublishingReport { PublishingReport(checked: 0, error: nil, notices: []) }
    func publishingLink(id: UInt32, link: String) async throws { throw KotoraError.Other(message: "Sample data: nothing is asked.") }
    func publishingReset(id: UInt32) async throws {}
    func noticesDue() async throws -> [Notice] { [] }
    func upcomingEpisodes(within: Int64) async throws -> [Notice] { [] }
    func notifySettings() async -> NotifySettings { NotifySettings(startedAiring: true, newEpisode: true, newChapter: false) }
    func setNotify(_ kind: NoticeKind, _ on: Bool) async throws {}
}
