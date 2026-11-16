# Changelog

All notable changes to Kotora. Versions follow [semantic versioning](https://semver.org/).

## [Unreleased]

## [0.1.3] - 2026-11-16

### Changed
- **Signing in to MyAnimeList is one click.** Kotora now has its own MyAnimeList client ID built in (a client ID is
  public by design, as in Taiga), so there is nothing to register or paste. The setup wizard is three steps:
  Welcome, Sign in, Done. A client ID you saved earlier keeps being used.
- **Your own client ID is an advanced option:** Settings → Account → **Use my own client ID…**, and **Use Kotora's
  built-in ID** to go back. `kot config client-id ID` still works, and so does `KOTORA_MAL_CLIENT_ID` (it wins).

### Packaging
- `kotora-bin` is on the AUR (prebuilt x86_64 binary). AUR `kotora` 0.1.2-2 fixes `kot --version`.

## [0.1.2] - 2026-11-16

### Added
- **`kot` with no command is a dashboard:** a now-playing line (player, title, episode, time, and whether an
  update is offered), then your Watching list sorted by what you can watch next: a progress bar with
  watched/total, and NEXT ("▶ Ep 6 on disk", else the next air time like "Wed 18:00 (in 2d)", else
  "aired · not on disk"), then the sync queue and the last sync. It fits the terminal width, uses colour only
  on a terminal with `NO_COLOR` unset, and is plain text when piped. The data comes in one request, and an
  older Kotora that doesn't know it makes `kot` print the status lines instead.
- **`kot status --short`** prints the status as one line.
- **`kot schedule`** is another name for `kot airing`.

### Fixed
- **`kot` picks the right anime when a title matches several:** a title that is exactly what you typed (any
  title, any case) wins over titles that only contain it. `kot next`, `kot +1` and `kot set` prefer the anime you
  are watching or rewatching, and `kot next` then the ones whose next episode is in your library. If more than
  one is left, the "N anime match" error lists those first.

## [0.1.1] - 2026-11-16

### Added
- **Play next in the tray:** the menu bar (macOS) or tray (Linux) menu lists up to 8 Watching anime whose next
  episode is a file in your library folders ("Title · Ep N", most recently updated first); picking one opens it
  as ▶ Play does. Opening the menu (macOS) or the window looks for new files.

### Changed
- Library folders are no longer walked every 5 minutes: Kotora looks again when the window is shown or the tray
  menu opens (at most once a minute), on Rescan, and when the folders change. At rest nothing runs.

### Fixed
- Now Playing: when the playing file matches no anime, the "Offer at N%" label and the threshold tick are
  hidden (no update is ever offered), and the button says "Pick anime…" instead of "Wrong anime…".

## [0.1.0] - 2026-11-16

First release. MyAnimeList only. Linux (built from source or the AUR) and macOS (Apple silicon, a DMG).

### Added
- **Your list:** Watching, Plan to watch, Completed, On hold and Dropped, as rows, a grid or large cards, with
  covers, progress, score, airing information, status chips, a Sort menu and search. A right-click menu on rows,
  posters and Home cards. The list is kept in a local SQLite database and works offline.
- **Home:** shelves for what you're watching, what's next and what's airing.
- **MyAnimeList sign-in** with PKCE (no client secret). A setup wizard asks which service you use and walks you
  through getting a Client ID; `kot config client-id ID` does the same. Tokens are stored in a private file. The
  account card opens a Profile page; the sidebar shows your MyAnimeList picture.
- **Edits and the sync queue:** change progress (`+1`), score and status, and rewatch a completed anime. Changes
  wait in a visible queue, are sent in order, retried with backoff, and conflicts with changes made elsewhere are
  shown.
- **Detection:** mpv (IPC socket), MPRIS players on Linux (VLC, mpc-qt, Celluloid, Haruna, SMPlayer, …), IINA and
  VLC's web interface on macOS, and a zero-setup process source that finds a player by its open video file. A
  watched-time rule decides when an episode counts. Now Playing shows the live position. When mpv or IINA is seen
  by its open file only, Kotora offers once to turn on precise detection (one line in `mpv.conf`, only when you
  click).
- **Recognition:** file names are parsed with anitomy-ng and matched to your list, with season handling and
  anime-relations (a "Season 2 - 15" release goes to the right MyAnimeList entry).
- **Updates ask first:** an offer to record the episode (Update, Not now, Wrong anime…). Updating automatically is
  a switch, off by default, with Undo. It still asks when the episode would skip one or hasn't aired yet.
- **Not aired yet:** `+1`, the editor and `kot +1` / `kot set` ask "Mark anyway?" for an episode past the airing
  estimate (`kot`: `--anyway`). Only the total episode count is a hard limit.
- **Add as Watching at episode N:** one button on Now Playing's "Not on your list" card adds the anime and records
  the playing episode in one change.
- **Search and add:** search your list and MyAnimeList together (Ctrl K), open a MyAnimeList id or link, add with
  one click, and a details page with synopsis, score, studios and your own entry.
- **Airing:** this week and this season from MyAnimeList, and a season picker for the seasons before and after.
- **Library folders and Play next episode:** Settings → Library takes the folders where your anime are. Kotora
  finds the episodes and links them to your list. ▶ on Watching rows, Play on the details page, "Up next" on Now
  Playing, a middle-click or P, and `kot next TITLE` open the next episode in the player you choose.
- **Notifications** with a switch for each: Episode detected (off by default), Update offered (Update, Not now),
  Episode recorded (Undo), Started airing and New episode. A new notification about an episode replaces the last.
- **Tray and start at login:** closing the window keeps Kotora running in the tray (Linux) or the menu bar (macOS),
  so detection keeps working. Settings can start Kotora at login and in the background.
- **macOS:** a menu bar (About, Settings… ⌘,, a Go menu), and the system's parts follow Kotora's theme.
- **Themes:** dark, light, or follow the system. A title language setting (romaji, English or Japanese).
- **Settings** in sections (General, Appearance, Account, Detection, Library, Notifications, About) with a search
  field.
- **Update check:** once a day Kotora asks kotora.moe whether there is a newer version and shows a banner. It never
  downloads anything by itself, and Settings → About Kotora → Check for updates turns it off. See Privacy in the
  README for what it sends.
- **小虎** beside the logo and in the window title, and a short claw-mark startup animation (Settings → Appearance →
  Startup animation; a fade with Reduce Motion).
- **`kot`:** `status`, `list`, `+1`, `set`, `add`, `next`, `queue`, `sync`, `now`, `confirm`, `dismiss`,
  `config`, `show`, `quit`.
- A single instance per user; `kot` and the app share one socket.
- `install.sh` for Linux, a desktop entry and icons, an AUR package, and a macOS DMG (ad-hoc signed, not
  notarized).

### Not in this release
- AniList, several accounts, a corrections UI, manga, and an AppImage (planned for later versions).
