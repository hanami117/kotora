Kotora for Windows (preview)
============================

Kotora is new on Windows: this is a preview. It was built and tested by machine (GitHub
Actions) and tried on few real PCs so far. Expect rough edges, and please report them.
It is not signed yet.

Needs Windows 10 (version 1809 or newer) or Windows 11, 64-bit (x64).


Install
-------
With the installer (Kotora-windows-x64-setup.exe): run it. Windows will say "Windows
protected your PC" (SmartScreen), because the program is not signed: click "More info",
then "Run anyway". Setup installs Kotora for you only, never asks for administrator rights,
adds a Start menu entry and puts kot (the command line) on your PATH. A new version is
installed the same way, over the old one. Uninstall in Windows Settings, Apps, Installed
apps: your settings, sign-in and saved list are kept unless you say otherwise.

From the zip: unpack it into a folder of its own, for example C:\Kotora (not inside Program
Files), and double-click kotora.exe. If SmartScreen has no "Run anyway": right-click
kotora.exe, Properties, tick "Unblock", OK, and start it again.

Then choose MyAnimeList or AniList and sign in in your browser.

Closing the window keeps Kotora running in the notification area (the tray, next to the
clock; it may be behind the ^ arrow). Click the icon to open the window again; right-click
it for Quit. To start Kotora with Windows: Settings, General, "Start Kotora when I log in".

kot.exe is the command line. In a terminal (PowerShell):
    kot status        is Kotora running, who is signed in
    kot list          your list
    kot now           what Kotora sees playing
    kot quit          quits Kotora
(From the zip, kot is not on your PATH: run .\kot in Kotora's folder.)


What should work
----------------
- The window, your list, covers, search, details, the Airing page, Settings.
- Sign-in to MyAnimeList, sync, edits (progress, score, status), the sync queue.
- Airing times in your own time zone.
- Only one Kotora at a time: starting it again shows the window of the running one.
- kot.exe (see above).
- Library folders (Settings, Library): add a folder with Choose... or type its path
  (D:\Anime), then "Play next episode" opens the file in your default player, mpv or VLC.
- Detection with no setup: open a video in mpv, mpv.net, MPC-HC, MPC-BE, VLC or PotPlayer
  and Kotora shows it on Now Playing within about 5 seconds. It reads the player's window
  title and the command line it was started with. It does not know how far in you are
  (PotPlayer excepted), so it offers the update once the file has been open for 15 minutes
  (Settings, Detection, "Count a file after it stays open"), and it always asks first.
- Precise detection (the real position and pause), one click each in Settings, Detection:
    mpv, mpv.net     "Turn on" adds one line to the player's mpv.conf
    MPC-HC, MPC-BE   "Turn on" switches on the player's web interface
    VLC              type the password of its web interface
    PotPlayer        nothing to do


Precise detection
-----------------
mpv and mpv.net: Settings, Detection, under mpv: Turn on. Kotora adds the line
    input-ipc-server=\\.\pipe\mpvsocket
to the mpv.conf of the player that is running: %APPDATA%\mpv.net\mpv.conf for mpv.net,
%APPDATA%\mpv\mpv.conf for mpv, or portable_config\mpv.conf beside a portable player
(Scoop's mpv too). A missing file is made. An existing file is first copied to
mpv.conf.kotora-backup and then only added to; a line that is already there is left alone.
Kotora shows what it wrote. Close the player and open it again once.

MPC-HC and MPC-BE: close the player, then Settings, Detection, under MPC-HC / MPC-BE:
Turn on. Kotora sets the player's own "web interface" setting (on, this computer only).
Or by hand in the player: View, Options, Player, Web Interface, tick "Listen on port".
A portable copy that keeps its settings in an .ini file needs the by-hand way.

VLC: Tools, Preferences, Show settings "All", Interface, Main interfaces: tick "Web".
Then Main interfaces, Lua: set a password under "Lua HTTP". Restart VLC. Then type the same
password in Kotora: Settings, Detection, VLC web interface, Save.


Notifications
-------------
Kotora shows Windows notifications (toasts): "Update ... to episode 6?" with Yes and Not now,
"Recorded" with Undo, and news about new episodes. Switch each kind on or off in Settings,
Notifications.
- The buttons work while Kotora runs (it runs in the tray). A notification left over after
  Kotora quit does nothing when clicked.
- While a video plays full screen, Windows may hold notifications back and only list them
  in the notification centre (Windows Settings, System, Notifications, "Do not disturb").
  The question is also in Kotora's window, and kot confirm answers it.


Updates
-------
Once a day Kotora asks kotora.moe whether a new version is out and shows a banner with a
Download button for the new installer. Nothing is downloaded by itself. Switch it off in
Settings, General.


What does not work yet
----------------------
- Detection without setup has limits: a file with a title of its own inside (not its file
  name) may show under that title; a player started with a custom window title is not
  seen; other players than the six above are not seen at all.
- A notification clicked after Kotora quit does nothing (see above).
- The programs are not signed: SmartScreen warns at every new version.


Where Kotora keeps its files
----------------------------
    Settings        %APPDATA%\kotora\config.toml
    List, sign-in   %LOCALAPPDATA%\kotora\
    Covers          %LOCALAPPDATA%\kotora\Cache\
    Log             %LOCALAPPDATA%\kotora\Logs\kotora.log

(Paste a path into the address bar of Explorer to open it.)
To remove Kotora completely: uninstall it and answer Yes when it asks about your settings
and list (from the zip: quit it, delete the folder you unpacked and the two kotora folders
above).


How to report a problem
-----------------------
Please send:
1. What you did, what you expected and what happened.
2. A screenshot of the window (Win+Shift+S). Your list is private: blur or crop it as you
   like.
3. The log file: %LOCALAPPDATA%\kotora\Logs\kotora.log
   Do NOT send the other files in %LOCALAPPDATA%\kotora: tokens.json is your MyAnimeList
   sign-in.
4. Your Windows version (Win+R, winver) and, for a blank or broken window, your graphics
   card.
5. The version: Settings, About (or   kot --version   in PowerShell).

For a window that stays blank or crashes at start, run this in PowerShell in Kotora's
folder and send the file gpu.txt it makes:
    $env:RUNE_GPU_LOG = "1"; .\kotora.exe 2>&1 | Out-File gpu.txt

Report at https://github.com/hanami117/kotora/issues


Kotora is free software under the GPL, version 3 or later. Source: https://github.com/hanami117/kotora
