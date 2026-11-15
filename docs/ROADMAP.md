# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); the user's decisions of 2026-11-14 are in its section 12.
v0.1 is MyAnimeList; AniList comes in v0.2.

## Status

- **Stage 0, design:** done; `design` merged into `main` 2026-11-15.
- **Stage 0.5, look:** mockup and design spec approved by the user 2026-11-15.
- **Stage 1, skeleton:** next.

## Stage 0: design

- [x] Research: Taiga today, alternatives, building blocks, service terms, detection on Wayland and macOS
- [x] docs/DESIGN.md, docs/ROADMAP.md, CLAUDE.md
- [x] The user answers the open questions (DESIGN.md §12, "Decisions (2026-11-14)"); the design is updated
- [x] Merge the `design` branch (2026-11-15)

## Stage 0.5: look

- [x] `mockup/kotora.html`: static mockup of every v0.1 screen, Linux and macOS chrome
- [x] `docs/design-spec.md`: palette from the C2 icon, type, spacing, states, Rune tokens
- [x] `mockup/shots/`: screenshots for review
- [x] Review by the user (2026-11-15: approved)

## Lightness budgets (from Stage 1 on)

Kotora has no Electron, no bridge and no audio, so it must stay small. Measure in release builds at the end
of every stage and write the numbers here. A stage that breaks a budget explains why or fixes it.
The first numbers are targets; Stage 1 measures the real baseline and may adjust them once, with a reason.

| what | target | how to measure |
|---|---|---|
| release binary (`kotora`, stripped) | 25 MB or less | `ls -l target/release/kotora` |
| package (DMG / tarball, compressed) | 15 MB or less | size of the packaging output |
| memory at rest, window open, list of 300 entries | 100 MB or less | Linux: PSS from `/proc/<pid>/smaps_rollup`; macOS: `footprint` |
| memory at rest, window closed (tray / hidden) | 50 MB or less | same |
| CPU at rest | 0 %, no timers waking more than once a minute | `top`/`pidstat` over 60 s; no busy poll |
| start to first frame | 300 ms or less (warm) | `KOTORA_FRAME_LOG` or a timestamp log |
| `kot status` round trip | 20 ms or less | `time kot status` |

Rules that keep it there: no async runtime, no web view, workers only when there is work, covers decoded at
display size and evicted, detection by events (MPRIS signals, mpv IPC) not by polling, and every new
dependency checked for its size (`cargo bloat` when in doubt).

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
