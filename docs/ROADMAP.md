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
  detection GUI. The detection engine (`stage-5-detect`) was merged into `stage-5` on 2026-11-15 and
  wired. After the user's first live test of it, a second batch (2026-11-15): real covers in Now
  Playing, WebP covers, open by MAL id, a search page, the profile picture and a setup wizard.
  Waiting for the user's live test of that batch. The release moves to Stage 6.
- **anime-relations (moved from v0.3 into v0.1):** built on branch `relations` (2026-11-15, on top of
  `stage-5`), waiting for review and the user's live test with their Re:Zero releases. See below.
- **Airing and titles (2026-11-15, user-approved):** built on branch `airing` (on top of `stage-5` at
  9c26d8f): stripped binaries from `install.sh`, the title language setting, and the Airing page
  (the seasons browser, moved from v0.4). Waiting for review and the user's live test. See below.
- **macOS detection (2026-11-16):** built on branch `mac-detection` (on top of `stage-5` at 38d2f8b). The
  libproc open-file scan is tested for real against fake `mpv`, `IINA` and `VLC` processes; IINA gets
  precise detection from Settings → Players (one click when IINA uses a config folder, else the steps).
  Waiting for the user's live test on the Mac. No tray, login item or `.app` yet (next stage).
- **Mac integration and tray (2026-11-16):** built on branch `mac-integration` (on top of `mac-detection`).
  The macOS menu bar (About, Settings… ⌘,, a Go menu), a tray icon (Linux StatusNotifierItem, macOS
  menu bar item) so closing the window keeps detection running, Settings → Startup (start at login,
  start in the background) and `packaging/macos/build.sh` for a minimal ad-hoc-signed
  `target/macos/Kotora.app`. Waiting for the user's live test on both systems. The Linux tray brings
  Rune's `dbus` feature back (about +2 MB on Linux, which was 29.9 MB): measure on Arch and decide.

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
| CPU at rest | 0 %, no timers waking more than once a minute (one exception since Stage 5: the process scan, see below) | `top`/`pidstat` over 60 s; no busy poll | macOS `--hidden`: 0.01 s CPU in 60 s, 0 idle wakeups. Code: `poll` returns false, no `wake_after`, socket threads block in `accept`/`read` | macOS `--hidden`: 0.01 s CPU in 60 s. Code: sign-in waits only while a sign-in runs (at most 5 min), no refresh timer | macOS `--hidden`: 0.01 s CPU in 60 s. Cover workers start only with work and end when idle | macOS `--hidden`: 0.00 s CPU in 60 s, 0 idle wakeups. The queue worker thread starts with an edit and ends when nothing is left to send; while a change waits for a retry it sleeps until then (30 s doubling to 15 min), never polls | macOS `--hidden`, detection on, no player: 0.00 s CPU in 60 s, 0 idle wakeups. A fake mpv playing for 27 s: 0.00 s CPU (no position polling; one wake at the threshold). VLC's web interface (macOS, opt-in, off until its password is set) is the one poll: 60 s while VLC is not running, 30 s idle, 15 s playing |Linux `--hidden`: 0 ticks of CPU in 60 s, 0 context switches of the main thread. A search thread starts with a pause in typing and ends with the answer; a details thread with the page; no timer |
| start to first frame | 300 ms or less (warm) | `KOTORA_FRAME_LOG` or a timestamp log | headless (no window): 27–31 ms from `main` to the first encoded frame, 1280×800 at 2x; window creation is live only | not re-measured (headless scenes unchanged in cost); live number still the user's | not re-measured | not re-measured | not re-measured |not re-measured |
| `kot status` round trip | 20 ms or less | `time kot status` | 2.6 ms median, 3.3 ms max (50 runs, hidden app) | not re-measured | not re-measured | 2.2 ms median, 3.0 ms max (50 runs, hidden app) | not re-measured |1.3 ms median, 2.1 ms max (50 runs, hidden app, Linux) |

