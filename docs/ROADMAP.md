# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); the user's decisions of 2026-11-14 are in its section 12.
v0.1 is MyAnimeList; AniList comes in v0.2.

## Status

- **Stage 0, design:** done; `design` merged into `main` 2026-11-15.
- **Stage 0.5, look:** mockup and design spec approved by the user 2026-11-15.
- **Stage 1, skeleton:** built on branch `stage-1` (2026-11-15), waiting for review. The anitomy decision was
  taken on 2026-11-15 (see Stage 1). The SQLite store was finished in Stage 2.
- **Stage 2, MyAnimeList read only:** built on branch `stage-2` (2026-11-15, on top of `stage-1`), waiting for
  the user's MAL login test. All code is tested against recorded JSON; nothing has talked to the real MAL yet.
- **Polish after Stage 2** (2026-11-15, same branch): macOS traffic lights moved off the logo, Settings
  page redone (Accounts and About cards), real covers in the list, Rune's `dbus` feature dropped
  (binary size), the anitomy decision recorded.
- **Stage 3, edits and sync queue:** built on branch `stage-3` (2026-11-15, on top of `stage-2`), waiting
  for review and the user's live write test. Every write path is tested against recorded-shape JSON
  and fake services only; nothing has written to the real MAL account.

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

| what | target | how to measure | Stage 1 (2026-11-15) | Stage 2 (2026-11-15) | Polish (2026-11-15) | Stage 3 (2026-11-15) |
|---|---|---|---|---|---|---|
| release binary (`kotora`, stripped) | 30 MB or less (was 25, see below) | `ls -l target/release/kotora` after `strip` | macOS arm64 19.2 MB; Linux arm64 (Docker) 25.2 MB. `kot` 0.5 MB | macOS arm64 23.1 MB (+3.9 MB: SQLite, ureq with rustls). `kot` 0.6 MB. Linux arm64 (Docker) 29.3 MB (+4.1 MB): 0.7 MB under the target, so the next stage must watch it | macOS arm64 23.3 MB (+0.2 MB: covers). Linux arm64 (Docker) 27.4 MB (−1.9 MB: Rune `dbus` dropped, covers added). `kot` 0.5 MB | macOS arm64 23.5 MB (23,538,104 bytes; +0.2 MB: queue, editor, queue page). Linux arm64 (Docker) 27.6 MB (27,561,088 bytes; +0.2 MB). `kot` 0.5 MB |
| package (DMG / tarball, compressed) | 15 MB or less | size of the packaging output | no DMG yet; `tar.gz` of both stripped binaries: macOS 8.7 MB, Linux 11.1 MB | `tar.gz` of both stripped binaries, macOS: 10.7 MB | not re-measured | not re-measured |
| memory at rest, window open, list of 300 entries | 100 MB or less | Linux: PSS from `/proc/<pid>/smaps_rollup`; macOS: `footprint` | live only (agents never open a window); to be measured by the user | live only; to be measured by the user | live only. Headless snap of `list-covers` vs `list` at 2x: +0.15 MB RSS; covers are capped at 16 MB in memory (a 300-entry list at 2x is about 10 MB if all are shown) | live only; to be measured by the user |
| memory at rest, window closed (tray / hidden) | 50 MB or less | same | macOS `--hidden`, 303 fake entries: 12 MB footprint (peak 89 MB during start) | macOS `--hidden`, empty scratch config: 12 MB footprint (peak 89 MB during start) | macOS `--hidden`, empty scratch config: 12 MB footprint (covers never load without a window) | macOS `--hidden`, empty scratch config: 12 MB footprint (peak 88 MB during start) |
| CPU at rest | 0 %, no timers waking more than once a minute | `top`/`pidstat` over 60 s; no busy poll | macOS `--hidden`: 0.01 s CPU in 60 s, 0 idle wakeups. Code: `poll` returns false, no `wake_after`, socket threads block in `accept`/`read` | macOS `--hidden`: 0.01 s CPU in 60 s. Code: sign-in waits only while a sign-in runs (at most 5 min), no refresh timer | macOS `--hidden`: 0.01 s CPU in 60 s. Cover workers start only with work and end when idle | macOS `--hidden`: 0.00 s CPU in 60 s, 0 idle wakeups. The queue worker thread starts with an edit and ends when nothing is left to send; while a change waits for a retry it sleeps until then (30 s doubling to 15 min), never polls |
| start to first frame | 300 ms or less (warm) | `KOTORA_FRAME_LOG` or a timestamp log | headless (no window): 27–31 ms from `main` to the first encoded frame, 1280×800 at 2x; window creation is live only | not re-measured (headless scenes unchanged in cost); live number still the user's | not re-measured | not re-measured |
| `kot status` round trip | 20 ms or less | `time kot status` | 2.6 ms median, 3.3 ms max (50 runs, hidden app) | not re-measured | not re-measured | 2.2 ms median, 3.0 ms max (50 runs, hidden app) |

Stage 3 changes no target and adds no dependency: +0.2 MB on macOS and on Linux, where the binary is
2.4 MB under the 30 MB target. `cargo bloat` on Linux: .text 18.5 MiB (18.3 before); `kotora` and
`kotora_core` together 0.34 MiB. Stage 4 (anitomy, MPRIS on the zbus Rune already has) must check again.

Stage 2 changes no target. The binary grew from 19.2 to 23.1 MB on macOS (SQLite and ureq with rustls were
the expected few MB), so the 30 MB target holds on macOS. On Linux it is 29.3 MB, only 0.7 MB below it: Stage 3 and 4 (anitomy, zbus
sources) must be checked with `cargo bloat`, and the target may need one more honest adjustment.

