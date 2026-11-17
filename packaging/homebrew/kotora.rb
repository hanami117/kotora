# The Homebrew Cask for the tap hanami117/homebrew-kotora. The published copy lives in that repo
# (Casks/kotora.rb) and must be kept in sync with this file: change both, or neither.
# Users install it with: brew install --cask hanami117/kotora/kotora
#
# Fill in at release time:
#   - version   the release version, without the v. From build 2 of a version on (the BUILD file;
#               a rebuild without code changes) it is "<version>,<build>", e.g. "0.1.0,2".
#   - sha256    the DMG's checksum (Kotora-<version>.dmg.sha256 on the release)
# Test: brew audit --cask --strict hanami117/kotora/kotora; brew install --cask …; brew uninstall --zap …
#
# homebrew/cask proper needs a notarized app; the tap doesn't. The app is signed ad hoc, so the
# first launch is refused by Gatekeeper (the caveats below say what to do).
# The cask installs the same DMG as the website. The app sees the cask's receipt instead, so its
# update notice points to the package manager and its update check reports the channel "homebrew"
# (core/src/update.rs).
cask "kotora" do
  version "0.2.1"
  sha256 "95e5aee4e0416a11aaecb6b3fce149dde9f3ae88b10e467b3066b620320e86ef"

  url "https://github.com/hanami117/kotora/releases/download/v#{version}/Kotora-#{version}.dmg"
  name "Kotora"
  desc "Anime tracker that notices the episode you play and updates MyAnimeList and AniList"
  homepage "https://kotora.moe/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # The app's update notice only points to the package manager; it never replaces itself.
  auto_updates false
  depends_on arch: :arm64
  depends_on macos: :ventura

  app "Kotora.app"
  binary "#{appdir}/Kotora.app/Contents/MacOS/kot"

  uninstall quit: "io.github.hanami117.kotora"

  # Kotora keeps its files in XDG folders on macOS too (core/src/paths.rs). The sign-in token is in
  # tokens.json under the data folder, so zap removes it as well.
  zap trash: [
    "~/.cache/kotora",
    "~/.config/kotora",
    "~/.local/share/kotora",
  ]

  caveats <<~EOS
    Kotora is not notarized, so macOS refuses the first launch. Open
      System Settings → Privacy & Security
    and click "Open Anyway". Or run:
      xattr -dr com.apple.quarantine #{appdir}/Kotora.app
  EOS
end
