//! Windows: the players Kotora finds with no setup, and what they show.
//!
//! Windows has no public way to list the files a process has open (see `procfs.rs` for Linux
//! and macOS), so this source reads two other things, every [`crate::procfs::INTERVAL`]:
//!
//! 1. the **command line** of a known player (`mpv.exe "D:\Anime\Show - 03.mkv"`): a double
//!    click or "Open with" puts the file's full path there. It is read once per process;
//! 2. the **window title** (`Show - 03.mkv - mpv`): it follows the playlist and files opened
//!    from inside the player, but mostly gives the file name without its folder.
//!
//! The title says what plays now, so it wins: the command line only adds the folder when it
//! names the same file. A player whose window shows nothing ("mpv", "VLC media player") has
//! no file, whatever its command line says. Only a player with no window at all is taken at
//! its command line.
//!
//! No position, duration or pause comes from either, so the watch hub counts the time the file
//! stays shown (`watch.rs`, `process_minutes`). PotPlayer is the exception: it answers three
//! window messages with its position, length and play state, so it is followed like a player
//! with a real interface.
//!
//! Everything here is plain logic over the [`Sys`] trait, so it is tested on every system with
//! fake processes and titles. The real calls are in `winproc.rs`.

use std::collections::{HashMap, HashSet};

use crate::procfs::{Kind, VIDEO_EXTS, key};
use crate::run::Input;
use crate::{Led, PlayState, Playing, SourceStatus, Switch, Via};

/// Which program a process is: it decides how its window title reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Mpv,
    MpvNet,
    MpcHc,
    MpcBe,
    PotPlayer,
    Vlc,
    MpcQt,
}

impl Family {
    /// The words a player puts after " - " in its title, and shows alone when nothing plays.
    fn markers(self) -> &'static [&'static str] {
        match self {
            Family::Mpv => &["mpv"],
            Family::MpvNet => &["mpv.net", "mpv"],
            Family::MpcHc => &["mpc-hc", "media player classic"],
            Family::MpcBe => &["mpc-be"],
            Family::PotPlayer => &["potplayer"],
            Family::Vlc => &["vlc media player", "vlc"],
            Family::MpcQt => &["media player classic qute theater", "mpc-qt"],
        }
    }

    /// The text is the player's own name, with or without a version ("MPC-BE x64 1.8.3").
    fn is_marker(self, text: &str) -> bool {
        let t = text.trim().to_lowercase();
        self.markers()
            .iter()
            .any(|m| t == *m || t.strip_prefix(m).is_some_and(|rest| rest.starts_with(' ')))
    }

    /// The mpv family reads an `mpv.conf` (precise mode); `true`: mpv.net.
    pub fn mpv_net(self) -> Option<bool> {
        match self {
            Family::Mpv => Some(false),
            Family::MpvNet => Some(true),
            _ => None,
        }
    }

    pub fn is_mpc(self) -> bool {
        matches!(self, Family::MpcHc | Family::MpcBe)
    }
}

/// The player an executable's name stands for: `mpv.exe`, `mpvnet.exe`, `mpc-hc64.exe`, …
pub fn kind_of_exe(name: &str) -> Option<(Kind, Family)> {
    let name = name.trim().to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    let k = |player, switch, family| Some((Kind { player, switch }, family));
    match name {
        "mpv" => k("mpv", Switch::Mpv, Family::Mpv),
        "mpvnet" => k("mpv.net", Switch::Mpv, Family::MpvNet),
        "mpc-hc64" | "mpc-hc" => k("MPC-HC", Switch::Mpc, Family::MpcHc),
        "mpc-be64" | "mpc-be" => k("MPC-BE", Switch::Mpc, Family::MpcBe),
        "potplayermini64" | "potplayermini" | "potplayer64" | "potplayer" => {
            k("PotPlayer", Switch::PotPlayer, Family::PotPlayer)
        }
        "vlc" => k("VLC", Switch::Vlc, Family::Vlc),
        "mpc-qt" => k("mpc-qt", Switch::MpcQt, Family::MpcQt),
        _ => None,
    }
}

/// Whether a file name or path ends in a video extension.
pub fn has_video_ext(path: &str) -> bool {
    match crate::file_name_on(path, true).rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            VIDEO_EXTS.iter().any(|v| ext.eq_ignore_ascii_case(v))
        }
        _ => false,
    }
}

