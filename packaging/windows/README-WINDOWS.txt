Kotora for Windows: test build
==============================

This is an early test build. It is not signed, has no installer and does not update itself.
It was built and tested by machine only (GitHub Actions); you are among the first people to
run it on a real Windows desktop. Expect rough edges, and please report them.

Needs Windows 10 or 11, 64-bit (x64).


How to run
----------
1. Unpack the zip into a folder of its own, for example C:\Kotora (not inside Program Files).
2. Double-click kotora.exe.
3. Windows will say "Windows protected your PC" (SmartScreen), because the program is not
   signed. Click "More info", then "Run anyway". If there is no "Run anyway": right-click
   kotora.exe, Properties, tick "Unblock", OK, and start it again.
4. Kotora asks for a MyAnimeList Client ID and walks you through it, then you sign in to
   MyAnimeList in your browser.

Closing the window keeps Kotora running in the notification area (the tray, next to the
clock; it may be behind the ^ arrow). Click the icon to open the window again; right-click
it for Quit.

kot.exe is the command line. In a terminal (PowerShell) in the same folder:
    .\kot status        is Kotora running, who is signed in
    .\kot list          your list
    .\kot now           what Kotora sees playing
    .\kot quit          quits Kotora


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


What does not work yet
----------------------
- Detection without setup has limits: a file with a title of its own inside (not its file
  name) may show under that title; a player started with a custom window title is not
  seen; other players than the six above are not seen at all.
- Desktop notifications (toasts). The banner inside the window still shows.
- Start at login (the switch in Settings is greyed out).
- The update check (this build never looks for a newer version).
- No program icon on kotora.exe, no installer, no Start menu entry.
- kot has no time limit when the app hangs: press Ctrl+C.


Where Kotora keeps its files
----------------------------
    Settings        %APPDATA%\kotora\config.toml
    List, sign-in   %LOCALAPPDATA%\kotora\
    Covers          %LOCALAPPDATA%\kotora\Cache\
    Log             %LOCALAPPDATA%\kotora\Logs\kotora.log

(Paste a path into the address bar of Explorer to open it.)
To remove Kotora completely: quit it, delete the folder you unpacked and the two kotora
folders above.


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
5. The version: run   .\kotora.exe --version | more   in PowerShell, or see Settings, About.

For a window that stays blank or crashes at start, run this in PowerShell in Kotora's
folder and send the file gpu.txt it makes:
    $env:RUNE_GPU_LOG = "1"; .\kotora.exe 2>&1 | Out-File gpu.txt

Report at https://github.com/hanami117/kotora/issues or straight to the person who gave
you this build.


Kotora is free software under the GPL, version 3 or later. Source: https://github.com/hanami117/kotora
