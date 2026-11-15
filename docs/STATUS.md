# Kotora: project status

Last updated: 2026-11-15 (stage 5). Read this together with `CLAUDE.md`, `docs/DESIGN.md`
and `docs/ROADMAP.md` before starting work on a new machine.

## Where things are

- Repository: `hanami117/kotora`, **private** until we publish it.
- `main`: design, roadmap, look (stages 0 and 0.5).
- `stage-1`: the workspace skeleton (not merged; waiting for the user's review and live test):
  - `core`, `detect`, `app`, `ctl` crates; Rune pinned to Uzumaki's rev
  - the app: sidebar and list page on fake data, scenes, `--hidden`, single instance
  - `kot status | show | quit`
  - `install.sh`, `scripts/linux-check/`, `deny.toml`
  - lightness budgets measured (docs/ROADMAP.md)
- `stage-2` (on top of `stage-1`, not merged, not pushed): MyAnimeList read only.
  - `core`: SQLite store (migrations by `user_version`), config, token file, PKCE sign-in, MAL client with
    rate limiter and 429 handling, `ListService` trait, loopback listener
  - `app`: the real list from the store and MAL (fake data only in the headless scenes), search, sort,
    Refresh, sign-in and sign-out, sign-in screens, Settings account card
  - read only: no write to the MAL account; `+1` is hidden in the real app
  - tested against recorded JSON only; **not yet run against the real MAL**
  - polish (2026-11-15): macOS traffic lights in a band above the logo, Settings with Accounts and
    About cards, real covers in the list (disk cache, decoded at display size), Rune's `dbus` feature
    dropped to save size on Linux
- `stage-3` (on top of `stage-2`, not merged, not pushed): edits and the sync queue.
  - `core`: `pending_op` (schema 2) and `queue.rs` (merge, conflict check on `updated_at`, backoff, sign in
    again), MAL `GET /anime/{id}?fields=my_list_status` and `PATCH /anime/{id}/my_list_status`, one shared
    rate budget
  - `app`: inline editor, `+1`, waiting tags, the Sync queue page, the worker thread (only while there is work)
  - `kot list | +1 | set | queue | sync`
  - tested against recorded-shape JSON and fake services; **nothing has written to the real MAL yet**
- `stage-4` (on top of `stage-3`, not merged, not pushed): detection; completes v0.1.
  - `detect`: mpv IPC (folder watch while the socket is missing), MPRIS (Linux, zbus 5 blocking, the
    copy AT-SPI builds), VLC's web interface (macOS, opt-in), the watch hub (threshold from the clock,
    no poll), desktop notifications
  - `core`: `recognize.rs` (anitomy-ng, list match, seasons, cache in schema 3, one MAL search),
    `config.toml` `[updates]`, `[players]`, `[vlc]` and a line writer, "keep the higher progress" setting
  - `app`: Now Playing page and banner, Update / Not now / Wrong anime…, auto-update with Undo,
    Settings Updates and Players cards
  - `kot now | confirm | dismiss`
  - tested with recorded messages, a fake mpv on a temp socket and a fake MPRIS player on a private bus
- `stage-5` (on top of `stage-4`, pushed for review, not merged): after the first live test.
  - `core`: `check_client_id` / `save_client_id`, `[notices]` in `config.toml`; MAL `browse` (search
    rows) and `details` (`GET /anime/{id}`); schema 4: `pending_op.added` (adds through the queue,
    `Store::add_to_list`) and the `details` cache (list titles, others a week / newest 100)
  - `app`: Client ID guide and field (Welcome, Settings), search across the list plus MAL results with
    add, the details page, Now Playing links and "Add as Watching", the detection GUI (per-source
    player lines, the one-time precise-detection banner)
  - `kot config client-id ID`, `kot add QUERY|ID [--status S]`
  - tested with recorded-shape JSON, fake services and scenes; **nothing has talked to MAL**
- `stage-5-detect` (another session, same base): the detection engine (process scan, watched-time
  rule, macOS `libproc`, `mpv.conf` helper). Merge it into `stage-5`, then wire the one
  `TODO(stage-5-detect)` in `app/src/precise.rs`.
- Decided: anitomy = `anitomy-ng` 1.0.10 (MPL-2.0), added in Stage 4: +1.0 MB on macOS, +1.6 MB on Linux
  with its `regex` (ROADMAP budgets).
- Still open: the 15-minute refresh, the build-time Client ID default (see docs/ROADMAP.md, Stage 2).

## Git rules (the repository will be public)

- Commit as `hanami117 <140802113+hanami117@users.noreply.github.com>`, set in the
  repository's local git config. Never use a work identity or email.
- Every commit and tag is SSH-signed (`gpg.format ssh`, `commit.gpgsign true`,
  `tag.gpgsign true`). If signing fails, stop: never disable it.