/// Splits a Windows command line the way the C runtime does (`CommandLineToArgvW`): the
/// program name first, then the arguments, with quotes and backslashes before quotes undone.
pub fn split_command_line(line: &str) -> Vec<String> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    // The program name: quoted up to the next quote, else up to the first blank. No escapes.
    let mut prog = String::new();
    if c.first() == Some(&'"') {
        i = 1;
        while i < c.len() && c[i] != '"' {
            prog.push(c[i]);
            i += 1;
        }
        i += 1;
    } else {
        while i < c.len() && !c[i].is_whitespace() {
            prog.push(c[i]);
            i += 1;
        }
    }
    out.push(prog);
    loop {
        while i < c.len() && matches!(c[i], ' ' | '\t') {
            i += 1;
        }
        if i >= c.len() {
            break;
        }
        let mut arg = String::new();
        let mut quoted = false;
        while i < c.len() {
            match c[i] {
                '\\' => {
                    let start = i;
                    while i < c.len() && c[i] == '\\' {
                        i += 1;
                    }
                    let n = i - start;
                    if i < c.len() && c[i] == '"' {
                        // 2n backslashes and a quote: n backslashes, and the quote opens or
                        // closes. 2n+1: n backslashes and a quote character.
                        arg.extend(std::iter::repeat_n('\\', n / 2));
                        if n % 2 == 1 {
                            arg.push('"');
                            i += 1;
                        }
                    } else {
                        arg.extend(std::iter::repeat_n('\\', n));
                    }
                }
                '"' => {
                    if quoted && c.get(i + 1) == Some(&'"') {
                        arg.push('"');
                        i += 2;
                    } else {
                        quoted = !quoted;
                        i += 1;
                    }
                }
                ' ' | '\t' if !quoted => break,
                ch => {
                    arg.push(ch);
                    i += 1;
                }
            }
        }
        out.push(arg);
    }
    out
}

/// The video files a command line names, in order: arguments that end in a video extension
/// and aren't options. `file:///C:/…` becomes a path; other URLs are skipped.
pub fn media_args(command_line: &str) -> Vec<String> {
    split_command_line(command_line)
        .into_iter()
        .skip(1)
        .filter_map(|a| {
            if a.starts_with('-') {
                return None;
            }
            if a.contains("://") {
                return crate::windows_file_url(&a).filter(|p| has_video_ext(p));
            }
            has_video_ext(&a).then_some(a)
        })
        .collect()
}

/// One visible top-level window of a player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Win {
    /// The window handle, for [`Sys::potplayer`].
    pub hwnd: isize,
    pub title: String,
    pub class: String,
}

/// What a window title says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// A file name, a full path, or the title the file carries.
    Media(String),
    /// The player's main window, with nothing playing.
    Idle,
}

/// MPC-HC's main window class (`MPC_WND_CLASS_NAME` in its source).
const MPC_HC_CLASS: &str = "MediaPlayerClassicW";

/// Reads one window title. `None`: not a window that says what plays (a dialog, a tool window).
///
/// - `Show - 03.mkv` and `D:\Anime\Show - 03.mkv` (MPC-HC, mpv.net): the whole title is the file;
/// - `Show - 03.mkv - VLC media player`, `… - mpv`, `… - MPC-BE x64 1.8.3`, `… - PotPlayer`, and
///   any translated ending: the part before the last " - " that leaves a video file name;
/// - `Some Title - mpv`: a file that carries a title, kept as it is;
/// - `mpv`, `No file - mpv`, `VLC media player`, `MPC-BE x64 1.8.3`: nothing plays.
pub fn read_title(family: Family, win: &Win) -> Option<Shown> {
    let t = win.title.trim();
    if t.is_empty() {
        return None;
    }
    if has_video_ext(t) {
        return Some(Shown::Media(t.to_string()));
    }
    for (i, _) in t.rmatch_indices(" - ") {
        let name = t[..i].trim_end();
        if has_video_ext(name) {
            return Some(Shown::Media(name.to_string()));
        }
    }
    if family.is_marker(t) {
        return Some(Shown::Idle);
    }
    if let Some(i) = t.rfind(" - ")
        && family.is_marker(&t[i + 3..])
    {
        let name = t[..i].trim();
        return Some(if name.is_empty() || name.eq_ignore_ascii_case("no file") {
            Shown::Idle
        } else {
            Shown::Media(name.to_string())
        });
    }
    // MPC-HC shows the bare name, or the title the file carries, with nothing after it.
    if family == Family::MpcHc && win.class == MPC_HC_CLASS {
        return Some(Shown::Media(t.to_string()));
    }
    None
}

