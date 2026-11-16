//! Players as Settings and Now Playing show them, and the one-time "precise detection" offer for
//! mpv (Stage 5).
//!
//! Since Stage 5 a player can be seen two ways: by its open file (the process source, `via`
//! "process": no setup, but no position or pause) or by its own interface (mpv's IPC socket,
//! MPRIS, VLC's web interface: the position too). [`PlayerState`] is the app's copy of a source's
//! state, keyed by the source's label, so the views say honestly which one sees a player.
//!
//! When mpv is seen only by its process, a banner offers once to turn on its IPC socket: one click
//! appends `input-ipc-server=…` to the user's `mpv.conf` (never twice, never on its own), shows
//! what was written, and works from mpv's next start. The answer is remembered in `config.toml`
//! (`[notices] mpv_precise`), so the banner never comes back; Settings keeps a Turn on button.

use kotora_core::config;
use kotora_core::paths;
use kotora_detect::{Led, SourceStatus, Switch};
use rune::{El, TextStyle, col, el, icon, row, text};

use crate::icons;
use crate::model::State;
use crate::theme::{self, PAL};
use crate::ui::{button, ghost, small_button};

/// The process source's label (`kotora_detect::Via::Process.label()`).
pub const VIA_PROCESS: &str = "process";
/// mpv's IPC source's label (`Via::MpvIpc.label()`).
pub const VIA_IPC: &str = "IPC socket";

/// One source's state for one player: "process · playing", "IPC socket · not running".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerState {
    pub switch: Switch,
    /// The source's label: "IPC socket", "MPRIS", "web interface", "process".
    pub via: &'static str,
    pub line: String,
    pub led: Led,
}

impl From<SourceStatus> for PlayerState {
    fn from(s: SourceStatus) -> Self {
        PlayerState { switch: s.switch, via: s.via.label(), line: s.line, led: s.led }
    }
}

fn live(led: Led) -> bool {
    matches!(led, Led::Playing | Led::Idle)
}

/// What turning on precise detection did (the app's copy of `kotora_detect::mpvconf::Enabled`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precise {
    /// Lines appended to the file.
    Wrote { file: String, text: String },
    /// `mpv.conf` already sets a socket: nothing written. `watched`: Kotora watches that path.
    AlreadySet { file: String, socket: String, watched: bool },
    Failed(String),
}

/// The offer's state in this session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// The banner was closed in this session.
    pub hidden: bool,
    /// Writing `mpv.conf` now.
    pub busy: bool,
    pub done: Option<Precise>,
    /// `mpv.conf` already sets a socket Kotora watches (checked at start and after a write):
    /// mpv only needs a restart, so nothing is offered.
    pub conf_watched: bool,
}

/// `/home/x/.config/mpv/mpv.conf` → `~/.config/mpv/mpv.conf` (shown, never saved).
pub(crate) fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && path.starts_with(&h) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

impl State {
    /// mpv is seen by its process only (no IPC socket answers) and `mpv.conf` doesn't set a
    /// socket Kotora watches yet: precise detection is worth offering.
    pub fn mpv_process_only(&self) -> bool {
        self.mpv_seen_by_process() && !self.precise.conf_watched
    }

    /// mpv is seen by its process only: no IPC socket answers.
    fn mpv_seen_by_process(&self) -> bool {
        self.prefs.players.mpv
            && self.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_PROCESS && live(p.led))
            && !self.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_IPC && live(p.led))
    }

    /// The banner shows: the offer (once ever), or its answer until closed.
    pub fn precise_banner(&self) -> bool {
        if self.precise.hidden {
            return false;
        }
        self.precise.done.is_some() || self.precise.busy || (!self.prefs.precise_answered && self.mpv_process_only())
    }

    /// "Turn on precise detection": write `mpv.conf` (on a short thread), and remember the
    /// answer.
    pub fn enable_precise(&mut self) {
        if self.precise.busy {
            return;
        }
        self.remember_precise();
        self.precise.hidden = false;
        let Some(h) = self.handle.clone() else { return };
        self.precise.busy = true;
        let _ = std::thread::Builder::new().name("kotora-mpv-conf".into()).spawn(move || {
            let r = write_mpv_conf();
            h.update_if(move |s, _| s.precise_written(r));
        });
    }

    pub fn precise_written(&mut self, r: Precise) -> bool {
        self.precise.busy = false;
        if matches!(r, Precise::Wrote { .. } | Precise::AlreadySet { watched: true, .. }) {
            self.precise.conf_watched = true;
        }
        self.precise.done = Some(r);
        true
    }

    /// "No thanks": the banner goes and never comes back (Settings keeps the button).
    pub fn decline_precise(&mut self) {
        self.remember_precise();
        self.precise.hidden = true;
    }

    /// Close the answer.
    pub fn close_precise(&mut self) {
        self.precise.hidden = true;
    }

    fn remember_precise(&mut self) {
        if self.prefs.precise_answered {
            return;
        }
        self.prefs.precise_answered = true;
        if self.handle.is_none() {
            return;
        }
        std::thread::spawn(|| {
            let _ = config::write_setting(&paths::config_file(), "notices", "mpv_precise", "true");
        });
    }
}

