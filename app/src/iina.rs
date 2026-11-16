//! Precise detection for IINA (macOS only), under IINA in Settings → Players.
//!
//! IINA is seen with no setup by its open file (the process source). For the position its mpv
//! needs the IPC socket, which only IINA's own settings turn on (`kotora_detect::iinaconf`):
//! - IINA reads the `mpv.conf` of a config folder ("Use config directory"): Turn on appends the
//!   socket there, once, and shows what was written, as for mpv (`precise.rs`).
//! - Otherwise the hint says which IINA settings to change, with Check again.
//!
//! IINA's preferences are only read, never written. The check runs at start and on Check again,
//! on a short thread (it runs `defaults`); never headless.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kotora_detect::iinaconf::{self, Setup};
use kotora_detect::mpvconf::Enabled;
use kotora_detect::run::Detector;
use kotora_detect::{Led, Switch};
use rune::{El, col, row, text};

use crate::model::State;
use crate::precise::{Precise, VIA_IPC, VIA_PROCESS};
use crate::theme::{self, PAL};
use crate::ui::small_button;

/// The hint's state in this session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Iina {
    /// What IINA's settings say (`None`: not read yet, or they couldn't be read).
    pub setup: Option<Setup>,
    /// The socket that [`Setup::Set`] names is one Kotora watches (worked out with the setup,
    /// so the view reads no file).
    pub watched: bool,
    /// Writing or checking now.
    pub busy: bool,
    /// What Turn on did.
    pub done: Option<Precise>,
}

/// What to add in IINA's Settings by hand.
pub const STEPS: &str = "In IINA: Settings \u{2192} Advanced, turn on Enable advanced settings, then under Additional mpv options add input-ipc-server with the value /tmp/iina.sock. Restart IINA.";

/// IINA's socket when nothing else is set (also in `kotora_detect::mpv::default_sockets`).
const DEFAULT_SOCKET: &str = "/tmp/iina.sock";

fn live(led: Led) -> bool {
    matches!(led, Led::Playing | Led::Idle)
}

/// The socket Kotora watches for IINA: the first watched path with "iina" in it.
fn iina_socket() -> Option<std::path::PathBuf> {
    crate::precise::watched_sockets().into_iter().find(|p| kotora_detect::mpv::switch_for(p) == Switch::Iina)
}

impl State {
    /// IINA is seen by its open file only: no IPC socket answers.
    pub fn iina_seen_by_process(&self) -> bool {
        self.prefs.players.iina
            && self.players.iter().any(|p| p.switch == Switch::Iina && p.via == VIA_PROCESS && live(p.led))
            && !self.players.iter().any(|p| p.switch == Switch::Iina && p.via == VIA_IPC && live(p.led))
    }

    /// Check again: re-reads IINA's settings.
    pub fn check_iina(&mut self) {
        self.iina.done = None;
        if let Some(h) = self.handle.clone() {
            self.iina.busy = true;
            check(h, self.glue.as_ref().map(|g| g.detector()));
        }
    }

    /// Turn on: appends the socket to the mpv.conf IINA reads.
    pub fn enable_iina(&mut self) {
        if self.iina.busy {
            return;
        }
        let Some(h) = self.handle.clone() else { return };
        self.iina.busy = true;
        let detector = self.glue.as_ref().map(|g| g.detector());
        let _ = std::thread::Builder::new().name("kotora-iina-conf".into()).spawn(move || {
            let r = write(detector.as_deref());
            let setup = read(detector.as_deref());
            h.update_if(move |s, _| s.iina_written(r, setup));
        });
    }

    pub fn iina_written(&mut self, r: Precise, setup: Option<(Setup, bool)>) -> bool {
        self.iina.busy = false;
        self.iina_answer(r.clone());
        self.iina.done = Some(r);
        if let Some((setup, watched)) = setup {
            self.iina.setup = Some(setup);
            self.iina.watched = watched;
        }
        true
    }
}

