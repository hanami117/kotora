import AuthenticationServices
import SwiftUI

/// The first page: one button per service. The sign-in opens in a browser sheet
/// (ASWebAuthenticationSession) and comes back to `io.github.hanami117.kotora://oauth/…`.
struct SignInScreen: View {
    @Environment(\.palette) private var p
    @Environment(\.webAuthenticationSession) private var auth
    @EnvironmentObject private var model: AppModel
    let error: String?
    let busy: Bool
    let lookAround: (() -> Void)?

    var body: some View {
        GeometryReader { geo in content(minHeight: geo.size.height) }
            .background(p.bg)
            .task {
                #if DEBUG
                // `-signIn mal|anilist`: press the button at launch (a test of the browser sheet).
                switch UserDefaults.standard.string(forKey: "signIn") {
                case "mal": model.signIn(.mal, with: auth)
                case "anilist": model.signIn(.aniList, with: auth)
                default: break
                }
                #endif
            }
    }

    private func content(minHeight: CGFloat) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                Spacer(minLength: 48)
                Image("Logo").resizable().frame(width: 132, height: 132)
                Text("Kotora").font(Inter.semibold(30)).foregroundStyle(p.text).padding(.top, 18).padding(.bottom, 6)
                Text("Your anime and manga list, kept up to date on MyAnimeList or AniList.")
                    .font(Inter.regular(16)).foregroundStyle(p.dim).multilineTextAlignment(.center)
                Spacer().frame(height: 28)
                if busy {
                    HStack(spacing: 12) {
                        ProgressView().tint(p.accent)
                        Text("Signing in…").font(Inter.regular(15)).foregroundStyle(p.dim)
                    }
                    .frame(height: 110)
                } else {
                    button("Sign in with MyAnimeList", .mal, primary: true)
                    button("Sign in with AniList", .aniList, primary: false).padding(.top, 10)
                    let noIds = [Service.mal, .aniList].filter { !model.hasClientId($0) }
                    if !noIds.isEmpty {
                        Text("This build has no Client ID for \(noIds.map(\.label).joined(separator: " or ")) yet.")
                            .font(Inter.regular(13)).foregroundStyle(p.onHold).multilineTextAlignment(.center).padding(.top, 12)
                    }
                }
                if let error {
                    Text(error).font(Inter.regular(13)).foregroundStyle(p.dropped).multilineTextAlignment(.center).padding(.top, 14)
                        .accessibilityIdentifier("signInError")
                    if model.lastSignIn == .aniList && !busy { TokenPasteField().padding(.top, 12) }
                }
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "lock").font(.system(size: 14)).foregroundStyle(p.faint).padding(.top, 2)
                    Text("The sign-in opens in your browser. Kotora never sees your password and has no server: your list goes straight to the service.")
                        .font(Inter.regular(13)).foregroundStyle(p.faint).lineSpacing(3)
                }
                .padding(.top, 18)
                if let lookAround {
                    Button("Look around with sample data", action: lookAround)
                        .font(Inter.medium(14)).foregroundStyle(p.accentHi).frame(height: 40).padding(.top, 12)
                }
                Spacer(minLength: 24)
                Text("No account with Kotora · no ads · no tracking").font(Inter.regular(12)).foregroundStyle(p.faint).padding(.bottom, 18)
            }
            .padding(.horizontal, 28)
            .frame(minHeight: minHeight)
        }
        .scrollBounceBehavior(.basedOnSize)
    }

    private func button(_ title: String, _ service: Service, primary: Bool) -> some View {
        let on = model.hasClientId(service)
        return Button { model.signIn(service, with: auth) } label: {
            Text(title).font(Inter.semibold(15))
                .foregroundStyle(primary ? p.onAccent : p.text)
                .frame(maxWidth: .infinity).frame(height: 50)
                .background(RoundedRectangle(cornerRadius: 12).fill(primary ? p.accent : p.raised))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(primary ? .clear : p.borderStrong, lineWidth: 1))
        }
        .disabled(!on)
        .opacity(on ? 1 : 0.45)
    }
}