/// The file a player shows, and the window that says so (for PotPlayer's messages).
///
/// `media`: the video files of its command line. `wins`: its visible windows.
pub fn shown_file(
    family: Family,
    media: &[String],
    wins: &[Win],
) -> (Option<String>, Option<isize>) {
    let read: Vec<(Shown, isize)> = wins
        .iter()
        .filter_map(|w| read_title(family, w).map(|s| (s, w.hwnd)))
        .collect();
    let medias = || {
        read.iter().filter_map(|(s, h)| {
            if let Shown::Media(m) = s {
                Some((m, *h))
            } else {
                None
            }
        })
    };
    // A title that is a file name over one that is only a title.
    if let Some((name, hwnd)) = medias()
        .find(|(m, _)| has_video_ext(m))
        .or_else(|| medias().next())
    {
        return (Some(with_folder(name, media)), Some(hwnd));
    }
    if wins.iter().any(|w| !w.title.trim().is_empty()) {
        // It has a window and the window shows no file: nothing plays (the command line may
        // still name a file that ended).
        return (None, None);
    }
    (media.first().cloned(), None)
}

/// The full path from the command line when it names the file the title shows.
fn with_folder(name: &str, media: &[String]) -> String {
    if crate::is_absolute_on(name, true) {
        return name.to_string();
    }
    let lower = name.to_lowercase();
    let same = |m: &&String| {
        let file = crate::file_name_on(m, true).to_lowercase();
        // The title may be the name without its extension.
        file == lower || file.rsplit_once('.').is_some_and(|(stem, _)| stem == lower)
    };
    media
        .iter()
        .find(same)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// A process the scan could open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    /// The full path of the executable.
    pub exe: String,
    pub command_line: String,
}

/// PotPlayer's answers, as it gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pot {
    /// 2: playing, 1: paused, anything else: stopped.
    pub status: i64,
    pub position_ms: i64,
    pub duration_ms: i64,
}

/// A position a player gave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
}

impl Pot {
    /// `None` when the answers make no sense (another program's window, nothing loaded).
    pub fn pos(self) -> Option<Pos> {
        let playing = match self.status {
            2 => true,
            1 => false,
            _ => return None,
        };
        if self.duration_ms <= 0
            || self.position_ms < 0
            || self.position_ms > self.duration_ms + 5_000
        {
            return None;
        }
        Some(Pos {
            position: self.position_ms as f64 / 1000.0,
            duration: self.duration_ms as f64 / 1000.0,
            playing,
        })
    }
}

/// The system, as this source needs it. Windows: `winproc::System`; tests: a fake.
pub trait Sys {
    /// Every process now: (id, executable name).
    fn processes(&mut self) -> Vec<(u32, String)>;
    /// The executable's path and the command line. `None`: the process can't be opened
    /// (another user's, or gone).
    fn info(&mut self, pid: u32) -> Option<ProcInfo>;
    /// The visible, titled top-level windows of these processes.
    fn windows(&mut self, pids: &HashSet<u32>) -> HashMap<u32, Vec<Win>>;
    /// PotPlayer's state, asked from its window.
    fn potplayer(&mut self, hwnd: isize) -> Option<Pot>;
}

/// One player process and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub pid: u32,
    pub kind: Kind,
    pub family: Family,
    pub exe: String,
    pub file: Option<String>,
    pub pos: Option<Pos>,
}

struct Known {
    kind: Kind,
    family: Family,
    exe: String,
    media: Vec<String>,
}

/// Finds the players and what they show; remembers the processes between scans, so a process
/// is opened once and one that isn't a player costs a name compare.
pub struct Scanner<S: Sys> {
    sys: S,
    /// pid → (executable name, the player it is).
    known: HashMap<u32, (String, Option<Known>)>,
}

impl<S: Sys> Scanner<S> {
    pub fn new(sys: S) -> Scanner<S> {
        Scanner {
            sys,
            known: HashMap::new(),
        }
    }

    pub fn scan(&mut self) -> Vec<Seen> {
        let procs = self.sys.processes();
        let live: HashSet<u32> = procs.iter().map(|(p, _)| *p).collect();
        self.known.retain(|p, _| live.contains(p));
        for (pid, name) in procs {
            if self.known.get(&pid).is_some_and(|(n, _)| *n == name) {
                continue;
            }
            let known = match kind_of_exe(&name) {
                Some((kind, family)) => match self.sys.info(pid) {
                    Some(info) => Some(Known {
                        kind,
                        family,
                        media: media_args(&info.command_line),
                        exe: info.exe,
                    }),
                    // Not ours to open, or not ready: asked again at the next scan.
                    None => continue,
                },
                None => None,
            };
            self.known.insert(pid, (name, known));
        }
        let players: HashSet<u32> = self
            .known
            .iter()
            .filter(|(_, (_, k))| k.is_some())
            .map(|(p, _)| *p)
            .collect();
        if players.is_empty() {
            return Vec::new();
        }
        let wins = self.sys.windows(&players);
        let mut out = Vec::new();
        for pid in players {
            let Some((_, Some(k))) = self.known.get(&pid) else {
                continue;
            };
            let (file, hwnd) = shown_file(
                k.family,
                &k.media,
                wins.get(&pid).map_or(&[], Vec::as_slice),
            );
            let (kind, family, exe) = (k.kind, k.family, k.exe.clone());
            let pos = match (family, &file, hwnd) {
                (Family::PotPlayer, Some(_), Some(h)) => self.sys.potplayer(h).and_then(Pot::pos),
                _ => None,
            };
            out.push(Seen {
                pid,
                kind,
                family,
                exe,
                file,
                pos,
            });
        }
        out.sort_by_key(|s| s.pid);
        out
    }

