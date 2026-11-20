import Foundation

/// The Profile page, the "Open on …" links and the AniList token paste (the desktop's profile.rs,
/// ctxmenu.rs/details.rs links and setup.rs token field). Its own protocol, next to `Repository`,
/// so the two repositories gain it in this file.
protocol AccountRepository: AnyObject, Sendable {
    func profile(_ service: Service) async -> ProfileInfo?
    func serviceLinks(kind: MediaKind, id: UInt32) -> [ServiceLink]
    /// Network. AniList: the address the browser ended on, or the token AniList showed.
    func signInWithToken(_ raw: String) async throws -> Account
}

extension CoreRepository: AccountRepository {
    func profile(_ service: Service) async -> ProfileInfo? {
        await Task.detached(priority: .userInitiated) { self.kotora.profile(service: service) }.value
    }

    func serviceLinks(kind: MediaKind, id: UInt32) -> [ServiceLink] { kotora.serviceLinks(kind: kind, id: id) }

    func signInWithToken(_ raw: String) async throws -> Account {
        try await Task.detached(priority: .userInitiated) { try self.kotora.signInWithToken(raw: raw) }.value
    }
}

extension SampleRepository: AccountRepository {
    /// The desktop's made-up profile (`profile::fake`).
    func profile(_ service: Service) async -> ProfileInfo? {
        let counts: [(ListStatus, UInt32)] = [(.watching, 11), (.completed, 236), (.onHold, 7), (.dropped, 2), (.planToWatch, 48)]
        return ProfileInfo(service: .mal, userName: "hanami", pictureUrl: nil, joinedLine: "MyAnimeList · Joined Mar 2, 2019",
                           openLabel: "Open on MyAnimeList", profileUrl: profileUrl(service: .mal, userName: "hanami"),
                           days: 63.1, episodes: 3790, meanScore: 7.62,
                           counts: counts.map { ProfileCount(status: $0.0, count: $0.1) }, total: 304,
                           rewatchedLine: "Rewatched 4 times", pendingLine: nil,
                           scoreSpread: [0, 0, 1, 2, 5, 14, 38, 51, 27, 9], scoredLine: "147 scored · mean 7.62",
                           signedInLine: "Signed in as hanami on MyAnimeList.")
    }

    func serviceLinks(kind: MediaKind, id: UInt32) -> [ServiceLink] {
        [ServiceLink(service: .mal, label: "Open on MyAnimeList", url: mediaUrl(service: .mal, kind: kind, id: id))]
    }

    func signInWithToken(_ raw: String) async throws -> Account { throw KotoraError.NoClientId(message: "Sample data: no sign-in.") }
}
