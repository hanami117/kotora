# Changelog

All notable changes to Kotora. Versions follow [semantic versioning](https://semver.org/).

## [Unreleased]

### Added
- **Not aired yet:** `+1`, the editor and `kot +1` / `kot set` ask "Mark anyway?" for an episode past the airing
  estimate (`kot`: `--anyway` skips the question). Only the total episode count is a hard limit.
- **Add as Watching at episode N:** one button on Now Playing's "Not on your list" card adds the anime and
  records the playing episode in one change.
- **Notifications with buttons:** Settings → Notifications has a switch for each: Episode detected (off by
  default), Update offered (with Update and Not now) and Episode recorded (with Undo). The buttons act as the
  banner does. A new notification about an episode replaces the last. Linux: where the notification server
  shows buttons (KDE, GNOME, …); macOS: in Kotora.app.
- **Precise detection prompt:** when the player you watch with (mpv, IINA) is seen by its open file only, Now
  Playing offers once to turn on precise detection, with Turn on and Not now.

### Changed
- **Update automatically asks** instead of recording when the episode would skip one (two or more ahead of your
  progress) or hasn't aired yet by the estimate. The banner and `kot now` say why.

## [0.1.0] - date to be set

First release. MyAnimeList only; Linux first, macOS from source.

### Added
- **Your list:** Watching, Plan to watch, Completed, On hold and Dropped, with covers, progress, score, airing
  information, status chips, sorting and search. The list is kept in a local SQLite database and works offline.
- **MyAnimeList sign-in** with PKCE (no client secret). The Welcome page guides you through getting a Client ID;
  `kot config client-id ID` does the same. Tokens are stored in a private file.
- **Edits and the sync queue:** change progress (`+1`), score and status. Changes wait in a visible queue,
  are sent in order, retried with backoff, and conflicts with changes made elsewhere are shown.
- **Detection:** mpv (IPC socket), MPRIS players on Linux (VLC, mpc-qt, Celluloid, Haruna, SMPlayer, …), VLC's web
  interface and IINA on macOS, and a zero-setup process source that finds a player by its open video file. A
  watched-time rule decides when an episode counts. Desktop notifications.
- **Recognition:** file names are parsed with anitomy-ng and matched to your list, with season handling.
- **Updates ask first:** an offer to record the episode (Update, Not now, Wrong anime…). Automatic updating is a
  switch, off by default, with Undo.
- **Search and add:** search your list and MyAnimeList together, add with one click, and a details page with
  synopsis, score, studios and your own entry.
- **Settings:** account, players, updates, the `mpv.conf` helper for precise detection, and About.
- **`kot`:** `status`, `list`, `+1`, `set`, `add`, `queue`, `sync`, `now`, `confirm`, `dismiss`, `config`, `show`, `quit`.
- A single instance per user; `kot` and the app share one socket.
- `install.sh` for Linux, a desktop entry and icons, and an AUR package draft.

### Not in this release
- AniList, several accounts, a corrections UI, library folders, AppImage and DMG (planned for 0.3).