- Use the HTTPS remote with the `gh` credential helper. SSH host aliases for
  `github.com` may point to a different account.
- One branch per topic; nothing reaches `main` before it has been tested live.

## Decisions (2026-11-14)

Full details in `docs/DESIGN.md` section 12.

1. MyAnimeList first (v0.1). AniList in v0.2 (v0.3 since 2026-11-15), strictly as a client of the
   user's own list (AniList API terms, clause 5, 2026-08-18).
2. Torrent/RSS feeds: later, opt-in, no default sources.
3. Automatic progress updates: a Settings switch, off by default.
4. Own identity with a tiger icon that nods to Taiga (see below).
5. CLI name: `kot` (decided 2026-11-15).
6. macOS uses XDG paths, like Uzumaki.
7. Tokens in a 0600 file with a warning; no keyring.
8. Streaming sites in the browser: recommend MAL-Sync.
9. v0.1 packaging: `install.sh` + AUR. AppImage and DMG from v0.2 (v0.3 since 2026-11-15).
11. (2026-11-15) Last episode sets Completed.
12. (2026-11-15, after the first live test on Arch) Stage 5 comes before the release: the Client ID
    from the app, zero-config detection, search and add, details page. AniList, library folders,
    corrections UI, AppImage/DMG move to v0.3. DESIGN.md §12.12.
10. Separate project from Taiga, with clear credit (README + About).

Players for v0.1: mpv (IPC socket + MPRIS), VLC (MPRIS), mpc-qt (MPRIS) on
Linux; mpv, IINA and VLC on macOS.

## Icon

Chosen 2026-11-15: **`C2.svg`**, the angular "ninja" tiger head with a bokken, navy tile.
The other finalist was `A2.svg` (feisty tiger cub, night-blue tile). Both stay in `assets/icons/concepts/`.
- `sheet-v1.png`: all first-round concepts on light and dark backgrounds.

Two refinements of C2 were rejected (one too intimidating, one too childish).
C2 still needs a simplified small variant for 16-32 px (the bokken may drop out there), following
Uzumaki's `assets/icons` pipeline (`rsvg-convert`).

## Open questions

- `+1` (and recording a detected episode) on the last episode: set Completed automatically? (the user)
- VLC on macOS: built through its web interface (opt-in, password in `config.toml`); to verify live.
- README still lists AniList, MyAnimeList and Kitsu; update it to MAL first.

## MyAnimeList app registration (2026-11-15)

- Registered as app type "other" (no client secret), non-commercial, homepage `https://kotora.moe`.
- Redirect: `http://127.0.0.1:47321/callback`. Port 47321 is fixed; sign-in must use exactly this URL.
- The Client ID is NOT in the repo (DESIGN.md 8.1). It lives in the user's local
  `~/.config/kotora/config.toml` (`[mal] client_id`) on each machine.

## Domain

`kotora.moe` (homepage; a privacy page can go there later).

## Live test of stage 2 (the user)

The agent never signed in. On the Mac (then on Arch), from the `stage-2` checkout:

1. Put the Client ID in `~/.config/kotora/config.toml`:
   ```
   [mal]
   client_id = "your client id"
   ```
   The app registration needs the redirect `http://127.0.0.1:47321/callback`.
2. `cargo run --release -p kotora` (or build and start `kotora`). Click **Sign in**: the browser opens, you
   approve, the tab says "Kotora is signed in", and the list loads.
3. Check: the five status counts match MyAnimeList, search and Sort work, **Refresh** works, the sidebar says
   "Synced just now". Quit and start again with the network off (`KOTORA_OFFLINE=1 kotora`): the saved list
   shows at once.
4. Settings: **Sign out** removes `~/.local/share/kotora/tokens.json` and the saved list.
   `ls -l ~/.local/share/kotora/` should show `tokens.json` as `-rw-------`.
5. Tell the agent what you see. If MAL answers an error, copy the red line; it never contains a token.

The first real call also confirms two guesses: the `fields=` list of the animelist request, and that a
403 or 429 does not appear on a normal list of a few hundred entries.

## Live check of the polish (the user)

1. Mac: the traffic lights sit in their own band above the Kotora logo, lined up with it; dragging that
   band moves the window; in full screen the band goes away.