/// At start and on Check again (never headless; on a short thread).
pub fn check(h: crate::model::Handle, detector: Option<Arc<Detector>>) {
    let _ = std::thread::Builder::new().name("kotora-iina-conf".into()).spawn(move || {
        let read = read(detector.as_deref());
        h.update_if(move |s, _| {
            let (setup, watched) = match read {
                Some((setup, watched)) => (Some(setup), watched),
                None => (None, false),
            };
            let changed = s.iina.setup != setup || s.iina.watched != watched || s.iina.busy;
            s.iina.setup = setup;
            s.iina.watched = watched;
            s.iina.busy = false;
            changed
        });
    });
}

/// IINA's setup, and whether Kotora watches the socket it sets. A socket IINA sets that Kotora
/// doesn't watch yet (a custom path) is added to the running detection here, so it needs no
/// restart and no `config.toml`. Blocks (runs `defaults`).
fn read(detector: Option<&Detector>) -> Option<(Setup, bool)> {
    let setup = iinaconf::check().ok()?;
    let watched = matches!(&setup, Setup::Set { socket, .. } if watch(detector, socket));
    Some((setup, watched))
}

/// Kotora watches this socket now: it is in `[players] mpv_sockets` (or the default), or it is
/// added to the running detection.
fn watch(detector: Option<&Detector>, socket: &Path) -> bool {
    if crate::precise::watched_sockets().iter().any(|p| p == socket) {
        return true;
    }
    detector.is_some_and(|d| {
        d.watch_socket(socket);
        true
    })
}

