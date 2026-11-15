# Kotora: a native anime tracker for Linux and macOS (Rust + Rune)

Kotora (小虎, "little tiger") is a tribute to Taiga, the Windows anime tracker. It keeps the user's list on
an online service (MyAnimeList first, AniList from v0.3) up to date, recognises the episode playing in the user's
own media player, and asks before it records the episode.

- App id, desktop entry, icon name, Wayland `app_id` and macOS bundle id: `io.github.hanami117.kotora`.
- Repo: `hanami117/kotora`. It is private now and will be public, so treat every commit as public.
- Licence: GPL-3.0-or-later.

## Read first

1. `docs/DESIGN.md`: goals, non-goals, v0.1 scope, architecture, terms of use, and the user's decisions (§12).
2. `docs/ROADMAP.md`: stages and current status. Update it when a stage moves.
3. Uzumaki, our sister app on the same stack, is the knowledge base. It is a sibling checkout,
   `../uzumaki` (public repo `hanami117/uzumaki`).
   - Read its `docs/rune-notes.md` before any UI work.
   - DESIGN.md §6 lists exactly which Uzumaki files and patterns to copy (`ctl/src/lib.rs` socket
     protocol, `app/src/ctl.rs` single instance, `app/src/scenes.rs`, `app/src/art.rs`, `install.sh`,
     `packaging/`, `scripts/linux-check/`, `deny.toml`).
   - **Never modify, build or run anything in the Uzumaki checkout.** Read only.

## Architecture (decided unless DESIGN.md says "open")

- **UI:** Rust + [Rune](https://github.com/anomalyco/rune), pinned to one commit of its `dev` branch.
  Start on Uzumaki's rev and bump both apps together.
- **Crates:**
  - `core`: no UI. Store, services, sync queue, recognition.
  - `detect`: now-playing sources (MPRIS, mpv IPC, …).
  - `app`: the Rune app.
  - `ctl`: the `kot` protocol and binary.
- **Storage:** SQLite (`rusqlite`, bundled), offline-first. The online service is the source of truth.
  Local edits go into a visible `pending_op` queue that syncs and reports conflicts. Tokens are in the
  a 0600 file (no keyring), never in the database or in logs.
- **No async runtime of our own:** blocking `ureq` and `zbus` on worker threads. Results reach the UI
  through `AppHandle::update_if`, returning true only on a visible change.
- **`kot`:** JSON lines over `$XDG_RUNTIME_DIR/kotora.sock` (`KOTORA_CTL_SOCKET` overrides). The same
  socket gives single instance.

## Constraints

- **Service terms.**
  - AniList (v0.3) clause 5 (2026-08-18) forbids use in competing list or tracker services. Kotora is only a
    client for the user's own account: never keep a list of our own built from AniList data, never
    export AniList data as a backup, never crawl or hoard, and respect the rate limit (budget 25 requests
    a minute).
  - MAL (v0.1) API licence: keep data client-side and only as long as needed; never store credentials.
- **Defaults decided 2026-11-14** (DESIGN.md §12): updates ask first (the auto-update switch is OFF by
  default); the MAL rate budget is 20 requests a minute; v0.1 players are mpv, VLC, mpc-qt (macOS: mpv,
  IINA, VLC); no torrent feeds by default; streaming sites are left to MAL-Sync; Kotora stays separate
  from Taiga and credits it in the README and About.
- **No streaming, downloading or DRM work.** Kotora tracks and doesn't play or fetch video. No scraping
  of streaming sites inside the app.
- **No secrets in the repo:**
  - An AniList or MAL client id is public by design. The MAL Client ID goes in `config.toml` or a
    build-time default (`KOTORA_MAL_CLIENT_ID`); see DESIGN.md §8.1.
  - A client secret must never be committed; the design avoids needing one (implicit grant, PKCE `plain` for MAL).
- **No personal data in committed files:** no real names, e-mail addresses, home paths with usernames,
  tokens, or the user's real list or file names. Fixtures are made up or anonymised.
- **Licences:** dependencies must pass `cargo deny check licenses` (GPL-3.0-compatible allowlist, MPL-2.0
  included for anitomy). Data reuse needs its notice:
  - anisthesia player data: MIT
  - anime-relations: CC0
  - anime-offline-database: ODbL, download at runtime, attribution in About

## Rules that keep the user's setup safe

- **Never run the app windowed from an agent.** It would talk to the user's real account and players.
  Check UI headless:
  ```
  ./target/debug/kotora --snap /tmp/out.png --scene <name> --size 1280x800 --scale 1
  ```
  `--tree FILE` dumps the element tree, `--script FILE` drives input, and `--list-scenes` lists scenes.
  Every view and state gets a scene with fake data. Guard network, detectors, the token file and the socket
  with `rune::is_headless()`.
- **Never touch the user's account without their OK.**
  - Reads with a copied token go in a scratch config dir (`chmod go-rwx`, deleted afterwards).
  - Every write to the real list (progress, status, score) needs explicit approval first.
- **A second copy for tests** needs `KOTORA_TEST_INSTANCE=1` (no single-instance check), a scratch
  `KOTORA_CTL_SOCKET`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME`, and
  `KOTORA_OFFLINE=1` unless network is the point. Never share the user's database, socket or token file.
- **Don't touch the user's mpv config, players or desktop settings.** Detector tests use recorded
  messages, a fake MPRIS player on a private bus (`dbus-run-session`) and temp sockets.
- **Performance:**
  - `virtual_rows` for lists; `poll` and `update_if` return true only on visible change
  - never block in view, handler or poll
  - covers kept at display size (Uzumaki's `art.rs` rules)
  - measure with release builds

## Git rules (strict: the repo will be public)

- **Commit only as the repo-local identity** (`git config user.name` / `user.email` in this repo: the
  GitHub noreply address). Never pass another identity (`-c user.*`, `--author`) or e-mail.
- **Every commit is SSH-signed** (repo-local `commit.gpgsign`).
  - If signing fails (key not in the agent), **stop and report**.
  - Never disable signing, never use `--no-gpg-sign`, never touch `~/.ssh` or the global git config.
- **Commit messages:** clean and meaningful (imperative subject ≤ 72 chars, body explaining why),
  ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (use the model actually doing the work).
- **Branches:** work on topic branches. Don't commit to `main` unless the user says so.
- **Pushes:** never push, open PRs, change repo visibility or publish releases unless the user asks.
- Before committing, check the diff for personal data (names, e-mails, `/home/<user>` or
  `/Users/<user>` paths, tokens).

## Working style

- The user is cost-conscious. Work in stages (docs/ROADMAP.md) and stop after each one for approval.
- Use cheaper models or subagents for routine work (reading docs, compile-fix loops, mechanical edits);
  keep contracts and review on the main model.
- The user is on Arch Linux, with a MacBook for macOS checks. Tell the user what to
  build, restart and test live; agents only check headless.
- Write in plain, short English: in docs, commit messages, UI text and `kot` output.