2. Arch: the window looks as before (the desktop's title bar, no band).
3. Settings, wide and narrow window: two cards side by side, stacked below about 1100 pt wide.
   The kotora.moe and Source code links open the browser.
4. After signing in: covers appear in the list within a second or two (tiles first). Restart with
   `KOTORA_OFFLINE=1`: the covers come from `~/.cache/kotora/covers` at once. Sign out: that folder goes.
5. Memory with the window open on a long list (Completed), after scrolling to the end and back:
   `footprint $(pgrep -x kotora)` (Mac) or `grep Pss /proc/$(pgrep -x kotora)/smaps_rollup` (Arch).

## Live test of stage 3 (the user)

**These are the first writes to your real MyAnimeList list. Nothing is sent until you do it, and only for
the entries you pick.** Use an entry you don't mind changing and changing back. The agent never wrote to
the account.

Build from the `stage-3` checkout: `cargo build --release -p kotora -p kotora-ctl`, then start
`target/release/kotora` (quit any other copy first).

1. Read only first: the list loads as in stage 2. The sidebar shows no Sync queue badge.
2. One `+1` on a Watching entry. At once: the number goes up, the row says "Waiting to sync", the badge
   says 1. Within a few seconds the badge goes away. Open the entry on myanimelist.net and reload: the
   episode count matches. Then set it back (row press → editor → `−` → Save) and check MAL again.
3. The editor: press a row, change the score, Save. Check MAL. Change it back.
4. Offline: quit, start with `KOTORA_OFFLINE=1`, press `+1` twice on one entry. The queue page shows one
   change "Progress N → N+2 (2 edits merged)". Quit, start normally: it goes out by itself. Check MAL, set it
   back.
5. Conflict: start with `KOTORA_OFFLINE=1`, change a score in Kotora; then change the same entry's score on
   myanimelist.net. Quit, start normally: the queue page shows a conflict card with both values and sends
   nothing. Pick **Take theirs** (or Keep mine) and check MAL.
6. `kot`: `kot list`, `kot list all`, `kot +1 <part of a title>`, `kot set <title> <ep>`, `kot queue`,
   `kot sync`. Set anything you changed back.
7. Tell the agent what you saw. If a change fails, the queue page shows MAL's words in red; copy them (they
   never contain a token).

The first real call also confirms the two guesses of this stage: `GET /anime/{id}?fields=my_list_status`
returns the list status, and `PATCH` answers with the saved entry and its new `updated_at`.

## Incident during stage 4 (2026-11-15, the agent)

While measuring, four short test copies of the app were started with scratch settings that zsh did not
apply (`env $E` is one word in zsh). They ran with the real paths on the Mac:
- one read of the MAL list with the real token at 19:55 (no write: the queue was empty; `tokens.json`
  unchanged);
- **the real database was migrated to schema 3** (a new, empty `recognition` table). A `stage-3`
  build now refuses it ("the database is from a newer Kotora"). Either test with the `stage-4` build, or
  go back with
  `sqlite3 ~/.local/share/kotora/kotora.db "DROP TABLE recognition; PRAGMA user_version = 2;"`
  (with Kotora closed). The agent did not touch the database again.
- No player socket existed, so no player was contacted. The agent now checks the scratch socket exists
  before any measurement.

## Live test of stage 4 (the user)

Build from the `stage-4` checkout: `cargo build --release -p kotora -p kotora-ctl`, start
`target/release/kotora`. Use a file named like a show on your Watching list, one episode past your
progress (e.g. `Show Title - 06.mkv` when you are at 5). **Update writes to MAL** like `+1`: use an
entry you don't mind changing back.

**Arch:**
1. VLC: play the file. Settings → Players says "VLC · MPRIS · playing"; the sidebar card shows it.
   Now Playing shows the parsed title, episode, match and your progress.
2. Skip to about 70 % and let it play past 80 % (2 minutes of real play are needed): the banner and a
   notification ask "Update … to episode N?". Press **Not now**: nothing changes. Play another file and
   press **Update**: the row says "Waiting to sync", then MAL has it. Set it back.
3. mpc-qt: the same. Tell the agent what Settings shows: this confirms that mpc-qt gives a `file://` URL.
4. mpv: either with mpv-mpris, or with `input-ipc-server=$XDG_RUNTIME_DIR/mpv.sock` in your own mpv.conf
   (your choice; Kotora never edits it). Start mpv after Kotora too: it must be found without a restart.
5. `kot now`, `kot dismiss`, `kot confirm` while a file plays.
6. Wrong anime…: pick another title; play the file again: it remembers. "Not an anime": no more offers.
7. CPU with a player open and playing: `pidstat -p $(pgrep -x kotora) 5 12` should stay near 0.

**Mac:**
1. mpv with `input-ipc-server=/tmp/mpv.sock`, and IINA with `input-ipc-server=/tmp/iina.sock` in
   Settings → Advanced → additional mpv options: same checks as above (banner, notification, Update).
   The notification may come from "Script Editor".
2. VLC: turn on the web interface (Preferences → Show All → Interface → Main interfaces: Web; Lua: a
   password), put `[vlc] password = "…"` in `~/.config/kotora/config.toml`, restart Kotora. Settings
   says "web interface · playing" while VLC plays. Tell the agent whether the file name shows: this
   checks VLC's `status.json` fields on macOS.
