import SwiftUI
import UserNotifications

@main
struct KotoraApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    init() {
        // Covers and avatars: the system's URL cache in Caches (never backed up), 100 MB at
        // most, as the Android app's Coil disk cache.
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("kotora/covers", isDirectory: true)
        URLCache.shared = URLCache(memoryCapacity: 16 << 20, diskCapacity: 100 << 20, directory: dir)
        UNUserNotificationCenter.current().delegate = NotificationDelegate.shared
    }

    var body: some Scene {
        WindowGroup {
            Themed { RootView() }
                .environmentObject(model)
                .onOpenURL { model.finishSignIn($0.absoluteString) }
                .modifier(LaunchExtras())
        }
        .onChange(of: phase) { _, now in
            switch now {
            case .active:
                if model.list.waiting > 0, let r = model.repo { model.flusher.request(r) }
                if case .signedIn = model.session, !model.isSample { Task { await Notifier.check(chapters: false) } }
            case .background:
                if model.list.waiting > 0 && !model.isSample { Flusher.schedule() }
                if case .signedIn = model.session, !model.isSample { Notifier.scheduleRefresh() }
            default: break
            }
        }
        .backgroundTask(.appRefresh(Flusher.taskId)) { await Flusher.backgroundRun() }
        .backgroundTask(.appRefresh(Notifier.taskId)) { await Notifier.backgroundRun() }
    }
}

struct RootView: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel

    var body: some View {
        Group {
            switch model.session {
            case .loading:
                p.bg.ignoresSafeArea()
            case .broken(let text):
                Text(text).font(Inter.regular(15)).foregroundStyle(p.dropped).multilineTextAlignment(.center)
                    .padding(32).frame(maxWidth: .infinity, maxHeight: .infinity).background(p.bg)
            case .signedOut(let error, let busy):
                SignInScreen(error: error, busy: busy, lookAround: Self.lookAround(model))
            case .signedIn:
                SignedInView()
            }
        }
        .task { Self.debugLaunch(model) }
    }

    /// Debug builds only, as on Android: the made-up lists, without signing in.
    static func lookAround(_ model: AppModel) -> (() -> Void)? {
        #if DEBUG
        return { model.lookAround(SampleRepository()) }
        #else
        return nil
        #endif
    }

    /// Debug builds: `-sample YES` opens the sample lists at launch, `-plusOne <id>` then does a +1
    /// (the Undo bar), for screenshots. `-redirect <url>` hands a made-up sign-in redirect to the
    /// same code as `onOpenURL` (a test without Client IDs; `simctl openurl` asks first).
    static func debugLaunch(_ model: AppModel) {
        #if DEBUG
        let d = UserDefaults.standard
        // `-vaultCheck YES`: save, read and delete a made-up item with the vault's attributes.
        if d.bool(forKey: "vaultCheck") {
            let v = KeychainVault(), bytes = Data("vault-check".utf8)
            let ok = v.save("check", bytes) && v.load("check") == bytes
            v.delete("check")
            NSLog("kotora vault check: %@", ok && v.load("check") == nil ? "ok" : "FAILED")
        }
        if let url = d.string(forKey: "redirect") {
            Task {
                while model.session == .loading { try? await Task.sleep(for: .milliseconds(50)) }
                model.finishSignIn(url)
            }
        }
        guard d.bool(forKey: "sample") else { return }
        let sample = SampleRepository()
        model.lookAround(sample)
        let plus = UInt32(d.integer(forKey: "plusOne"))
        if plus > 0 {
            Task {
                if let row = try? await sample.entry(kind: .anime, id: plus) {
                    try? await Task.sleep(for: .milliseconds(300))
                    model.plusOne(row)
                }
            }
        }
        #endif
    }
}

enum Tab: String { case home, list, airing, search, settings }

struct RowRef: Hashable {
    let kind: MediaKind
    let id: UInt32
}

struct SignedInView: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var tab: Tab
    @State private var listPath: [RowRef]
    @State private var homePath: [RowRef] = []
    @State private var airingPath: [RowRef] = []

    /// Debug launch arguments for screenshots: `-screen list|details|airing|search|settings`, `-id 7`.
    init() {
        var screen = Prefs.startTab ?? "home"
        var id: UInt32 = 7
        #if DEBUG
        let d = UserDefaults.standard
        screen = d.string(forKey: "screen") ?? screen
        if d.integer(forKey: "id") > 0 { id = UInt32(d.integer(forKey: "id")) }
        #endif
        _tab = State(initialValue: screen == "details" ? .list : Tab(rawValue: screen) ?? .home)
        _listPath = State(initialValue: screen == "details" ? [RowRef(kind: .anime, id: id)] : [])
    }

    var body: some View {
        let manga = model.list.kind == .manga
        let account = model.session.active
        TabView(selection: $tab) {
            NavigationStack(path: $homePath) {
                HomeScreen(path: $homePath) { kind, status in
                    model.select(kind: kind, status: status)
                    listPath = []
                    tab = .list
                }
                .overlay(alignment: .bottom) { snack }
                .toolbar(.hidden, for: .navigationBar)
                .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
            }
            .tabItem { Label("Home", systemImage: "house") }
            .tag(Tab.home)

            NavigationStack(path: $listPath) {
                ListScreen(path: $listPath)
                    .overlay(alignment: .bottom) { snack }
                    .toolbar(.hidden, for: .navigationBar)
                    .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
            }
            .tabItem { Label("List", systemImage: "list.bullet") }
            .tag(Tab.list)

            NavigationStack(path: $airingPath) {
                Group {
                    if manga { PublishingScreen(path: $airingPath) } else { AiringScreen(path: $airingPath) }
                }
                .overlay(alignment: .bottom) { snack }
                .toolbar(.hidden, for: .navigationBar)
                .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
            }
                .tabItem { Label(manga ? "Publishing" : "Airing", systemImage: manga ? "book" : "calendar") }
                .tag(Tab.airing)

            SearchScreen()
                .overlay(alignment: .bottom) { snack }
                .tabItem { Label("Search", systemImage: "magnifyingglass") }
                .tag(Tab.search)

            SettingsScreen()
                .tabItem { Label("Settings", systemImage: "slider.horizontal.3") }
                .badge(Int(model.list.waiting))
                .tag(Tab.settings)
        }
        .tint(p.accent)
        .modifier(StartupState(tab: tab.rawValue))
        .animation(.easeOut(duration: 0.2), value: model.snack)
    }

    @ViewBuilder private var snack: some View {
        if let s = model.snack { SnackBar(snack: s).transition(.move(edge: .bottom).combined(with: .opacity)) }
    }
}

/// A page that comes in a later build (the same words as on Android).
struct Placeholder: View {
    @Environment(\.palette) private var p
    let title: String
    let text: String
    let account: Account?

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: title) { AccountMenu() }
            Text(text).font(Inter.regular(15)).foregroundStyle(p.faint).multilineTextAlignment(.center)
                .padding(.horizontal, 32).padding(.top, 48)
            Spacer()
        }
        .frame(maxWidth: .infinity)
        .background(p.bg)
    }
}
