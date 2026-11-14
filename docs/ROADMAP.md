# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); open questions are in its section 12.

## Status

- **Stage 0, design:** in review (branch `design`). No code yet.

## Stage 0: design (current)

- [x] Research: Taiga today, alternatives, building blocks, service terms, detection on Wayland and macOS
- [x] docs/DESIGN.md, docs/ROADMAP.md, CLAUDE.md
- [ ] The user answers the open questions (DESIGN.md §12); the design is updated and merged

## Stage 1: skeleton

- [ ] Cargo workspace: `core`, `detect`, `app`, `ctl`. Use the same edition, resolver, dev profile and
  Rune rev as Uzumaki, one `[workspace.package]`, and a `BUILD` file.
- [ ] `app`: a Rune window with the sidebar and an empty list page, fake data, `scenes`, and
  `--snap/--tree` checks.
- [ ] `ctl`: the protocol crate copied from Uzumaki's `ctl/src/lib.rs` (socket path, serve, Client,
  version line), `kot status`, and single instance with `Show`.
- [ ] `core`: SQLite store with migrations; data model types; tests.
- [ ] Decide on anitomy: run `anitomy-ng` against a corpus of 200+ real file names (no personal paths)
  and compare with upstream's test data.
- [ ] `deny.toml`, clippy clean, Linux check script (from Uzumaki's `scripts/linux-check`).

## Stage 2: AniList, read only

- [ ] Sign-in: implicit grant with a loopback redirect, and the PIN fallback. Token in the keyring;
  sign out.
- [ ] Fetch the user's anime list (`MediaListCollection`) into the store; covers through the art cache.
- [ ] List page by status: virtual rows, sorting, search filter, offline start from the cache.
- [ ] Rate limiter (25 requests a minute budget) with 429 and `Retry-After` handling.
- [ ] Scenes: signed out, loading, empty, list, offline.

## Stage 3: edits and sync queue

- [ ] Inline editor (progress, status, score) and `+1`.
- [ ] `pending_op` queue: coalescing, a worker, backoff, the 401 "sign in again" state, conflict
  detection on the remote `updatedAt`.
- [ ] Queue page with badge and per-op errors; conflict choice.
- [ ] `kot list`, `kot +1`, `kot set`, `kot queue`, `kot sync`.

## Stage 4: detection (completes v0.1)

- [ ] `detect`: MPRIS source (zbus, `xesam:url`, position), and the mpv IPC source (configured socket
  paths) on Linux and macOS.
- [ ] Recognition: parse, cache, match against the list, one service search, confidence.
- [ ] Update decision (threshold), Now Playing banner, desktop notification, Update / Not now / Wrong
  anime.
- [ ] `kot now`, `kot confirm`, `kot dismiss`.
- [ ] Settings page: account, detection sources with live status, threshold.
- [ ] Integration test with a fake MPRIS player on a private bus.

## Stage 5: release 0.1

- [ ] `install.sh` (copy Uzumaki's: `~/.local`, desktop file, icons, `--uninstall`).
- [ ] Icon, README with screenshots from `--snap` scenes, CHANGELOG.
- [ ] AUR PKGBUILD (source) and, if wanted, AppImage (Ubuntu 22.04 container) and DMG (ad-hoc signed).
- [ ] Live test by the user on Arch (Wayland and X11) and on the Mac.
- [ ] Make the repo public (the user's call).

## Later (v0.2 and on, order to be agreed)

- **v0.2:**
  - MyAnimeList (PKCE `plain`) and several accounts
  - the mpv Lua script (`extras/mpv/kotora.lua` → `kot report`)
  - corrections UI ("this file / folder is X")
  - local history with undo
- **v0.3:**
  - fallback detectors: Linux `/proc/<pid>/fd`, macOS `libproc`, opt-in window titles on X11, Hyprland,
    sway and wlroots
  - anime-relations episode mapping
  - library folders and "play next episode"
- **v0.4:**
  - seasons browser
  - search and add
  - details page
  - statistics
  - Discord Rich Presence
  - tray, hidden start, auto-start
- **Later:**
  - browser extension with native messaging
  - Jellyfin, Plex and Kodi sources
  - offline mapping data (anime-offline-database, downloaded at runtime)
  - Kitsu and Shikimori
  - light theme
  - update notice
  - possibly torrent feeds (open question)
