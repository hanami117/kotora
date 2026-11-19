import Foundation

/// What the screens need: the core (`CoreRepository`) or made-up data (`SampleRepository`, for
/// screenshots and "Look around" in debug builds). The same interface as the Android app's
/// `Repository`. Every call may be made from the main actor: the implementations move blocking
/// work off it themselves.
///
/// Errors are `KotoraError`; `message(of:)` gives the words to show.
protocol Repository: AnyObject, Sendable {
    var isSample: Bool { get }

    func hasClientId(_ service: Service) -> Bool

    func accounts() async throws -> [Account]
    func list(kind: MediaKind, status: ListStatus?, sort: Sort) async throws -> [Row]
    func counts(kind: MediaKind) async throws -> [StatusCount]
    func entry(kind: MediaKind, id: UInt32) async throws -> Row?
    func syncedAt(kind: MediaKind) async -> Int64?
    func waitingCount() async -> UInt32
    func mangaEnabled() async -> Bool
    func setMangaEnabled(_ on: Bool) async throws

    func plusOne(kind: MediaKind, id: UInt32) async throws -> Edit
    func undo(_ edit: Edit) async throws

    /// Network.
    func sync() async throws -> SyncReport
    /// Network.
    func flush() async throws -> FlushReport

    func signInStart(_ service: Service) async throws -> PendingSignIn
    /// Network.
    func signInFinish(_ pending: PendingSignIn, redirect: String) async throws -> Account
    func signOut(_ service: Service) async throws
    /// Which signed-in account the lists show (the core keeps it).
    func setActive(_ service: Service) async throws
}

/// The text to show for an error from the core (its `Display` words) or from the system.
func message(of error: Error) -> String {
    if let e = error as? KotoraError {
        switch e {
        case .Offline(let m), .SignInAgain(let m), .RateLimited(let m), .NotSignedIn(let m),
             .NoClientId(let m), .SignInFailed(let m), .NotFound(let m), .Other(let m):
            return m.isEmpty ? "Something went wrong." : m
        }
    }
    return error.localizedDescription
}
