# Kotora: project status

Last updated: 2026-11-15. Read this together with `CLAUDE.md`, `docs/DESIGN.md`
and `docs/ROADMAP.md` before starting work on a new machine.

## Where things are

- Repository: `hanami117/kotora`, **private** until we publish it.
- `main`: initial commit only (README, LICENSE GPL-3.0-or-later, .gitignore).
- `design` (this branch): design document, roadmap, agent rules, user decisions,
  icon concepts. Not merged into `main` yet.
- No code yet. Stage 1 (workspace skeleton) has not started.

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
5. CLI name: still open (see below).
6. macOS uses XDG paths, like Uzumaki.
7. Tokens in a 0600 file with a warning; no keyring.
8. Streaming sites in the browser: recommend MAL-Sync.
9. v0.1 packaging: `install.sh` + AUR. AppImage and DMG from v0.2.
10. Separate project from Taiga, with clear credit (README + About).

Players for v0.1: mpv (IPC socket + MPRIS), VLC (MPRIS), mpc-qt (MPRIS) on
Linux; mpv, IINA and VLC on macOS.

## Icon

Finalists in `assets/icons/concepts/`:

- `C2.svg`: angular "ninja" tiger head with a bokken, navy tile.
- `A2.svg`: feisty tiger cub, night-blue tile.
- `sheet-v1.png`: all first-round concepts on light and dark backgrounds.

Two refinements of C2 were rejected (one too intimidating, one too childish).
The final icon still needs a simplified small variant for 16-32 px, following
Uzumaki's `assets/icons` pipeline (`rsvg-convert`).

## Open questions

- CLI name: `kot` (free), `tora` (clashes with the AUR package `tora`, the TOra
  SQL IDE, which owns `/usr/bin/tora`), or `kotora`.
- VLC on macOS: detect it through its local HTTP interface (user enables it)?
- Final icon: C2 or A2.
- A MyAnimeList Client ID must be registered (free, myanimelist.net).
- README still lists AniList, MyAnimeList and Kitsu; update it to MAL first.

## Next steps

1. Pick the icon and the CLI name.
2. Merge `design` into `main`.
3. Stage 1: workspace skeleton based on Uzumaki (pinned Rune revision, app and
   ctl crates, headless `--snap` scenes, `install.sh`).