/// The socket Turn on writes: the one Kotora watches for IINA, else the default.
fn socket_to_write() -> PathBuf {
    iina_socket().unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

fn write(detector: Option<&Detector>) -> Precise {
    let socket = socket_to_write();
    match iinaconf::enable(&socket) {
        Ok(Enabled::Wrote { file, text }) => {
            // Watched from now on: IINA only has to be restarted.
            watch(detector, &socket);
            Precise::Wrote { file: file.display().to_string(), text }
        }
        Ok(Enabled::AlreadySet { file, socket: set }) => {
            let watched = watch(detector, &set);
            Precise::AlreadySet { file: file.display().to_string(), socket: set.display().to_string(), watched }
        }
        Err(e) => Precise::Failed(format!("Can't turn it on: {e}. {STEPS}")),
    }
}

/// The hint's words and its button.
fn words(s: &State) -> (String, Option<El>) {
    let tilde = crate::precise::tilde;
    if let Some(done) = &s.iina.done {
        return match done {
            Precise::Wrote { file, text: lines } => (format!("Added to {}: {}. Restart IINA to use it.", tilde(file), lines.lines().last().unwrap_or("").trim()), None),
            Precise::AlreadySet { file, socket, watched: true } => (format!("{} already sets input-ipc-server={}, which Kotora watches. Restart IINA to use it.", tilde(file), tilde(socket)), None),
            Precise::AlreadySet { file, socket, watched: false } => {
                (format!("{} sets input-ipc-server={}, which Kotora doesn't watch: add that path to [players] mpv_sockets in config.toml.", tilde(file), tilde(socket)), None)
            }
            Precise::Failed(e) => (e.clone(), Some(small_button("set/iina-check", "Check again", false, Some(State::check_iina)))),
        };
    }
    let check = || Some(small_button("set/iina-check", if s.iina.busy { "Checking\u{2026}" } else { "Check again" }, false, (!s.iina.busy).then_some(State::check_iina as fn(&mut State))));
    match &s.iina.setup {
        Some(Setup::Set { socket, file }) => {
            let watched = s.iina.watched;
            let from = file.as_ref().map_or("IINA's additional mpv options".to_string(), |f| tilde(&f.display().to_string()));
            if watched {
                (format!("{from} sets the IPC socket Kotora watches ({}). Restart IINA to use it.", tilde(&socket.display().to_string())), None)
            } else {
                (format!("{from} sets input-ipc-server={}, which Kotora doesn't watch: add that path to [players] mpv_sockets in config.toml.", tilde(&socket.display().to_string())), None)
            }
        }
        Some(Setup::ConfDir { file }) => (
            format!("Seen by its open file only: no position. IINA reads {}; Turn on adds its IPC socket there.", tilde(&file.display().to_string())),
            Some(small_button("set/iina-on", if s.iina.busy { "Turning on\u{2026}" } else { "Turn on" }, true, (!s.iina.busy).then_some(State::enable_iina as fn(&mut State)))),
        ),
        Some(Setup::Manual) | None => (format!("Seen by its open file only: no position. {STEPS}"), check()),
    }
}

/// Settings → Players, under IINA: how to get the position, while IINA is seen by its open file
/// only (or until the answer to Turn on is read).
pub fn settings_hint(s: &State) -> Option<El> {
    if !s.iina_seen_by_process() && s.iina.done.is_none() {
        return None;
    }
    let (line, action) = words(s);
    let mut r = row()
        .key("set/iina")
        .gap(10.0)
        .ml(19.0)
        .mb(4.0)
        .px(10.0)
        .py(8.0)
        .center_y()
        .radius(theme::R_MD)
        .fill(PAL.st_completed.alpha(0.08))
        .border(1.0, PAL.st_completed.alpha(0.25))
        .child(col().grow(1.0).min_w(0.0).child(text(line).key("set/iina/text").font(theme::LABEL).color(PAL.dim).wrap()));
    if let Some(a) = action {
        r = r.child(a);
    }
    Some(r)
}

/// Fake players for the scenes: IINA seen by its process only, playing.
pub fn fake_process_players() -> Vec<crate::precise::PlayerState> {
    use crate::precise::PlayerState;
    vec![
        PlayerState { switch: Switch::Mpv, via: VIA_IPC, line: "/tmp/mpv.sock: not running".into(), led: Led::Off },
        PlayerState { switch: Switch::Iina, via: VIA_PROCESS, line: "video file open".into(), led: Led::Playing },
        PlayerState { switch: Switch::Iina, via: VIA_IPC, line: "/tmp/iina.sock: not running".into(), led: Led::Off },
        PlayerState { switch: Switch::Vlc, via: VIA_PROCESS, line: "running, no video open".into(), led: Led::Idle },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn iina_by_process() -> State {
        let mut s = State::fake();
        s.now = None;
        s.players = fake_process_players();
        s
    }

    fn line(s: &State) -> String {
        words(s).0
    }

    #[test]
    fn the_hint_shows_while_iina_is_seen_by_its_open_file_only() {
        let s = iina_by_process();
        assert!(s.iina_seen_by_process() && settings_hint(&s).is_some());
        // Not read yet: the steps in IINA, with Check again.
        assert!(line(&s).contains("Additional mpv options") && words(&s).1.is_some());
        // The socket answers: no hint.
        let mut s = iina_by_process();
        s.players.push(crate::precise::PlayerState { switch: Switch::Iina, via: VIA_IPC, line: "playing".into(), led: Led::Playing });
        assert!(!s.iina_seen_by_process() && settings_hint(&s).is_none());
        // IINA switched off: none either.
        let mut s = iina_by_process();
        s.prefs.players.iina = false;
        assert!(settings_hint(&s).is_none());
    }

    #[test]
    fn the_hint_follows_iinas_settings() {
        let mut s = iina_by_process();
        s.iina.setup = Some(Setup::ConfDir { file: PathBuf::from("/nowhere/iina/mpv.conf") });
        let (l, b) = words(&s);
        assert!(l.contains("/nowhere/iina/mpv.conf") && b.is_some(), "{l}");
        s.iina.setup = Some(Setup::Set { socket: PathBuf::from("/tmp/iina.sock"), file: None });
        s.iina.watched = true;
        let (l, b) = words(&s);
        assert!(l.contains("Restart IINA") && b.is_none(), "{l}");
        s.iina.setup = Some(Setup::Set { socket: PathBuf::from("/tmp/other.sock"), file: None });
        s.iina.watched = false;
        assert!(line(&s).contains("doesn't watch"));
        // What Turn on wrote stays shown, without a button.
        s.iina_written(Precise::Wrote { file: "/nowhere/iina/mpv.conf".into(), text: "# Added by Kotora\ninput-ipc-server=/tmp/iina.sock\n".into() }, None);
        let (l, b) = words(&s);
        assert!(l.contains("input-ipc-server=/tmp/iina.sock") && l.contains("Restart IINA") && b.is_none(), "{l}");
        assert!(!s.iina.busy);
    }
}
