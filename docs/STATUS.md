# Kotora: project status

Last updated: 2026-11-15 (stage 2). Read this together with `CLAUDE.md`, `docs/DESIGN.md`
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
- Decided: anitomy = an existing Rust port (MPL-2.0), added and measured in Stage 3/4 (ROADMAP, Stage 1).
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

1. MyAnimeList first (v0.1). AniList in v0.2, strictly as a client of the
   user's own list (AniList API terms, clause 5, 2026-08-18).
2. Torrent/RSS feeds: later, opt-in, no default sources.
3. Automatic progress updates: a Settings switch, off by default.
4. Own identity with a tiger icon that nods to Taiga (see below).
5. CLI name: `kot` (decided 2026-11-15).
6. macOS uses XDG paths, like Uzumaki.
7. Tokens in a 0600 file with a warning; no keyring.
8. Streaming sites in the browser: recommend MAL-Sync.
9. v0.1 packaging: `install.sh` + AUR. AppImage and DMG from v0.2.
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

- VLC on macOS: detect it through its local HTTP interface (user enables it)?
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

## Next steps

1. The user reviews `stage-1` and `stage-2`, builds them on Arch and the Mac, runs the live tests above and
   measures the live budgets (window memory, first frame).
2. Decide on the 15-minute refresh.
3. Stage 3: edits and the sync queue (the first writes to the account; each needs approval).