/// The socket paths Kotora watches for mpv (`[players] mpv_sockets`, else the default).
pub(crate) fn watched_sockets() -> Vec<std::path::PathBuf> {
    let c = config::Config::load(&paths::config_file()).unwrap_or_default();
    crate::detect::settings(&c).mpv_sockets
}

/// At start (never headless; on a short thread): does `mpv.conf` already set a socket Kotora
/// watches? Then mpv only needs a restart and no offer is shown.
pub fn check_conf(h: crate::model::Handle) {
    let _ = std::thread::Builder::new().name("kotora-mpv-conf".into()).spawn(move || {
        let watched = kotora_detect::mpvconf::check().is_ok_and(|c| c.watched(&watched_sockets()));
        h.update_if(move |s, _| {
            let changed = s.precise.conf_watched != watched;
            s.precise.conf_watched = watched;
            changed
        });
    });
}

/// Appends mpv's IPC socket (the first path Kotora watches) to the user's `mpv.conf`. Runs on a
/// short thread.
fn write_mpv_conf() -> Precise {
    use kotora_detect::mpvconf::{self, Enabled};
    let sockets = watched_sockets();
    let Some(socket) = sockets.first() else {
        return Precise::Failed("No mpv socket path is set: add one to [players] mpv_sockets in config.toml.".into());
    };
    match mpvconf::enable(socket) {
        Ok(Enabled::Wrote { file, text }) => Precise::Wrote { file: file.display().to_string(), text },
        Ok(Enabled::AlreadySet { file, socket }) => {
            let watched = sockets.contains(&socket);
            Precise::AlreadySet { file: file.display().to_string(), socket: socket.display().to_string(), watched }
        }
        Err(e) => Precise::Failed(format!("Can't write mpv.conf: {e}")),
    }
}

/// The banner on top of the pages (below the Now Playing banner, never with it).
pub fn banner(s: &State) -> El {
    let info = PAL.st_completed;
    let base = row()
        .key("precise")
        .mx(theme::PAGE_PAD)
        .mt(14.0)
        .px(14.0)
        .py(12.0)
        .gap(14.0)
        .center_y()
        .radius(theme::R_LG)
        .fill(info.alpha(0.08))
        .border(1.0, info.alpha(0.3));
    let mono = TextStyle::new(12.0, 400.0).mono();
    let words = |head: &str, body: El| col().grow(1.0).min_w(0.0).gap(4.0).child(text(head.to_string()).font(theme::BODY_MED).color(PAL.text)).child(body);
    match &s.precise.done {
        Some(Precise::Wrote { file, text: lines }) => base
            .child(icon(&icons::CHECK, 18.0).color(PAL.ok()))
            .child(words(
                "Precise detection is on from mpv's next start",
                col()
                    .gap(4.0)
                    .child(text(format!("Added to {}:", tilde(file))).font(theme::LABEL).color(PAL.dim))
                    .child(text(lines.trim_end().to_string()).key("precise/written").font(mono).color(PAL.text).wrap()),
            ))
            .child(button("precise/ok", "OK", false, Some(State::close_precise))),
        Some(Precise::AlreadySet { file, socket, watched }) => base
            .child(icon(&icons::CHECK, 18.0).color(PAL.ok()))
            .child(words(
                "mpv.conf already sets an IPC socket",
                text(if *watched {
                    format!("{} has input-ipc-server={}, which Kotora watches. Nothing was changed. Restart mpv to use it.", tilde(file), tilde(socket))
                } else {
                    format!("{} has input-ipc-server={}, which Kotora doesn't watch. Nothing was changed. Add that path to [players] mpv_sockets in config.toml, then restart Kotora and mpv.", tilde(file), tilde(socket))
                })
                    .font(theme::LABEL)
                    .color(PAL.dim)
                    .wrap(),
            ))
            .child(button("precise/ok", "OK", false, Some(State::close_precise))),
        Some(Precise::Failed(e)) => base
            .border(1.0, PAL.status(kotora_core::Status::Dropped).alpha(0.3))
            .fill(PAL.status(kotora_core::Status::Dropped).alpha(0.08))
            .child(icon(&icons::ALERT, 18.0).color(PAL.status(kotora_core::Status::Dropped)))
            .child(words("Precise detection is not on", text(e.clone()).key("precise/error").font(theme::LABEL).color(PAL.dim).wrap()))
            .child(button("precise/ok", "OK", false, Some(State::close_precise))),
        None => base
            .child(el().size(8.0, 8.0).radius(4.0).fill(info))
            .child(words(
                "mpv is seen by its open file only",
                text("Kotora knows the episode but not how far in you are, so it counts the time the file is open. Precise detection adds one line to your mpv.conf (input-ipc-server); it works from mpv's next start.")
                    .font(theme::LABEL)
                    .color(PAL.dim)
                    .wrap(),
            ))
            .child(if s.precise.busy {
                button("precise/on", "Turning on\u{2026}", true, None)
            } else {
                button("precise/on", "Turn on precise detection", true, Some(State::enable_precise))
            })
            .child(ghost("precise/no", "No thanks", State::decline_precise)),
    }
}

