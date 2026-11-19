import SwiftUI

@main
struct KotoraApp: App {
    @StateObject private var model = CoreModel()
    @AppStorage("appearance") private var appearance = 0

    var body: some Scene {
        WindowGroup {
            Themed { RootView() }
                .environmentObject(model)
                .preferredColorScheme(appearance == 1 ? .light : appearance == 2 ? .dark : nil)
        }
    }
}

enum Tab: String { case list, airing, search, settings }

struct RootView: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: CoreModel
    @State private var tab: Tab
    @State private var listPath: [RowRef]
    @State private var airingPath: [RowRef] = []

    /// Launch arguments for screenshots: `-screen list|details|airing|search|settings`, `-id 7`.
    init() {
        let d = UserDefaults.standard
        let screen = d.string(forKey: "screen") ?? "list"
        _tab = State(initialValue: screen == "details" ? .list : Tab(rawValue: screen) ?? .list)
        let id = UInt32(d.integer(forKey: "id"))
        _listPath = State(initialValue: screen == "details" ? [RowRef(kind: .anime, id: id == 0 ? 7 : id)] : [])
    }

    var body: some View {
        if let err = model.startError {
            Text("The Kotora core didn't start:\n\(err)").font(Inter.regular(15)).padding()
        } else {
            TabView(selection: $tab) {
                NavigationStack(path: $listPath) {
                    ListScreen(path: $listPath)
                        .overlay(alignment: .bottom) { snack }
                        .toolbar(.hidden, for: .navigationBar)
                        .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
                }
                .tabItem { Label("List", systemImage: "list.bullet") }
                .tag(Tab.list)

                NavigationStack(path: $airingPath) {
                    AiringScreen(path: $airingPath)
                        .toolbar(.hidden, for: .navigationBar)
                        .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
                }
                .tabItem { Label("Airing", systemImage: "calendar") }
                .tag(Tab.airing)

                SearchScreen()
                    .tabItem { Label("Search", systemImage: "magnifyingglass") }
                    .tag(Tab.search)

                SettingsScreen()
                    .tabItem { Label("Settings", systemImage: "slider.horizontal.3") }
                    .badge(Int(model.waiting))
                    .tag(Tab.settings)
            }
            .tint(p.accent)
            .animation(.easeOut(duration: 0.2), value: model.snack)
        }
    }

    @ViewBuilder private var snack: some View {
        if let s = model.snack { SnackBar(snack: s).transition(.move(edge: .bottom).combined(with: .opacity)) }
    }
}
