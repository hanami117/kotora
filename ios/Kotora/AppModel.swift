import AuthenticationServices
import Foundation
import SwiftUI

/// The one core of the process (the app's and the background flush's), made on first use.
/// `kotora-mobile` sets its folders for the whole process, so there is never a second one.
enum Core {
    static let result: Result<CoreRepository, Error> = Result { try CoreRepository() }
    static var repository: CoreRepository? { try? result.get() }
    static var brokenMessage: String? {
        if case .failure(let e) = result { return "Kotora couldn't start: \(message(of: e))" }
        return nil
    }
}

enum Session: Equatable {
    case loading
    /// `busy`: a sign-in is being completed.
    case signedOut(error: String? = nil, busy: Bool = false)
    case signedIn(accounts: [Account])
    /// The core couldn't start (the database can't open).
    case broken(String)

    var active: Account? {
        guard case .signedIn(let a) = self else { return nil }
        return a.first(where: \.active) ?? a.first
    }

    var accounts: [Account] {
        if case .signedIn(let a) = self { return a }
        return []
    }
}

struct ListUi: Equatable {
    var kind: MediaKind = .anime
    var status: ListStatus = .watching
    var sort: ListSort = .updated
    /// The order's second direction (desktop `sort_rev`).
    var reverse = false
    var filter: QuickFilter = .all
    /// The "Filter your list" box.
    var query = ""
    var view: ListViewMode = .list
    var rows: [Row] = []
    /// The rows with their tags and score, and the empty state (`ListPage`).
    var lines: [PageLine] = []
    var empty: ListEmptyKind = .none
    var emptyTitle = ""
    var emptyHelp = ""
    var counts: [StatusCount] = []
    var syncedAt: Int64?
    var waiting: UInt32 = 0
    var syncing = false
    /// The last sync's problem ("Can't reach MyAnimeList."), shown in the sync line.
    var problem: String?
    var loaded = false
    /// The last sync or send couldn't reach the service (the queue page's offline notice).
    var offline = false
    /// The active service wants a new sign-in (the queue page's "Sign in again").
    var signInAgain = false
    /// Bumped on every read, so an open details page reads its entry again.
    var version = 0

    func count(_ s: ListStatus) -> UInt32 { counts.first { $0.status == s }?.count ?? 0 }
}

/// A message at the bottom, with Undo when it is an edit.
struct Snack: Identifiable, Equatable {
    let id = UUID()
    let text: String
    var undo: Edit?
    /// A removal from the list: Undo discards the queued removal.
    var removal: Removal?
}

/// The app's state: the session, the list page, sign-in. The same as the Android app's
/// `AppViewModel`; talks to a `Repository`, the screens never call the core themselves.
@MainActor
final class AppModel: ObservableObject {
    static let redirectScheme = "io.github.hanami117.kotora"

    @Published private(set) var session: Session = .loading
    @Published private(set) var list = ListUi()
    @Published private(set) var mangaEnabled = false
    @Published var snack: Snack?
    /// The service that asked to sign in again (the "Sign in again" banner); the list stays.
    @Published var expired: Service?
    /// The service of the last sign-in started (AniList shows the token paste after a problem).
    @Published private(set) var lastSignIn: Service?

    private var repoOverride: Repository?
    var repo: Repository? { repoOverride ?? Core.repository }
    var isSample: Bool { repo?.isSample == true }

    /// MyAnimeList's state and PKCE verifier for the sign-in in progress. In memory only: the
    /// browser sheet lives in this process, and nothing secret goes into backed-up defaults.
    private var pending: PendingSignIn?

    let flusher = Flusher()

    init() {
        flusher.onFlushed = { [weak self] in Task { await self?.reload() } }
        Task { await start() }
    }

    func start() async {
        guard let r = repo else {
            session = .broken(Core.brokenMessage ?? "Kotora couldn't start.")
            return
        }
        let accounts = (try? await r.accounts()) ?? []
        if accounts.isEmpty {
            session = .signedOut()
            return
        }
        session = .signedIn(accounts: accounts)
        mangaEnabled = await r.mangaEnabled()
        await reload()
        if r.isSample { return }
        if list.syncedAt == nil { sync() } else { requestFlush() }
    }

