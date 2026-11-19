import BackgroundTasks
import Foundation
import Network
import os

/// Sends the waiting changes (the core's queue), the iOS side of the Android app's `FlushWorker`:
/// - at once after every edit, and when the app comes to the front;
/// - when the network comes back while the app is open (Android's network constraint);
/// - in the background through a `BGAppRefreshTask` while changes wait (iOS picks the moment).
/// Edits made during a run are picked up by one more run. The queue itself survives restarts.
@MainActor
final class Flusher {
    nonisolated static let taskId = "io.github.hanami117.kotora.flush"
    private nonisolated static let log = Logger(subsystem: "io.github.hanami117.kotora", category: "flush")

    var onFlushed: (() -> Void)?
    private var running = false
    private var again = false
    private var lastRepo: Repository?
    private let monitor = NWPathMonitor()
    private var online = true

    init() {
        monitor.pathUpdateHandler = { [weak self] path in
            let up = path.status == .satisfied
            Task { @MainActor in self?.network(up) }
        }
        monitor.start(queue: DispatchQueue(label: "io.github.hanami117.kotora.network"))
    }

    private func network(_ up: Bool) {
        let cameBack = up && !online
        online = up
        if cameBack, let r = lastRepo { request(r) }
    }

    func request(_ repo: Repository) {
        guard !repo.isSample else { return }
        lastRepo = repo
        if running {
            again = true
            return
        }
        running = true
        Task {
            repeat {
                again = false
                let r = await Self.run(repo)
                if let r, r.waiting > 0 { Self.schedule() }
            } while again
            running = false
            onFlushed?()
        }
    }

    /// One run; `nil` when it failed as a whole (offline before the first change, …).
    nonisolated static func run(_ repo: Repository) async -> FlushReport? {
        do {
            let r = try await repo.flush()
            log.info("flush: sent \(r.sent), conflicts \(r.conflicts), failed \(r.failed), waiting \(r.waiting)")
            return r
        } catch {
            log.warning("flush failed: \(message(of: error), privacy: .public)")
            schedule()
            return nil
        }
    }

    /// Asks iOS for a background run. One request at a time (a new one replaces the old).
    nonisolated static func schedule() {
        let req = BGAppRefreshTaskRequest(identifier: taskId)
        req.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        do {
            try BGTaskScheduler.shared.submit(req)
        } catch {
            // The Simulator and a build without the background mode refuse; the next launch sends.
            log.info("background flush not scheduled: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// The background task's work: the process's own core, never sample data.
    nonisolated static func backgroundRun() async {
        guard let repo = Core.repository, await repo.waitingCount() > 0 else { return }
        let r = await run(repo)
        // As Android's retry rule: try again later while changes wait and nothing needs the user.
        if let r, r.waiting > 0, !r.signInAgain, r.conflicts == 0 { schedule() }
    }
}
