import AuthenticationServices
import SwiftUI

/// "Sign in again" (desktop ui.rs `alert/expired` and queue.rs `queue/expired`): the service no
/// longer accepts Kotora's sign-in. The saved list and the waiting changes stay.
struct SignInAgainBanner: View {
    @Environment(\.palette) private var p
    @Environment(\.webAuthenticationSession) private var auth
    @EnvironmentObject private var model: AppModel
    /// The queue page's words (with the count of waiting changes) instead of the list's.
    var queue = false

    var body: some View {
        if let s = model.expired {
            HStack(spacing: 12) {
                Text(text(s)).font(Inter.regular(14)).foregroundStyle(p.text)
                    .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                Button { model.signIn(s, with: auth) } label: {
                    Text("Sign in").font(Inter.semibold(13)).foregroundStyle(p.onAccent)
                        .padding(.horizontal, 14).frame(height: 34)
                        .background(RoundedRectangle(cornerRadius: 8).fill(p.accent))
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(p.onHold.opacity(0.10)))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.onHold.opacity(0.4), lineWidth: 1))
            .padding(.horizontal, 16).padding(.vertical, 6)
            .accessibilityIdentifier("signInAgain")
        }
    }

    private func text(_ s: Service) -> String {
        guard queue else { return "\(s.label) needs you to sign in again. Your saved list still works." }
        let n = model.list.waiting
        return "Sign in again. Your \(s.label) sign-in expired. \(n == 1 ? "1 change" : "\(n) changes") wait\(n == 1 ? "s" : "") in the queue; nothing is lost."
    }
}

/// AniList's fallback (desktop setup.rs `setup/token`): the address the browser ended on, or the
/// token AniList showed, pasted by hand. "Use token" checks it with AniList and signs in.
struct TokenPasteField: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    var lead = "Or paste the address your browser ended on, or the token AniList showed:"
    @State private var text = ""
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(lead).font(Inter.regular(13)).foregroundStyle(p.dim).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                TextField("", text: $text, prompt: Text("Paste the token from AniList’s page").foregroundStyle(p.faint))
                    .font(.system(size: 12, design: .monospaced)).foregroundStyle(p.text)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .submitLabel(.go).onSubmit(use)
                    .onChange(of: text) { error = nil }
                    .padding(.horizontal, 10).frame(height: 40)
                    .background(RoundedRectangle(cornerRadius: 8).fill(p.field))
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(error == nil ? p.borderStrong : p.dropped.opacity(0.7), lineWidth: 1))
                    .accessibilityIdentifier("tokenField")
                Button(action: use) {
                    Group { if busy { ProgressView().tint(p.dim) } else { Text("Use token") } }
                        .font(Inter.medium(13)).foregroundStyle(p.text)
                        .padding(.horizontal, 14).frame(height: 40)
                        .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                }
                .buttonStyle(.borderless)
                .disabled(busy || text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let error {
                Text(error).font(Inter.regular(12)).foregroundStyle(p.dropped).fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("tokenError")
            }
        }
    }

    private func use() {
        let raw = text
        busy = true
        Task {
            error = await model.signInWithToken(raw)
            busy = false
            if error == nil { text = "" }
        }
    }
}

extension AppModel {
    /// Signs in with a pasted AniList token or address; the problem's words, or `nil` when it worked.
    func signInWithToken(_ raw: String) async -> String? {
        guard let r = repo as? AccountRepository else { return "Kotora couldn't start." }
        do {
            let a = try await r.signInWithToken(raw)
            snack = Snack(text: "Signed in to \(a.service.label) as \(a.userName).")
            expired = nil
            await start()
            return nil
        } catch {
            return message(of: error)
        }
    }
}
