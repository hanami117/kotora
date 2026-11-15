# Kotora roadmap

The plan, in stages. Each stage ends with a review by the user before the next one starts. The design and
its reasons are in [DESIGN.md](DESIGN.md); the user's decisions of 2026-11-14 are in its section 12.
v0.1 is MyAnimeList; AniList comes in v0.3 (moved from v0.2 on 2026-11-15, see Stage 5).

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
- **Stage 4, detection (completes v0.1):** built on branch `stage-4` (2026-11-15, on top of `stage-3`),
  waiting for review and the user's live player tests on Arch and the Mac. Detectors are tested with
  recorded messages, a fake mpv on a temp socket and a fake MPRIS player on a private bus (Docker).
- **Stage 5, after the first live test:** added 2026-11-15 from the user's feedback. Built on branch
  `stage-5` (on top of `stage-4`): Client ID from the app, search and add, details page, and the
  detection GUI. The detection engine is on `stage-5-detect`, to be merged into `stage-5`. Waiting for
  the merge and the user's live test. The release moves to Stage 6.

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

| what | target | how to measure | Stage 1 (2026-11-15) | Stage 2 (2026-11-15) | Polish (2026-11-15) | Stage 3 (2026-11-15) | Stage 4 (2026-11-15) |Stage 5 (2026-11-15) |
|---|---|---|---|---|---|---|---|--|
| release binary (`kotora`, stripped) | 30 MB or less (was 25, see below); `codegen-units = 1` since Stage 5 | `ls -l target/release/kotora` after `strip` | macOS arm64 19.2 MB; Linux arm64 (Docker) 25.2 MB. `kot` 0.5 MB | macOS arm64 23.1 MB (+3.9 MB: SQLite, ureq with rustls). `kot` 0.6 MB. Linux arm64 (Docker) 29.3 MB (+4.1 MB): 0.7 MB under the target, so the next stage must watch it | macOS arm64 23.3 MB (+0.2 MB: covers). Linux arm64 (Docker) 27.4 MB (−1.9 MB: Rune `dbus` dropped, covers added). `kot` 0.5 MB | macOS arm64 23.5 MB (23,538,104 bytes; +0.2 MB: queue, editor, queue page). Linux arm64 (Docker) 27.6 MB (27,561,088 bytes; +0.2 MB). `kot` 0.5 MB | macOS arm64 24.6 MB (24,563,488 bytes; +1.0 MB: anitomy-ng with `regex`, detectors, Now Playing). Linux arm64 (Docker) 29.2 MB (29,200,048 bytes; +1.6 MB): 0.8 MB under the 30 MB target, so v0.2 must watch it. `kot` 0.6 MB |**Linux x86_64 29.9 MB** (29,930,264 bytes) with `codegen-units = 1`; 33.9 MB (33,869,560) with the default profile, over the target: see below. Stage 4 built on the same machine: 33.5 MB (33,469,560), so Stage 5 adds 0.40 MB (Client ID, search, details, detection GUI). `kot` 0.56 MB (557,368; 575,648 with the default profile) |
| package (DMG / tarball, compressed) | 15 MB or less | size of the packaging output | no DMG yet; `tar.gz` of both stripped binaries: macOS 8.7 MB, Linux 11.1 MB | `tar.gz` of both stripped binaries, macOS: 10.7 MB | not re-measured | not re-measured | not re-measured |not re-measured |
| memory at rest, window open, list of 300 entries | 100 MB or less | Linux: PSS from `/proc/<pid>/smaps_rollup`; macOS: `footprint` | live only (agents never open a window); to be measured by the user | live only; to be measured by the user | live only. Headless snap of `list-covers` vs `list` at 2x: +0.15 MB RSS; covers are capped at 16 MB in memory (a 300-entry list at 2x is about 10 MB if all are shown) | live only; to be measured by the user | live only; to be measured by the user |live only; to be measured by the user |
| memory at rest, window closed (tray / hidden) | 50 MB or less | same | macOS `--hidden`, 303 fake entries: 12 MB footprint (peak 89 MB during start) | macOS `--hidden`, empty scratch config: 12 MB footprint (peak 89 MB during start) | macOS `--hidden`, empty scratch config: 12 MB footprint (covers never load without a window) | macOS `--hidden`, empty scratch config: 12 MB footprint (peak 88 MB during start) | macOS `--hidden`, scratch config, detection on (one mpv socket watched): 13 MB footprint (peak 89 MB during start) |Linux x86_64 `--hidden`, scratch config, players off, offline: PSS 19.6 MB (RSS 27.3 MB) |
| CPU at rest | 0 %, no timers waking more than once a minute | `top`/`pidstat` over 60 s; no busy poll | macOS `--hidden`: 0.01 s CPU in 60 s, 0 idle wakeups. Code: `poll` returns false, no `wake_after`, socket threads block in `accept`/`read` | macOS `--hidden`: 0.01 s CPU in 60 s. Code: sign-in waits only while a sign-in runs (at most 5 min), no refresh timer | macOS `--hidden`: 0.01 s CPU in 60 s. Cover workers start only with work and end when idle | macOS `--hidden`: 0.00 s CPU in 60 s, 0 idle wakeups. The queue worker thread starts with an edit and ends when nothing is left to send; while a change waits for a retry it sleeps until then (30 s doubling to 15 min), never polls | macOS `--hidden`, detection on, no player: 0.00 s CPU in 60 s, 0 idle wakeups. A fake mpv playing for 27 s: 0.00 s CPU (no position polling; one wake at the threshold). VLC's web interface (macOS, opt-in, off until its password is set) is the one poll: 60 s while VLC is not running, 30 s idle, 15 s playing |Linux `--hidden`: 0 ticks of CPU in 60 s, 0 context switches of the main thread. A search thread starts with a pause in typing and ends with the answer; a details thread with the page; no timer |
| start to first frame | 300 ms or less (warm) | `KOTORA_FRAME_LOG` or a timestamp log | headless (no window): 27–31 ms from `main` to the first encoded frame, 1280×800 at 2x; window creation is live only | not re-measured (headless scenes unchanged in cost); live number still the user's | not re-measured | not re-measured | not re-measured |not re-measured |
| `kot status` round trip | 20 ms or less | `time kot status` | 2.6 ms median, 3.3 ms max (50 runs, hidden app) | not re-measured | not re-measured | 2.2 ms median, 3.0 ms max (50 runs, hidden app) | not re-measured |1.3 ms median, 2.1 ms max (50 runs, hidden app, Linux) |

