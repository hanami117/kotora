# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); the user's decisions of 2026-11-14 are in its section 12.
v0.1 is MyAnimeList; AniList comes in v0.2.

## Status

- **Stage 0, design:** decisions recorded 2026-11-14 (branch `design`); waiting for the merge. No code yet.

## Stage 0: design (current)

- [x] Research: Taiga today, alternatives, building blocks, service terms, detection on Wayland and macOS
- [x] docs/DESIGN.md, docs/ROADMAP.md, CLAUDE.md
- [x] The user answers the open questions (DESIGN.md §12, "Decisions (2026-11-14)"); the design is updated
- [ ] Merge the `design` branch (the user's call)

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

## Stage 2: MyAnimeList, read only

- [ ] Sign-in: OAuth 2 PKCE (`plain`) with a loopback redirect and token refresh. The MAL Client ID comes
  from `config.toml` or a build-time default (`KOTORA_MAL_CLIENT_ID`); no secret in the repo. Token in a
  0600 file with a warning in Settings; sign out deletes it.
- [ ] Fetch the user's anime list (`GET /users/@me/animelist`, paged) into the store; covers through the
  art cache.
- [ ] List page by status: virtual rows, sorting, search filter, offline start from the cache.
- [ ] Rate limiter (MAL: 20 requests a minute budget, 1 s apart; limits undocumented) with 429 handling.
- [ ] A service trait in `core`, so AniList can be added in v0.2 without touching the queue.
- [ ] Scenes: signed out, no Client ID, loading, empty, list, offline.

## Stage 3: edits and sync queue

- [ ] Inline editor (progress, status, score) and `+1`.
- [ ] `pending_op` queue: coalescing, a worker (`PATCH my_list_status`), backoff, the 401 / failed-refresh
  "sign in again" state, conflict detection on the remote `updated_at`.
- [ ] Queue page with badge and per-op errors; conflict choice.
- [ ] `kot list`, `kot +1`, `kot set`, `kot queue`, `kot sync`.

## Stage 4: detection (completes v0.1)

- [ ] `detect`: MPRIS source (zbus, `xesam:url`, position) for VLC, mpv and mpc-qt; the mpv IPC source
  (configured socket paths) on Linux and macOS (IINA included); the VLC HTTP interface source on macOS.
- [ ] Verify mpc-qt's `xesam:url` (file URI?) on a real install, and VLC's `status.json` fields on macOS.
- [ ] Recognition: parse, cache, match against the list, one MAL search, confidence.
- [ ] Update decision (threshold), Now Playing banner, desktop notification, Update / Not now / Wrong
  anime.
- [ ] `kot now`, `kot confirm`, `kot dismiss`.
- [ ] Settings page: account, MAL Client ID, detection sources with live status, threshold, and the
  auto-update switch (off by default; scenes for both states).
- [ ] Integration test with a fake MPRIS player on a private bus.

## Stage 5: release 0.1

- [ ] `install.sh` (copy Uzumaki's: `~/.local`, desktop file, icons, `--uninstall`).
- [ ] Icon (placeholder until the separate icon work: a tiger that cites Taiga's), README with screenshots
  from `--snap` scenes and a clear credit to Taiga, About page credits, CHANGELOG.
- [ ] AUR PKGBUILD (source). AppImage and DMG are not part of v0.1 (v0.2).
- [ ] Live test by the user on Arch (Wayland and X11) and on the Mac.
- [ ] Make the repo public (the user's call).

## Later (v0.2 and on, order to be agreed)

- **v0.2:**
  - AniList (implicit grant; re-check the terms first) and several accounts
  - AppImage (Ubuntu 22.04 container) and DMG (ad-hoc signed)
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
  - Jellyfin, Plex and Kodi sources
  - offline mapping data (anime-offline-database, downloaded at runtime)
  - Kitsu and Shikimori
  - light theme
  - update notice
  - torrent feeds: opt-in, no default sources
  - streaming sites: MAL-Sync is recommended; no extension of our own for now