/// Settings → Players, under mpv: what precise detection would add, with Turn on.
pub fn settings_hint(s: &State) -> Option<El> {
    if !s.mpv_seen_by_process() {
        return None;
    }
    let (line, action) = match s.precise.conf_watched {
        true => ("mpv.conf sets the IPC socket Kotora watches; restart mpv to use it.".to_string(), None),
        false => ("Seen by its open file only: no position. Precise detection adds its IPC socket to mpv.conf.".to_string(), Some(small_button("set/precise-on", "Turn on", false, Some(State::enable_precise)))),
    };
    let mut r = row()
        .key("set/precise")
        .gap(10.0)
        .ml(19.0)
        .mb(4.0)
        .px(10.0)
        .py(8.0)
        .center_y()
        .radius(theme::R_MD)
        .fill(PAL.st_completed.alpha(0.08))
        .border(1.0, PAL.st_completed.alpha(0.25))
        .child(text(line).font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0));
    if let Some(a) = action {
        r = r.child(a);
    }
    Some(r)
}

/// Fake players for the scenes: mpv seen by its process only, playing.
pub fn fake_process_players() -> Vec<PlayerState> {
    vec![
        PlayerState { switch: Switch::Mpv, via: VIA_PROCESS, line: "playing".into(), led: Led::Playing },
        PlayerState { switch: Switch::Mpv, via: VIA_IPC, line: "$XDG_RUNTIME_DIR/mpv.sock: not running".into(), led: Led::Off },
        PlayerState { switch: Switch::Vlc, via: VIA_PROCESS, line: "running, nothing open".into(), led: Led::Idle },
        PlayerState { switch: Switch::Vlc, via: "MPRIS", line: "running, nothing playing".into(), led: Led::Idle },
        PlayerState { switch: Switch::MpcQt, via: "MPRIS", line: "not running".into(), led: Led::Off },
        PlayerState { switch: Switch::OtherMpris, via: VIA_PROCESS, line: "Celluloid, nothing open".into(), led: Led::Idle },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::State;

    fn process_only() -> State {
        let mut s = State::fake();
        s.now = None;
        s.players = fake_process_players();
        s
    }

    #[test]
    fn the_offer_shows_once_for_mpv_seen_by_its_process_only() {
        let mut s = process_only();
        assert!(s.mpv_process_only() && s.precise_banner());
        s.decline_precise();
        assert!(!s.precise_banner() && s.prefs.precise_answered);
        // A new session with the answer remembered: no banner, but Settings still offers it.
        let mut s = process_only();
        s.prefs.precise_answered = true;
        assert!(!s.precise_banner() && settings_hint(&s).is_some());
        // The socket answers: nothing to offer.
        let mut s = process_only();
        s.players.push(PlayerState { switch: Switch::Mpv, via: VIA_IPC, line: "playing".into(), led: Led::Playing });
        assert!(!s.mpv_process_only() && !s.precise_banner());
        // mpv switched off in Settings: nothing either.
        let mut s = process_only();
        s.prefs.players.mpv = false;
        assert!(!s.precise_banner());
    }

    #[test]
    fn turning_it_on_shows_what_was_written_until_closed() {
        let mut s = process_only();
        s.enable_precise();
        assert!(s.prefs.precise_answered);
        s.precise_written(Precise::Wrote { file: "/tmp/mpv/mpv.conf".into(), text: "input-ipc-server=/run/user/1000/mpv.sock\n".into() });
        assert!(s.precise_banner() && s.precise.conf_watched);
        s.close_precise();
        assert!(!s.precise_banner());
        // Settings now says to restart mpv, with no button.
        assert!(settings_hint(&s).is_some() && !s.mpv_process_only());
    }

    #[test]
    fn no_offer_when_mpv_conf_already_sets_a_watched_socket() {
        let mut s = process_only();
        s.precise.conf_watched = true;
        assert!(!s.mpv_process_only() && !s.precise_banner());
        // A socket Kotora doesn't watch: the answer says so and the offer stays possible.
        let mut s = process_only();
        s.precise_written(Precise::AlreadySet { file: "/tmp/mpv.conf".into(), socket: "/tmp/other.sock".into(), watched: false });
        assert!(!s.precise.conf_watched);
    }

    #[test]
    fn source_states_keep_their_label() {
        let p = PlayerState::from(SourceStatus { switch: Switch::Vlc, via: kotora_detect::Via::Mpris, line: "playing".into(), led: Led::Playing });
        assert_eq!(p.via, "MPRIS");
        assert_eq!(VIA_PROCESS, kotora_detect::Via::Process.label());
        assert_eq!(VIA_IPC, kotora_detect::Via::MpvIpc.label());
        assert_eq!(tilde("/nowhere/x"), "/nowhere/x");
    }
}