    func requestFlush() {
        guard let r = repo, !r.isSample else { return }
        flusher.request(r)
    }

    // ------------------------------------------------------------------------------- the list

    func reload() async {
        guard let r = repo, case .signedIn = session else { return }
        var l = list
        do {
            if let src = r as? ListPageSource {
                let page = try await src.listPage(kind: l.kind, query: ListQuery(status: l.status, filter: l.filter, query: l.query, sort: l.sort, reverse: l.reverse))
                l.lines = page.lines
                l.rows = page.lines.map(\.row)
                (l.empty, l.emptyTitle, l.emptyHelp) = (page.empty, page.emptyTitle, page.emptyHelp)
                l.view = await src.listView(kind: l.kind, status: l.status)
            }
            l.counts = try await r.counts(kind: l.kind)
            l.syncedAt = await r.syncedAt(kind: l.kind)
            l.waiting = await r.waitingCount()
            l.loaded = true
        } catch KotoraError.NotSignedIn {
            session = .signedOut()
            return
        } catch {
            l.problem = message(of: error)
            l.loaded = true
        }
        // Keep what changed meanwhile (a sync started or ended, another page picked).
        guard l.kind == list.kind, l.status == list.status, l.sort == list.sort, l.reverse == list.reverse,
              l.filter == list.filter, l.query == list.query else { return }
        l.syncing = list.syncing
        l.offline = list.offline
        l.signInAgain = list.signInAgain
        l.version = list.version + 1
        list = l
    }

    func select(kind: MediaKind? = nil, status: ListStatus? = nil, sort: ListSort? = nil, reverse: Bool? = nil,
                filter: QuickFilter? = nil, query: String? = nil) {
        let kind = kind ?? list.kind
        let kindChanged = kind != list.kind
        let newStatus = kindChanged ? .watching : (status ?? list.status)
        // A new page starts on All (desktop `go`); a new order starts in its own direction.
        if kindChanged || newStatus != list.status { list.filter = .all }
        if let sort, sort != list.sort { list.reverse = false }
        list.kind = kind
        list.status = newStatus
        list.sort = sort ?? list.sort
        list.reverse = reverse ?? list.reverse
        list.filter = filter ?? list.filter
        list.query = query ?? list.query
        Task {
            if kindChanged && kind == .manga && !mangaEnabled { switchManga(true) } else { await reload() }
        }
    }

    /// List, Grid or Large for the page's tab (saved per tab).
    func setView(_ v: ListViewMode) {
        list.view = v
        guard let src = repo as? ListPageSource else { return }
        let (kind, status) = (list.kind, list.status)
        Task { await src.setListView(kind: kind, status: status, view: v) }
    }

    func sync() {
        guard let r = repo, !list.syncing else { return }
        list.syncing = true
        Task {
            do {
                let f = try await r.flush()
                _ = try await r.sync()
                list.problem = nil
                expired = nil
                list.offline = false
                list.signInAgain = f.signInAgain
                if case .signedIn = session { session = .signedIn(accounts: try await r.accounts()) }
            } catch KotoraError.SignInAgain {
                expired = session.active?.service
                list.problem = "Sign in again: the service no longer accepts Kotora's sign-in."
                list.signInAgain = true
            } catch KotoraError.Offline(let m) {
                list.problem = m
                list.offline = true
            } catch {
                list.problem = message(of: error)
            }
            list.syncing = false
            await reload()
        }
    }

    /// Pull to refresh: returns when the sync is over, so the spinner stays until then.
    func refresh() async {
        sync()
        while list.syncing { try? await Task.sleep(for: .milliseconds(100)) }
    }