    /// Forgets everything (the source was switched off).
    pub fn reset(&mut self) {
        self.known.clear();
    }
}

/// How far a reported position may be from the one worked out before a new snapshot is sent.
const DRIFT: f64 = 2.5;

/// The file as last sent for a process, and the position with its time.
type Sent = (Option<String>, Option<(Pos, f64)>);

/// Turns scans into hub inputs, sending changes only.
#[derive(Default)]
pub struct Reporter {
    sent: HashMap<u32, Sent>,
    status: HashMap<Switch, (String, Led)>,
}

impl Reporter {
    pub fn apply(&mut self, seen: &[Seen], now: f64) -> Vec<Input> {
        let mut out = Vec::new();
        let pids: HashSet<u32> = seen.iter().map(|s| s.pid).collect();
        let mut gone: Vec<u32> = self
            .sent
            .keys()
            .copied()
            .filter(|p| !pids.contains(p))
            .collect();
        gone.sort_unstable();
        for pid in gone {
            if let Some((Some(_), _)) = self.sent.remove(&pid) {
                out.push(Input::Gone(key(pid)));
            }
        }
        for s in seen {
            let before = self.sent.get(&s.pid);
            let file_changed = before.is_none_or(|(f, _)| *f != s.file);
            let moved = match (before.and_then(|(_, p)| *p), s.pos) {
                (None, None) => false,
                (Some((old, at)), Some(new)) => {
                    let expected = old.position + if old.playing { now - at } else { 0.0 };
                    old.playing != new.playing
                        || (old.duration - new.duration).abs() > 1.0
                        || (new.position - expected).abs() > DRIFT
                }
                _ => true,
            };
            if !file_changed && !moved {
                continue;
            }
            match &s.file {
                Some(path) => out.push(Input::Update(snapshot(s, path, now))),
                None if before.is_some_and(|(f, _)| f.is_some()) => {
                    out.push(Input::Gone(key(s.pid)))
                }
                None => {}
            }
            self.sent
                .insert(s.pid, (s.file.clone(), s.pos.map(|p| (p, now))));
        }
        // One state per switch: playing if any of its processes shows a file.
        let mut now_status: HashMap<Switch, (String, Led)> = HashMap::new();
        for s in seen {
            let (words, led) = if s.file.is_some() {
                ("video file open", Led::Playing)
            } else {
                ("running, no video open", Led::Idle)
            };
            // "mpv.net: video file open" under the mpv switch; plain words when the names match.
            let line = if s.kind.player == s.kind.switch.label() {
                words.to_string()
            } else {
                format!("{}: {words}", s.kind.player)
            };
            let e = now_status
                .entry(s.kind.switch)
                .or_insert_with(|| (line.clone(), led));
            if led == Led::Playing && e.1 != Led::Playing {
                *e = (line, led);
            }
        }
        let mut switches: Vec<Switch> = self
            .status
            .keys()
            .chain(now_status.keys())
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        switches.sort_by_key(|s| Switch::ALL.iter().position(|a| a == s));
        for sw in switches {
            let st = now_status
                .remove(&sw)
                .unwrap_or_else(|| ("not running".to_string(), Led::Off));
            if self.status.get(&sw) != Some(&st) {
                out.push(Input::Status(SourceStatus {
                    switch: sw,
                    via: Via::Process,
                    line: st.0.clone(),
                    led: st.1,
                }));
                self.status.insert(sw, st);
            }
        }
        out
    }
}

