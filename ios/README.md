# Kotora for iOS (spike)

A small SwiftUI app over `kotora-mobile` (the same Rust core and UniFFI API as the Android app), to see
what Kotora looks like on an iPhone. Sample data only: no sign-in, no network.

- The list, its counts, details, +1 and Undo go through the real core (`Kotora` object, SQLite store,
  change queue). The sample list is written by `mobile/examples/demo_db.rs` through the core's store and
  copied into the app's data folder on first launch (`-resetSample` copies it again).
- The build has no Client IDs and the token vault only hands out a made-up token, so the core can't
  reach MyAnimeList or AniList. Sync only says that nothing is sent.
- Airing uses made-up weekly slots in Swift (kotora-mobile has no airing API yet); Search and sign-in
  are placeholders.
- Colours are the desktop's tokens (light and dark), Inter is the Android app's copy
  (`android/app/src/main/res/font`, OFL), the icon is `assets/icons/io.github.hanami117.kotora.svg`.

## Build and run (macOS, Xcode, no paid account)

```sh
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
brew install xcodegen
ios/scripts/build-rust.sh release        # static libs, Swift bindings, build/KotoraCore.xcframework
ios/scripts/demo-db.sh                   # build/demo.db
ios/scripts/run-sim.sh "iPhone 17"       # xcodegen, xcodebuild, boot, install, launch
```

`scripts/shots.sh <dir>` takes light and dark screenshots of every screen from the booted Simulator.
`build-rust.sh release --sim-only` skips the device library. Launch arguments for screenshots:
`-screen list|details|airing|search|settings`, `-id <anime id>`, `-appearance 0|1|2` (system, light, dark).

`Kotora.xcodeproj` is generated from `project.yml` (`xcodegen generate`) and is not checked in.

## On your own iPhone with a free Apple ID

Build the device library too (`build-rust.sh release`, without `--sim-only`), run `xcodegen generate`, open
`ios/Kotora.xcodeproj`, then Xcode → Settings → Accounts → add the Apple ID; target Kotora → Signing &
Capabilities → Team: "(Personal Team)"; bundle id: make it unique if Xcode asks. Connect the iPhone and
turn on Developer Mode (Settings → Privacy & Security). Run. Trust the developer on the phone (Settings →
General → VPN & Device Management). A free team's profile lasts 7 days; run from Xcode again to renew it.
