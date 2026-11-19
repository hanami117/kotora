import BackgroundTasks
import Foundation
import os
import UserNotifications

/// Started airing, New episode and New chapter, as on the desktop (Settings → Notifications; the
/// same switches, rules and words, from `kotora-mobile`). Kotora has no server, so no push:
///
/// - **New episode** notifications are scheduled ahead, at each Watching show's next estimated
///   slot (the next 7 days, at most 48), and scheduled again whenever the list changes, the app
///   comes to the front, or a background refresh runs. iOS shows them on time even when Kotora
///   isn't running. A +1 made since then removes the one for that episode on the next scheduling.
/// - **Started airing** needs the service's new airing status, and **New chapter** needs
///   MangaUpdates: both come from a background refresh (`BGAppRefreshTask`, sync then check) or
///   from opening the app. iOS decides when a background refresh runs (how often the app is
///   used, battery, Low Power Mode): it can be hours late, or not run at all if the app was
///   swiped away from the app switcher. Nothing in Kotora can make it exact.
/// - Each event is announced once (the core keeps the announced ones), as on the desktop.
@MainActor
enum Notifier {
    nonisolated static let taskId = "io.github.hanami117.kotora.refresh"
    private nonisolated static let log = Logger(subsystem: "io.github.hanami117.kotora", category: "notify")
    /// The scheduled episode notifications (id → due time), so a due one already shown by its
    /// schedule isn't shown twice.
    private nonisolated static let scheduledKey = "notify.scheduled"
    private nonisolated static let askedKey = "notify.asked"

    static var source: PagesSource? { Core.repository }

    /// Asks for permission once (the first visit to Airing or Publishing, or a switch turned on),
    /// never at the first launch. Returns whether notifications may be shown.
    @discardableResult
    nonisolated static func askPermission(force: Bool = false) async -> Bool {
        let center = UNUserNotificationCenter.current()
        let status = await center.notificationSettings().authorizationStatus
        switch status {
        case .authorized, .provisional, .ephemeral: return true
        case .denied: return false
        case .notDetermined:
            if !force && UserDefaults.standard.bool(forKey: askedKey) { return false }
            UserDefaults.standard.set(true, forKey: askedKey)
            return (try? await center.requestAuthorization(options: [.alert, .sound, .badge])) ?? false
        @unknown default: return false
        }
    }

    nonisolated static func permissionDenied() async -> Bool {
        await UNUserNotificationCenter.current().notificationSettings().authorizationStatus == .denied
    }

    /// One look: what is due now (from the saved list), then the episodes ahead. `chapters`: also
    /// ask MangaUpdates (only what is older than four hours) when New chapter is on.
    nonisolated static func check(chapters: Bool) async {
        guard let src = await source else { return }
        let center = UNUserNotificationCenter.current()
        let allowed = await center.notificationSettings().authorizationStatus == .authorized
        var due = (try? await src.noticesDue()) ?? []
        let settings = await src.notifySettings()
        if chapters, settings.newChapter, let repo = Core.repository, await repo.mangaEnabled() {
            if let r = try? await src.publishingRefresh(force: false) { due += r.notices }
        }
        guard allowed else { return }
        let now = Int64(Date().timeIntervalSince1970)
        let scheduled = UserDefaults.standard.dictionary(forKey: scheduledKey) as? [String: Int64] ?? [:]
        let delivered = Set(await center.deliveredNotifications().map(\.request.identifier))
        for n in due where !delivered.contains(n.id) && !(scheduled[n.id].map { $0 <= now } ?? false) {
            try? await center.add(request(n, at: nil))
        }
        await schedule(src, center: center)
    }

    /// The New episode notifications ahead, scheduled again from the saved list.
    nonisolated static func schedule(_ src: PagesSource, center: UNUserNotificationCenter) async {
        let now = Int64(Date().timeIntervalSince1970)
        let pending = await center.pendingNotificationRequests().map(\.identifier).filter { $0.hasPrefix("kotora-") && $0.contains("-ep") }
        center.removePendingNotificationRequests(withIdentifiers: pending)
        let all: [Notice] = (try? await src.upcomingEpisodes(within: 7 * 86_400)) ?? []
        let upcoming = all.filter { $0.at > now }.prefix(48)
        let saved = UserDefaults.standard.dictionary(forKey: scheduledKey) as? [String: Int64] ?? [:]
        var scheduled = saved.filter { $0.value > now - 3 * 86_400 }
        for n in upcoming {
            try? await center.add(request(n, at: n.at))
            scheduled[n.id] = n.at
        }
        UserDefaults.standard.set(scheduled, forKey: scheduledKey)
        log.info("scheduled \(upcoming.count) episode notifications")
    }

    private nonisolated static func request(_ n: Notice, at: Int64?) -> UNNotificationRequest {
        let c = UNMutableNotificationContent()
        c.title = n.title
        c.body = n.body
        c.sound = .default
        c.threadIdentifier = switch n.kind {
        case .newChapter: "new-chapter"
        case .newEpisode: "new-episode"
        case .startedAiring: "started-airing"
        }
        c.userInfo = ["kind": n.mediaKind == .manga ? "manga" : "anime", "id": Int(n.mediaId)]
        let trigger = at.map { UNTimeIntervalNotificationTrigger(timeInterval: max(1, TimeInterval($0) - Date().timeIntervalSince1970), repeats: false) }
        return UNNotificationRequest(identifier: n.id, content: c, trigger: trigger)
    }

    /// Asks iOS for a background refresh (one request at a time; iOS picks the moment).
    nonisolated static func scheduleRefresh() {
        let req = BGAppRefreshTaskRequest(identifier: taskId)
        req.earliestBeginDate = Date(timeIntervalSinceNow: 60 * 60)
        do {
            try BGTaskScheduler.shared.submit(req)
        } catch {
            log.info("background refresh not scheduled: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// The background refresh: the waiting changes, the list (for a start of airing and the
    /// progress), then the check. Asks for the next one first.
    nonisolated static func backgroundRun() async {
        scheduleRefresh()
        guard let repo = Core.repository, !((try? await repo.accounts()) ?? []).isEmpty else { return }
        // The waiting changes, then the list once the saved copy is an hour old (kotora-mobile).
        _ = try? await repo.backgroundRefresh()
        await check(chapters: true)
    }
}

/// Shows Kotora's notifications while the app is open too, as banners.
final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationDelegate()

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }

    /// A tap on a notification: its entry's details (on the list or not).
    @MainActor
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let id = info["id"] as? Int, id > 0 else { return }
        let kind: MediaKind = info["kind"] as? String == "manga" ? .manga : .anime
        NotificationCenter.default.post(name: .kotoraOpenEntry, object: RowRef(kind: kind, id: UInt32(id)))
    }
}
