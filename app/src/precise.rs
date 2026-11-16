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
    /// `done` is IINA's answer (Turn on in the Now Playing prompt), not mpv's.
    pub for_iina: bool,
    /// Turn on for IINA was clicked in the prompt: its answer goes into `done`.
    pub iina_pending: bool,
    /// Windows: the player `done` is about ("mpv.net"), when it isn't plain mpv.
    pub who: Option<String>,
    /// Windows: what else Turn on did (the file is new, where the backup is).
    pub note: Option<String>,
}

/// `/home/x/.config/mpv/mpv.conf` → `~/.config/mpv/mpv.conf` (shown, never saved).
pub(crate) fn tilde(path: &str) -> String {
    match kotora_core::paths::home_dir().map(|h| h.to_string_lossy().into_owned()) {
        Some(h) if !h.is_empty() && path.starts_with(&h) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

impl State {
    /// mpv is seen by its process only (no IPC socket answers) and `mpv.conf` doesn't set a
    /// socket Kotora watches yet: precise detection is worth offering.
    pub fn mpv_process_only(&self) -> bool {
        self.mpv_seen_by_process() && !self.precise.conf_watched
    }

    /// Windows: the process scan sees an mpv or mpv.net.
    #[cfg(windows)]
    pub fn mpv_process_live(&self) -> bool {
        self.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_PROCESS && live(p.led))
    }

    /// mpv is seen by its process only: no IPC socket answers.
    fn mpv_seen_by_process(&self) -> bool {
        self.prefs.players.mpv
            && self.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_PROCESS && live(p.led))
            && !self.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_IPC && live(p.led))
    }

    /// The banner shows: the offer (once ever), or its answer until closed. On Now Playing the
    /// prompt for the followed player ([`State::precise_prompt`]) asks instead.
    pub fn precise_banner(&self) -> bool {
        if self.precise.hidden {
            return false;
        }
        let prompted = self.page == crate::model::Page::NowPlaying && self.precise_prompt().is_some();
        self.precise.done.is_some() || self.precise.busy || (!self.prefs.precise_answered && self.mpv_process_only() && !prompted)
    }

    /// Now Playing's prompt: the followed player is seen by its open file only, precise
    /// detection is one click away for it (mpv; IINA when it reads a config folder's mpv.conf),
    /// and the prompt wasn't answered for that player.
    pub fn precise_prompt(&self) -> Option<Switch> {
        let n = self.now.as_ref()?;
        if n.via != VIA_PROCESS {
            return None;
        }
        match n.switch {
            Switch::Mpv if self.prefs.players.mpv && !self.prefs.precise_answered && !self.precise.conf_watched && !self.precise.busy => Some(Switch::Mpv),
            #[cfg(target_os = "macos")]
            Switch::Iina
                if self.prefs.players.iina
                    && !self.prefs.iina_precise_answered
                    && !self.iina.busy
                    && self.iina.done.is_none()
                    && matches!(self.iina.setup, Some(kotora_detect::iinaconf::Setup::ConfDir { .. })) =>
            {
                Some(Switch::Iina)
            }
            _ => None,
        }
    }

    /// The prompt's Turn on: the same action as Settings → Players. The answer shows in the
    /// banner on top (what was written, or why not).
    pub fn precise_prompt_on(&mut self) {
        match self.precise_prompt() {
            Some(Switch::Mpv) => self.enable_precise(),
            #[cfg(target_os = "macos")]
            Some(Switch::Iina) => {
                self.remember_iina_precise();
                self.precise.iina_pending = true;
                self.enable_iina();
            }
            _ => {}
        }
    }

    /// The prompt's Not now: remembered for that player, so it never comes back for it
    /// (Settings → Players keeps Turn on).
    pub fn precise_prompt_not_now(&mut self) {
        match self.now.as_ref().map(|n| n.switch) {
            Some(Switch::Mpv) => self.remember_precise(),
            Some(Switch::Iina) => self.remember_iina_precise(),
            _ => {}
        }
    }

    /// IINA answered Turn on from the prompt: the answer shows in the banner on top.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn iina_answer(&mut self, r: Precise) {
        if !self.precise.iina_pending {
            return;
        }
        self.precise.iina_pending = false;
        self.precise.for_iina = true;
        self.precise.hidden = false;
        self.precise.done = Some(r);
    }

    fn remember_iina_precise(&mut self) {
        if self.prefs.iina_precise_answered {
            return;
        }
        self.prefs.iina_precise_answered = true;
        if self.handle.is_none() {
            return;
        }
        std::thread::spawn(|| {
            let _ = config::write_setting(&paths::config_file(), "notices", "iina_precise", "true");
        });
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
        #[cfg(windows)]
        let detector = self.glue.as_ref().map(|g| g.detector());
        let _ = std::thread::Builder::new().name("kotora-mpv-conf".into()).spawn(move || {
            #[cfg(not(windows))]
            let (r, who, note) = (write_mpv_conf(), None, None);
            #[cfg(windows)]
            let (r, who, note) = write_mpv_conf_win(detector.as_deref());
            h.update_if(move |s, _| {
                s.precise.who = who;
                s.precise.note = note;
                s.precise_written(r)
            });
        });
    }

    pub fn precise_written(&mut self, r: Precise) -> bool {
        self.precise.busy = false;
        self.precise.for_iina = false;
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
        if self.precise.for_iina {
            // IINA's answer was shown once; mpv's offer may still come.
            self.precise.done = None;
            self.precise.for_iina = false;
            self.precise.hidden = false;
        }
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
#[cfg(not(windows))]
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

/// Windows, at start and when an mpv or mpv.net process appears (never headless; on a short
/// thread): the `mpv.conf` of each such player that runs (else of the config folders that
/// exist) is read. A pipe one of them sets is watched from now on, whatever its name, and when
/// every file sets one nothing is offered: the player only needs a restart.
#[cfg(windows)]
pub fn check_conf_win(h: crate::model::Handle, detector: std::sync::Arc<kotora_detect::Detector>) {
    use kotora_detect::mpvconf::{WinEnv, win_check};
    let _ = std::thread::Builder::new().name("kotora-mpv-conf".into()).spawn(move || {
        let targets = WinEnv::current().targets(&running_mpvs());
        let pipes: Vec<Option<std::path::PathBuf>> = targets.iter().map(|t| win_check(&t.file).ok().flatten()).collect();
        for pipe in pipes.iter().flatten() {
            detector.watch_socket(pipe);
        }
        let watched = !pipes.is_empty() && pipes.iter().all(Option::is_some);
        h.update_if(move |s, _| {
            let changed = s.precise.conf_watched != watched;
            s.precise.conf_watched = watched;
            changed
        });
    });
}

/// The mpv and mpv.net processes running now: (mpv.net?, program path).
#[cfg(windows)]
fn running_mpvs() -> Vec<(bool, std::path::PathBuf)> {
    kotora_detect::winproc::players().iter().filter_map(|p| Some((p.family.mpv_net()?, std::path::PathBuf::from(&p.exe)))).collect()
}

/// Windows: adds mpv's IPC pipe (the first name Kotora watches) to the `mpv.conf` of the
/// players that run, or of the config folders that exist: the file is created when missing and
/// backed up when not. Runs on a short thread. With the answer: the player's name and a note.
#[cfg(windows)]
fn write_mpv_conf_win(detector: Option<&kotora_detect::Detector>) -> (Precise, Option<String>, Option<String>) {
    use kotora_detect::mpvconf::{WinEnabled, WinEnv, win_enable};
    let sockets = watched_sockets();
    let Some(pipe) = sockets.first() else {
        return (Precise::Failed("No mpv pipe name is set: add one to [players] mpv_sockets in config.toml.".into()), None, None);
    };
    let results: Vec<_> = WinEnv::current().targets(&running_mpvs()).into_iter().map(|t| (t.player, win_enable(&t.file, pipe).map_err(|e| format!("Can't write {}: {e}", t.file.display())))).collect();
    // A pipe the user set under another name is watched from now on.
    for (_, r) in &results {
        if let (Ok(WinEnabled::AlreadySet { pipe, .. }), Some(d)) = (r, detector) {
            d.watch_socket(pipe);
        }
    }
    win_answer(results)
}

/// Puts the answers for one or two `mpv.conf` files into one: what was written wins over
/// "already set", which wins over an error.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn win_answer(results: Vec<(&'static str, Result<kotora_detect::mpvconf::WinEnabled, String>)>) -> (Precise, Option<String>, Option<String>) {
    use kotora_detect::mpvconf::WinEnabled;
    let both = |v: Vec<String>| v.join(" and ");
    let names = |keep: &dyn Fn(&Result<WinEnabled, String>) -> bool| {
        let mut v: Vec<String> = Vec::new();
        for (p, r) in &results {
            if keep(r) && !v.iter().any(|x| x == p) {
                v.push(p.to_string());
            }
        }
        both(v)
    };
    let wrote: Vec<_> = results.iter().filter_map(|(_, r)| if let Ok(WinEnabled::Wrote { file, text, created, backup }) = r { Some((file, text, *created, backup)) } else { None }).collect();
    if let Some((_, text, _, _)) = wrote.first() {
        let mut notes = Vec::new();
        for (file, _, created, backup) in &wrote {
            let name = tilde(&file.display().to_string());
            match backup {
                Some(b) => notes.push(format!("{name} was copied to {} first.", b.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                None if *created => notes.push(format!("{name} didn\u{2019}t exist: Kotora made it.")),
                None => {}
            }
        }
        let who = names(&|r| matches!(r, Ok(WinEnabled::Wrote { .. })));
        notes.push(format!("Close {who} and open it again once."));
        let file = both(wrote.iter().map(|(f, ..)| f.display().to_string()).collect());
        return (Precise::Wrote { file, text: text.replace("\r\n", "\n") }, Some(who), Some(notes.join(" ")));
    }
    let set: Vec<_> = results.iter().filter_map(|(_, r)| if let Ok(WinEnabled::AlreadySet { file, pipe }) = r { Some((file, pipe)) } else { None }).collect();
    if let Some((file, pipe)) = set.first() {
        let who = names(&|r| matches!(r, Ok(WinEnabled::AlreadySet { .. })));
        return (Precise::AlreadySet { file: file.display().to_string(), socket: pipe.display().to_string(), watched: true }, Some(who), None);
    }
    let error = results.into_iter().find_map(|(_, r)| r.err()).unwrap_or_else(|| "No mpv.conf was found: APPDATA is not set.".into());
    (Precise::Failed(error), None, None)
}

/// Appends mpv's IPC socket (the first path Kotora watches) to the user's `mpv.conf`. Runs on a
/// short thread.
#[cfg(not(windows))]
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
    let who = if s.precise.for_iina { "IINA" } else { s.precise.who.as_deref().unwrap_or("mpv") };
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
                &format!("Precise detection is on from {who}'s next start"),
                {
                    let c = col()
                        .gap(4.0)
                        .child(text(format!("Added to {}:", tilde(file))).font(theme::LABEL).color(PAL.dim))
                        .child(text(lines.trim_end().to_string()).key("precise/written").font(mono).color(PAL.text).wrap());
                    match s.precise.note.as_ref().filter(|_| !s.precise.for_iina) {
                        Some(note) => c.child(text(note.clone()).key("precise/note").font(theme::LABEL).color(PAL.dim).wrap()),
                        None => c,
                    }
                },
            ))
            .child(button("precise/ok", "OK", false, Some(State::close_precise))),
        Some(Precise::AlreadySet { file, socket, watched }) => base
            .child(icon(&icons::CHECK, 18.0).color(PAL.ok()))
            .child(words(
                "mpv.conf already sets an IPC socket",
                text(if *watched {
                    format!("{} has input-ipc-server={}, which Kotora watches. Nothing was changed. Restart {who} to use it.", tilde(file), tilde(socket))
                } else {
                    format!("{} has input-ipc-server={}, which Kotora doesn't watch. Nothing was changed. Add that path to [players] mpv_sockets in config.toml, then restart Kotora and {who}.", tilde(file), tilde(socket))
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
                if cfg!(windows) { "mpv is seen by its window only" } else { "mpv is seen by its open file only" },
                text(if cfg!(windows) {
                    "Kotora knows the episode but not how far in you are, so it counts the time the file is open. Precise detection adds one line to the player\u{2019}s mpv.conf (input-ipc-server): the file is made if it is missing, and copied first if it exists. Restart the player once afterwards."
                } else {
                    "Kotora knows the episode but not how far in you are, so it counts the time the file is open. Precise detection adds one line to your mpv.conf (input-ipc-server); it works from mpv's next start."
                })
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

/// "Turn on precise detection for IINA: Kotora counts at 80% instead of after 15 min".
pub fn prompt_line(s: &State, sw: Switch) -> String {
    let minutes = s.prefs.players.process_minutes.clamp(1, 180);
    format!("Turn on precise detection for {}: Kotora counts at {}% instead of after {minutes} min", sw.label(), s.prefs.threshold)
}

/// Now Playing's prompt for the followed player (on top of the page, under the episode banner):
/// what precise detection changes, with Turn on and Not now.
pub fn prompt(s: &State) -> Option<El> {
    let sw = s.precise_prompt()?;
    let player = sw.label();
    let what = match sw {
        #[cfg(target_os = "macos")]
        Switch::Iina => match &s.iina.setup {
            Some(kotora_detect::iinaconf::Setup::ConfDir { file }) => format!("Adds one line to {} (input-ipc-server). It works from IINA's next start.", tilde(&file.display().to_string())),
            _ => "Adds one line to IINA's mpv.conf (input-ipc-server). It works from IINA's next start.".to_string(),
        },
        _ if cfg!(windows) => "Adds one line to its mpv.conf (input-ipc-server): the file is made if it is missing, and copied first if it exists. Restart the player once afterwards.".to_string(),
        _ => "Adds one line to your mpv.conf (input-ipc-server). It works from mpv's next start.".to_string(),
    };
    // "mpv.net" on Windows, where the mpv switch covers both.
    let player = match (&s.now, sw) {
        (Some(n), Switch::Mpv) if cfg!(windows) => n.player.as_str(),
        _ => player,
    };
    let seen_by = if cfg!(windows) { "window" } else { "open file" };
    let info = PAL.st_completed;
    Some(
        row()
            .key("precise-prompt")
            .mx(theme::PAGE_PAD)
            .mt(14.0)
            .px(14.0)
            .py(12.0)
            .gap(14.0)
            .center_y()
            .radius(theme::R_LG)
            .fill(info.alpha(0.10))
            .border(1.0, info.alpha(0.45))
            .child(icon(&icons::PLAY, 18.0).color(info))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(4.0)
                    .child(
                        text(prompt_line(s, sw))
                            .key("precise-prompt/text")
                            .font(theme::BODY_MED)
                            .color(PAL.text)
                            .wrap(),
                    )
                    .child(text(format!("{player} is seen by its {seen_by} only, so Kotora can't tell how far in you are. {what}")).font(theme::LABEL).color(PAL.dim).wrap()),
            )
            .child(button("precise-prompt/on", "Turn on", true, Some(State::precise_prompt_on)))
            .child(button("precise-prompt/not-now", "Not now", false, Some(State::precise_prompt_not_now))),
    )
}

/// Settings → Players, under mpv: what precise detection would add, with Turn on.
pub fn settings_hint(s: &State) -> Option<El> {
    settings_hint_on(s, cfg!(windows))
}

/// `windows`: the Windows wording, and Turn on also while no mpv runs (there the file may not
/// exist yet, and nothing else says how to make it).
pub(crate) fn settings_hint_on(s: &State, windows: bool) -> Option<El> {
    let seen = s.mpv_seen_by_process();
    let ipc_live = s.players.iter().any(|p| p.switch == Switch::Mpv && p.via == VIA_IPC && live(p.led));
    if !seen && !(windows && s.prefs.players.mpv && !ipc_live && !s.precise.conf_watched) {
        return None;
    }
    let turn_on = || Some(small_button("set/precise-on", if s.precise.busy { "Turning on\u{2026}" } else { "Turn on" }, false, if s.precise.busy { None } else { Some(State::enable_precise) }));
    let (line, action) = match (s.precise.conf_watched, windows) {
        (true, true) => ("mpv.conf sets the IPC pipe Kotora watches: close the player and open it again to use it.".to_string(), None),
        (true, false) => ("mpv.conf sets the IPC socket Kotora watches; restart mpv to use it.".to_string(), None),
        (false, true) if seen => ("Seen by its window only: no position. Precise detection adds one line to the player\u{2019}s mpv.conf (made if missing, copied first if it exists).".to_string(), turn_on()),
        (false, true) => ("Precise detection is off: mpv and mpv.net are seen by their window only, with no position. Turn on adds one line to mpv.conf (made if missing, copied first if it exists).".to_string(), turn_on()),
        (false, false) => ("Seen by its open file only: no position. Precise detection adds its IPC socket to mpv.conf.".to_string(), Some(small_button("set/precise-on", "Turn on", false, Some(State::enable_precise)))),
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

    /// The followed player (mpv) seen by its open file only, on Now Playing.
    fn followed_by_process(sw: Switch) -> State {
        let mut s = State::fake();
        s.players = fake_process_players();
        if let Some(n) = &mut s.now {
            n.via = VIA_PROCESS;
            n.switch = sw;
        }
        s.page = crate::model::Page::NowPlaying;
        s
    }

    #[test]
    fn the_prompt_asks_for_the_followed_player_until_answered() {
        let s = followed_by_process(Switch::Mpv);
        assert_eq!(s.precise_prompt(), Some(Switch::Mpv));
        assert!(!s.precise_banner(), "the page's prompt instead of the offer on top");
        assert!(prompt(&s).is_some());
        // By IPC: nothing to turn on.
        let mut s = followed_by_process(Switch::Mpv);
        s.now.as_mut().unwrap().via = VIA_IPC;
        assert_eq!(s.precise_prompt(), None);
        // mpv.conf already sets the socket: only a restart is needed.
        let mut s = followed_by_process(Switch::Mpv);
        s.precise.conf_watched = true;
        assert_eq!(s.precise_prompt(), None);
        // VLC has no precise mode to turn on.
        assert_eq!(followed_by_process(Switch::Vlc).precise_prompt(), None);
    }

    #[test]
    fn not_now_is_remembered_per_player() {
        let mut s = followed_by_process(Switch::Mpv);
        s.precise_prompt_not_now();
        assert!(s.prefs.precise_answered && !s.prefs.iina_precise_answered);
        assert_eq!(s.precise_prompt(), None);
        // IINA's is its own answer.
        let mut s = followed_by_process(Switch::Iina);
        s.precise_prompt_not_now();
        assert!(s.prefs.iina_precise_answered && !s.prefs.precise_answered);
    }

    #[test]
    fn turn_on_is_settings_turn_on() {
        let mut s = followed_by_process(Switch::Mpv);
        s.precise_prompt_on();
        assert!(s.prefs.precise_answered, "answered, as with Settings' button");
        s.precise_written(Precise::Wrote { file: "/tmp/mpv/mpv.conf".into(), text: "input-ipc-server=/tmp/mpv.sock\n".into() });
        assert!(s.precise_banner() && s.precise.conf_watched && s.precise_prompt().is_none());
    }

    #[test]
    fn the_prompt_says_threshold_and_minutes() {
        let mut s = followed_by_process(Switch::Mpv);
        s.prefs.threshold = 90;
        s.prefs.players.process_minutes = 20;
        assert_eq!(prompt_line(&s, Switch::Mpv), "Turn on precise detection for mpv: Kotora counts at 90% instead of after 20 min");
        assert_eq!(prompt_line(&s, Switch::Iina), "Turn on precise detection for IINA: Kotora counts at 90% instead of after 20 min");
    }

    #[test]
    fn windows_answers_say_which_file_and_what_else_was_done() {
        use kotora_detect::mpvconf::WinEnabled;
        use std::path::PathBuf;
        let text = "# Added by Kotora\r\ninput-ipc-server=\\\\.\\pipe\\mpvsocket\r\n".to_string();
        // mpv.net fresh from its installer: the file is made.
        let made = WinEnabled::Wrote { file: PathBuf::from("/a/mpv.net/mpv.conf"), text: text.clone(), created: true, backup: None };
        let (r, who, note) = win_answer(vec![("mpv.net", Ok(made.clone()))]);
        assert_eq!(r, Precise::Wrote { file: "/a/mpv.net/mpv.conf".into(), text: text.replace("\r\n", "\n") });
        assert_eq!(who.as_deref(), Some("mpv.net"));
        assert_eq!(note.as_deref(), Some("/a/mpv.net/mpv.conf didn\u{2019}t exist: Kotora made it. Close mpv.net and open it again once."));
        // An existing file: where its copy is.
        let kept = WinEnabled::Wrote { file: PathBuf::from("/a/mpv/mpv.conf"), text: text.clone(), created: false, backup: Some(PathBuf::from("/a/mpv/mpv.conf.kotora-backup")) };
        let (_, who, note) = win_answer(vec![("mpv", Ok(kept.clone()))]);
        assert_eq!(who.as_deref(), Some("mpv"));
        assert_eq!(note.as_deref(), Some("/a/mpv/mpv.conf was copied to mpv.conf.kotora-backup first. Close mpv and open it again once."));
        // Two players, one already set: what was written is what is shown.
        let set = WinEnabled::AlreadySet { file: PathBuf::from("/a/mpv/mpv.conf"), pipe: PathBuf::from(r"\\.\pipe\mine") };
        let (r, who, _) = win_answer(vec![("mpv", Ok(set.clone())), ("mpv.net", Ok(made))]);
        assert!(matches!(r, Precise::Wrote { .. }));
        assert_eq!(who.as_deref(), Some("mpv.net"));
        // Only "already set": watched from now on, so only a restart is asked.
        let (r, who, note) = win_answer(vec![("mpv", Ok(set))]);
        assert_eq!(r, Precise::AlreadySet { file: "/a/mpv/mpv.conf".into(), socket: r"\\.\pipe\mine".into(), watched: true });
        assert_eq!((who.as_deref(), note), (Some("mpv"), None));
        // An error alone is the answer; beside a written file it isn't.
        let (r, ..) = win_answer(vec![("mpv", Err("Can't write X: denied".into()))]);
        assert_eq!(r, Precise::Failed("Can't write X: denied".into()));
        let (r, ..) = win_answer(vec![("mpv", Err("Can't write X: denied".into())), ("mpv.net", Ok(kept))]);
        assert!(matches!(r, Precise::Wrote { .. }));
    }

    #[test]
    fn windows_settings_offer_turn_on_before_mpv_ever_ran() {
        let mut s = State::fake();
        s.now = None;
        s.players = Vec::new();
        assert!(settings_hint_on(&s, false).is_none(), "elsewhere mpv is found first");
        assert!(settings_hint_on(&s, true).is_some());
        // mpv.conf already sets the pipe: nothing to turn on while no mpv runs.
        s.precise.conf_watched = true;
        assert!(settings_hint_on(&s, true).is_none());
        // The pipe answers: nothing to say.
        s.precise.conf_watched = false;
        s.players = vec![PlayerState { switch: Switch::Mpv, via: VIA_IPC, line: "playing".into(), led: Led::Playing }];
        assert!(settings_hint_on(&s, true).is_none());
        // Switched off: nothing.
        s.players = Vec::new();
        s.prefs.players.mpv = false;
        assert!(settings_hint_on(&s, true).is_none());
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