Stage 5 and the process scan: the one timer that wakes more than once a minute. The process source
(`detect/src/procfs.rs`) scans every 5 s while `[players] process_scan` is on (the default), because an
unprivileged program gets no kernel notification for new processes (the proc connector and fanotify
need root). A scan reads a process name only for a new pid and open files only for players: about
0.2 ms of CPU per scan, ~0.02 % CPU measured on Linux. Turning the switch off in Settings stops it.

Stage 5 and the position tick (2026-11-15). Now Playing showed the position but it stood still,
because nothing redrew while a file played. `ui::poll` now calls `now::tick`: only while a followed
player is playing with a known position, and the window is on screen (`AppHandle::is_visible`), it
asks for one `wake_after` for the next time what is drawn changes, as Uzumaki's player bar does.
On the Now Playing page that is the next whole second of the position (never faster than once a
second, even at 2x). On any other page only the sidebar card's rounded percent shows the position,
so the wake is for the next percent (about 14 s for a 24 min episode). Paused, idle, a hidden
window, a process-only source (no position) and the end of the file set no wake at all. Showing the
window, a page change and every player report run `poll` first, so the timer restarts by itself.
A minimised window still counts as shown for Rune, so it ticks without drawing (the poll is a few
microseconds; Rune skips the frame). Tests in `now.rs`: playing and shown has a wake within a
second; paused, hidden, no position, nothing playing and the end have none.
- Idle, re-measured on Linux x86_64 release, `--hidden`, test instance, detection on, no player,
  60 s: the main thread 0 context switches and 0 ticks of CPU, so 0 wakes from the app's own timers.
  The whole process: 1 tick of CPU, from the process scan (12 wakes, the known exception above).
  A zbus helper thread (`blocking-1`) wakes every 500 ms on a futex timeout (120 in 60 s, 0 CPU);
  the same at the commit before this change, so it is not from this change. To look at later.