Stage 5 and the binary target: this is the first measurement on Linux x86_64 (the earlier Linux
numbers were arm64 in Docker). The same Stage 4 code is 33.5 MB stripped on x86_64 against 29.2 MB on
arm64: x86_64 code for this stack (Rune, wgpu, naga, zbus) is about 14 % larger, so the 30 MB target was
already over on x86_64 before Stage 5, which adds 0.40 MB. `.text` is 24.4 MB; by symbols, Rune about
3.4 MB, naga 1.2, zbus with zvariant 1.4, wgpu_core 0.8, `kotora` 0.55. Measured on the
same machine, stripped: the default release profile 33.87 MB (full build 1 m 30 s); with
`codegen-units = 1` 29.93 MB (2 m 02 s); with that and fat LTO 28.12 MB (3 m 06 s). Neither changes
behaviour. The workspace now sets `codegen-units = 1` in `[profile.release]` (Uzumaki keeps the default;
the Rune rev is still shared). Fat LTO is the next step if a later stage needs room. macOS arm64 is not
re-measured; it can only shrink with the same setting.

Stage 4 changes no target. New dependencies: `anitomy-ng` (MPL-2.0) with `regex`/`regex-automata`/
`regex-syntax`/`aho-corasick` (MIT or Apache-2.0); zbus 5 in `detect` is the copy Rune's AT-SPI already
builds (`cargo tree -d`: one zbus, 5.19.0), with its `blocking-api` feature added; `ureq` and `libc` were
already in the build. `cargo bloat` on Linux: .text 19.7 MiB (18.5 before); zbus 1.3 MiB (1.0 before: the blocking API and Kotora's MPRIS and notification calls), `regex_syntax` 0.18 MiB, `kotora` and `kotora_core` 0.51 MiB together (0.34 before); `anitomy_ng`, `regex_automata` and `aho_corasick` are below the top 25.

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
- [x] A service trait in `core` (`ListService`), so AniList can be added in v0.3 without touching the queue.
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
- (Done after Stage 4: the last episode sets Completed, DESIGN.md §12.11.)
- The editor's progress is a stepper only; long shows are faster with `kot set`. A typed number could follow.
- A full sync that fetched the list just before a change was sent can show the old value until the next
  sync (the change itself is on MAL). Rare; the 15-minute refresh would fix it.
- Sign out deletes waiting changes together with the saved list (the MAL licence rule); the Settings note
  says so. A confirmation when changes wait could follow.
- The queue shows ages ("1 h"), not clock times.

## Stage 4: detection (completes v0.1)

Built 2026-11-15 on `stage-4`. Tested with recorded messages, a fake mpv on a temp socket, a fake MPRIS
player on a private bus (`dbus-run-session` in the Linux Docker check) and fake services; the first
real players are the user's live test (STATUS.md).

- [x] `detect`: every source is a blocking thread that sleeps until something happens; nothing polls
  except VLC's web interface on macOS (opt-in, slow).
  - **mpv IPC** (Linux and macOS; IINA through "additional mpv options"): `observe_property` for path,
    title, duration, pause, speed, idle; the position is read only after a seek, restart or pause.
    While the socket is missing, inotify (Linux) or kqueue (macOS) waits on its folder. Paths:
    `[players] mpv_sockets` in `config.toml`, else `$XDG_RUNTIME_DIR/mpv.sock` (Linux) or
    `/tmp/mpv.sock` and `/tmp/iina.sock` (macOS; a path with "iina" in it is IINA).
  - **MPRIS** (Linux): zbus 5 blocking API, the copy AT-SPI already builds (one zbus). Match rules for
    `NameOwnerChanged` of `org.mpris.MediaPlayer2.*` and signals at `/org/mpris/MediaPlayer2`, then
    `GetAll`. Only players with a `file://` `xesam:url` count (browsers and music players are skipped).
  - **VLC web interface** (macOS): `/requests/status.json` on 127.0.0.1 with the password from
    `[vlc] password`; one connection attempt a minute while VLC is not running, 30 s idle, 15 s playing.
  - **Watch hub:** follows the player that started playing last, and works out the position from the
    clock and the speed between reports. It wakes once, when the threshold is due. A file counts as
    watched at the threshold (default 80 %) with real play time of a quarter of it, at most 2 minutes
    (a jump to the end doesn't count), or when the player stops after half.
  - Desktop notification: `org.freedesktop.Notifications` (Linux), `osascript` (macOS).
- [ ] Verify mpc-qt's `xesam:url` (file URI?) on a real install, and VLC's `status.json` fields on macOS
  (the user's live test; the parsers follow the documented shapes).
- [x] Recognition (`core/src/recognize.rs`): `anitomy-ng` 1.0.10 parses the name (the folder's name when
  the file has no title); a match on every list title with the season split off ("2nd Season",
  "Season 2", "S2", "II") is sure, a close one (letter-pair similarity ≥ 75 %) or an episode past the
  count is unsure. The user's pick and "not an anime" are cached (schema 3, `recognition`); with no match
  on the list, one MAL search (`GET /anime?q=`, shared rate budget, signed in and online only), cached
  30 days. A hit on the list is unsure; a hit elsewhere shows "not on your list" (adding comes in Stage 5).
  anitomy check: 240 made-up names (10 titles × 8 patterns × 3 episodes) all right; upstream anitomy's
  own `test/data.json` (run by hand, not in the repo): 160 of 182 with upstream's title and episode.
- [x] Update decision, Now Playing banner (on every page), desktop notification, Update / Not now /
  Wrong anime… The banner asks once the file counts as watched; Update queues the progress like `+1`
  (and Plan to watch becomes Watching). "Wrong anime…" opens a picker of the 8 closest list titles and
  "Not an anime". Never an episode at or below the progress (unless rewatching) or past the count.
  With "Update automatically" on, a sure match is queued at once and the banner offers Undo.
- [x] `kot now`, `kot confirm`, `kot dismiss`.
- [x] Settings: Updates card ("Update automatically", off by default; "Offer the update at" 70/80/90/100 %;
  "Keep the higher progress in conflicts", on by default, now a real switch for the queue) and Players
  card (a switch and a live status line per player, Linux or macOS set). Settings write single lines of
  `config.toml` (comments kept, 0600). The Accounts card already had the account and Client ID state.
- [x] Integration test with a fake MPRIS player on a private bus (`detect/tests/mpris_bus.rs`, run by the
  Linux check's `test` step).
- Scenes: `now-playing-page`, `-paused`, `-offer`, `-offer-page`, `-recorded`, `-unsure`,
  `-unsure-offer`, `-picking`, `-not-on-list`, `-unknown`, `-empty`; `settings-auto-on`,
  `settings-players-off`.

Open after Stage 4:
- Decided 2026-11-15: the last episode sets Completed (`+1`, editor, `kot`, recording), when the total
  is known. The banner says "Completed" and Undo restores the status (DESIGN.md §12.11).
- Recording an episode of a Plan to watch entry sets Watching (decided here, as MAL's site does; easy to
  drop).
- The notification has no buttons: Update is in the app or `kot confirm`. On macOS, `osascript` may show
  it as "Script Editor" until there is an app bundle (v0.3).
- VLC's web password and the mpv socket paths are set in `config.toml`, not in Settings; socket path
  changes need a restart.
- "Wrong anime…" lists the closest titles only, no search; the corrections UI is v0.3.
- The page's position is worked out from the clock when the page is drawn; it doesn't move by itself
  (no timer).

## Stage 5: after the first live test (part of v0.1)

Why (user feedback, 2026-11-15): the user's first live test of Stage 4 on Arch (KDE Wayland) showed two
blockers and two gaps. The Welcome page asked them to edit `~/.config/kotora/config.toml` by hand for
the Client ID, and their mpv had neither an IPC socket nor mpv-mpris, so nothing was detected. And
without search-and-add and a details page, a show found by detection but missing from the list was a
dead end. These items come forward from v0.3/v0.4 into v0.1; AniList, library folders with "play next",
the corrections UI and AppImage/DMG move to v0.3 to make room (DESIGN.md §12.12).

Built 2026-11-15 in two parallel branches. **Detection lives on branch `stage-5-detect`** (the process
detector, the watched-time rule, macOS `libproc` and the `mpv.conf` helper; its own session and notes);
everything else, including the detection GUI, is on `stage-5`. `stage-5-detect` is merged into
`stage-5` before the live test, and the one wiring point is marked `TODO(stage-5-detect)` in
`app/src/precise.rs`. All of `stage-5` is tested with recorded-shape JSON, fake services and scenes;
nothing has talked to MAL.

- [x] **Client ID from the app** (`app/src/clientid.rs`, `core::config::check_client_id`): the
  Welcome page's three steps (Open the API page; app type "other", non-commercial, the redirect URL
  with Copy; paste the ID) and a field with Save. The ID is trimmed (quotes too) and must be hex, 20
  to 64 characters (MAL's are 32); a pasted secret, app name or half an ID gets a line that says what
  is wrong. Save writes `[mal] client_id` with the comment-keeping writer (0600) on a short thread and
  re-reads the sign-in state, so Sign in works at once. Settings → Accounts shows it masked (last 4)
  with Change. `kot config client-id ID` does the same through the running app. Scenes:
  `no-client-id`, `no-client-id-error`, `client-id-saved`, `settings-client-id-change`,
  `settings-no-client-id`.
- [ ] **Zero-config detection:** on `stage-5-detect` (see that branch's notes for the process scan,
  its CPU and the watched-time rule).
  - [x] GUI (`app/src/precise.rs`): each player's line keeps its source's label, so it reads
    "mpv · process · playing" or "IPC socket · …"; Now Playing says "No position from the player" for
    a process source instead of an empty bar. When mpv is seen by its process only (and its IPC
    socket doesn't answer), a banner offers once to turn on precise detection: one click appends
    `input-ipc-server=…` to `mpv.conf` and shows what was written, or that the file already sets a
    socket, or the error. The click or "No thanks" is remembered in `config.toml` (`[notices]
    mpv_precise = true`); Settings keeps a Turn on button under mpv. Scenes: `precise-offer`,
    `precise-written`, `precise-already-set`, `precise-failed`, `settings-players-process`,
    `now-playing-process`.
  - [ ] Wiring after the merge: `write_mpv_conf` in `precise.rs` calls
    `kotora_detect::mpvconf::enable` with the first watched mpv socket (the TODO says how).
- [x] **Search and add** (`app/src/search.rs`, `core::mal::browse_anime`): with a query, the list
  page becomes search results: the list's matches across every status (the page's own first, each
  with its status tag), then MAL's results that are not on the list. MAL is asked only signed in and
  online, for 3 letters or more, 400 ms after typing stops (`GET /anime?q=&limit=10` with type,
  episodes, status, season and mean score); answers stay 10 minutes in memory (20 queries) and go
  with sign out. One rate budget for the whole app, as before. A result row: cover, title, type,
  episodes, airing, season, ★ score, English title, "+ Watching" and a menu for Plan to watch,
  Completed, On hold, Dropped. Adding is optimistic and goes through the queue (`Store::add_to_list`,
  schema 4's `pending_op.added`: not on MAL is no conflict, a full sync keeps it until sent, Discard
  removes it, Completed starts at the last episode). `kot add QUERY|ID [--status S]` (a title must
  find one anime, else kot lists ids). Scenes: `search-online` (`-pending`, `-menu`, `-added`,
  `-short`, `-offline`, `-error`, `-none`).
- [x] **Details page** (`app/src/details.rs`, `core::mal::fetch_details`): a list row's title, a
  search result, the Now Playing title or "Details" on its not-on-list card opens the anime in the
  main area: cover (MAL's large one at 200 × 284), titles (main, English, Japanese, synonyms), type,
  episodes, airing, season, MAL score, rank, popularity, members, aired dates, studios, genres,
  source, length, synopsis; on the right the user's entry (status, progress with `+1`, score,
  started and finished dates, Rewatching, Waiting to sync; Edit opens the editor's fields there) or
  Add to list with every status. Back or Escape returns (the sidebar keeps that page lit); "Open on
  MyAnimeList" opens the browser. Information comes from the store's `details` table when younger
  than 3 days, else `GET /anime/{id}` (signed in, online), and is saved: for titles on the list, and
  others at most a week and the newest 100 (DESIGN.md: no hoarding); sign out deletes it. A fetch
  also refreshes a saved entry's titles, count and cover. Now Playing's "Not on your list" card has
  "Add as Watching", which also remembers the file as that anime. Scenes: `details-on-list`
  (`-editor`), `details-not-on-list` (`-added`), `details-loading`, `details-error`.
- [x] Scenes for every new view and state (above), checked as PNGs at 1280 × 800 and 960 × 600, and
  tests that find their parts; a press on a row's title opens details, elsewhere the editor.
- [x] Budgets measured (table above; Linux x86_64 from this stage on; `codegen-units = 1` keeps the
  binary under 30 MB there).
- [ ] Live test by the user on Arch, after `stage-5-detect` is merged (STATUS.md).

Decided in this stage (easy to change):
- The search box searches the whole list, not only the page's status, once it holds a query; the chips
  hide while searching. Clearing it brings the status page back.
- Search results hide anime already on the list (they are shown above, with their status). MAL's
  default leaves out NSFW titles in search.
- A row's title opens details; the rest of the row still opens the inline editor.

Open after Stage 5:
- Start and finish dates and the rewatching flag show on the details page but are not editable yet.
- The details page has no related anime or recommendations (more MAL fields, more requests).
- If `stage-5-detect` adds a database migration too, it must come after schema 4 here (merge note).

## Stage 6: release 0.1

- [ ] `install.sh` (copy Uzumaki's: `~/.local`, desktop file, icons, `--uninstall`).
- [ ] Icon (placeholder until the separate icon work: a tiger that cites Taiga's), README with screenshots
  from `--snap` scenes and a clear credit to Taiga, About page credits, CHANGELOG.
- [ ] AUR PKGBUILD (source). AppImage and DMG are not part of v0.1 (v0.3).
- [ ] Live test by the user on Arch (Wayland and X11) and on the Mac.
- [ ] Make the repo public (the user's call).

## Later (v0.2 and on, order to be agreed)

Reordered 2026-11-15 after the first live test (Stage 5): the open-file detectors, search and add and
the details page moved into v0.1; AniList, AppImage/DMG, the corrections UI and library folders moved
to v0.3.

- **v0.2:**
  - the mpv Lua script (`extras/mpv/kotora.lua` → `kot report`)
  - local history with undo
- **v0.3:**
  - AniList (implicit grant; re-check the terms first) and several accounts
  - AppImage (Ubuntu 22.04 container) and DMG (ad-hoc signed)
  - corrections UI ("this file / folder is X")
  - library folders and "play next episode"
  - opt-in window-title detectors on X11, Hyprland, sway and wlroots
  - anime-relations episode mapping
- **v0.4:**
  - seasons browser
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