Polish (2026-11-15): `cargo bloat --release --crates` (now the `bloat` step of `scripts/linux-check`)
on Linux: rune 4.1 MiB, std 2.0, naga 1.2, zbus 1.0, wgpu_core 0.95, winit 0.59, accesskit_unix 0.55,
rustls 0.42, harfrust 0.37, image 0.34, skrifa 0.34 (.text 18.3 MiB). Easy waste removed: Rune's `dbus`
feature, unused by Kotora, which brought a second zbus (6, next to AT-SPI's 5) and a tokio runtime.
What is left is Rune's own stack; the rest of the duplicates (`png`, `miniz_oxide`, `hashbrown`,
`rustix`, `objc2`) are inside Rune and its dependencies. The next big item would be AT-SPI accessibility
(zbus 5 + accesskit_unix, about 1.5 MiB), which we keep. Stage 4 must reuse that zbus 5 for MPRIS.

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
- [x] Decide on anitomy (decided 2026-11-15): use an existing Rust port of anitomy (MPL-2.0, already
  allowed by `deny.toml`). It is added in Stage 3 (or 4, with recognition), not before, and its size is
  measured then with `cargo bloat` against the binary budget. Only if it is too heavy or causes problems
  (bad parses on our test names, an unmaintained port) do we write our own small parser later. The
  test with 200+ made-up file names and upstream's test data runs when it is added.
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
  Covers (polish, 2026-11-15): `app/src/art.rs` asks for a row's cover from the view; up to 4 worker
  threads start only when there is work and end when the queue is empty. They read the disk cache
  (`$XDG_CACHE_HOME/kotora/covers`, 64 MB, deleted on sign out) or download the file, decode it at the
  exact drawn size (68 × 96 px at 2x), and memory keeps at most 16 MB of covers. The tile shows while a
  cover loads or when there is none. Scene: `list-covers`.
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

Built 2026-11-15 on `stage-3`. Tested with recorded-shape JSON, fake services and a hidden test copy with
`KOTORA_OFFLINE=1`; the first real write is the user's live test, after their OK (STATUS.md).

- [x] Inline editor (progress, status, score) and `+1`. A press on a row opens the editor under it
  (status chips, a progress stepper capped at the episode count, score 1–10 or none); Save queues one
  change, Cancel or Escape closes. Picking Completed fills the progress. `+1` is back in the real app and
  hides at the last episode. Scenes: `list-editor`, `list-editor-changed`, `list-editor-unknown-count`,
  `list-plus-one`.
- [x] `pending_op` queue (schema 2, `core/src/queue.rs`): an edit changes the saved entry and queues the
  change in one transaction; edits to a waiting change merge (3→4→5 is one "progress 5"), and an edit back
  to the old value drops it. The worker (`app/src/queue.rs`) is a thread that starts with work and ends
  when nothing is left. For each change it reads the entry (`GET /anime/{id}?fields=my_list_status`),
  compares the remote `updated_at` with the base the edit started from, and sends
  `PATCH /anime/{id}/my_list_status` with the changed fields only. A change made elsewhere is a conflict,
  except when only the progress differs and ours is higher (ours is sent). No network or 429: backoff
  30 s doubling to 15 min. 401 with a refused refresh: "sign in again", the queue is held and kept; signing in
  sends it. Other refusals: `failed` with MAL's words. A full sync lays waiting changes over the fetched list.
  One rate budget (20 a minute) is now shared by every MAL client in the app.
- [x] Queue page with badge and per-op errors; conflict choice. Conflict cards (Yours / MyAnimeList, Keep
  mine / Take theirs; "Remove here too" when it was removed there), Waiting (with merged-edit count and
  the retry reason), Failed (MAL's words, Edit, Discard), Sent today; offline alert with Try now,
  sign-in-again alert, Sync now. Rows say "Waiting to sync", "Conflict in queue" or "Sync failed". Scenes:
  `queue-page`, `queue-empty`, `queue-offline`, `queue-sign-in-again`, `queue-conflict-removed`,
  `queue-sending`.
- [x] `kot list [STATUS|all]`, `kot +1 TITLE`, `kot set TITLE EP [--status S] [--score N|none]`,
  `kot queue`, `kot sync`. TITLE is any part of a title (all titles searched) and must match one entry.
- [ ] Live test by the user: the first real writes (STATUS.md). Needs the user's OK each time.

Open after Stage 3:
- The "keep the higher progress" rule is always on; the Settings switch (mockup) comes with the Updates
  card in Stage 4.
- `+1` to the last episode does not set Completed by itself (MAL's site does); to be decided.
- The editor's progress is a stepper only; long shows are faster with `kot set`. A typed number could follow.
- A full sync that fetched the list just before a change was sent can show the old value until the next
  sync (the change itself is on MAL). Rare; the 15-minute refresh would fix it.
- Sign out deletes waiting changes together with the saved list (the MAL licence rule); the Settings note
  says so. A confirmation when changes wait could follow.
- The queue shows ages ("1 h"), not clock times.

## Stage 4: detection (completes v0.1)

- [ ] `detect`: MPRIS source (zbus, `xesam:url`, position; use the zbus major that Rune's AT-SPI
  accessibility already pulls, today 5, so there is one copy; blocking API, no tokio) for VLC, mpv and mpc-qt; the mpv IPC source
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