/// What this source knows: the player and the file; a position only from PotPlayer.
pub fn snapshot(s: &Seen, path: &str, now: f64) -> Playing {
    Playing {
        key: key(s.pid),
        player: s.kind.player.to_string(),
        switch: s.kind.switch,
        via: Via::Process,
        path: Some(path.to_string()),
        title: None,
        position: s.pos.map(|p| p.position),
        duration: s.pos.map(|p| p.duration),
        state: if s.pos.is_none_or(|p| p.playing) {
            PlayState::Playing
        } else {
            PlayState::Paused
        },
        rate: 1.0,
        at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(title: &str) -> Win {
        Win {
            hwnd: 1,
            title: title.into(),
            class: "x".into(),
        }
    }

    fn media(family: Family, title: &str) -> Option<String> {
        match read_title(family, &win(title)) {
            Some(Shown::Media(m)) => Some(m),
            _ => None,
        }
    }

    #[test]
    fn executables_map_to_players() {
        let of = |n: &str| kind_of_exe(n).map(|(k, f)| (k.player, k.switch, f));
        assert_eq!(of("mpv.exe"), Some(("mpv", Switch::Mpv, Family::Mpv)));
        assert_eq!(
            of("mpvnet.exe"),
            Some(("mpv.net", Switch::Mpv, Family::MpvNet))
        );
        assert_eq!(
            of("MPC-HC64.EXE"),
            Some(("MPC-HC", Switch::Mpc, Family::MpcHc))
        );
        assert_eq!(
            of("mpc-be64.exe"),
            Some(("MPC-BE", Switch::Mpc, Family::MpcBe))
        );
        assert_eq!(
            of("PotPlayerMini64.exe"),
            Some(("PotPlayer", Switch::PotPlayer, Family::PotPlayer))
        );
        assert_eq!(of("vlc.exe"), Some(("VLC", Switch::Vlc, Family::Vlc)));
        assert_eq!(
            of("mpc-qt.exe"),
            Some(("mpc-qt", Switch::MpcQt, Family::MpcQt))
        );
        assert_eq!(of("mpv"), Some(("mpv", Switch::Mpv, Family::Mpv)));
        for other in [
            "explorer.exe",
            "mpv.com",
            "vlc-cache-gen.exe",
            "mpvpaper.exe",
            "",
        ] {
            assert!(kind_of_exe(other).is_none(), "{other}");
        }
    }

    #[test]
    fn command_lines_split_like_windows() {
        let s = |l: &str| split_command_line(l);
        assert_eq!(
            s(r#""C:\Program Files\mpv\mpv.exe" "D:\Anime\Show - 03.mkv""#),
            [r"C:\Program Files\mpv\mpv.exe", r"D:\Anime\Show - 03.mkv"]
        );
        assert_eq!(
            s(r"mpv.exe D:\a.mkv   --fs"),
            ["mpv.exe", r"D:\a.mkv", "--fs"]
        );
        // A backslash before the closing quote of a folder: two make one, then the quote closes.
        assert_eq!(s(r#"x.exe "D:\Anime\\" b"#), ["x.exe", r"D:\Anime\", "b"]);
        assert_eq!(s(r#"x.exe "say \"hi\"" c"#), ["x.exe", r#"say "hi""#, "c"]);
        assert_eq!(s(r#"x.exe "a""b""#), ["x.exe", r#"a"b"#]);
        assert_eq!(s(r#"x.exe a"b c"d"#), ["x.exe", "ab cd"]);
        assert_eq!(s(r#"x.exe """#), ["x.exe", ""]);
        assert_eq!(s("x.exe"), ["x.exe"]);
        assert_eq!(s(""), [""]);
    }

    #[test]
    fn the_command_line_names_the_video_files() {
        assert_eq!(
            media_args(
                r#""C:\mpv\mpv.exe" --fs --sub-file=D:\s.mkv "D:\Anime\Show - 03.mkv" D:\x.ass"#
            ),
            [r"D:\Anime\Show - 03.mkv"]
        );
        assert_eq!(
            media_args(r#"mpc-hc64.exe "D:\A\Show - 01.mkv" "D:\A\Show - 02.mkv" /play /close"#),
            [r"D:\A\Show - 01.mkv", r"D:\A\Show - 02.mkv"]
        );
        assert_eq!(
            media_args(r#"vlc.exe --started-from-file "file:///D:/Anime/Show%20-%2004.mkv""#),
            [r"D:\Anime\Show - 04.mkv"]
        );
        assert!(media_args(r#"mpv.exe https://example.invalid/v.mkv"#).is_empty());
        assert!(media_args(r#""C:\mpv\mpv.exe""#).is_empty());
        // The program's own path never counts, whatever it is called.
        assert!(media_args(r#""C:\odd.mkv\mpv.exe""#).is_empty());
    }

    #[test]
    fn titles_say_what_plays() {
        let f = "[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv";
        // The ending each player uses.
        assert_eq!(
            media(Family::Mpv, &format!("{f} - mpv")).as_deref(),
            Some(f)
        );
        assert_eq!(
            media(Family::Vlc, &format!("{f} - VLC media player")).as_deref(),
            Some(f)
        );
        assert_eq!(
            media(Family::MpcBe, &format!("{f} - MPC-BE x64 1.8.3")).as_deref(),
            Some(f)
        );
        assert_eq!(
            media(Family::PotPlayer, &format!("{f} - PotPlayer")).as_deref(),
            Some(f)
        );
        assert_eq!(
            media(Family::MpvNet, &format!("{f} - mpv.net 7.1.1.0")).as_deref(),
            Some(f)
        );
        // The bare name and the full path (MPC-HC, mpv.net).
        assert_eq!(media(Family::MpcHc, f).as_deref(), Some(f));
        assert_eq!(media(Family::MpvNet, f).as_deref(), Some(f));
        assert_eq!(
            media(Family::MpcHc, r"D:\Anime - TV\Show - 03.mkv").as_deref(),
            Some(r"D:\Anime - TV\Show - 03.mkv")
        );
        // A translated ending still leaves a video file name before it.
        assert_eq!(
            media(Family::Vlc, &format!("{f} - Lecteur multim\u{e9}dia VLC")).as_deref(),
            Some(f)
        );
        // A title the file carries: kept, without the player's name.
        assert_eq!(
            media(Family::Mpv, "Lanterns of Kirigaoka - Episode 6 - mpv").as_deref(),
            Some("Lanterns of Kirigaoka - Episode 6")
        );
    }

    #[test]
    fn titles_with_nothing_playing_are_idle() {
        for (fam, t) in [
            (Family::Mpv, "mpv"),
            (Family::Mpv, "No file - mpv"),
            (Family::MpvNet, "mpv.net"),
            (Family::MpvNet, "mpv.net 7.1.1.0"),
            (Family::Vlc, "VLC media player"),
            (Family::MpcHc, "Media Player Classic Home Cinema"),
            (Family::MpcHc, "MPC-HC"),
            (Family::MpcBe, "MPC-BE x64 1.8.3"),
            (Family::PotPlayer, "PotPlayer"),
            (Family::MpcQt, "Media Player Classic Qute Theater"),
        ] {
            assert_eq!(read_title(fam, &win(t)), Some(Shown::Idle), "{t}");
        }
        // Not a window that says what plays.
        for (fam, t) in [
            (Family::Vlc, "Open Media"),
            (Family::Mpv, ""),
            (Family::PotPlayer, "Preferences"),
            (Family::Mpv, "mpvish thing"),
        ] {
            assert_eq!(read_title(fam, &win(t)), None, "{t}");
        }
        // MPC-HC's main window shows a title with nothing after it; its dialogs don't count.
        let main = Win {
            hwnd: 1,
            title: "Lanterns of Kirigaoka 6".into(),
            class: MPC_HC_CLASS.into(),
        };
        assert_eq!(
            read_title(Family::MpcHc, &main),
            Some(Shown::Media("Lanterns of Kirigaoka 6".into()))
        );
        assert_eq!(read_title(Family::MpcHc, &win("Options")), None);
    }

    #[test]
    fn the_title_wins_and_the_command_line_adds_the_folder() {
        let cmd = vec![
            r"D:\Anime\Show - 01.mkv".to_string(),
            r"D:\Anime\Show - 02.mkv".to_string(),
        ];
        let shown = |title: &str| shown_file(Family::Mpv, &cmd, &[win(title)]).0;
        assert_eq!(
            shown("Show - 01.mkv - mpv").as_deref(),
            Some(r"D:\Anime\Show - 01.mkv")
        );
        // The playlist moved on: the title says so.
        assert_eq!(
            shown("show - 02.MKV - mpv").as_deref(),
            Some(r"D:\Anime\Show - 02.mkv")
        );
        // A file opened from inside the player: the name alone.
        assert_eq!(
            shown("Other - 05.mkv - mpv").as_deref(),
            Some("Other - 05.mkv")
        );
        // A title without the extension that is one of the files.
        assert_eq!(
            shown("Show - 02 - mpv").as_deref(),
            Some(r"D:\Anime\Show - 02.mkv")
        );
        // A title the file carries, matching nothing: taken as it is, never the first file.
        assert_eq!(shown("Episode 2 - mpv").as_deref(), Some("Episode 2"));
        // The window shows nothing: the file of the command line ended.
        assert_eq!(shown("No file - mpv"), None);
        assert_eq!(shown("mpv"), None);
        // A window we can't read: not a guess from the command line either.
        assert_eq!(shown("something else"), None);
        // No window at all: the command line is all there is.
        assert_eq!(
            shown_file(Family::Mpv, &cmd, &[]).0.as_deref(),
            Some(r"D:\Anime\Show - 01.mkv")
        );
        assert_eq!(shown_file(Family::Mpv, &[], &[]).0, None);
        // A dialog beside the main window changes nothing.
        let both = [
            win("Open File"),
            Win {
                hwnd: 9,
                ..win("Show - 01.mkv - mpv")
            },
        ];
        assert_eq!(
            shown_file(Family::Mpv, &cmd, &both),
            (Some(r"D:\Anime\Show - 01.mkv".to_string()), Some(9))
        );
    }

    /// Fake processes and windows.
    #[derive(Default)]
    struct Fake {
        procs: Vec<(u32, String, Option<String>)>,
        wins: HashMap<u32, Vec<Win>>,
        pot: Option<Pot>,
        opened: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl Sys for Fake {
        fn processes(&mut self) -> Vec<(u32, String)> {
            self.procs.iter().map(|(p, n, _)| (*p, n.clone())).collect()
        }

        fn info(&mut self, pid: u32) -> Option<ProcInfo> {
            self.opened.set(self.opened.get() + 1);
            let (_, name, cmd) = self.procs.iter().find(|(p, _, _)| *p == pid)?;
            Some(ProcInfo {
                exe: format!(r"C:\Players\{name}"),
                command_line: cmd.clone()?,
            })
        }

        fn windows(&mut self, pids: &HashSet<u32>) -> HashMap<u32, Vec<Win>> {
            self.wins
                .iter()
                .filter(|(p, _)| pids.contains(p))
                .map(|(p, w)| (*p, w.clone()))
                .collect()
        }

        fn potplayer(&mut self, _hwnd: isize) -> Option<Pot> {
            self.pot
        }
    }

    fn files(seen: &[Seen]) -> Vec<(u32, &str, Option<&str>)> {
        seen.iter()
            .map(|s| (s.pid, s.kind.player, s.file.as_deref()))
            .collect()
    }

    #[test]
    fn the_scan_finds_players_by_process_command_line_and_title() {
        let mut fake = Fake {
            procs: vec![
                (10, "explorer.exe".into(), Some("explorer.exe".into())),
                (
                    20,
                    "mpvnet.exe".into(),
                    Some(r#""C:\Players\mpvnet.exe" "D:\Anime\Show - 03.mkv""#.into()),
                ),
                (30, "vlc.exe".into(), Some(r#""C:\Players\vlc.exe""#.into())),
                (
                    40,
                    "mpc-hc64.exe".into(),
                    Some(r#""C:\Players\mpc-hc64.exe""#.into()),
                ),
                // Another user's player: it can't be opened.
                (50, "mpv.exe".into(), None),
            ],
            ..Default::default()
        };
        fake.wins
            .insert(20, vec![win("Show - 03.mkv - mpv.net 7.1.1.0")]);
        fake.wins.insert(30, vec![win("VLC media player")]);
        fake.wins.insert(40, vec![win("Other - 07.mkv")]);
        let opened = fake.opened.clone();
        let mut sc = Scanner::new(fake);
        let seen = sc.scan();
        assert_eq!(
            files(&seen),
            [
                (20, "mpv.net", Some(r"D:\Anime\Show - 03.mkv")),
                (30, "VLC", None),
                (40, "MPC-HC", Some("Other - 07.mkv"))
            ]
        );
        assert_eq!(seen[0].exe, r"C:\Players\mpvnet.exe");
        assert_eq!(
            opened.get(),
            4,
            "the three players and the one that can't be opened; never explorer"
        );
        // The next scan opens only the one that failed.
        sc.scan();
        assert_eq!(opened.get(), 5);
        // mpv.net moves to a file opened from inside it; MPC-HC closes.
        sc.sys
            .wins
            .insert(20, vec![win("Show - 04.mkv - mpv.net 7.1.1.0")]);
        sc.sys.procs.retain(|(p, _, _)| *p != 40);
        assert_eq!(
            files(&sc.scan()),
            [(20, "mpv.net", Some("Show - 04.mkv")), (30, "VLC", None)]
        );
        // A process id reused by another program is read again.
        sc.sys.procs[1] = (20, "notepad.exe".into(), Some("notepad.exe".into()));
        assert_eq!(files(&sc.scan()), [(30, "VLC", None)]);
    }

    #[test]
    fn potplayer_gives_its_position() {
        let mut fake = Fake {
            procs: vec![(
                7,
                "PotPlayerMini64.exe".into(),
                Some(r#""C:\Players\PotPlayerMini64.exe" "D:\Anime\Show - 03.mkv""#.into()),
            )],
            ..Default::default()
        };
        fake.wins.insert(7, vec![win("Show - 03.mkv - PotPlayer")]);
        fake.pot = Some(Pot {
            status: 2,
            position_ms: 600_000,
            duration_ms: 1_420_000,
        });
        let mut sc = Scanner::new(fake);
        let seen = sc.scan();
        assert_eq!(
            seen[0].pos,
            Some(Pos {
                position: 600.0,
                duration: 1420.0,
                playing: true
            })
        );
        let p = snapshot(&seen[0], seen[0].file.as_deref().unwrap(), 3.0);
        assert_eq!(
            (p.position, p.duration, p.state, p.via),
            (Some(600.0), Some(1420.0), PlayState::Playing, Via::Process)
        );
        // Answers that make no sense are dropped: the time open counts instead.
        for pot in [
            Pot {
                status: 0,
                position_ms: 0,
                duration_ms: 0,
            },
            Pot {
                status: 2,
                position_ms: 5,
                duration_ms: 0,
            },
            Pot {
                status: 2,
                position_ms: 9_000_000,
                duration_ms: 1_420_000,
            },
            Pot {
                status: -1,
                position_ms: 1,
                duration_ms: 2,
            },
        ] {
            assert_eq!(pot.pos(), None, "{pot:?}");
        }
        sc.sys.pot = None;
        let seen = sc.scan();
        let p = snapshot(&seen[0], "x.mkv", 3.0);
        assert_eq!(
            (p.position, p.duration, p.state),
            (None, None, PlayState::Playing)
        );
    }

    fn kinds(inputs: &[Input]) -> Vec<String> {
        inputs
            .iter()
            .map(|i| match i {
                Input::Update(p) => format!(
                    "update {} {}{}",
                    p.key,
                    p.path.as_deref().unwrap_or(""),
                    p.position.map(|x| format!(" @{x}")).unwrap_or_default()
                ),
                Input::Gone(k) => format!("gone {k}"),
                Input::Status(s) => format!("status {:?} {}", s.switch, s.line),
                Input::Settings => "settings".into(),
            })
            .collect()
    }

    #[test]
    fn the_reporter_sends_changes_only() {
        let (kind, family) = kind_of_exe("mpvnet.exe").unwrap();
        let f = |file: Option<&str>| {
            vec![Seen {
                pid: 7,
                kind,
                family,
                exe: String::new(),
                file: file.map(String::from),
                pos: None,
            }]
        };
        let mut r = Reporter::default();
        assert_eq!(
            kinds(&r.apply(&f(Some("a.mkv")), 1.0)),
            ["update proc:7 a.mkv", "status Mpv mpv.net: video file open"]
        );
        assert!(r.apply(&f(Some("a.mkv")), 6.0).is_empty());
        assert_eq!(
            kinds(&r.apply(&f(Some("b.mkv")), 11.0)),
            ["update proc:7 b.mkv"]
        );
        assert_eq!(
            kinds(&r.apply(&f(None), 16.0)),
            ["gone proc:7", "status Mpv mpv.net: running, no video open"]
        );
        assert!(r.apply(&f(None), 21.0).is_empty());
        assert_eq!(
            kinds(&r.apply(&f(Some("c.mkv")), 26.0)),
            ["update proc:7 c.mkv", "status Mpv mpv.net: video file open"]
        );
        assert_eq!(
            kinds(&r.apply(&[], 31.0)),
            ["gone proc:7", "status Mpv not running"]
        );
        assert!(r.apply(&[], 36.0).is_empty());
        // A player that starts with nothing open sends its state only.
        assert_eq!(
            kinds(&r.apply(&f(None), 41.0)),
            ["status Mpv mpv.net: running, no video open"]
        );
    }

    #[test]
    fn a_position_is_sent_again_only_when_it_jumps_or_pauses() {
        let (kind, family) = kind_of_exe("PotPlayerMini64.exe").unwrap();
        let f = |position: f64, playing: bool| {
            vec![Seen {
                pid: 7,
                kind,
                family,
                exe: String::new(),
                file: Some("a.mkv".into()),
                pos: Some(Pos {
                    position,
                    duration: 1420.0,
                    playing,
                }),
            }]
        };
        let mut r = Reporter::default();
        assert_eq!(
            kinds(&r.apply(&f(100.0, true), 0.0)),
            [
                "update proc:7 a.mkv @100",
                "status PotPlayer video file open"
            ]
        );
        // Playing on as the clock says: nothing to send, scan after scan.
        assert!(r.apply(&f(105.0, true), 5.0).is_empty());
        assert!(r.apply(&f(161.0, true), 60.0).is_empty());
        // A seek.
        assert_eq!(
            kinds(&r.apply(&f(900.0, true), 65.0)),
            ["update proc:7 a.mkv @900"]
        );
        // A pause, then still paused.
        assert_eq!(
            kinds(&r.apply(&f(903.0, false), 70.0)),
            ["update proc:7 a.mkv @903"]
        );
        assert!(r.apply(&f(903.0, false), 300.0).is_empty());
        assert_eq!(
            kinds(&r.apply(&f(903.0, true), 305.0)),
            ["update proc:7 a.mkv @903"]
        );
    }
}
