# Changelog

All notable changes to Kotora. Versions follow [semantic versioning](https://semver.org/).

## [Unreleased]

### Added
- **Manga (MyAnimeList):** an Anime | Manga switch under the account in the sidebar. Its first press turns
  Manga on (off by default, so nobody pays extra requests): syncs then read your MyAnimeList manga list too.
  Manga mode has its own Reading, Plan to read, Completed, On hold and Dropped pages (filter box, sort, chips for
  Publishing, Finished and On hiatus), a details page (authors, serialization, published dates, chapters and
  volumes) and an editor for status, chapters, volumes, score and rereads. Edits go through the same sync queue
  as anime. Progress shows chapters, volumes or both, whichever you have read; `+1` steps chapters (volumes for
  a list read by volume). The switch is remembered (`[display] list_kind`). Home, Now Playing, Airing and Search
  stay anime only. AniList has no manga in Kotora yet: there the switch stays on Anime, with a note.
- **Manga: posters, search, add and remove.** The manga pages have the List / Grid / Large views, kept
  apart from the anime pages' (`[manga_views]`); posters show what you read (Ch and/or Vol) with `+1` on
  hover (always on Reading). With the switch on Manga, Search (Ctrl/⌘ K) searches your manga and
  MyAnimeList's (a MyAnimeList manga link opens its page); results add as Reading or another status, as does
  a manga's details page. Remove from list on a manga's right-click menu and its details page works like the
  anime one: it asks first, leaves at once, waits in the sync queue with Undo, and a manga changed on
  MyAnimeList meanwhile becomes a conflict instead of being deleted. The right-click menu also has Move to,
  Score, +1 chapter, Edit and Open on MyAnimeList. `kot add --manga QUERY|ID` and `kot remove --manga
  TITLE|ID` do the same from the terminal (Manga must be on).
- **`kot manga on|off` and `kot list --manga [STATUS|all]`**: the same switch from the terminal, and the saved
  manga list in aligned columns, by the same rule (chapters, volumes or both). The account mirror and list
  export leave manga out.
- **AniList manga (core and `kot`):** with AniList as your list service, `kot manga on` now works and syncs read
  your AniList manga list (`kot list --manga`, scores in your AniList score format). Chapters, volumes, status,
  score, rereads (AniList's Rereading) and removal go through the sync queue as on MyAnimeList; manga search and
  details read authors from AniList's staff (AniList has no magazine, so no serialization). Manga, manhwa,
  manhua, light novel and one-shot get MyAnimeList's labels. Same rate budget as anime (25 requests a minute);
  AniList data is still never exported. The window's switch on AniList comes with the manga UI.
- **Export and import your manga list:** with Manga on, Settings → Account has Export manga list… and Import
  manga list… beside the anime ones, and `kot export --manga FILE` writes the file. It is MyAnimeList's own manga
  export format (status, chapters and volumes read, score, times read, rereading, start and finish dates where
  Kotora has them). The import reads `.xml` or `.xml.gz`, compares first (new, different, same, entries it can't
  use) and works as the anime one: "Add new only" or "Add new and update", a higher chapter or volume count wins,
  a score fills an empty one, you choose when the statuses disagree, everything goes through the sync queue, and
  "Undo import" takes back what hasn't been sent. An anime file given to the manga import (or the other way round)
  is refused with a message saying which list it is. MyAnimeList only: AniList's API terms don't allow exporting
  from AniList, and importing a manga list into AniList comes with AniList manga.

### Fixed
- A failed manga sync is no longer silent: the manga pages and the sidebar say so (with Retry), and `kot list
  --manga` says why instead of "wasn't read yet".

## [0.2.1] - 2026-11-17

### Added
- **Windows (preview):** Kotora now runs on Windows 10 and 11 (x64). An installer for your account only (no
  admin rights, Start menu entry, `kot` on your PATH) and a zip. Players are found with no setup (mpv, mpv.net,
  MPC-HC, MPC-BE, VLC, PotPlayer), with one-click precise detection for mpv and MPC; notifications with Yes /
  Not now, start at login, the tray icon and the daily update check work as on Linux and macOS. Not signed yet:
  Windows SmartScreen asks once ("More info", "Run anyway").
- **Export your list:** Settings → Account → Export list… writes your anime list as a MyAnimeList XML file (the
  format MyAnimeList's own export uses, which MyAnimeList, AniList, Kitsu and Taiga import): status, episodes,
  score, rewatches, and start and finish dates where Kotora has them. Also `kot export FILE`. MyAnimeList lists
  only: AniList's API terms don't allow exporting list data from AniList, and Kotora says so instead.
- **Import a list:** Settings → Account → Import list… reads a MyAnimeList XML export (`.xml`, or `.xml.gz` as
  MyAnimeList ships it) and compares it with your list first: new, different, same, and entries it can't use.
  Pick "Add new only" or "Add new and update": a higher episode count wins, a score fills an empty one, and you
  choose when the statuses disagree (say Dropped against Completed). Everything goes through the sync queue at
  the service's pace, works offline, and "Undo import" takes back what hasn't been sent. Into AniList, the file's
  MyAnimeList ids are matched to AniList's anime first. Anime only for now; dates and notes are not imported.
- **Mirror your list to the other service:** signed in to both MyAnimeList and AniList, turn on Settings → Account
  → "Mirror edits to AniList" (or MyAnimeList). Kotora first compares the two lists by MyAnimeList id and shows
  what differs: only on one side, different values, the same, and anime it can't match. Then pick once: your main
  list wins (the other becomes a copy), merge (what is missing is added on each side, the one further along wins,
  and you choose when statuses disagree), or don't align (only new edits). From then on every change you make in
  Kotora, and every edit made on the main service's website, also goes to the other list through its own queue at
  its own pace; a failure on one side never holds up the other. Edits made on the mirror's website are never
  overwritten: they show in the Sync queue with "Use …'s" and "Leave both as they are". Scores are compared on
  MyAnimeList's 1 to 10 scale, so a MyAnimeList 7 and an AniList 73 count as the same and neither is rounded.
  Turning the mirror on never removes anything. Anime only; AniList entries without a MyAnimeList id are listed
  as not mirrored. The mirror is marked Beta in Settings for now.
- **Remove from list:** in an entry's right-click menu, on its details page and as `kot remove TITLE` (or its
  id). Kotora asks first, then the anime leaves your list at once and the removal waits in the sync queue like
  any change, so it works offline. Undo on the note that shows, or on the Sync queue page, puts it back until
  it is sent. If the entry changed on MyAnimeList or AniList after you removed it, the queue shows a conflict
  instead of deleting it: Remove anyway, or Keep it. With the mirror on, the anime is removed from the other
  service too (the confirm says so), through the mirror's queue with the same conflict check. An anime removed
  on the service's website is not removed from the mirror: only removals made in Kotora are mirrored, so a
  list that loads short can never delete anything on the other service.
- **The list filter searches online when nothing matches:** type a title that isn't on your list in the list's
  filter box (Ctrl F) and, after a short pause, MyAnimeList's or AniList's results show right under "No match in
  your list". Open one for its details, or add it with + Watching. It searches like the Search page does
  (3 letters or more, the same pause and rate limit, answers kept for a while), and not at all while something on
  your list matches.

### Changed
- **Scores in your AniList score format:** the list, the posters, the editor, the details page, the Score menu
  and `kot` show and set scores the way you score on AniList: 100 points (73), 10 points with a decimal (7.3),
  10 points, 5 stars or 3 smileys. Kotora reads the format from your AniList settings with each sync.
  `kot set TITLE --score 7.3` (or `73`, `***`, `:)`) takes it the same way. MyAnimeList stays 1 to 10.
- **The account chip shows the mirror:** with the mirror on, the sidebar's account chip reads "MyAnimeList ⇄
  AniList" (main ⇄ mirror), says where changes also go when you rest the pointer on it, and shows a dot while the
  mirror has changes waiting, in conflict or failed, or website edits to settle. The Sync queue badge counts the
  mirror's waiting changes the same way as the main list's.
- **kotora-bin is built by the release workflow:** a tag push builds the Linux tarball in an Arch container,
  adds it and its checksum to the release, and (after approval) updates kotora-bin on the AUR.

### Fixed
- **AniList scores keep their precision.** A 73 was saved as 7 and could go back to AniList as 70. Kotora now
  keeps every score on AniList's 100-point scale, and picking the score that already shows (4 stars for a 73)
  changes nothing. The saved list is upgraded on first start (a copy, `kotora.db.schema-11.bak`, is kept beside it).
- **A file's season beats an old pick.** A file naming season 3 no longer matches the season 1 you once picked for
  the same title: the list's season 3 wins, else a search hit for that season ("not on your list: add?"). Files
  without a season keep their picks.
- **The Linux tray's Play next works on KDE Plasma.** Episodes showed as bold section headers or greyed out, the
  long ones lost their episode number, and picking one didn't play it. Every episode is now a plain item that
  plays like the Up next button, long titles are shortened with "…" and keep " · Ep N", and Show and Quit sit
  above the Play next list.
- **Now Playing follows list changes.** When an entry is added or removed (a sync, the queue) while a file plays,
  Kotora recognises the file again instead of showing "In your list ()" with no status or cover.

## [0.2.0] - 2026-11-16

### Added
- **AniList:** sign in with AniList instead of MyAnimeList (anime lists). Pick it on the first screen or in
  Settings → Account ("Use AniList"); Kotora keeps one service at a time, and each keeps its own saved list,
  sign-in and waiting changes, so switching back loses nothing. Sign-in is one click: Kotora has its own
  AniList API client built in, so it opens AniList's page in your browser, you press Approve, and you are back
  in Kotora (no client secret, nothing to register). "Use my own client ID…" in Settings → Account is there
  for the advanced case, and a token pasted from AniList's page works when the browser can't reach Kotora. Rewatches
  are AniList's "Repeating"; scores go to AniList in your own score format. Detection, the sync queue, search,
  details, Airing and `kot` work the same with either service.
- **Exact airing times on AniList:** the next episode's time comes from AniList's schedule, so breaks and delays
  no longer throw off "not aired yet" and the Airing timetable.
- **Alt 1 to 5 for the list pages:** Settings → General → Keyboard → List page shortcuts picks Ctrl (⌘ on macOS)
  or Alt (Option) for the keys that open Watching, Plan to watch, Completed, On hold and Dropped (`[keys] pages`
  in config.toml). They work while you type in a search box too. Ctrl , Ctrl K and Ctrl F stay as they are.

### Changed
- **`kot config client-id ID`** saves your own client ID for the service in use (MyAnimeList or AniList), and
  **Use Kotora's built-in ID** in Settings → Account works for both.
- A saved list from a newer Kotora is refused with "Update Kotora to open it", and left as it is.

### Fixed
- **An anime added from Now Playing** ("Add as Watching") gets its episode count, type and cover right away.
  Before, it kept only its title ("3 / ?", a lettered tile, no type) until a later sync, or for good on AniList
  while the add waited to be sent. Anime already saved that way are filled in at the next sync.
- **Linux shortcuts:** Ctrl , opens Settings and Ctrl 1 to 5 open Watching, Plan to Watch, Completed, On Hold
  and Dropped. They only worked from the macOS menu bar before.
- **`kot --version`** shows branch and commit only for builds made in Kotora's own git repository. A build
  from a source tarball that sits inside another git checkout (an AUR helper's clone, say) no longer picks
  up that checkout's facts.

### Upgrading from 0.1
- Kotora 0.2 upgrades the saved list's database (`~/.local/share/kotora/kotora.db`) when it first starts, and
  keeps the 0.1 file beside it as `kotora.db.schema-9.bak`. Your list, sign-in, waiting changes and settings
  carry over; MyAnimeList stays your service until you choose AniList.
- Kotora 0.1 can't open the upgraded database: it says the saved list is from a newer Kotora, and changes
  nothing. To go back to 0.1, quit Kotora, put `kotora.db.schema-9.bak` back as `kotora.db` and delete
  `kotora.db-wal` and `kotora.db-shm` if they are there. Changes made in 0.2 are lost; the next 0.2 start
  upgrades the file again. Kotora 0.1 doesn't know the AniList sign-in and may drop it: sign in to AniList
  again after coming back to 0.2.

## [0.1.3] - 2026-11-16

### Changed
- **Signing in to MyAnimeList is one click.** Kotora now has its own MyAnimeList client ID built in (a client ID is
  public by design, as in Taiga), so there is nothing to register or paste. The setup wizard is three steps:
  Welcome, Sign in, Done. A client ID you saved earlier keeps being used.
- **Your own client ID is an advanced option:** Settings → Account → **Use my own client ID…**, and **Use Kotora's
  built-in ID** to go back. `kot config client-id ID` still works, and so does `KOTORA_MAL_CLIENT_ID` (it wins).

### Packaging
- `kotora-bin` is on the AUR (prebuilt x86_64 binary). AUR `kotora` 0.1.2-2 fixes `kot --version`.

## [0.1.2] - 2026-11-16

### Added
- **`kot` with no command is a dashboard:** a now-playing line (player, title, episode, time, and whether an
  update is offered), then your Watching list sorted by what you can watch next: a progress bar with
  watched/total, and NEXT ("▶ Ep 6 on disk", else the next air time like "Wed 18:00 (in 2d)", else
  "aired · not on disk"), then the sync queue and the last sync. It fits the terminal width, uses colour only
  on a terminal with `NO_COLOR` unset, and is plain text when piped. The data comes in one request, and an
  older Kotora that doesn't know it makes `kot` print the status lines instead.
- **`kot status --short`** prints the status as one line.
- **`kot schedule`** is another name for `kot airing`.

### Fixed
- **`kot` picks the right anime when a title matches several:** a title that is exactly what you typed (any
  title, any case) wins over titles that only contain it. `kot next`, `kot +1` and `kot set` prefer the anime you
  are watching or rewatching, and `kot next` then the ones whose next episode is in your library. If more than
  one is left, the "N anime match" error lists those first.

## [0.1.1] - 2026-11-16

### Added
- **Play next in the tray:** the menu bar (macOS) or tray (Linux) menu lists up to 8 Watching anime whose next
  episode is a file in your library folders ("Title · Ep N", most recently updated first); picking one opens it
  as ▶ Play does. Opening the menu (macOS) or the window looks for new files.

### Changed
- Library folders are no longer walked every 5 minutes: Kotora looks again when the window is shown or the tray
  menu opens (at most once a minute), on Rescan, and when the folders change. At rest nothing runs.

### Fixed
- Now Playing: when the playing file matches no anime, the "Offer at N%" label and the threshold tick are
  hidden (no update is ever offered), and the button says "Pick anime…" instead of "Wrong anime…".

## [0.1.0] - 2026-11-16

First release. MyAnimeList only. Linux (built from source or the AUR) and macOS (Apple silicon, a DMG).

### Added
- **Your list:** Watching, Plan to watch, Completed, On hold and Dropped, as rows, a grid or large cards, with
  covers, progress, score, airing information, status chips, a Sort menu and search. A right-click menu on rows,
  posters and Home cards. The list is kept in a local SQLite database and works offline.
- **Home:** shelves for what you're watching, what's next and what's airing.
- **MyAnimeList sign-in** with PKCE (no client secret). A setup wizard asks which service you use and walks you
  through getting a Client ID; `kot config client-id ID` does the same. Tokens are stored in a private file. The
  account card opens a Profile page; the sidebar shows your MyAnimeList picture.
- **Edits and the sync queue:** change progress (`+1`), score and status, and rewatch a completed anime. Changes
  wait in a visible queue, are sent in order, retried with backoff, and conflicts with changes made elsewhere are
  shown.
- **Detection:** mpv (IPC socket), MPRIS players on Linux (VLC, mpc-qt, Celluloid, Haruna, SMPlayer, …), IINA and
  VLC's web interface on macOS, and a zero-setup process source that finds a player by its open video file. A
  watched-time rule decides when an episode counts. Now Playing shows the live position. When mpv or IINA is seen
  by its open file only, Kotora offers once to turn on precise detection (one line in `mpv.conf`, only when you
  click).
- **Recognition:** file names are parsed with anitomy-ng and matched to your list, with season handling and
  anime-relations (a "Season 2 - 15" release goes to the right MyAnimeList entry).
- **Updates ask first:** an offer to record the episode (Update, Not now, Wrong anime…). Updating automatically is
  a switch, off by default, with Undo. It still asks when the episode would skip one or hasn't aired yet.
- **Not aired yet:** `+1`, the editor and `kot +1` / `kot set` ask "Mark anyway?" for an episode past the airing
  estimate (`kot`: `--anyway`). Only the total episode count is a hard limit.
- **Add as Watching at episode N:** one button on Now Playing's "Not on your list" card adds the anime and records
  the playing episode in one change.
- **Search and add:** search your list and MyAnimeList together (Ctrl K), open a MyAnimeList id or link, add with
  one click, and a details page with synopsis, score, studios and your own entry.
- **Airing:** this week and this season from MyAnimeList, and a season picker for the seasons before and after.
- **Library folders and Play next episode:** Settings → Library takes the folders where your anime are. Kotora
  finds the episodes and links them to your list. ▶ on Watching rows, Play on the details page, "Up next" on Now
  Playing, a middle-click or P, and `kot next TITLE` open the next episode in the player you choose.
- **Notifications** with a switch for each: Episode detected (off by default), Update offered (Update, Not now),
  Episode recorded (Undo), Started airing and New episode. A new notification about an episode replaces the last.
- **Tray and start at login:** closing the window keeps Kotora running in the tray (Linux) or the menu bar (macOS),
  so detection keeps working. Settings can start Kotora at login and in the background.
- **macOS:** a menu bar (About, Settings… ⌘,, a Go menu), and the system's parts follow Kotora's theme.
- **Themes:** dark, light, or follow the system. A title language setting (romaji, English or Japanese).
- **Settings** in sections (General, Appearance, Account, Detection, Library, Notifications, About) with a search
  field.
- **Update check:** once a day Kotora asks kotora.moe whether there is a newer version and shows a banner. It never
  downloads anything by itself, and Settings → About Kotora → Check for updates turns it off. See Privacy in the
  README for what it sends.
- **小虎** beside the logo and in the window title, and a short claw-mark startup animation (Settings → Appearance →
  Startup animation; a fade with Reduce Motion).
- **`kot`:** `status`, `list`, `+1`, `set`, `add`, `next`, `queue`, `sync`, `now`, `confirm`, `dismiss`,
  `config`, `show`, `quit`.
- A single instance per user; `kot` and the app share one socket.
- `install.sh` for Linux, a desktop entry and icons, an AUR package, and a macOS DMG (ad-hoc signed, not
  notarized).

### Not in this release
- AniList, several accounts, a corrections UI, manga, and an AppImage (planned for later versions).
