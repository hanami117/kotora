# Kotora design

Status: design updated with the user's decisions of 2026-11-14 (section 12); no code yet. Researched
2026-11-14; facts about outside projects and services are dated and sourced at the end, and things not
confirmed are marked **(unverified)**.

Kotora (小虎, "little tiger") is a native anime tracker for Linux and macOS, written in Rust with the
[Rune](https://github.com/anomalyco/rune) GPU UI framework. It is a tribute to
[Taiga](https://taiga.moe/): it keeps your list on an online service up to date (MyAnimeList first, AniList
from v0.2), notices the episode you are watching in your own media player and offers to record it.

---------------------------------------------------------------------------------------------------

## 1. Why, and where the field stands

- **Taiga** (GPL-3.0, C++) is still Windows-only in its stable form: v1.4.1, released 2024-03-21. A real
  cross-platform **v2 rewrite on Qt 6 / C++23** is under active development on its default `develop`
  branch (277 commits in 2026 so far), and it has built on Linux since 2026-09. Its media detection is
  still `#ifdef Q_OS_WINDOWS` (Anisthesia: window titles, open files, UI Automation). It has no torrent
  feature yet and no release date. The homepage still says "Windows only".
- **Trackma** (Python, GPL-3.0, maintained) comes closest on Linux. It has offline sync, many services
  and good detection (inotify, lsof, MPRIS, Plex/Kodi/Jellyfin). Its GTK/Qt UI is dated, and it has no
  seasons, statistics or history views.
- **Hachidori** (macOS) has been stale since 2024. **Tundra** (Rust, MAL only, MPRIS) is small.
  **MAL-Sync** covers browsers only.
- **Seanime** (Go, polished, cross-platform) is a media-server and streaming hub centred on AniList. It is
  not a light tracker for whatever player you already use.

**There is no polished, maintained, native tracker for Linux and macOS that works like Taiga.** Taiga v2
may become one; whatever it ships, its open problem is native detection on Wayland and macOS. That is
exactly where Kotora should be strongest.

## 2. Goals

1. **Track without typing.** Play an episode in your usual player and Kotora recognises it and asks
   "Episode 5 of *Frieren*: update?". One click or one key records it.
2. **Native and fast on Linux (Wayland and X11) and macOS.** It uses the Rune GPU UI, idles at near-zero
   CPU and starts instantly from a local cache.
3. **Offline-first.** Every change lands locally at once and goes into a visible queue that syncs when the
   service is reachable. Failures are never silent (Taiga issue #1257).
4. **Recognition you can see and correct.** Kotora shows why a file matched, and "this file is X"
   corrections are remembered.
5. **Scriptable.** `kot`, a command line over a Unix socket (like Uzumaki's `uzu`), plus an mpv script.
6. **Honest packaging:** install.sh and AUR for v0.1; AppImage and DMG from v0.2. Same tooling as
   Uzumaki.

## 3. Non-goals

- **No streaming, no downloading, no DRM work.** Kotora tracks; it doesn't play or fetch video. Torrent
  feeds are "later, opt-in, no default sources" (section 12), not a plan.
- **Not a list host.** The online service is the source of truth for the list. Kotora keeps a cache and a
  queue, never a competing list or account system. This is also a terms-of-use requirement (section 8).
- **Not a Taiga fork.** Kotora stays separate from Taiga and credits it (section 12.10).
- **No Windows build** (Taiga exists there), and no mobile.
- **No mass metadata crawling** or mirror of a service's catalogue.
- **No scraping of streaming sites** in the app. For browsers we recommend MAL-Sync and ship no extension
  of our own for now.

## 4. Features

### v0.1: the smallest useful tracker
- **MyAnimeList sign-in** (OAuth 2 with PKCE, `code_challenge_method=plain`, loopback redirect;
  section 8.1). It needs a free MAL **Client ID**, which the user or the packager registers (section 8.1
  says where it goes). The token is kept in a 0600 file, with a warning (section 11).
- **List view:** the user's MAL anime list by status (Watching, Plan to watch, Completed, On hold,
  Dropped; "rewatching" is a flag on a Watching or Completed entry). Rows show cover, title, progress
  `5 / 12`, score and a `+1` button. Search filters the list. The list is cached locally and opens
  instantly offline.
- **Edit an entry** (progress updates are the core of v0.1; status and score ride on the same queue):
  progress, status and score. Optimistic local update, then queued sync.
- **Now-playing detection** from these players:
  - **mpv** (Linux and macOS): the JSON IPC socket, plus MPRIS on Linux through mpv-mpris.
  - **VLC:** MPRIS on Linux. On macOS VLC has no MPRIS; see section 5.5.
  - **mpc-qt** (the Linux MPC-HC clone): MPRIS. It publishes `org.mpris.MediaPlayer2.MpcQt` (confirmed
    from its source, section 5.5); `xesam:url` content is **(unverified)**. MPC-HC itself is Windows-only
    and out of scope.
  - **IINA** (macOS): mpv's IPC through "additional mpv options".
  - Other MPRIS players that publish a `file://` `xesam:url` (Haruna, Celluloid, …) work without being
    named targets.
- **Recognition:**
  1. An anitomy-style parse of the file name: title, episode, season, release group.
  2. A match against the user's own list (titles and alternative titles from MAL).
  3. Otherwise, one rate-limited MAL search (`GET /anime?q=`).
  4. Results are cached.
- **Update with confirmation (the default):** when the episode passes a threshold (default 80 % watched,
  or the player stops after at least 50 %), the app shows a banner and a desktop notification: "Update
  *X* to episode N?". The choices are Update, Not now, and "Wrong anime…".
- **Auto-update switch:** Settings has a switch, **OFF by default**, built like Uzumaki's settings
  switches. When on, a recognised episode that passes the threshold is recorded without asking. A
  low-confidence match still asks. Every auto-update goes to the history with an undo.
- **Sync queue:** pending changes are shown with their state. Kotora retries with backoff, honours
  `Retry-After`, and asks to sign in again when the token expires.
- **`kot`:** `status`, `now`, `list [status]`, `+1 <title>`, `set <title> <ep>`, `confirm` / `dismiss`,
  `queue`, `sync`, `quit`.
- **Headless checks:** `--snap/--scene/--tree` scenes for every view, with fake data.

### Later (roughly in order; see docs/ROADMAP.md)
- **AniList** (v0.2; implicit grant, section 8.2) as a second service. Any number of accounts, one
  primary. Still a pure client of the user's own list (AniList terms clause 5).
- **The mpv Lua script:** reports file, position and end to `kot`, so no IPC socket setup is needed.
- **Fallback detectors:**
  - Linux: `/proc/<pid>/fd` of known players.
  - macOS: `libproc` open files.
  - Window titles where a compositor allows it: X11, Hyprland, sway or wlroots foreign-toplevel. These
    are opt-in.
- **Recognition corrections UI:** "this file / folder is X", episode offsets, and anime-relations
  (CC0) for numbering that continues across seasons.
- **History:** a local log of what was watched and when, with undo.
- **Seasons browser:** current, next and past seasons from the signed-in service (MAL: `GET
  /anime/season/{year}/{season}`), fetched on demand. Add to Plan to watch.
- **Library folders:** scan for which episodes are available locally; play the next episode in your
  player.
- **Search and add** any anime. Entry details page.
- **Statistics:** time watched, score distribution.
- **Sharing:** Discord Rich Presence (opt-in).
- **Streaming sites:** use MAL-Sync (browser extension). No extension of our own for now.
- **Media servers:** Jellyfin, Plex, Kodi JSON-RPC.
- Kitsu and Shikimori (low priority; Kitsu's health is doubtful).
- Tray icon, auto-start, hidden mode (`kotora --hidden`, as in Uzumaki).
- **Torrent RSS feeds** with filters: later and opt-in, with no default sources.

## 5. Architecture

### 5.1 Workspace (proposed; Uzumaki's layout and conventions)

```
Cargo.toml          workspace, resolver 3, edition 2024; one [workspace.package] version and license
BUILD               build number (Uzumaki's scheme: versions plus package rebuilds)
core/               kotora-core: no UI, no platform code. Data model, SQLite store, services
                    (MAL first, AniList from v0.2), sync engine and queue, recognition (parse, match,
                    cache).
                    Most tests live here.
detect/             kotora-detect: now-playing sources behind one trait. MPRIS (zbus), mpv IPC,
                    later /proc, libproc and window titles. Emits `Playing` events.
app/                kotora: the Rune app. State, views, scenes; spawns core workers and detectors.
ctl/                kotora-ctl: the `kot` protocol (Request/Reply JSON lines), serve/Client, `kot`
                    binary, socket path, single instance, version line (build.rs git facts).
extras/mpv/         kotora.lua (later): the mpv script.
packaging/          install.sh at the top; appimage/, aur/, aur-bin/, macos/, release.sh (later).
docs/               DESIGN.md, ROADMAP.md, rune-notes (pointer to Uzumaki's), protocol notes.
```

`core` and `detect` stay free of Rune, so they build and test fast and `kot` can link `core` for offline
reads (as `uzu` reads Uzumaki's cache).

### 5.2 Threads and data flow

```
 detect (thread per source) ──Playing{path|title, player, pos, dur}──▶ recognizer (core)
                                                                         │ Match{media, ep, why}
                                                                         ▼
 Rune UI state ◀── AppHandle::update_if ── app glue ──▶ UpdateDecision (threshold) ──▶ banner/notify
       │ user edits / confirms                                                          │ confirm
       ▼                                                                                ▼
 core::Store (SQLite, single writer thread) ◀──── enqueue op ──── sync worker (per account, rate limit)
                                                                      │ HTTP (ureq, blocking, own thread)
                                                                      ▼
                                                               MAL REST (v0.1) / AniList GraphQL (v0.2)
```

- The Rune state lives on the UI thread. Workers report through `AppHandle::update_if`, the pattern in
  Uzumaki's `docs/rune-notes.md` §6A, returning true only on a visible change.
- HTTP: `ureq` 3, blocking on worker threads, as in Uzumaki. No async runtime of our own. (Rune pulls in
  tokio internally; we don't need it.)
- One SQLite writer thread owns the connection and receives commands over a channel. Reads for the UI go
  through snapshots that the store sends after each change.

### 5.3 Data model (SQLite, `rusqlite` with bundled SQLite, migrations by `user_version`)

| table | purpose |
|---|---|
| `account` | service (`mal` in v0.1, `anilist` from v0.2), user id and name, primary flag; the token is in the 0600 token file, not here |
| `media` | keyed by (service, id); MAL id is the native key in v0.1, and from v0.2 AniList rows carry `idMal` for cross-mapping; titles (main, English, Japanese, synonyms), episodes, format, season/year, status, cover URL, `fetched_at`. Only media on the user's list or recently shown are kept (no hoarding) |
| `entry` | the cached remote list entry: account, media, status, progress, score, rewatching flag, started/completed dates, remote `updated_at` (the base for conflict checks; MAL's `list_status.updated_at`). The status enum is Kotora's own, mapped per service (MAL `plan_to_watch` / `on_hold`, AniList `PLANNING` / `PAUSED`); scores are stored as 0-10 integers and converted per service later |
| `pending_op` | the queue: account, media, kind (`progress`, `status`, `score`, `delete`, …), new value, the base `updated_at` seen, created, attempts, `next_try`, last error, state (`queued`/`sending`/`conflict`/`failed`) |
| `recognition` | cache: normalised parsed title (+ season, group) → media id, source (`auto`/`user`), confidence, last used |
| `history` | local log: media, episode, when, how (detected/manual), op id (for undo) |
| `kv` | small settings that are not user-edited (last sync cursor etc.) |

User-edited settings live in `config.toml` (TOML, as Uzumaki's `uzu` config). The UI writes it too.

### 5.4 Sync engine (offline-first)

- **Local first:** an edit updates `entry` (displayed at once) and appends a `pending_op` in one
  transaction.
- **Coalescing:** consecutive ops of the same kind on the same entry merge. Progress 3→4→5 becomes one
  "progress = 5" while queued.
- **Worker:** per account, serial. It sends the oldest op, using `PATCH /v2/anime/{id}/my_list_status`
  (form-encoded) on MAL and, from v0.2, `SaveMediaListEntry` on AniList. On success it stores the
  returned entry (new `updated_at`) and drops the op. A service is a trait in `core`, so the queue and
  worker know nothing about MAL or AniList.
- **Conflicts:** if the remote entry changed after our base, for example from the phone, we neither
  overwrite nor drop. The op goes to `conflict` and the UI offers "keep mine / take theirs". Progress-only
  conflicts where ours is higher resolve automatically to the higher number (setting).
- **Errors:**
  - 429: wait `Retry-After`.
  - 5xx or network: exponential backoff to 15 min.
  - 401: mark the account "sign in again" and keep the queue.
  - 4xx validation: `failed` with the message, shown in the queue view.
  - Nothing is silent. A non-empty queue shows a badge.
- **Rate limits:** a token bucket per service.
  - **MAL (v0.1):** the API documents no limit **(unverified)**, but it is known to answer 429 or block
    on bursts. Budget 20 requests a minute, at least 1 second between requests, and back off on 429 or
    403. Refreshing the token counts as a request.
  - **AniList (v0.2):** officially 90 a minute but currently 30 "degraded", with burst limiting, so
    budget 25 a minute.
- **Pull:** the full list is fetched at start (MAL: `GET /users/@me/animelist` with `limit=100`, paged
  with `paging.next`, fields chosen to carry list status and alternative titles), on demand, and every
  15 min while the window is visible. It is merged under pending ops: local pending values win until
  sent.

### 5.5 Detection pipeline

1. **Sources** (`detect`, each one optional, all feeding one stream of
   `Playing { source, player, path: Option<PathBuf>, title: Option<String>, position, duration, state }`):

   | source | platform | gives | notes |
   |---|---|---|---|
   | mpv JSON IPC | Linux, macOS | path, title, position, duration, pause | the user sets `input-ipc-server` (mpv.conf, IINA "additional mpv options"); we watch the configured socket path(s). v0.1 |
   | MPRIS (zbus) | Linux | `xesam:url` (file URI for VLC, mpv-mpris, …), title, position, length | watch `NameOwnerChanged` + `PropertiesChanged`; browsers give title only (weak hint). v0.1 |
   | VLC HTTP interface | macOS | file name (`information.category.meta.filename`), time, length, state from `/requests/status.json` | VLC has no MPRIS on macOS. The user enables the web interface and sets a password in VLC's preferences; Kotora stores that password in the config, so it is opt-in and the Settings page says so. Field names **(unverified)**; check against a live VLC in stage 4. A `libproc` open-files fallback comes later. v0.1 |
   | mpv Lua script → `kot report` | Linux, macOS | path, position, end | no socket setup (mpv Lua has no sockets, so the script runs `kot` as a subprocess). Later |
   | `/proc/<pid>/fd` | Linux | open video files of known players | same user, no permissions; several open files possible (prefetch). Later |
   | `libproc` open files | macOS | same | believed to need no permission for the same user **(unverified)**. Later |
   | window titles | X11, Hyprland, sway, wlroots (foreign-toplevel) | title | **not available on GNOME or KDE Wayland** without an extension or KWin script; opt-in. Later |
   | browser extension | all | URL, page title | native messaging. Later |

   **Players in v0.1:**

   | player | Linux | macOS |
   |---|---|---|
   | mpv | IPC socket, and MPRIS with mpv-mpris | IPC socket |
   | VLC | MPRIS | HTTP interface (above) |
   | mpc-qt | MPRIS (below) | not available (no macOS build known) **(unverified)** |
   | IINA | n/a | mpv IPC through "additional mpv options" |

   **mpc-qt and MPRIS** (checked 2026-11-14 against the source on GitHub, `src/ipc/mpris.cpp`):
   - **Confirmed:** it serves MPRIS on the session bus as `org.mpris.MediaPlayer2.MpcQt` (a `.instanceN`
     suffix for further instances) at `/org/mpris/MediaPlayer2`.
   - **Confirmed:** `xesam:url` is the now-playing URL as a `QUrl` string; `xesam:title` is mpv's
     media-title, falling back to the URL's file name; `mpris:length` is set only when the duration is
     known (microseconds); `Position` is the mpv time position in microseconds.
   - **Unverified:** that `xesam:url` is a `file://` URI for local files (likely, from `QUrl`, but not
     seen); no `input-ipc-server` option was found in the part of the source read, so no mpv IPC
     fallback. Stage 4 checks this against a real mpc-qt before we claim support.
   - MPC-HC proper is Windows-only and not a target.

2. **Choose:** the most recent playing source wins. Ties go to the order in the table above.
3. **Parse:** run the file name (or title) through the anitomy parser. If the file name lacks a title, use
   folder names.
4. **Recognise:**
   1. Recognition cache (user corrections first).
   2. Fuzzy match against the user's list titles.
   3. One service search (MAL `GET /anime?q=`, rate-limited).
   4. Season and episode mapping via anime-relations (later).
   5. Low confidence: show "Not sure: is this *X*?".
5. **Decide:** watch time and position cross the threshold. The banner, notification and `kot now`
   show the offer; `kot confirm` or the button enqueues the op. Never offer an episode at or below the
   current progress, unless the entry is Rewatching. With the auto-update switch on (default off), the op
   is enqueued without the offer when the match is confident.

### 5.6 Paths, ids, process model

- **App id** `io.github.hanami117.kotora`: desktop file, icon name, Wayland `app_id` (`rune::set_app_id`)
  and macOS bundle id.
- **Paths:** XDG everywhere, on macOS too, as Uzumaki does for settings (decided, section 12.6):
  - config: `$XDG_CONFIG_HOME/kotora/config.toml`
  - tokens: `$XDG_DATA_HOME/kotora/tokens.json`, mode 0600 (section 11)
  - data: `$XDG_DATA_HOME/kotora/kotora.db` (the queue, corrections and history are not re-downloadable,
    so not cache)
  - cache: `$XDG_CACHE_HOME/kotora/` (covers, API responses)
  - log: `$XDG_STATE_HOME/kotora/kotora.log`, and `~/Library/Logs/Kotora` on macOS
- **Socket:** `$XDG_RUNTIME_DIR/kotora.sock` (Linux; `/tmp/kotora-<uid>` fallback), and
  `kotora-<uid>` in `confstr(_CS_DARWIN_USER_TEMP_DIR)` on macOS. `KOTORA_CTL_SOCKET` overrides it.
  Copy `socket_path`, `runtime_dir`, `serve` and `Client` from Uzumaki's `ctl/src/lib.rs`.
- **Single instance:** a second `kotora` connects to the socket, sends `Show`, and exits. This is
  Uzumaki's macOS path (`app/src/ctl.rs` `other_copy_running`), used on both platforms because Kotora
  owns no MPRIS name. `KOTORA_TEST_INSTANCE=1` skips the check, for a test copy beside the user's.
- **Test switches** (Uzumaki's pattern): `KOTORA_TEST_INSTANCE`, `KOTORA_CTL_SOCKET`, plus scratch
  `XDG_*` dirs. `KOTORA_OFFLINE=1` means no network: services answer from fixtures.

### 5.7 UI (v0.1)

- **Sidebar:** the list statuses with counts, Now Playing, Queue (badge), Settings. The service is not
  hard-wired into the UI: v0.2 adds AniList as a second account without a redesign.
- **List page:** virtual rows (`virtual_rows`, ~56 pt), sortable by title, progress, score or last
  updated. Each row has `+1`; clicking a row opens an inline editor.
- **Now Playing** banner on top of every page while something is recognised: cover, "*Title*, episode N
  of M", watched percentage, and Update / Not now / Wrong anime.
- **Sign-in** page (MAL; says what a Client ID is and where to put it, and shows an error when none is
  set) and an empty state.
- **Settings:** account, detection sources (each with a status line: "MPRIS: VLC playing" or "mpv socket:
  not found"), threshold, the **auto-update switch** (off by default, with a one-line explanation), the
  MAL Client ID field, theme.
- **Design language:** Uzumaki's dark, minimal look with Inter, but Kotora has its own identity: a tiger
  icon that cites Taiga's, and its own accent colour (tiger orange is the starting idea). **Icon:
  placeholder for now; the icon work is a separate task.** Light theme later.

## 6. What we take from Uzumaki (copy, then adapt; same author, GPL-3.0-or-later)

| need | copy from Uzumaki (paths in that repo) |
|---|---|
| Rune pin, dev profile opt-level | `Cargo.toml` (workspace, `[profile.dev.package."*"] opt-level = 2`), `app/Cargo.toml` rune `rev = "f3d04f3f2069d177d04c963b0ba9f6b2065e46eb"`; start on the same rev, bump both apps together |
| Rune know-how | `docs/rune-notes.md` (read it whole: threads → UI, virtual rows, text input, images, headless) |
| headless scenes | `app/src/scenes.rs` pattern, `--snap/--scene/--tree/--script`, `rune::is_headless()` guards |
| socket protocol and `kot` skeleton | `ctl/src/lib.rs` (`socket_path`, `runtime_dir` incl. macOS `confstr`, `serve`, `Client`, version line, `BUILD` parsing), `ctl/build.rs` (git facts for `--version`), `ctl/src/boot.rs` (`kot` starts the app hidden via `systemd-run --user`) |
| single instance and hidden start | `app/src/main.rs` (`--hidden`, test instance, `App::windowless`, `keep_running`), `app/src/ctl.rs` (`other_copy_running`, socket thread hopping to the UI with `update_if`) |
| D-Bus with zbus 5 blocking | `app/src/mpris.rs` (we consume MPRIS instead of serving it, but the connection setup is the same), `app/src/desktop.rs` (`file_uri`, FileManager1 ShowItems) |
| settings and XDG dirs | `app/src/settings.rs` `config_dir`, `ctl/src/config` (TOML) |
| artwork cache | `app/src/art.rs` (LRU by bytes, disk cache, downscale to display size, never evict covers on screen) |
| macOS bits | `app/src/logfile.rs` (stderr to ~/Library/Logs), `app/src/nowplaying.rs` objc2 setup, `menu_bar_mac.rs` |
| tray | `app/src/tray_linux.rs`, `tray_mac.rs` (later) |
| update notice | `app/src/update.rs` (`latest.json`, once a day, channel per package) (later) |
| install and packages | `install.sh` (user install to `~/.local`, desktop file, icons, `--uninstall`), `packaging/appimage/{container.sh,build.sh,deps-ubuntu.sh}` (Ubuntu 22.04 container, glibc 2.35), `packaging/aur*/PKGBUILD`, `packaging/macos/build.sh` (bundle, ad-hoc sign, DMG), `packaging/release.sh` (prepares, never publishes), `packaging/homebrew/` |
| checks | `scripts/linux-check/` (Docker build/test/clippy/snap), `deny.toml` (licence allowlist: add MPL-2.0 for anitomy), `.github/workflows/` |

Kotora is simpler than Uzumaki: it has no Electron bridge, no audio and no DRM. Everything is in-process
Rust.

## 7. What we take from Taiga, and what we improve

Behaviour, not code, unless noted. Taiga is GPL-3.0, so code reuse would be legal with attribution, but
its C++ and Qt don't port directly.

**Credit:** the README and the About page say plainly that Kotora is a tribute to Taiga and name its
author and the data we reuse (Anisthesia, anime-relations, anitomy). We stay a separate project and report
bugs we find in Taiga or its data upstream when they are relevant.

**Take:**
- the flow: detect → recognise → wait for a threshold → ask or auto-update
- list pages by status with quick `+1`
- the Now Playing panel
- a recognition cache with manual corrections
- episode relations (`erengy/anime-relations`, **CC0**: use the data file directly)
- the player definitions idea: `erengy/anisthesia` `players.anisthesia` (**MIT**). Executable names and
  title regexes are usable as data for our /proc and window-title detectors, with the MIT notice.
- the update queue
- history, seasons browser, statistics, Discord sharing
- anitomy (**MPL-2.0**, compatible with GPL-3.0-or-later), through a Rust port

**Improve:**
- **Native detection** on Wayland and macOS. Use file paths (mpv IPC, MPRIS `xesam:url`, open files)
  rather than window titles, which break with every player update (Taiga issues #1265, #1271, #1256) and
  don't exist on Wayland.
- **A visible sync queue with conflict handling.** Taiga users hit silent failures (#1257), MAL outages
  (#588, #1250) and Kitsu URL changes (#1249).
- **Transparent recognition:** show the parse and the match reason, and make correction one action.
  Recognition errors are Taiga's biggest issue category (#1267, #1269, #1253, #1254).
- **Scriptable** through `kot`, and an mpv script.
- **Modern UI** that scales with the display. Taiga #1264: text can't be enlarged.
- **Headless snapshot tests** for every view.

## 8. Services, terms and auth

### 8.1 MyAnimeList (v0.1)
- **Endpoints:** authorize `https://myanimelist.net/v1/oauth2/authorize`, token
  `https://myanimelist.net/v1/oauth2/token`, API `https://api.myanimelist.net/v2`.
- **OAuth 2 with PKCE, `code_challenge_method=plain` only** (MAL does not support S256; the `oauth2`
  crate's typed API assumes S256, so hand-roll this small flow). The `code_verifier` (43 to 128
  characters) is also the `code_challenge`. Register the app type "other" (no client secret; the
  exchange then sends only the client id and the verifier) with a loopback redirect, for example
  `http://127.0.0.1:47321/callback` (registered 2026-11-15). The exact redirect must be registered, so the port is fixed
  and shown in the docs.
  1. Open the browser at the authorize URL with `client_id`, `code_challenge`, `state`, `redirect_uri`.
  2. Our loopback listener receives `code`, checks `state`, and exchanges it with the verifier.
  3. Store access and refresh tokens. Access tokens are short-lived (about an hour **(unverified)**)
     and refresh tokens last about a month, so refresh before use and on 401. If refresh fails, the
     account shows "sign in again" and the queue is kept.
- **The Client ID** is free (MAL account, "API" page, create an ID) and public by design. It is **not a
  secret** and the design never needs a client secret. It is read in this order:
  1. `[mal] client_id` in `config.toml` (or the Settings field), so a user can bring their own;
  2. a build-time default, `KOTORA_MAL_CLIENT_ID` in the environment at build time, which a packager may
     set for official builds;
  3. otherwise sign-in is disabled and Settings explains how to register an ID.
  - **Never commit a client secret.** If one ever exists in a registration, it is not used. The repo may
    hold no real Client ID until the user decides to publish one.
  - The registered redirect must match the loopback URL exactly, whichever ID is used.
- **API licence** (2019): it treats open source as non-commercial. Store data client-side only "as long as
  reasonably necessary", never store credentials (we store tokens, never a password), and honour removal
  requests. Also keep the MAL data to the user's own list and recently shown media.
- **Calls:** list `GET /users/@me/animelist` (fields for list status, number of episodes and alternative
  titles; `limit=100`, follow `paging.next`), update `PATCH /anime/{id}/my_list_status` with
  `num_watched_episodes`, `status`, `score` (0-10) and `is_rewatching`, search `GET /anime?q=` (query of
  at least 3 characters **(unverified)**). Rate limit: section 5.4.

### 8.2 AniList (v0.2)
- **Terms clause 5 (changed 2026-08-18):** "Use of the AniList API within competing, non-complementary
  services of the same nature is prohibited. This includes … anime and manga list or tracker services.
  The restriction applies to all data provided through the API, including both user data and media data."
  The old exception for services that sync with AniList accounts was removed. The commit message was
  "Please stop emailing me. No you can't use the API."
  - **Our reading:** a client that manages the user's own AniList list, with AniList as the source of
    truth, is complementary. That is what Taiga, Trackma and every mobile AniList client do. A tracker
    with its own list that borrows AniList's data is not.
  - **So Kotora:** never keeps its own list. It never exports AniList data as a backup (clause 2), never
    hoards (clause 3), and never uses AniList media data for a local-only or other-service list.
  - This is a real risk for the project, and one reason AniList is second (section 12.1). Check the
    terms again before v0.2 starts.
- **Naming:** "Kotora" is fine. "for AniList" would be required only if we used the name.
- **OAuth:** no PKCE and no refresh tokens. Tokens last 1 year. The code grant needs the client secret,
  which an open-source desktop app can't keep, so the plan is the **implicit grant**:
  1. Open the browser at `https://anilist.co/api/v2/oauth/authorize?client_id=…&response_type=token`.
  2. The redirect goes to a loopback page `http://127.0.0.1:<fixed port>/callback`, whose JS posts the URL
     fragment back to our listener. The exact redirect is registered with the client.
  3. **Fallback:** the PIN page (`https://anilist.co/api/v2/oauth/pin`), where the user pastes the token.
  4. The token goes in the 0600 token file. Expiry triggers a "sign in again" banner.
- **Rate limit:** 30 requests a minute (degraded) plus burst limiting, and 403 during outages. Budget 25
  a minute and back off on 429.
- **API:** GraphQL. Hand-written queries in `.graphql` files with `serde` structs (or `graphql_client`
  codegen), `SaveMediaListEntry` for writes, `MediaListCollection` for the list, and `Media.idMal` for
  cross-mapping.

### 8.3 Kitsu, Shikimori (much later)
- Kitsu uses the OAuth password grant (we'd handle the password; dislike) and its health is doubtful.
- Shikimori needs a client secret, a User-Agent with the app name, and allows 5 requests a second and 90
  a minute.

### 8.4 Mapping data
- v0.1 is MAL only and needs no mapping. From v0.2, AniList `idMal` covers AniList↔MAL.
- For offline titles and synonyms and other services, later: **anime-offline-database**.
  - manami-project archived it on 2026-07-04; the continuation is `cedya77/anime-offline-database`.
  - It is ODbL-1.0 + DbCL-1.0, released weekly.
  - **Download at runtime** (no redistribution), show the attribution in About, and keep any derived
    tables local.
- `Kometa-Team/Anime-IDs` (MIT) is an alternative. Avoid `Fribb/anime-lists` until it has a licence; it
  has none today.

## 9. Building blocks (crates to evaluate in stage 1)

- **Filename parsing:**
  - **`anitomy-ng`** 1.0.10 (MPL-2.0, pure Rust, conformance-tested against upstream data by its own
    account; young, 7 stars): the first candidate.
  - Fallback: `anitomy` 0.2 (FFI to C++ v1, dormant) or our own port.
  - Either way, we keep our own test corpus of real file names.
- **Core crates:**
  - SQLite: `rusqlite` 0.42 with `bundled`
  - HTTP: `ureq` 3 (already Uzumaki's choice)
  - D-Bus: `zbus` 5 with the blocking API (the same version Rune builds)
  - JSON: `serde`/`serde_json`
  - config: `toml`
  - macOS open files: `libproc`
- **Token storage:** a 0600 file, no keyring (decided, section 12.7), written with a temp file and
  rename. No keyring crate.
- **mpv IPC:** hand-rolled (JSON lines over a Unix socket) or `mpvipc`.
- **Licences:** every dependency must pass `cargo deny check licenses` with Uzumaki's allowlist (it
  already has MPL-2.0).

## 10. Testing

- **`core`:** unit tests with fixtures. Recorded MAL (and later AniList) responses are anonymised, with no real user data
  in the repo. Coverage includes the queue state machine, coalescing, conflicts, recognition (a file-name
  corpus) and migrations.
- **`detect`:** parsers for MPRIS metadata and mpv events are tested on recorded messages. A fake MPRIS
  player on a private session bus is used in an integration test (`dbus-run-session`).
- **`app`:** scenes for every view and state (signed out, empty list, list, editor, Now Playing banner,
  low-confidence match, queue with conflict, offline, no Client ID set, token-file warning, auto-update
  switch on and off). Checks run headless with `--snap/--tree`, plus
  `--script` for flows.
- **`kot`:** a protocol round-trip both ways, and an old-client/new-server serde compatibility check
  (`serde(default)`), as in Uzumaki.
- **Never against the user's real account** without their OK. Read-only checks use a copy of the token in
  a scratch config; writes need explicit approval each time.

## 11. Security and privacy

- **Tokens** go only in `tokens.json` in the data dir, mode 0600, never in the database or in logs. The
  file is **not encrypted**, so anyone who can read it as that user can use the account. Kotora says so
  in Settings and in the docs ("your token is stored in a file readable only by you") and offers
  sign-out, which deletes it. The data dir is mode 0700 and the socket 0600. The VLC web password
  (macOS) in `config.toml` has the same caveat; the file is 0600.
- No telemetry. An update check (later) is opt-out and anonymous, as in Uzumaki.
- Logs never contain tokens, and contain file paths only at debug level.

## 12. Decisions (2026-11-14)

The user answered the open questions. These are decided; the rest of this document follows them.

1. **MyAnimeList first.** The user uses MAL. v0.1 is MAL sign-in (OAuth 2 PKCE `plain`; the user or
   packager registers a free MAL Client ID, section 8.1), the MAL list view and MAL progress updates.
   AniList moves to v0.2, still a pure client of the user's own list (AniList terms clause 5). This
   changed auth (8.1), rate limits and data model (5.3, 5.4), recognition search (5.5), the roadmap
   stages and the scenes (10). The AniList terms question is no longer blocking; re-check the terms
   before v0.2.
2. **Torrent and RSS feeds:** later, opt-in, no default sources.
3. **Auto-update without confirmation:** a Settings switch, off by default. The default flow is the
   confirmation banner.
4. **Identity:** Kotora's own, with a tiger icon that cites Taiga. Icon chosen 2026-11-15: concept C2
   (angular tiger head with a bokken); a small 16-32 px variant is still to do.
5. **CLI name `kot`:** confirmed.
6. **macOS paths:** XDG, like Uzumaki: confirmed.
7. **No keyring:** the token is in a 0600 file, with a warning (section 11): confirmed.
8. **Streaming sites:** recommend MAL-Sync (browser extension). No extension of our own for now.
9. **v0.1 packaging:** install.sh and AUR. AppImage and DMG from v0.2.
10. **Taiga:** stay separate, credit it clearly (README and About), and report upstream bugs when
    relevant.
11. **Last episode completes (2026-11-15):** like the MAL website, when progress reaches the last
    episode (`+1`, the inline editor, `kot +1` / `kot set`, or a recorded episode) the status becomes
    Completed in the same queued op. Only when the total is known, and not when the change names a
    status itself. Plan to watch still becomes Watching on the first recorded episode (unless that
    episode is the last). The banner says "Completed"; Undo restores progress and status.

**Players for v0.1:** mpv (IPC socket and MPRIS), VLC (MPRIS on Linux), mpc-qt (MPRIS; `xesam:url`
unverified). macOS: mpv, IINA, VLC (HTTP interface). MPC-HC is Windows-only. Details in section 5.5.

## 13. Sources

Taiga and alternatives:
- Taiga releases, branches and issues: https://github.com/erengy/taiga/releases,
  https://github.com/erengy/taiga/commits/develop, issues #92, #588, #1171, #1249, #1250, #1253, #1254,
  #1256, #1257, #1264, #1265, #1267, #1269, #1271
- Taiga v2 detection gating: https://github.com/erengy/taiga/blob/develop/src/track/media.cpp
- Taiga's Linux build fix: https://github.com/erengy/taiga/commit/4f3a9104a2e20f6a8847c3ac2be614e4ab4d1044
- Taiga homepage: https://taiga.moe/
- Anisthesia (MIT) and its player data: https://github.com/erengy/anisthesia,
  https://github.com/erengy/anisthesia/blob/main/data/players.anisthesia
- anime-relations (CC0): https://github.com/erengy/anime-relations
- anitomy (MPL-2.0): https://github.com/erengy/anitomy
- Trackma: https://github.com/z411/trackma
- Hachidori: https://github.com/Atelier-Shiori/hachidori
- Tundra: https://github.com/m4tx/tundra
- MAL-Sync: https://github.com/MALSync/MALSync
- Seanime: https://github.com/5rahim/seanime
- Miru DMCA notice: https://github.com/github/dmca/blob/master/2025/10/2025-10-22-crunchyroll.md

Services and data:
- AniList terms and the clause 5 change:
  https://github.com/AniList/docs/blob/master/docs/guide/terms-of-use.md,
  https://github.com/AniList/docs/commit/03281c0a4bbf0c7f2097e0c935cddaed1096aa65
- AniList auth: https://docs.anilist.co/guide/auth/implicit
- AniList rate limits: https://docs.anilist.co/guide/rate-limiting
- MAL authorization: https://myanimelist.net/apiconfig/references/authorization
- MAL API v2 reference: https://myanimelist.net/apiconfig/references/api/v2
- MAL API licence: https://myanimelist.net/static/apiagreement.html
- Kitsu API: https://github.com/hummingbird-me/api-docs
- Shikimori API: https://shikimori.io/api/doc
- anime-offline-database: https://github.com/manami-project/anime-offline-database,
  https://github.com/cedya77/anime-offline-database
- Kometa Anime-IDs: https://github.com/Kometa-Team/Anime-IDs
- Fribb anime-lists: https://github.com/Fribb/anime-lists
- ODbL: https://opendatacommons.org/licenses/odbl/1-0/

Detection:
- mpv-mpris: https://github.com/hoyon/mpv-mpris
- mpv IPC: https://mpv.io/manual/master/#json-ipc
- MPRIS metadata spec: https://www.freedesktop.org/wiki/Specifications/mpris-spec/metadata/
- Chromium's MPRIS (no URL):
  https://chromium.googlesource.com/chromium/src.git/+/main/components/system_media_controls/linux/system_media_controls_linux.cc
- Wayland foreign-toplevel support: https://wayland.app/protocols/ext-foreign-toplevel-list-v1,
  https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1
- KWin: https://bugs.kde.org/show_bug.cgi?id=502647
- GNOME Window Calls extension: https://github.com/ickyicky/window-calls
- MediaRemote restrictions since macOS 15.4: https://github.com/ungive/mediaremote-adapter
- libproc crate: https://docs.rs/libproc
- IINA mpv options: https://github.com/lhc70000/iina/pull/1181
- mpc-qt MPRIS source: https://github.com/mpc-qt/mpc-qt/blob/master/src/ipc/mpris.cpp
- VLC HTTP interface (macOS path, unverified): https://wiki.videolan.org/VLC_HTTP_requests/

Crates:
- anitomy-ng: https://github.com/tylergibbs2/anitomy-ng
- rusqlite: https://crates.io/crates/rusqlite
- oauth2: https://crates.io/crates/oauth2
