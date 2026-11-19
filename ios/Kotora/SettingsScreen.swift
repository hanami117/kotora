import SwiftUI

struct SettingsScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: CoreModel
    @AppStorage("appearance") private var appearance = 0

    var body: some View {
        let _ = model.version
        VStack(spacing: 0) {
            AppBar(title: "Settings") { EmptyView() }
            ScrollView {
                VStack(spacing: 0) {
                    SectionTitle(text: "Accounts")
                    Card {
                        ForEach(model.accounts, id: \.userName) { a in
                            ListLine(title: "\(a.userName) · \(a.service.label)", subtitle: "Sample account · nothing is sent") {
                                Avatar(letter: String(a.userName.prefix(1)).uppercased(), anilist: a.service == .aniList)
                            } trailing: {
                                if a.active { Tag(text: "Main", style: .plain).foregroundStyle(p.watching) }
                            }
                        }
                        ListLine(title: "Sign in to MyAnimeList", subtitle: "Comes later on iPhone (needs an iOS client id)") {
                            Avatar(letter: "M", size: 32).opacity(0.45)
                        } trailing: { Image(systemName: "lock").foregroundStyle(p.faint) }
                        ListLine(title: "Sign in to AniList", subtitle: "Comes later on iPhone") {
                            Avatar(letter: "A", anilist: true, size: 32).opacity(0.45)
                        } trailing: { Image(systemName: "lock").foregroundStyle(p.faint) }
                        ListLine(title: "Sync queue", subtitle: model.waiting == 0 ? "Nothing waiting" : "\(model.waiting) changes waiting · kept on this phone", divider: false) {
                            Image(systemName: "tray.full").foregroundStyle(p.dim)
                        } trailing: {
                            if model.waiting > 0 { Tag(text: "\(model.waiting)", style: .wait) }
                        }
                    }

                    SectionTitle(text: "Lists")
                    Card {
                        ListLine(title: "Manga lists", subtitle: "Show Anime | Manga on the list page", divider: false) {
                            EmptyView()
                        } trailing: {
                            Toggle("", isOn: Binding(get: { model.mangaEnabled }, set: { model.setMangaEnabled($0) }))
                                .labelsHidden().tint(p.accent)
                        }
                    }

                    SectionTitle(text: "Appearance")
                    Card {
                        Segmented(items: [(0, "System"), (1, "Light"), (2, "Dark")], selection: $appearance, height: 28)
                            .padding(12)
                    }

                    SectionTitle(text: "About")
                    Card {
                        ListLine(title: "Kotora \(coreVersion())", subtitle: "iOS spike · GPL-3.0-or-later · a tribute to Taiga", divider: false) {
                            Image("Logo").resizable().frame(width: 36, height: 36)
                        } trailing: { EmptyView() }
                    }
                    Text("Sample data from the Rust core (kotora-mobile). No account, no network.")
                        .font(Inter.regular(12)).foregroundStyle(p.faint).multilineTextAlignment(.center)
                        .padding(24)
                }
            }
        }
        .background(p.bg)
    }
}

struct SearchScreen: View {
    @Environment(\.palette) private var p

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: "Search") { EmptyView() }
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").foregroundStyle(p.faint)
                Text("Search anime").font(Inter.regular(15)).foregroundStyle(p.faint)
                Spacer()
            }
            .padding(.horizontal, 14).frame(height: 46)
            .background(RoundedRectangle(cornerRadius: 12).fill(p.field))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(p.borderStrong, lineWidth: 1))
            .padding(.horizontal, 16)
            Spacer()
            Text("Search needs the network; the spike has none.").font(Inter.regular(14)).foregroundStyle(p.faint)
            Spacer()
        }
        .background(p.bg)
    }
}
