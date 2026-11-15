# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); the user's decisions of 2026-11-14 are in its section 12.
v0.1 is MyAnimeList; AniList comes in v0.2.

## Status

- **Stage 0, design:** done; `design` merged into `main` 2026-11-15.
- **Stage 0.5, look:** mockup and design spec approved by the user 2026-11-15.
- **Stage 1, skeleton:** built on branch `stage-1` (2026-11-15), waiting for review. The anitomy decision is
  still open (see below). The SQLite store was finished in Stage 2.
- **Stage 2, MyAnimeList read only:** built on branch `stage-2` (2026-11-15, on top of `stage-1`), waiting for
  the user's MAL login test. All code is tested against recorded JSON; nothing has talked to the real MAL yet.

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

| what | target | how to measure | Stage 1 (2026-11-15) | Stage 2 (2026-11-15) |
|---|---|---|---|---|
| release binary (`kotora`, stripped) | 30 MB or less (was 25, see below) | `ls -l target/release/kotora` after `strip` | macOS arm64 19.2 MB; Linux arm64 (Docker) 25.2 MB. `kot` 0.5 MB | macOS arm64 23.1 MB (+3.9 MB: SQLite, ureq with rustls). `kot` 0.6 MB. Linux arm64 (Docker) 29.3 MB (+4.1 MB): 0.7 MB under the target, so the next stage must watch it |
| package (DMG / tarball, compressed) | 15 MB or less | size of the packaging output | no DMG yet; `tar.gz` of both stripped binaries: macOS 8.7 MB, Linux 11.1 MB | `tar.gz` of both stripped binaries, macOS: 10.7 MB |
| memory at rest, window open, list of 300 entries | 100 MB or less | Linux: PSS from `/proc/<pid>/smaps_rollup`; macOS: `footprint` | live only (agents never open a window); to be measured by the user | live only; to be measured by the user |
| memory at rest, window closed (tray / hidden) | 50 MB or less | same | macOS `--hidden`, 303 fake entries: 12 MB footprint (peak 89 MB during start) | macOS `--hidden`, empty scratch config: 12 MB footprint (peak 89 MB during start) |
| CPU at rest | 0 %, no timers waking more than once a minute | `top`/`pidstat` over 60 s; no busy poll | macOS `--hidden`: 0.01 s CPU in 60 s, 0 idle wakeups. Code: `poll` returns false, no `wake_after`, socket threads block in `accept`/`read` | macOS `--hidden`: 0.01 s CPU in 60 s. Code: sign-in waits only while a sign-in runs (at most 5 min), no refresh timer |
| start to first frame | 300 ms or less (warm) | `KOTORA_FRAME_LOG` or a timestamp log | headless (no window): 27–31 ms from `main` to the first encoded frame, 1280×800 at 2x; window creation is live only | not re-measured (headless scenes unchanged in cost); live number still the user's |
| `kot status` round trip | 20 ms or less | `time kot status` | 2.6 ms median, 3.3 ms max (50 runs, hidden app) | not re-measured |

Stage 2 changes no target. The binary grew from 19.2 to 23.1 MB on macOS (SQLite and ureq with rustls were
the expected few MB), so the 30 MB target holds on macOS. On Linux it is 29.3 MB, only 0.7 MB below it: Stage 3 and 4 (anitomy, zbus
sources) must be checked with `cargo bloat`, and the target may need one more honest adjustment.

The one adjustment (Stage 1): the binary target goes from 25 to 30 MB. Stage 1 is almost only Rune (wgpu, winit,
parley, resvg, zbus, AccessKit): 25.2 MB stripped on Linux before any Kotora feature. Stages 2–4 add
rusqlite (bundled SQLite), ureq with rustls, and anitomy, a few MB together. Rune without `ogg-audio` saved
1.3 MB on macOS; thin LTO did not help (19.7 MB). Measured with the same release profile as Uzumaki.

How the user measures the live numbers (release build, window open on the Watching list):

