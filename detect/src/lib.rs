//! Now-playing sources and the watch timer.
//!
//! - [`mpv`]: mpv's JSON IPC socket (Linux and macOS; IINA too, through its "additional mpv
//!   options"). Event driven: `observe_property` and seek events. While the socket is missing, a
//!   folder watch (inotify / kqueue, [`dirwatch`]) waits for it; nothing polls.
//! - `mpris` (Linux): MPRIS players on the session bus (VLC, mpc-qt, mpv with mpv-mpris, others
//!   that report a `file://` URL), through zbus 5's blocking API: the zbus copy Rune's AT-SPI
//!   accessibility already builds, so there is one. Signals only.
//! - [`vlc`] (macOS): VLC's web interface. VLC has no events there, so this source polls, slowly,
//!   and only when the user set it up (it needs VLC's password). See `vlc.rs` for the timings.
//! - [`procfs`] (Linux; macOS through `libproc`): no setup at all. Which video file a known
//!   player has open, from the process table, every 5 s. No position, so the hub counts the time
//!   the file stays open, and ignores it when an IPC or MPRIS source shows the same file.
//! - [`winplayers`] (Windows): no setup either. Windows has no public way to list a process's
//!   open files, so this reads the player's command line and its window title, every 5 s.
//!   PotPlayer also answers window messages with its position.
//! - [`mpc`] (Windows): MPC-HC's and MPC-BE's web interface, polled like VLC's.
//! - [`mpvconf`]: reads and extends the user's `mpv.conf` for the IPC socket ("precise mode"),
//!   only when the user asks.
//! - [`iinaconf`] (macOS): the same for IINA, which reads its own settings and, when set, the
//!   `mpv.conf` of its config folder.
//!
//! Every source reports [`Playing`] snapshots to the [`watch::Watch`] hub. The hub picks the
//! player to follow and works out, from the last position and the clock, when the episode passes
//! the threshold. It sleeps until then; there is no tick.
//!
//! Nothing here knows about anime, the list or the UI: the app gets [`Report`]s.

#[cfg(unix)]
pub mod dirwatch;
pub mod comics;
#[cfg(target_os = "macos")]
pub mod iinaconf;
#[cfg(target_os = "macos")]
pub mod libproc;
pub mod mpc;
pub mod mpv;
pub mod mpvconf;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod procfs;
pub mod run;
pub mod vlc;
pub mod watch;
pub mod winplayers;
#[cfg(windows)]
pub mod winproc;

pub use run::{Detector, Settings, start};

/// VLC is followed through its web interface here (macOS and Windows; Linux has MPRIS).
pub const VLC_WEB: bool = cfg!(any(target_os = "macos", windows));
pub use watch::{Report, Track};

use std::sync::OnceLock;
use std::time::Instant;

/// How a player is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Via {
    MpvIpc,
    Mpris,
    VlcHttp,
    /// MPC-HC's and MPC-BE's web interface (Windows).
    MpcHttp,
    /// The open files of the player's process (`procfs`): no position, duration or pause.
    /// On Windows the process's command line and window title (`winplayers`).
    Process,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::MpvIpc => "IPC socket",
            Via::Mpris => "MPRIS",
            Via::VlcHttp | Via::MpcHttp => "web interface",
            Via::Process => "process",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

/// What one player shows now, as its source last saw it.
#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    /// Which player instance: `mpv:/run/user/1000/mpv.sock`, `mpris:org.mpris.MediaPlayer2.vlc`.
    pub key: String,
    /// "mpv", "VLC", "mpc-qt", "IINA", or the name an MPRIS player gives.
    pub player: String,
    /// The Settings switch this player belongs to (see [`Switch`]).
    pub switch: Switch,
    pub via: Via,
    /// The local file, when the player plays one.
    pub path: Option<String>,
    pub title: Option<String>,
    /// Seconds into the file at `at`.
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub state: PlayState,
    /// Playback speed (1.0 normal).
    pub rate: f64,
    /// When `position` was true, on [`clock`]'s scale.
    pub at: f64,
}

impl Playing {
    /// The file this snapshot is about: the path, else the title.
    pub fn file(&self) -> Option<&str> {
        self.path.as_deref().or(self.title.as_deref()).filter(|s| !s.is_empty())
    }
}

/// The per-player switches of Settings → Players.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Switch {
    Mpv,
    Iina,
    Vlc,
    MpcQt,
    OtherMpris,
    /// MPC-HC and MPC-BE (Windows).
    Mpc,
    /// PotPlayer (Windows).
    PotPlayer,
}

impl Switch {
    pub const ALL: [Switch; 7] = [Switch::Mpv, Switch::Iina, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris, Switch::Mpc, Switch::PotPlayer];