    func plusOne(_ row: Row) {
        guard let r = repo else { return }
        Task {
            do {
                let edit = try await r.plusOne(kind: row.kind, id: row.id)
                await reload()
                requestFlush()
                snack = Snack(text: edit.message, undo: edit)
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    func undo(_ edit: Edit) {
        guard let r = repo else { return }
        snack = nil
        Task {
            do {
                try await r.undo(edit)
                await reload()
                requestFlush()
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    func entry(_ kind: MediaKind, _ id: UInt32) async -> Row? {
        try? await repo?.entry(kind: kind, id: id)
    }

    func switchManga(_ on: Bool) {
        guard let r = repo else { return }
        Task {
            try? await r.setMangaEnabled(on)
            mangaEnabled = on
            if !on && list.kind == .manga { list.kind = .anime; list.status = .watching }
            if on && !r.isSample { sync() } else { await reload() }
        }
    }

    // ------------------------------------------------------------------------------- sign-in

    func hasClientId(_ service: Service) -> Bool { repo?.hasClientId(service) == true }

    /// Opens the service's sign-in page in a browser sheet (ASWebAuthenticationSession, with the
    /// browser's saved sessions: never a web view of our own); the redirect comes back through
    /// `finishSignIn`.
    func signIn(_ service: Service, with auth: WebAuthenticationSession) {
        guard let r = repo else { return }
        lastSignIn = service
        Task {
            do {
                let p = try await r.signInStart(service)
                pending = p
                if !signedIn { session = .signedOut() }
                guard let url = URL(string: p.url) else { throw KotoraError.Other(message: "The sign-in page's address is broken.") }
                let back = try await auth.authenticate(using: url, callbackURLScheme: Self.redirectScheme, preferredBrowserSession: .shared)
                finishSignIn(back.absoluteString)
            } catch let e as ASWebAuthenticationSessionError where e.code == .canceledLogin {
                pending = nil // closed by the user: back to the buttons, no message
            } catch {
                fail(message(of: error))
            }
        }
    }

    /// The browser came back to `io.github.hanami117.kotora://oauth/…`.
    func finishSignIn(_ redirect: String) {
        guard let r = repo else { return }
        let service: Service
        if redirect.contains("://oauth/mal") { service = .mal } else if redirect.contains("://oauth/anilist") { service = .aniList } else { return }
        // AniList's implicit grant needs nothing kept; MyAnimeList's verifier and state must be ours.
        let p = pending.flatMap { $0.service == service ? $0 : nil }
            ?? (service == .aniList ? PendingSignIn(service: .aniList, url: "", state: "", verifier: "") : nil)
        guard let p else {
            fail("That sign-in was started before Kotora restarted. Sign in again.")
            return
        }
        pending = nil
        if signedIn { snack = Snack(text: "Signing in to \(service.label)…") } else { session = .signedOut(busy: true) }
        Task {
            do {
                let a = try await r.signInFinish(p, redirect: redirect)
                if expired == service { expired = nil }
                if signedIn { snack = Snack(text: "Signed in to \(service.label) as \(a.userName).") }
                await start()
            } catch {
                fail(message(of: error))
            }
        }
    }

    private var signedIn: Bool { if case .signedIn = session { return true } else { return false } }

    /// A sign-in problem: on the sign-in page when signed out, at the bottom when adding a second account.
    private func fail(_ text: String) {
        if signedIn { snack = Snack(text: text) } else { session = .signedOut(error: text) }
    }

    /// Show the lists of another signed-in account (MyAnimeList and AniList can both be signed in).
    func setActive(_ service: Service) {
        guard let r = repo, service != session.active?.service else { return }
        Task {
            do {
                try await r.setActive(service)
                list = ListUi(kind: list.kind == .manga && mangaEnabled ? .manga : .anime)
                await start()
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }

    func signOut(_ service: Service) {
        guard let r = repo else { return }
        Task {
            try? await r.signOut(service)
            list = ListUi()
            await start()
        }
    }

    /// Debug builds: the made-up lists, without signing in.
    func lookAround(_ sample: Repository) {
        repoOverride = sample
        list = ListUi()
        Task { await start() }
    }

    func leaveSample() {
        repoOverride = nil
        list = ListUi()
        Task { await start() }
    }
}