3. Settings: switch "Update automatically" on, play an episode past 80 %: it is recorded with Undo in
   the banner. Press Undo, check MAL. Switch it off again.
4. CPU: `top -pid $(pgrep -x kotora)` with VLC set up shows a wake every 15 s while it plays (the one
   poll); with only mpv/IINA, 0.
5. Memory with the window open: `footprint $(pgrep -x kotora)`.

## Live test of stage 5 (the user, on Arch)

First merge `stage-5-detect` into `stage-5` and wire the TODO in `app/src/precise.rs` (the detection
parts below need it). Build: `cargo build --release -p kotora -p kotora-ctl`, quit any running Kotora,
start `target/release/kotora`. **Adding and editing write to MAL** like `+1`: use titles you don't mind
adding (Kotora can't remove an entry yet; remove a test add on myanimelist.net, then Refresh).

1. Client ID guide, without touching your real settings: quit Kotora, then
   `mkdir -p /tmp/kot-cfg && XDG_CONFIG_HOME=/tmp/kot-cfg target/release/kotora`. The Welcome page shows
   three steps. "Open the API page" opens myanimelist.net/apiconfig. Copy, then paste somewhere: it is
   `http://127.0.0.1:47321/callback`. Paste `hello world` and Save: a red line says why. Paste your
   Client ID and Save: "Client ID saved" and Sign in works (your token in `~/.local/share/kotora` still
   applies, so the list may load at once). `stat -c %a /tmp/kot-cfg/kotora/config.toml` says 600.
   Quit, `rm -rf /tmp/kot-cfg`, start Kotora normally.
2. Settings → Accounts: the Client ID shows as dots and its last four characters. Change, paste
   `abc`, Save: "too short". Cancel. `kot config client-id YOUR_ID` answers "Saved the Client ID".
3. Search: type two letters: your list's matches, and "Type 3 letters or more". Type the title of a show
   you don't have: about half a second later "On MyAnimeList" lists results with covers, type,
   episodes, season and ★ score. The ▾ menu shows the other statuses; Escape closes it. Retype the same
   query: the results come back at once (from memory). With `KOTORA_OFFLINE=1`, search says it needs
   the network and still searches your list.
4. Add (writes to MAL): "+ Watching" on one result: it moves up to "On your list" with "Waiting to
   sync", the Sync queue shows "Add as Watching", then it goes out. Check myanimelist.net.
5. Details: click a title in the list (the title, not the row): cover, titles, score, rank, dates,
   studios, genres, synopsis; on the right your entry with its start date. Edit, change the score,
   Save: it goes through the queue (writes to MAL; set it back). `+1` works there. Back and Escape
   return to the list. "Open on MyAnimeList" opens the browser. Click a search result: its page has
   "Add as Watching" and the other statuses. Open the same title again with `KOTORA_OFFLINE=1`: it
   shows from the saved copy.
6. `kot add EXACT TITLE` adds one (writes to MAL); `kot add` with a common word lists ids to choose.
7. Detection (needs the merge): start mpv with no `input-ipc-server` and no mpv-mpris and play an
   episode. Settings → Players says "mpv · process · playing"; Now Playing says "No position from the
   player". A blue banner offers precise detection. **This edits your mpv.conf**; back it up first
   (`cp ~/.config/mpv/mpv.conf ~/.config/mpv/mpv.conf.bak`, if you have one). Click "Turn on precise
   detection": the banner shows the line added. Restart mpv: Settings says "IPC socket · playing" and
   Now Playing shows the position. Restart Kotora: the banner doesn't come back ("No thanks" is also
   remembered; delete `[notices]` from `config.toml` to see it again).
8. A file of a show not on your list: Now Playing's "Not on your list" card has "Add as Watching"
   (writes to MAL) and "Details"; after adding, the episode is offered as usual.
9. Tell the agent what you saw; copy any red line (they never contain a token). CPU at rest with the
   window open and covered: `pidstat -p $(pgrep -x kotora) 5 12`; memory:
   `grep Pss /proc/$(pgrep -x kotora)/smaps_rollup`.

## Next steps

1. The user reviews `stage-1`, `stage-2` and `stage-3`, builds them on Arch and the Mac, runs the live tests
   above (stage 3 writes need the user's OK) and measures the live budgets (window memory, first frame).
2. Decide on the 15-minute refresh, and the open points of Stage 3 (docs/ROADMAP.md).
3. Stage 4 review and the live player tests above; then the open points of Stage 4 (docs/ROADMAP.md),
   among them whether the last episode sets Completed.
4. Stage 5: merge `stage-5-detect` into `stage-5`, wire the precise-detection TODO, run the live test
   above, then Stage 6: release 0.1.
