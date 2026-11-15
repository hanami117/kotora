# Kotora: project status

Last updated: 2026-11-15 (stage 1). Read this together with `CLAUDE.md`, `docs/DESIGN.md`
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
- Still open from stage 1: the SQLite store and the anitomy decision.

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

## Next steps

1. The user reviews `stage-1`, builds it on Arch and the Mac, and measures the live budgets
   (window memory, first frame).
2. Finish stage 1: the SQLite store and the anitomy decision.
3. Stage 2: MyAnimeList, read only.