- Playing and visible: the app cannot be opened windowed from an agent, so the CPU of a tick is
  measured headless (`cargo test --release -p kotora tick_cost -- --ignored --nocapture`: poll,
  view, layout and draw list, no GPU). Now Playing page: 4–7 ms a tick, so 0.4–0.7 % of one core at
  one tick a second. Other pages: 0.3 ms a tick, one tick in about 14 s, so under 0.01 %. The live
  number (`pidstat -p $(pgrep -x kotora) 60 1` with a file playing and the page open) is the user's.
  If it is too high, the next step is Rune's `LiveText` and `Live` rect for the clock and bar, which
  skip the view (see Uzumaki's `player_bar.rs`).

Stage 5 and the binary target (user decision, 2026-11-15): the 30 MB target is indicative, not a hard
limit. `codegen-units = 1` stays, since it costs only build time. This is the first measurement on Linux x86_64 (the earlier Linux
numbers were arm64 in Docker). The same Stage 4 code is 33.5 MB stripped on x86_64 against 29.2 MB on
arm64: x86_64 code for this stack (Rune, wgpu, naga, zbus) is about 14 % larger, so the 30 MB target was
already over on x86_64 before Stage 5, which adds 0.40 MB. `.text` is 24.4 MB; by symbols, Rune about
3.4 MB, naga 1.2, zbus with zvariant 1.4, wgpu_core 0.8, `kotora` 0.55. Measured on the
same machine, stripped: the default release profile 33.87 MB (full build 1 m 30 s); with
`codegen-units = 1` 29.93 MB (2 m 02 s); with that and fat LTO 28.12 MB (3 m 06 s). Neither changes
behaviour. The workspace now sets `codegen-units = 1` in `[profile.release]` (Uzumaki keeps the default;
the Rune rev is still shared). Fat LTO is the next step if a later stage needs room. macOS arm64 is not
re-measured; it can only shrink with the same setting.

Stage 5, second batch (2026-11-15): Linux x86_64 stripped, `codegen-units = 1`: 30.04 MB
(30,041,112 bytes) before the batch's WebP decoder, 30.27 MB (30,272,536) with it, the rest of the
batch included. So the binary is now 0.27 MB over the indicative 30 MB target (user decision above).
New dependency: `image-webp` 0.2 (MIT OR Apache-2.0, with `quick-error`); fat LTO stays the next step
if room is needed.

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
  with Copy; paste the ID) and a field with Save (the steps became the setup wizard, below). The ID
  is trimmed (quotes too) and must be hex, 20 to 40 characters (MAL's are 32; 41 to 64 is the
  Client Secret's shape and is refused as such); a pasted secret, app name or half an ID gets a line
  that says what is wrong. Save writes `[mal] client_id` with the comment-keeping writer (0600) on a short thread and
  re-reads the sign-in state, so Sign in works at once. Settings → Accounts shows it masked (last 4)
  with Change. `kot config client-id ID` does the same through the running app. Scenes:
  `no-client-id`, `no-client-id-error`, `client-id-saved`, `settings-client-id-change`,
  `settings-no-client-id`.
- [x] **Zero-config detection** (from `stage-5-detect`, merged 2026-11-15): the process source
  (`detect/src/procfs.rs`, macOS `libproc.rs`) sees mpv, VLC, mpc-qt, Celluloid, Haruna and SMPlayer
  (IINA on macOS) by their open video file, with no setup. It knows no position or pause, so the watch
  hub counts the time the file stays open: the threshold share of the duration when known, else
  `[players] process_minutes` (default 15). When an IPC or MPRIS source shows the same file, it wins.
  `detect/src/mpvconf.rs` finds `mpv.conf` the way mpv does and appends `input-ipc-server` on request.
  - [x] GUI (`app/src/precise.rs`): each player's line keeps its source's label, so it reads
    "mpv · process · playing" or "IPC socket · …"; Now Playing says "No position from the player" for
    a process source instead of an empty bar. When mpv is seen by its process only (and its IPC
    socket doesn't answer), a banner offers once to turn on precise detection: one click appends
    `input-ipc-server=…` to `mpv.conf` and shows what was written, or that the file already sets a
    socket, or the error. The click or "No thanks" is remembered in `config.toml` (`[notices]
    mpv_precise = true`); Settings keeps a Turn on button under mpv. Scenes: `precise-offer`,
    `precise-written`, `precise-already-set`, `precise-failed`, `settings-players-process`,
    `now-playing-process`.
  - [x] Wiring after the merge: `write_mpv_conf` calls `kotora_detect::mpvconf::enable` with the
    first watched mpv socket and shows what it did (lines written, or the socket already set and
    whether Kotora watches it). At start `mpvconf::check` runs on a short thread; when `mpv.conf`
    already sets a watched socket, no banner is offered and Settings says to restart mpv.
  - [x] A player line prefers the IPC or MPRIS source over "process" when they tie (both not
    running). With "Update automatically" on, a process track still asks (no position or pause).
  - [x] Settings → Players: "Find players by their open files" (`process_scan`) and "Count a file
    after it stays open" (`process_minutes`, a 1–180 stepper in steps of 5).
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
- [x] Live test by the user on Arch (first round, 2026-11-15): asked for the batch below.

Second batch after the live test (2026-11-15, one commit each):
- [x] **Real covers in Now Playing:** the sidebar card and the Now Playing card use the cover cache
  like the list rows, at their own pixel size: the entry's cover, else (not on the list) the cover
  from the saved details, which the detect thread fetches once (signed in, online) and saves.
  Scenes: `now-playing-covers`, `now-playing-not-on-list-cover`.
- [x] **WebP covers:** MAL serves some covers as `.webp` (58 of a 322-entry list showed only the
  tile). They are decoded with `image-webp` alone (pure Rust, MIT OR Apache-2.0): +0.23 MB stripped
  on Linux x86_64 (30,041,112 → 30,272,536 bytes); `image`'s `webp` feature would add 0.56 MB, as it
  links the encoder too. Test: a lossy and a lossless-with-alpha fixture (`app/tests/*.webp`).
- [x] **Open by MAL id:** a pasted `myanimelist.net/anime/<id>[/slug]` link (or `anime.php?id=`)
  opens the details page at once; a bare number keeps the text search (it may be a title) and adds
  an "Open MyAnimeList #id" line, opened with Enter. Works for anime not on the list. Scene:
  `search-mal-id`.
- [x] **Search page:** a Search entry at the top of the sidebar (Ctrl K hint) opens a page with a
  wide field, focused, and the same results as before (the list's matches, then MyAnimeList's, the
  id line). Ctrl K opens it from anywhere, Ctrl F too off the list pages; Escape returns (also from
  inside the field). The list page's box is now a quick filter of the list (no MyAnimeList search)
  with a "Search MyAnimeList for …" line on top that takes the query along. Scenes: `search-page`,
  `list-filter`; the `search-online*` scenes show the page.
- [x] **Profile picture:** `GET /users/@me?fields=picture` (one request: name and picture) at sign-in
  and after a sync at most daily; the URL is kept in `kv` (`mal:picture`, `mal:picture_at`), the
  image in the cover cache (disk and memory, both cleared on sign out). Drawn round in the account
  card over the initial, which stays as the fallback. Scene: `account-picture`.
- [x] **Setup wizard** (`app/src/setup.rs`): replaces the no-Client-ID Welcome card, five steps with
  an indicator and Back / Next: Welcome; create the Client ID (the API page, every form field with
  what to put, the redirect URL with Copy, "you do NOT need the Client Secret"); paste (saving moves
  on; "check again" for an ID written by hand); sign in (MyAnimeList's own page, the waiting state
  with the link and Copy when the browser didn't open, denied and timeout with Try again); done (how
  many anime loaded, the players hint and a link to Settings → Players). Settings → Accounts has
  "Set up again" (closable, Escape too). A browser that can't be started no longer fails the
  sign-in: the link shows instead. Scenes: `no-client-id`, `no-client-id-error`, `setup-*`.
- [x] Scenes checked as PNGs at 1280 × 800 and 960 × 600; `cargo build`, `cargo test --workspace` and
  `cargo clippy --workspace --all-targets` clean.
- [ ] Live test of the second batch by the user on Arch.

Third batch (2026-11-15, user-approved): merges and rewatching.
- [x] **Merged** `relations` (anime-relations) and `manga-prep` (`core/src/manga`, no UI) into
  `stage-5`; detect.rs keeps both the not-on-list cover and the file's own episode, the cover looked up
  after the redirect. About credits anime-relations; a redirect to an anime off the list says why on
  the "Not on your list" card and in `kot now`.
- [x] **Rewatching (MAL, as Taiga):** the status stays Completed, `is_rewatching` on, the progress
  counts again. Schema 5: `entry.times_rewatched` and the rewatch fields in `pending_op`, so the
  queue sends them like the rest (`PATCH my_list_status` with `is_rewatching`,
  `num_times_rewatched`); the list, entry and details reads ask for `num_times_rewatched`.
  - Start: "Rewatch" in a completed row's editor, on the details page, and `kot rewatch TITLE|ID`;
    "Stop rewatching" ends one without counting it.
  - Detection: a completed anime that plays again asks "Watching it again?" with "Start rewatch at
    episode N" (or `kot confirm`); never automatic, even with "Update automatically" on.
  - The last episode of a rewatch: one queued change, `is_rewatching` off, the count +1, the
    progress at the total, still Completed (`Change::completes`, shared with the app and `kot`).
  - UI: "Rewatching" tag on rows and details, "Rewatched: twice" on details.
  - Decided: the Watching page also shows rewatches, as Taiga's Currently Watching list does
    (Taiga's list treats a rewatching entry as watching). The sidebar's Watching count stays MAL's
    status count, and the page says "+ 1 rewatching" next to its own, so the two numbers explain
    each other. The Completed page still shows them too, since they are Completed on MAL.
  - Scenes: `list-rewatching`, `list-editor-rewatch`, `details-rewatch`, `details-rewatching`,
    `now-playing-rewatch-offer`, `now-playing-rewatch-started`.
  - To check live (read only): that MAL returns `num_times_rewatched` for the
    `list_status{num_times_rewatched}` field syntax on the list. Tested with recorded-shape JSON only.
- [ ] Live test of the third batch by the user (rewatch writes need the user's OK on their account).

Decided in this stage (easy to change):
- The list page's box filters the whole list, not only the page's status, once it holds a query; the
  chips hide while it does. Clearing it brings the status page back. MyAnimeList is searched only
  from the search page (second batch).
- A bare number in a search box is offered as a MyAnimeList id (Enter) but still searched as text;
  a pasted link opens at once.
- Search results hide anime already on the list (they are shown above, with their status). MAL's
  default leaves out NSFW titles in search.
- A row's title opens details; the rest of the row still opens the inline editor.

Open after Stage 5:
- Start and finish dates and the rewatching flag show on the details page but are not editable yet.
- The details page has no related anime or recommendations (more MAL fields, more requests).
- The merge needed no database migration: `stage-5-detect` only added `[players]` keys.
- `mpv.conf` is checked once at start; a change made by hand while Kotora runs shows after a restart.

## anime-relations: episode redirection (part of v0.1)

Why (2026-11-15): releases number split seasons on ("Season 2 - 15" is episode 2 of "2nd Season
Part 2" on MAL) or count from the first season. Recognition found the season it read, saw an episode
past its count and asked. Taiga solves this with erengy/anime-relations (CC0); Kotora now reads the
same file. Moved forward from v0.3; the corrections UI stays there. Built on branch `relations`.

- [x] Parser (`core/src/recognize/relations.rs`): `::meta` (version 1.x, `last_modified`) and
  `::rules`; MAL | Kitsu | AniList ids, `?` (unknown id or open end), `~` (the source's id, per
  field or for all), `!` (the destination's own rule), single episodes, ranges and range-to-one-episode
  rules; titles from the comment above a rule. Bad lines are counted and skipped. All 556 rule lines of
  the current file read (810 rules after `!`, 792 with MAL ids), indexed by MAL id.
- [x] Applied after recognition (`recognize::redirect`): the cache, the list match, the search and
  the user's pick. The anime and the episode offered change; the reason reads "episode 15 of Re:Zero
  kara Hajimeru Isekai Seikatsu 2nd Season is episode 2 of Part 2" in Now Playing and `kot now`
  (nothing new in `kot`). A destination not on the list is "not on your list" with its title from the
  rule's comment, so "Add as Watching" works. One step, no chains (as Taiga).
- [x] Tests with inline fixtures: the format, the real Re:Zero rules (S2 numbered 1-25 → Part 2
  1-12; absolute 30 → 2nd Season 5; season 3 numbered 51-66 or 1-16), "S02E03" and absolute "15"
  giving the same episode, the user's pick and a search hit, the download policy.
- [x] Data: the file is built in (`core/data/anime-relations.txt`, 50 KB, `last_modified`
  2026-10-19) and refreshed from GitHub at most once a week, at the first recognised file of a run, with
  the ETag; a day after a failed try; never offline or headless; off with `[recognition]
  relations_download = false`. The last good copy stays (DESIGN.md §11).
- [x] Costs (Linux x86_64, release, stripped): `kotora` 30,137,368 bytes, +102,272 (+0.10 MB: 50 KB
  of data, the rest code) over `stage-5` built the same way (30,035,096). Reading the file: 0.4-0.5 ms,
  once per run, at the first file. No timer, no new thread at rest.
- [ ] Live test by the user (STATUS.md has the list for Stage 5; see below).
- [x] After the merge into `stage-5` (2026-11-15): the About page names anime-relations next to
  anitomy, and when a rule moves the file to an anime that isn't on the list, the "Not on your list"
  card (and `kot now`) say why ("Episode 15 of … is episode 2 of … (anime-relations)"). Scene:
  `now-playing-relation-not-on-list`.

Live test: play a Season 2 release numbered 1-25 at episode 15 or later, and a Season 3 release.
Now Playing (and `kot now`) should name 2nd Season Part 2 (or the 3rd Season) with the moved episode
and the reason.

## Airing, titles and a stripped install (branch `airing`, 2026-11-15)

Why: the user asked for a calendar of what airs (this week, and the season) next to the list, for
titles in English or Japanese, and for a smaller installed binary. Approved 2026-11-15; one signed
commit each. The seasons browser moves here from v0.4 as "Airing".

- [x] **`install.sh` strips** the binaries it installs (`install -s`; the build in `target/` keeps
  its symbols): `kotora` 36.3 → 30.5 MB, `kot` 722 → 560 KB. Tested in a scratch `HOME`: install,
  then `--uninstall` removes every file Kotora installed (`update-desktop-database` leaves its own
  `mimeinfo.cache` and `intent.cache`, as before).
- [x] **Titles: Romaji / English / Japanese** (Settings → Display; `[display] titles`, romaji by
  default). A missing title falls back to romaji. The shown title is derived once per entry when the
  list or the setting changes (`Entry::retitle`), never per frame: list rows and the title sort,
  details, Now Playing and the sidebar card, the banner, search results, the queue, Airing, and `kot`
  (it prints what the app shows). Matching, recognition and `kot`'s title lookup use every title.
  Scenes: `settings-titles-japanese`, `list-titles-english`, `list-titles-japanese`,
  `queue-titles-english`, `now-playing-titles-japanese`, `details-titles-japanese`,
  `search-titles-english`, `airing-season-english`.
- [x] **Airing** (`app/src/airing.rs`, `core/src/airing.rs`), a sidebar entry under Kotora, data from
  MyAnimeList only:
  - `GET /anime/season/{year}/{season}?sort=anime_num_list_users` with start and end date,
    broadcast, episodes, status, type, mean, members, alternative titles, picture and
    `my_list_status`, in pages of 100: 4 pages of the current season, 2 of the one before (only its
    shows still airing are kept). At most 6 requests, through the app's one rate budget (20 a
    minute). Saved per season in the store (schema 6, `season`), asked again after 12 h (or with
    Refresh); only the two seasons shown are kept; sign out deletes them.
  - The user's own airing entries that the two seasons don't have (long runners): the list sync now
    asks for `start_date` and `broadcast` too (same request), kept in `media`.
  - The timetable is Kotora's own estimate (DESIGN.md §5.8): start date's first slot + (N − 1)
    weeks, JST to the user's zone, moved forward when the progress is ahead. "Times are estimated" on
    the page; shows without a slot time are listed apart, without a time.
  - **This week:** the next 7 days from today (decided: today on top rather than Monday, so the
    countdowns are in view), grouped by day, today lit, local times; the list's shows marked, with
    "in 2 d 4 h". **This season:** a cover grid by members, filters TV / ONA / Movie and On my list /
    Not on my list. Every line and card has "+ Watching" / "+ Plan" (through the queue, as search)
    and opens the details page.
  - Lightness: everything drawn is derived once (`airing_rebuild`) when the data, a filter or the
    list changes, and once a minute while the page is on screen (the countdowns; `airing::tick`
    asks for one `wake_after` to the next minute, none on other pages or with the window hidden).
    No network in view or poll; virtual rows for both views (the grid is rows of cards). Covers at
    display size through the cover cache: a card is 128 × 181 pt, about 0.5 MB at 2x, and the grid
    builds about 50 cards, so the cache's cap goes from 16 to 32 MB (a smaller cap would drop covers
    still drawn and load them again while scrolling).
  - Never headless, offline or signed out; the saved seasons show offline with the error.
  - `kot airing`: the list's episodes in the next 7 days, by day, with times and countdowns.
  - Scenes: `airing-week`, `airing-week-mine`, `airing-season`, `airing-season-tv`,
    `airing-season-mine`, `airing-season-not-mine`, `airing-season-added`, `airing-season-english`,
    `airing-loading`, `airing-empty`, `airing-error`, `airing-error-saved`, `airing-signed-out`,
    `airing-details`; checked as PNGs at 1280 × 800 and 960 × 600.
  - Costs (Linux x86_64, release, stripped, `codegen-units = 1`): `kotora` 30,514,200 → 30,772,376
    bytes (+258,176, +0.26 MB, titles and Airing together); `kot` 560,072 → 578,632 (`kot
    airing`). No new crate: `libc` was already in the tree.
- [ ] Live test by the user (read only; an add from Airing is a write to the list, like any add):
  - Settings → Display → English, then Japanese: the list, a details page, Now Playing, the queue,
    search and `kot list` follow at once, and after a restart.
  - Sync once (the list now carries start dates and slots), open Airing: the first open asks
    MyAnimeList for up to 6 pages; a second open within 12 h asks nothing.
  - This week: compare a few times on the list with the real broadcast (or a schedule site); check a
    late-night show (after midnight JST) lands on the right local day; countdowns change once a
    minute.
  - This season: filters, covers while scrolling (no flicker of tiles coming back), "+ Plan" on a
    show (queued, then sent), a card opens its details and Back returns to Airing.
  - `kot airing`; offline (`KOTORA_OFFLINE=1` in a test copy): the saved season shows.
  - CPU with Airing open and idle: one wake a minute (`pidstat`).

What the estimate gets wrong (it is marked "estimated" everywhere):
- Breaks (a skipped week for a holiday or sports), delays, double episodes and recaps shift the real
  dates; only the user's progress moves the estimate (forward only).
- MyAnimeList's slot is the Japanese TV broadcast. Simulcasts on streaming sites often come an hour
  or more later, or on another day.
- A start date that isn't the regular slot's day (an early premiere, a late-night show listed by the
  next calendar day) is handled one way only: a slot one day before the start date is taken as the
  first; anything else starts at the next slot.
- Shows with no slot time, or a start date without a day (`2027-01`), have no times.
- Unknown episode counts end at MyAnimeList's end date when there is one, else never.

Later (v0.3, opt-in): AniList's `airingSchedule` has the exact time of each episode (AniChart's
data); use it for the Airing times after re-checking AniList's terms (clause 5 allows a client of the
user's own account; a schedule is not the user's list, so it needs a fresh look).

## Stage 6: release 0.1

- [x] `install.sh` (from Uzumaki's: `~/.local`, desktop file, PNG and SVG icons, `--uninstall`, refuses
  to run while Kotora runs). Tested only with a scratch `HOME`: install, then uninstall leaves no file.
- [x] Final icon (tiger head with bokken, `assets/icons/`), PNG sizes 16 to 512 in `assets/icons/hicolor/`.
- [x] Desktop entry: `packaging/linux/io.github.hanami117.kotora.desktop` (no AppStream metainfo: Uzumaki has none).
- [x] README with screenshots from `--snap` scenes (`docs/screenshots/*.webp`, 376 KB) and the credit to Taiga.
- [x] CHANGELOG (0.1.0, date to be set), `THIRD_PARTY_NOTICES.md` (from `cargo metadata`; `cargo deny` is not
  installed here, so run `cargo deny check licenses` once before the tag).
- [x] AUR `packaging/aur/PKGBUILD` (source package, sha256 `SKIP` until the tag) and `.SRCINFO`.
- [ ] About page credits (check the text against the README).
- [ ] `packaging/release.sh` (tarball and its sha256; copy Uzumaki's), then tag `v0.1.0`, fill in the sha256
  and the CHANGELOG date, regenerate `.SRCINFO`.
- [ ] Test the AUR package (`makepkg`, `namcap`, `extra-x86_64-build`) and push it to the AUR.
- [x] Check that `/usr/bin/kot` is free in Arch's file lists (`pacman -Fy; pacman -F usr/bin/kot`): empty on
  Arch (the user, 2026-11-15). No package or AUR name `kot` or `kotora` was found on 2026-11-15.
- [ ] Merge `stage-5` into `stage-6` (the live Now Playing position fix), then rebuild the screenshots.
- [ ] Live test by the user on Arch (Wayland and X11) and on the Mac.
- [ ] Make the repo public (the user's call).
- AppImage and DMG are not part of v0.1 (v0.3).

## Later (v0.2 and on, order to be agreed)

Reordered 2026-11-15 after the first live test (Stage 5): the open-file detectors, search and add and
the details page moved into v0.1; AniList, AppImage/DMG, the corrections UI and library folders moved
to v0.3.

- **v0.2:**
  - local history with undo
  - (the mpv Lua script was dropped on 2026-11-15: the one-click precise mode covers it)
- **v0.3:**
  - AniList (implicit grant; re-check the terms first) and several accounts
  - AppImage (Ubuntu 22.04 container) and DMG (ad-hoc signed)
  - corrections UI ("this file / folder is X")
  - library folders and "play next episode"
  - opt-in window-title detectors on X11, Hyprland, sway and wlroots
  - Airing: AniList's `airingSchedule` (exact per-episode times, AniChart's data) as an opt-in,
    after re-checking AniList's terms
  - (anime-relations episode mapping moved into v0.1, 2026-11-15: done on branch `relations`)
- **v0.4:**
  - (the seasons browser moved forward as Airing, 2026-11-15)
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
  - **Manga (experimental)**, added 2026-11-15:
    - YACReader first: read its library database (`<library>/.yacreaderlibrary/library.ydb`) read
      only, for the current page, page count and read flag
    - generic fallback for other readers: count a chapter when the next chapter file opens, or after
      N minutes open (the `/proc` detector)
    - a manga file name parser (`v03`, `c021`, `Ch.21.5`)
    - MAL manga list sync
    - Komga and Kavita server APIs later still
  - Manga, what exists (2026-11-15, branch `manga-prep`; core only, no UI, not wired in):
    - `core/src/manga/name.rs`: the file name parser (series, volume, chapter, decimals, ranges,
      group, language, year, extras, one-shots; folders give a missing series or volume)
    - `core/src/manga/yacreader.rs`: library list from `YACReaderLibrary.ini`, and a read-only,
      lock-free reader of `library.ydb` (`opened_since`). Fixtures follow YACReader 10.5.1's
      schema (DB 10.5.0). The library list was checked against a real install; the database
      itself was not (the library's drive was not mounted)
    - `manga::chapter_read`: read flag, or `current_page >= num_pages - 1`
  - Manga, what is left:
    - UI: a Manga view and scenes with fake data
    - MAL manga list: service calls, store tables, sync queue
    - match a series to the MAL manga list (recognition for manga), then offer the chapter
    - wiring: a worker that reads `opened_since` and feeds the same ask-first flow as episodes
    - watch the `.ydb`: `detect::dirwatch` only reports create, move and attrib in a folder; it
      needs `IN_MODIFY` / `IN_CLOSE_WRITE` (and kqueue `NOTE_WRITE` on the file), or a slow poll of
      the file's mtime
    - the generic fallback for other readers (`/proc` detector), Komga and Kavita