    /// The key in `config.toml`'s `[players]`.
    pub fn config_key(self) -> &'static str {
        match self {
            Switch::Mpv => "mpv",
            Switch::Iina => "iina",
            Switch::Vlc => "vlc",
            Switch::MpcQt => "mpc_qt",
            Switch::OtherMpris => "other_mpris",
            Switch::Mpc => "mpc",
            Switch::PotPlayer => "potplayer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Switch::Mpv => "mpv",
            Switch::Iina => "IINA",
            Switch::Vlc => "VLC",
            Switch::MpcQt => "mpc-qt",
            Switch::OtherMpris => "Other MPRIS players",
            Switch::Mpc => "MPC-HC / MPC-BE",
            Switch::PotPlayer => "PotPlayer",
        }
    }

    /// The switches Settings shows on this system.
    pub fn here() -> &'static [Switch] {
        if cfg!(windows) {
            &[Switch::Mpv, Switch::Mpc, Switch::Vlc, Switch::PotPlayer, Switch::MpcQt]
        } else if cfg!(target_os = "macos") {
            &[Switch::Mpv, Switch::Iina, Switch::Vlc]
        } else { &[Switch::Mpv, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris]
        }
    }
}

/// A source's state for Settings and the Now Playing page: "mpv · IPC socket · playing".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceStatus {
    pub switch: Switch,
    pub via: Via,
    pub line: String,
    pub led: Led,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Led {
    /// Playing a file.
    Playing,
    /// Running, nothing playing (or paused).
    Idle,
    /// Not running, or not found.
    Off,
    /// Needs the user (not set up, a refused password, a missing folder).
    Problem,
}

/// Seconds since the first call, monotonic: the time scale of [`Playing::at`].
pub fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The file name of a path a player reported: what follows the last `/`, and on Windows the
/// last `\` too (players there report either).
pub fn file_name(path: &str) -> &str {
    file_name_on(path, cfg!(windows))
}

fn file_name_on(path: &str, windows: bool) -> &str {
    let cut = if windows { path.rfind(['/', '\\']) } else { path.rfind('/') };
    cut.map_or(path, |i| &path[i + 1..])
}

/// Whether a path a player reported is absolute: `/x`, and on Windows `C:\x`, `C:/x` and `\\server\x`.
pub fn is_absolute(path: &str) -> bool {
    is_absolute_on(path, cfg!(windows))
}

fn is_absolute_on(path: &str, windows: bool) -> bool {
    let b = path.as_bytes();
    path.starts_with('/') || (windows && (path.starts_with("\\\\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))))
}

/// `file:///media/b%20c.mkv` → `/media/b c.mkv`; `None` for any other URL. On Windows
/// `file:///C:/v/b.mkv` → `C:\v\b.mkv`.
pub fn file_url_path(url: &str) -> Option<String> {
    let path = file_url_path_raw(url)?;
    Some(if cfg!(windows) { windows_url_path(&path) } else { path })
}

/// A `file:` URL as a Windows path, on every system (the Windows players' command lines).
pub(crate) fn windows_file_url(url: &str) -> Option<String> {
    file_url_path_raw(url).map(|p| windows_url_path(&p))
}

/// `/C:/v/b.mkv` → `C:\v\b.mkv`; a path with no drive letter keeps its slashes.
fn windows_url_path(path: &str) -> String {
    let b = path.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { path[1..].replace('/', "\\") } else { path.to_string() }
}

fn file_url_path_raw(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    // `file://localhost/x` and `file:///x`.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_paths_on_both_kinds_of_system() {
        assert_eq!(file_name_on("/v/Show - 01.mkv", false), "Show - 01.mkv");
        assert_eq!(file_name_on("Show - 01.mkv", false), "Show - 01.mkv");
        // A backslash is an ordinary character in a Unix file name.
        assert_eq!(file_name_on("/v/a\\b.mkv", false), "a\\b.mkv");
        assert_eq!(file_name_on("D:\\Anime\\Show - 01.mkv", true), "Show - 01.mkv");
        assert_eq!(file_name_on("D:/Anime/Show - 01.mkv", true), "Show - 01.mkv");
        assert_eq!(file_name_on("D:\\Anime/Show\\01.mkv", true), "01.mkv");
        assert!(is_absolute_on("/v/x.mkv", false) && is_absolute_on("/v/x.mkv", true));
        assert!(!is_absolute_on("C:\\v\\x.mkv", false) && !is_absolute_on("x.mkv", true) && !is_absolute_on("C:x.mkv", true));
        assert!(is_absolute_on("C:\\v\\x.mkv", true) && is_absolute_on("c:/v/x.mkv", true) && is_absolute_on("\\\\nas\\v\\x.mkv", true));
        assert_eq!(windows_url_path("/C:/v/A B - 03.mkv"), "C:\\v\\A B - 03.mkv");
        assert_eq!(windows_url_path("/v/x.mkv"), "/v/x.mkv");
    }

    #[test]
    fn file_urls_decode() {
        assert_eq!(file_url_path("file:///v/A%20B%20-%2003.mkv").as_deref(), Some("/v/A B - 03.mkv"));
        assert_eq!(file_url_path("file://localhost/v/x.mkv").as_deref(), Some("/v/x.mkv"));
        assert_eq!(file_url_path("file:///v/%E3%81%82.mkv").as_deref(), Some("/v/\u{3042}.mkv"));
        assert_eq!(file_url_path("https://example.invalid/x.mkv"), None);
        assert_eq!(file_url_path("file://host/x"), None);
        assert_eq!(file_url_path("file:///bad%zz").as_deref(), Some("/bad%zz"));
    }
}
