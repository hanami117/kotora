import Foundation
import os
import Security

/// The real thing: `kotora-mobile` (Rust) through UniFFI. One per process (`AppModel`'s and the
/// background flush's), so the core's rate budgets cover both.
///
/// Files, as on Android (no backup of the list or the sign-in):
/// - `Application Support/Kotora/data`: the database (and the core's state); `…/config`:
///   `config.toml`. The whole `Kotora` folder is excluded from iCloud and computer backups.
/// - `Caches/kotora`: caches (never backed up; the system may clear it).
/// - The tokens: the Keychain (`KeychainVault`), this device only, not synchronised.
final class CoreRepository: Repository, @unchecked Sendable {
    /// Internal, not private: `Extras/Insights.swift` adds the Statistics and settings calls.
    let kotora: Kotora

    let isSample = false

    init() throws {
        let fm = FileManager.default
        let support = try fm.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        var root = support.appendingPathComponent("Kotora", isDirectory: true)
        let data = root.appendingPathComponent("data", isDirectory: true)
        let config = root.appendingPathComponent("config", isDirectory: true)
        let cache = try fm.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("kotora", isDirectory: true)
        for dir in [data, config, cache] {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try root.setResourceValues(values)

        // The Keychain outlives the app (a delete and a new install keep it); the database
        // doesn't. A sign-in without its database is a leftover: forget it, as an Android
        // uninstall would.
        let vault = KeychainVault()
        if !fm.fileExists(atPath: data.appendingPathComponent("kotora.db").path) {
            vault.deleteAll()
        }

        kotora = try Kotora(
            paths: Paths(configDir: config.path, dataDir: data.path, cacheDir: cache.path),
            ids: ClientIds(mal: Self.buildSetting("KotoraMalClientId"), anilist: Self.buildSetting("KotoraAniListClientId")),
            platform: IOSPlatform(vault: vault))
    }

    /// A Client ID from Info.plist (filled from Config/*.xcconfig); empty when none.
    private static func buildSetting(_ key: String) -> String {
        let v = (Bundle.main.object(forInfoDictionaryKey: key) as? String ?? "").trimmingCharacters(in: .whitespaces)
        return v.hasPrefix("$(") ? "" : v
    }

    /// Blocking core calls go to a worker thread, never the main one.
    private func io<T: Sendable>(_ f: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try f() }.value
    }

    private func io<T: Sendable>(_ f: @escaping @Sendable () -> T) async -> T {
        await Task.detached(priority: .userInitiated) { f() }.value
    }

    func hasClientId(_ service: Service) -> Bool { kotora.hasClientId(service: service) }

    func accounts() async throws -> [Account] { await io { self.kotora.accounts() } }
    func list(kind: MediaKind, status: ListStatus?, sort: Sort) async throws -> [Row] {
        try await io { try self.kotora.list(kind: kind, status: status, sort: sort) }
    }
    func counts(kind: MediaKind) async throws -> [StatusCount] { try await io { try self.kotora.counts(kind: kind) } }
    func entry(kind: MediaKind, id: UInt32) async throws -> Row? { try await io { try self.kotora.entry(kind: kind, id: id) } }
    func syncedAt(kind: MediaKind) async -> Int64? { await io { self.kotora.syncedAt(kind: kind) } }
    func waitingCount() async -> UInt32 { await io { self.kotora.waitingCount() } }
    func mangaEnabled() async -> Bool { await io { self.kotora.mangaEnabled() } }
    func setMangaEnabled(_ on: Bool) async throws { try await io { try self.kotora.setMangaEnabled(on: on) } }
    func plusOne(kind: MediaKind, id: UInt32) async throws -> Edit { try await io { try self.kotora.plusOne(kind: kind, id: id) } }
    func undo(_ edit: Edit) async throws { try await io { try self.kotora.undo(edit: edit) } }
    func sync() async throws -> SyncReport { try await io { try self.kotora.sync() } }
    func flush() async throws -> FlushReport { try await io { try self.kotora.flushQueue() } }
    func signInStart(_ service: Service) async throws -> PendingSignIn { try await io { try self.kotora.signInStart(service: service) } }
    func signInFinish(_ pending: PendingSignIn, redirect: String) async throws -> Account {
        try await io { try self.kotora.signInFinish(pending: pending, redirect: redirect) }
    }
    func setActive(_ service: Service) async throws { try await io { try self.kotora.setActiveService(service: service) } }
    func backgroundRefresh() async throws -> BackgroundReport { try await io { try self.kotora.backgroundRefresh() } }
    func signOut(_ service: Service) async throws {
        try await io { try self.kotora.signOut(service: service) }
        URLCache.shared.removeAllCachedResponses() // the covers and the avatar
    }
}

/// The core's window on iOS: the user's time zone, the log, the Keychain-kept tokens.
private final class IOSPlatform: Platform, @unchecked Sendable {
    private let vault: KeychainVault
    private let logger = Logger(subsystem: "io.github.hanami117.kotora", category: "core")

    init(vault: KeychainVault) { self.vault = vault }

    func utcOffset(atUnix: Int64) -> Int32 {
        Int32(TimeZone.current.secondsFromGMT(for: Date(timeIntervalSince1970: TimeInterval(atUnix))))
    }

    func log(level: LogLevel, message: String) {
        switch level {
        case .debug: logger.debug("\(message, privacy: .public)")
        case .info: logger.info("\(message, privacy: .public)")
        case .warn: logger.warning("\(message, privacy: .public)")
        case .error: logger.error("\(message, privacy: .public)")
        }
    }

    func tokenLoad(service: String) -> Data? { vault.load(service) }
    func tokenSave(service: String, bytes: Data) -> Bool { vault.save(service, bytes) }
    func tokenDelete(service: String) { vault.delete(service) }
}

/// The tokens at rest: one Keychain item per service (`mal`, `anilist`), readable after the first
/// unlock (so the background flush works), on this device only: not in iCloud Keychain, and not
/// restored to another phone from a backup. The same role as Android's `KeystoreVault`.
/// Never logs a token.
final class KeychainVault: @unchecked Sendable {
    private static let serviceAttr = "io.github.hanami117.kotora.token"
    private let logger = Logger(subsystem: "io.github.hanami117.kotora", category: "vault")

    private func query(_ service: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.serviceAttr,
            kSecAttrSynchronizable as String: false,
        ]
        if let service { q[kSecAttrAccount as String] = service }
        return q
    }

    func load(_ service: String) -> Data? {
        var q = query(service)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status != errSecSuccess && status != errSecItemNotFound {
            logger.warning("a saved sign-in can't be read (\(status)); sign in again")
        }
        return status == errSecSuccess ? out as? Data : nil
    }

    func save(_ service: String, _ bytes: Data) -> Bool {
        SecItemDelete(query(service) as CFDictionary)
        var q = query(service)
        q[kSecValueData as String] = bytes
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(q as CFDictionary, nil)
        if status != errSecSuccess { logger.warning("the sign-in can't be saved (\(status))") }
        return status == errSecSuccess
    }

    func delete(_ service: String) {
        SecItemDelete(query(service) as CFDictionary)
    }

    func deleteAll() {
        SecItemDelete(query(nil) as CFDictionary)
    }
}
