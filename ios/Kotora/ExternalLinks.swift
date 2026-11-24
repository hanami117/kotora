import SwiftUI

/// "Open on MyAnimeList" / "Open on AniList" (desktop details.rs `det/open-mal`, ctxmenu.rs
/// `ctx/mal`, profile.rs `profile/open-mal`): opens the page in the browser.
struct OpenOnButton: View {
    @Environment(\.palette) private var p
    @Environment(\.openURL) private var openURL
    let label: String
    let url: String

    var body: some View {
        Button { if let u = URL(string: url) { openURL(u) } } label: {
            HStack(spacing: 8) {
                Text(label).font(Inter.medium(13)).foregroundStyle(p.text)
                Image(systemName: "arrow.up.right.square").font(.system(size: 13)).foregroundStyle(p.dim)
            }
            .padding(.horizontal, 12).frame(height: 34)
            .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
        .accessibilityAddTraits(.isLink)
    }
}

extension AppModel {
    /// The entry's "Open on …" links (the active service's page).
    func serviceLinks(_ kind: MediaKind, _ id: UInt32) -> [ServiceLink] {
        (repo as? AccountRepository)?.serviceLinks(kind: kind, id: id) ?? []
    }
}

/// The long-press menu's "Open on {service}" items (desktop ctxmenu.rs `open_items`).
struct OpenOnMenuItems: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openURL) private var openURL
    let kind: MediaKind
    let id: UInt32

    var body: some View {
        ForEach(model.serviceLinks(kind, id), id: \.url) { l in
            Button { if let u = URL(string: l.url) { openURL(u) } } label: { Label(l.label, systemImage: "arrow.up.right.square") }
        }
    }
}

/// About → Links and Support (desktop settings.rs `Row::Links`, `Row::Support`).
struct AboutLinks: View {
    @Environment(\.palette) private var p
    @Environment(\.openURL) private var openURL

    var body: some View {
        ListLine(title: "Links", subtitle: "The website, and the source code on GitHub.") { EmptyView() } trailing: { EmptyView() }
        HStack(spacing: 16) {
            link("kotora.moe", siteUrl())
            link("Source code", sourceUrl())
            Spacer()
        }
        .padding(.horizontal, 16).padding(.top, -6).padding(.bottom, 12)
        Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "heart").font(.system(size: 16)).foregroundStyle(p.accentHi).padding(.top, 2)
            VStack(alignment: .leading, spacing: 10) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Support Kotora").font(Inter.medium(15)).foregroundStyle(p.text)
                    Text("Kotora is free and built in my spare time. If you enjoy it, you can support it on Ko-fi.")
                        .font(Inter.regular(13)).foregroundStyle(p.dim).fixedSize(horizontal: false, vertical: true)
                }
                OpenOnButton(label: "Support on Ko-fi", url: supportUrl())
            }
            Spacer(minLength: 0)
        }
        .padding(16)
    }

    private func link(_ title: String, _ url: String) -> some View {
        Button(title) { if let u = URL(string: url) { openURL(u) } }
            .font(Inter.medium(14)).foregroundStyle(p.accentHi).buttonStyle(.borderless)
            .accessibilityAddTraits(.isLink)
    }
}
