import AuthenticationServices
import SwiftUI

/// M2's settings, as on Android: the account (sign out), the waiting changes, Manga lists, About.
/// The rest comes in M5. The theme follows the system on both phones.
struct SettingsScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @Environment(\.webAuthenticationSession) private var auth

    var body: some View {
        let accounts = model.session.accounts
        let waiting = model.list.waiting
        NavigationStack {
        VStack(spacing: 0) {
            AppBar(title: "Settings") { AccountMenu() }
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    SectionTitle(text: "Account")
                    Card {
                        ForEach(accounts, id: \.service) { a in
                            ListLine(title: a.service.label, subtitle: a.active ? "\(a.userName) · shown" : a.userName) {
                                EmptyView()
                            } trailing: {
                                if model.isSample {
                                    smallButton("Leave") { model.leaveSample() }
                                } else {
                                    HStack(spacing: 8) {
                                        if !a.active { smallButton("Show") { model.setActive(a.service) } }
                                        smallButton("Sign out") { model.signOut(a.service) }
                                    }
                                }
                            }
                        }
                        if !model.isSample {
                            ForEach([Service.mal, .aniList].filter { s in !accounts.contains { $0.service == s } }, id: \.self) { s in
                                ListLine(title: s.label, subtitle: "Not signed in") { EmptyView() } trailing: {
                                    smallButton("Sign in") { model.signIn(s, with: auth) }.disabled(!model.hasClientId(s))
                                }
                            }
                        }
                        QueueLink(waiting: waiting)
                        // Mirror edits to the other service (MirrorScreen.swift).
                        MirrorSettingsRow()
                    }
                    Text("Sign out removes this phone's copy of the list, its waiting changes and the sign-in.")
                        .font(Inter.regular(12)).foregroundStyle(p.faint)
                        .padding(.horizontal, 16).padding(.top, 6).padding(.bottom, 4)

                    SectionTitle(text: "Lists")
                    Card {
                        ListLine(title: "Manga lists", subtitle: "Also sync your manga list", divider: false) {
                            EmptyView()
                        } trailing: {
                            Toggle("", isOn: Binding(get: { model.mangaEnabled }, set: { model.switchManga($0) }))
                                .labelsHidden().tint(p.accent)
                        }
                    }

                    // Statistics, Appearance, Startup, Notifications, Fill volumes (Extras/SettingsExtras.swift).
                    SettingsExtras()

                    SectionTitle(text: "About")
                    Card {
                        ListLine(title: "Kotora \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? coreVersion())",
                                 subtitle: "A tribute to Taiga. GPL-3.0-or-later.") { EmptyView() } trailing: { EmptyView() }
                        ListLine(title: "Data", subtitle: "From MyAnimeList and AniList, for your own account. No Kotora server.") {
                            EmptyView()
                        } trailing: { EmptyView() }
                        NavigationLink { LicencesScreen() } label: {
                            ListLine(title: "Licences", subtitle: "Kotora, the fonts and the parts it is built from") { EmptyView() } trailing: {
                                Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(p.faint)
                            }
                        }
                        .buttonStyle(.plain)
                        AboutLinks()
                    }
                    .padding(.bottom, 24)
                }
            }
        }
        .background(p.bg)
        .toolbar(.hidden, for: .navigationBar)
        }
    }

    private func smallButton(_ title: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(Inter.medium(13)).foregroundStyle(p.text)
                .padding(.horizontal, 14).frame(height: 34)
                .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
    }
}

/// About → Licences: every part the app ships, in the desktop's words (the core's list).
struct LicencesScreen: View {
    @Environment(\.palette) private var p

    var body: some View {
        ScrollView {
            Card {
                let lines = licenceLines(android: false)
                ForEach(Array(lines.enumerated()), id: \.offset) { i, l in
                    ListLine(title: l.name, subtitle: l.licence.isEmpty ? nil : l.licence, divider: i < lines.count - 1) { EmptyView() } trailing: { EmptyView() }
                }
            }
            .padding(.vertical, 16)
        }
        .background(p.bg)
        .navigationTitle("Licences")
        .navigationBarTitleDisplayMode(.inline)
    }
}
