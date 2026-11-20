import SwiftUI

/// What the app does at launch from the settings (`Prefs`): the theme, the intro over everything
/// on a cold start, and (signed in) the page and the Anime | Manga switch to open on, as the
/// desktop's `startup.rs` plans them.
struct LaunchExtras: ViewModifier {
    @ObservedObject private var prefs = Prefs.shared
    @State private var intro = LaunchExtras.introAtLaunch()

    func body(content: Content) -> some View {
        content
            .overlay {
                if intro {
                    Themed {
                        IntroView { intro = false }
                    }
                    .transition(.identity)
                }
            }
            .preferredColorScheme(prefs.colorScheme)
    }

    /// A cold start plays it (this is made once per process), unless Settings → Appearance →
    /// Startup animation is off. Debug builds: not with `-sample` (screenshots) unless `-intro YES`;
    /// `-intro NO` never.
    @MainActor static func introAtLaunch() -> Bool {
        guard !Prefs.shared.introPlayed else { return false }
        Prefs.shared.introPlayed = true
        var on = Prefs.shared.settings.intro
        #if DEBUG
        let d = UserDefaults.standard
        if d.object(forKey: "intro") != nil { on = d.bool(forKey: "intro") } else if d.bool(forKey: "sample") { on = false }
        #endif
        return on
    }
}

/// Signed in: remembers the page and the switch as they change (`[startup] last_page`,
/// `[display] list_kind`), and at the first appearance puts the switch where Mode says.
struct StartupState: ViewModifier {
    @EnvironmentObject private var model: AppModel
    let tab: String

    func body(content: Content) -> some View {
        content
            .onChange(of: tab) { _, t in Prefs.shared.set(.lastPage(value: t)) }
            .onChange(of: model.list.kind) { _, k in
                if Prefs.shared.settings.listKind != k { Prefs.shared.set(.listKind(kind: k)) }
            }
            .task {
                guard !StartupGate.done else { return }
                StartupGate.done = true
                let k = Prefs.shared.startKind(mangaOn: model.mangaEnabled)
                if k != model.list.kind { model.select(kind: k) }
            }
    }
}

@MainActor enum StartupGate {
    static var done = false
}

extension Prefs {
    /// The tab to open on (Open on: Last page), read before the first view.
    nonisolated static var startTab: String? {
        MainActor.assumeIsolated { shared.startTab }
    }
}