- start to first frame: `KOTORA_FRAME_LOG=/tmp/kot-frame.log kotora`, then read the file.
- memory, macOS: `footprint $(pgrep -x kotora)`; Linux: `grep Pss /proc/$(pgrep -x kotora)/smaps_rollup`.
- CPU at rest: `top -pid $(pgrep -x kotora)` (macOS) or `pidstat -p $(pgrep -x kotora) 5 12` (Linux), window
  idle and covered.

Rules that keep it there: no async runtime, no web view, workers only when there is work, covers decoded at
display size and evicted, detection by events (MPRIS signals, mpv IPC) not by polling, and every new
dependency checked for its size (`cargo bloat` when in doubt).

## Stage 1: skeleton

- [x] Cargo workspace: `core`, `detect`, `app`, `ctl`. Use the same edition, resolver, dev profile and
  Rune rev as Uzumaki, one `[workspace.package]`, and a `BUILD` file.
- [x] `app`: a Rune window with the sidebar and the list page (fake data, design-spec tokens), `scenes` for
  every status, chips, offline, empty and loading, and `--snap/--tree/--script` checks.
- [x] `ctl`: the protocol crate copied from Uzumaki's `ctl/src/lib.rs` (socket path, serve, Client,
  version line), `kot status`, `kot show`, `kot quit`, and single instance with `Show`.
- [x] `core`: SQLite store with migrations; data model types; tests (done in Stage 2: schema version 1 holds
  `account`, `media`, `entry` and `kv`; `pending_op` comes with the queue in Stage 3).
- [ ] Decide on anitomy: run `anitomy-ng` against a corpus of 200+ real file names (no personal paths)
  and compare with upstream's test data. (Not started.)
- [x] `deny.toml`, clippy clean, Linux check script (from Uzumaki's `scripts/linux-check`), `install.sh`.
- [x] Lightness budgets measured (table above).

## Stage 2: MyAnimeList, read only

Built 2026-11-15 on `stage-2`. Everything below is tested with recorded JSON and fake transports; the first
real sign-in and sync are the user's live test (STATUS.md says what to run).

- [x] Sign-in: OAuth 2 PKCE (`plain`) with the loopback redirect `http://127.0.0.1:47321/callback` and token
  refresh (before use, and once on a 401). The Client ID is read only from `config.toml` (`[mal] client_id`).
  Token in a 0600 file (folder 0700) with a warning in Settings; sign out deletes it and the saved list.
  *Open:* the build-time default `KOTORA_MAL_CLIENT_ID` is not built; the user asked for `config.toml` only.
- [x] Fetch the user's anime list (`GET /users/@me/animelist`, paged) into the store. The first live call
  confirms the `fields=` parameter MAL accepts (not verifiable offline).
  *Open:* covers. Rows still show the generated tiles; the cover URL is saved in `media`, and the art cache
  (Uzumaki's `art.rs` rules) is its own piece of work.
- [x] List page by status: virtual rows, sort (Last updated, Title, Score, Progress; the Sort control cycles),
  search over every saved title (Ctrl/Cmd F focuses it), offline start from the cache. `+1` is hidden in the
  real app: nothing is written to the account until Stage 3.
  *Open:* the 15-minute refresh while the window is visible (DESIGN.md §5.4). It needs a timer, so it waits
  until we decide how to keep CPU at 0. Today: sync at start, after sign-in, and on the Refresh button / Retry.
- [x] Rate limiter (20 requests a minute, 1 s apart) with 429 handling (`Retry-After`, at most 3 retries).
- [x] A service trait in `core` (`ListService`), so AniList can be added in v0.2 without touching the queue.
- [x] Scenes: signed out, no Client ID, signing in, sign-in failed, loading, empty, list, offline, syncing,
  sign in again, search, no match, sort, Settings (signed in and out).
- [ ] Live test by the user: sign in with the real MAL account, read the list (STATUS.md).

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
