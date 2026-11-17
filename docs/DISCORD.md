# Discord Rich Presence

Settings → General → **Show on Discord** shows what you watch as your Discord status. It is off by default;
when it is off, Kotora starts nothing for it.

## What it does

- Kotora talks to the **Discord app on the same computer** over its local socket (a Unix socket
  `discord-ipc-0` to `9` in `$XDG_RUNTIME_DIR` or `$TMPDIR`, including the Flatpak and Snap folders; on Windows
  the named pipe `\\.\pipe\discord-ipc-N`). Kotora makes no connection of its own, and Discord's servers fetch
  the cover image themselves.
- One worker thread, started only while the switch is on. Discord not running: it looks again every 30 seconds,
  quietly. Updates go out at most every 15 seconds (Discord's limit).
- Watching an episode: "Watching Kotora", the title, "Episode 7 of 24", and the player's start and end times
  while it plays ("Paused" and no clock while it is paused). Reading a chapter (when reading detection reports
  one): the title and "Chapter 362" (Discord has no reading type, so it shows as "Playing Kotora").
- The status is cleared when nothing plays, when you turn the switch off and when Kotora quits.
- Options: show the cover; show a button ("View on AniList" or "View on MyAnimeList"; you don't see your own
  buttons, others do); hide while paused or idle; hide the title (shows "Watching anime", no cover, no button).
- `config.toml`:

  ```toml
  [discord]
  enabled = false
  client_id = ""        # your own Discord Application ID; empty: the one built into Kotora
  cover = true
  buttons = true
  hide_paused = false
  hide_title = false
  ```

## Creating the Discord Application (once, by whoever builds Kotora, or by you for your own copy)

1. Open <https://discord.com/developers/applications> and sign in. **New Application**; the name is what
   Discord shows after "Watching": **Kotora**. Accept the terms.
2. **Rich Presence → Art Assets** (the page may be named "Rich Presence" or "Assets"): upload the Kotora icon
   (a square PNG, 1024 × 1024 is best, at least 512 × 512) with the key **`kotora`** exactly. Kotora uses it as
   the small image, and as the large one when there is no cover (or the cover is off or the title is hidden).
   Assets can take a few minutes to appear.
3. Copy the **Application ID** (General Information). It is public, not a secret; the client secret is not
   needed and must not be used.
4. Paste the ID:
   - for everyone: into `DEFAULT_CLIENT_ID` in `core/src/discord.rs` (a placeholder, `"0"`, until then; with
     the placeholder the switch does nothing and Settings says so);
   - for your own copy: `[discord] client_id = "…"` in `config.toml`, which wins over the built-in one.

Covers are the https image URLs of MyAnimeList or AniList. Discord accepts an external image URL as the large
image; if a URL is not https or is over 256 characters, the icon is shown instead.
