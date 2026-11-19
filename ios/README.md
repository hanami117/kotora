# Kotora for iOS

A SwiftUI app over `kotora-mobile` (the same Rust core and UniFFI API as the Android app), at the
Android app's M2 level: sign-in, the real list from the core's store, sync, +1 and Undo through the
core's queue, a background flush. The screens, words and data layer mirror `android/` one to one
(`Data/Repository.swift` ↔ `data/Repository.kt`, `AppModel.swift` ↔ `ui/AppViewModel.kt`).

- **Sign-in**: MyAnimeList (PKCE) and AniList (implicit grant) in a browser sheet
  (`ASWebAuthenticationSession`, the shared Safari session: never a web view of our own). The
  redirects are the Android app's: `io.github.hanami117.kotora://oauth/mal` and `…/oauth/anilist`;
  ONE MyAnimeList client (app type "other") and ONE AniList client serve both phones. Client IDs
  are in `Config/Shared.xcconfig` (public by design, the same as `android/gradle.properties`).
- **Tokens**: the Keychain, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, not synchronised
  (no iCloud Keychain, not restored to another phone). A new install forgets a sign-in left in the
  Keychain by a deleted one (the database is gone, as after an Android uninstall).
- **Files**: `Application Support/Kotora/{data,config}`, excluded from iCloud and computer backups;
  caches and covers in `Caches`.
- **Sync**: pull to refresh (sends the waiting changes, then fetches the list). +1 and Undo go
  into the core's queue and are sent at once, when the network comes back, and by a
  `BGAppRefreshTask` (`io.github.hanami117.kotora.flush`) while changes wait.
- **Debug builds** have "Look around with sample data" (made-up lists in memory, nothing sent).
- Airing, Search, the editor and the service's details pages come later (M3/M4), on both phones.

## Build and run (macOS, Xcode, no paid account)

```sh
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
brew install xcodegen
ios/scripts/build-rust.sh release        # static libs, Swift bindings, build/KotoraCore.xcframework
ios/scripts/run-sim.sh "iPhone 17"       # xcodegen, xcodebuild, boot, install, launch (debug)
```

`build-rust.sh release --sim-only` skips the device library. `Kotora.xcodeproj` is generated from
`project.yml` (`xcodegen generate`) and is not checked in. The Simulator build is ad-hoc signed: an
unsigned app gets no Keychain.

Debug launch arguments (`run-sim.sh "iPhone 17" <args>` or `xcrun simctl launch booted io.github.hanami117.kotora.debug <args>`):
- `-sample YES`: open the sample lists; with `-screen list|details|airing|search|settings`,
  `-id <anime id>`, `-plusOne <anime id>` (the Undo bar). `scripts/shots.sh <dir>` takes light and
  dark screenshots of every screen this way.
- `-redirect '<url>'`: hand a sign-in redirect to the app, as the browser would (a test without an
  account: `io.github.hanami117.kotora://oauth/anilist#error=access_denied`). `simctl openurl` works
  too, but iOS asks "Open in Kotora?" first.
- `-signIn mal|anilist`: press the sign-in button at launch.
- `-vaultCheck YES`: save, read and delete a made-up Keychain item (logs "kotora vault check: ok").

## On your own iPhone with a free Apple ID

1. `ios/scripts/build-rust.sh release` (with the device library), then `cd ios && xcodegen generate`.
2. Put your team in `ios/Config/Local.xcconfig` (git-ignored):
   `DEVELOPMENT_TEAM = <your team id>`. If Xcode says the bundle id is taken, add
   `PRODUCT_BUNDLE_IDENTIFIER = io.github.hanami117.kotora.debug.<something>` there too. (Or set
   both in Xcode → target Kotora → Signing & Capabilities: Team "(Personal Team)"; `xcodegen generate`
   resets that.)
3. Xcode → Settings → Accounts: add the Apple ID. Open `ios/Kotora.xcodeproj`, connect the iPhone,
   turn on Developer Mode on it (Settings → Privacy & Security), pick it as the run destination, Run.
4. On the phone: Settings → General → VPN & Device Management → trust the developer. A free team's
   profile lasts 7 days: run from Xcode again to renew it (the list and sign-in stay).
