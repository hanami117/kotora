//! Now-playing sources (docs/DESIGN.md §5.5) and the watch timer.
//!
//! - [`mpv`]: mpv's JSON IPC socket (Linux and macOS; IINA too, through its "additional mpv
//!   options"). Event driven: `observe_property` and seek events. While the socket is missing, a
//!   folder watch (inotify / kqueue, [`dirwatch`]) waits for it; nothing polls.
//! - `mpris` (Linux): MPRIS players on the session bus (VLC, mpc-qt, mpv with mpv-mpris, others
//!   that report a `file://` URL), through zbus 5's blocking API: the zbus copy Rune's AT-SPI
//!   accessibility already builds, so there is one. Signals only.
//! - [`vlc`] (macOS): VLC's web interface. VLC has no events there, so this source polls, slowly,
//!   and only when the user set it up (it needs VLC's password). See `vlc.rs` for the timings.
//!
//! Every source reports [`Playing`] snapshots to the [`watch::Watch`] hub. The hub picks the
//! player to follow and works out, from the last position and the clock, when the episode passes
//! the threshold. It sleeps until then; there is no tick.
//!
//! Nothing here knows about anime, the list or the UI: the app gets [`Report`]s.

pub mod dirwatch;
pub mod mpv;
pub mod mpvconf;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod notify;
pub mod run;
pub mod vlc;
pub mod watch;

pub use run::{Detector, Settings, start};
pub use watch::{Report, Track};

use std::sync::OnceLock;
use std::time::Instant;

/// How a player is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Via {
    MpvIpc,
    Mpris,
    VlcHttp,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::MpvIpc => "IPC socket",
            Via::Mpris => "MPRIS",
            Via::VlcHttp => "web interface",
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
}

impl Switch {
    pub const ALL: [Switch; 5] = [Switch::Mpv, Switch::Iina, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris];

    /// The key in `config.toml`'s `[players]`.
    pub fn config_key(self) -> &'static str {
        match self {
            Switch::Mpv => "mpv",
            Switch::Iina => "iina",
            Switch::Vlc => "vlc",
            Switch::MpcQt => "mpc_qt",
            Switch::OtherMpris => "other_mpris",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Switch::Mpv => "mpv",
            Switch::Iina => "IINA",
            Switch::Vlc => "VLC",
            Switch::MpcQt => "mpc-qt",
            Switch::OtherMpris => "Other MPRIS players",
        }
    }

    /// The switches Settings shows on this system (DESIGN.md §12: v0.1 players).
    pub fn here() -> &'static [Switch] {
        if cfg!(target_os = "macos") { &[Switch::Mpv, Switch::Iina, Switch::Vlc] } else { &[Switch::Mpv, Switch::Vlc, Switch::MpcQt, Switch::OtherMpris] }
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

/// `file:///media/b%20c.mkv` → `/media/b c.mkv`; `None` for any other URL.
pub fn file_url_path(url: &str) -> Option<String> {
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
    fn file_urls_decode() {
        assert_eq!(file_url_path("file:///v/A%20B%20-%2003.mkv").as_deref(), Some("/v/A B - 03.mkv"));
        assert_eq!(file_url_path("file://localhost/v/x.mkv").as_deref(), Some("/v/x.mkv"));
        assert_eq!(file_url_path("file:///v/%E3%81%82.mkv").as_deref(), Some("/v/\u{3042}.mkv"));
        assert_eq!(file_url_path("https://example.invalid/x.mkv"), None);
        assert_eq!(file_url_path("file://host/x"), None);
        assert_eq!(file_url_path("file:///bad%zz").as_deref(), Some("/bad%zz"));
    }
}
