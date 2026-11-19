//! Windows: the set-ups of Settings → Detection that the other systems don't need.
//!
//! - **MPC-HC / MPC-BE**: seen with no setup by their window title, without a position. Their
//!   web interface gives the file's path, the position and the pause, but it is off in the
//!   player. Turn on sets the player's own setting in the registry (web interface on, this
//!   computer only), when the user clicks for it and while the player is closed (the player
//!   writes its settings back when it closes). `kotora_detect::mpc` has the details.
//! - **VLC**: the password of its web interface, typed here instead of into `config.toml`.
//!
//! The state and the views are plain, so the scenes show them on every system. Only the
//! actions are Windows code, and none runs headless.

use kotora_detect::{Led, Switch};
use rune::{El, TextEdit, TextStyle, Tx, row, text, text_input};

use crate::model::State;
use crate::precise::VIA_PROCESS;
use crate::theme::{self, PAL};
use crate::ui::small_button;

/// The web interface's label (`kotora_detect::Via::MpcHttp.label()`).
pub const VIA_WEB: &str = "web interface";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WinSetup {
    /// Changing MPC's setting now.
    pub mpc_busy: bool,
    /// What Turn on did, or why it didn't.
    pub mpc_done: Option<Result<String, String>>,
    /// The VLC password field.
    pub vlc_draft: TextEdit,
    /// The password was saved in this session.
    pub vlc_saved: bool,
}

fn live(led: Led) -> bool {
    matches!(led, Led::Playing | Led::Idle)
}

impl State {
    /// MPC-HC or MPC-BE is seen by its window only: its web interface doesn't answer.
    pub fn mpc_seen_by_window(&self) -> bool {
        self.prefs.players.mpc
            && self
                .players
                .iter()
                .any(|p| p.switch == Switch::Mpc && p.via == VIA_PROCESS && live(p.led))
            && !self.mpc_web_live()
    }

    fn mpc_web_live(&self) -> bool {
        self.players
            .iter()
            .any(|p| p.switch == Switch::Mpc && p.via == VIA_WEB && live(p.led))
    }

    /// Turn on MPC's web interface (on a short thread; nothing headless).
    pub fn enable_mpc_web(&mut self) {
        #[cfg(windows)]
        if let (false, Some(h), false) = (
            self.winsetup.mpc_busy,
            self.handle.clone(),
            rune::is_headless(),
        ) {
            let detector = self.glue.as_ref().map(|g| g.detector());
            self.winsetup.mpc_busy = true;
            let _ = std::thread::Builder::new()
                .name("kotora-mpc-web".into())
                .spawn(move || {
                    let r = turn_on_mpc_web(detector.as_deref());
                    h.update_if(move |s, _| s.mpc_web_answer(r));
                });
        }
    }

    pub fn mpc_web_answer(&mut self, r: Result<String, String>) -> bool {
        self.winsetup.mpc_busy = false;
        self.winsetup.mpc_done = Some(r);
        true
    }

    /// Saves the VLC password field to `config.toml` and gives it to the running detection.
    pub fn save_vlc_password(&mut self) {
        let password = self.winsetup.vlc_draft.text.trim().to_string();
        if password.is_empty() {
            return;
        }
        self.winsetup.vlc_draft = TextEdit::default();
        self.winsetup.vlc_saved = true;
        self.prefs.vlc_set_up = true;
        crate::detect::save_setting(
            self,
            "vlc",
            "password",
            kotora_core::config::quote(&password),
        );
    }
}

/// Sets "web interface on, this computer only" for the MPC players that keep their settings
/// in the registry. `Err`: what the user has to do instead.
#[cfg(windows)]
fn turn_on_mpc_web(detector: Option<&kotora_detect::Detector>) -> Result<String, String> {
    use kotora_core::{config, paths};
    use kotora_detect::mpc::reg;
    if let Some(p) = kotora_detect::winproc::players()
        .iter()
        .find(|p| p.family.is_mpc())
    {
        return Err(close_first(p.kind.player));
    }
    let mut done: Vec<(&str, u16)> = Vec::new();
    for place in [reg::MPC_HC, reg::MPC_BE] {
        match reg::enable(&place) {
            Ok(Some(web)) => {
                done.push((place.player, web.port.unwrap_or(kotora_detect::mpc::PORT)))
            }
            Ok(None) => {}
            Err(e) => {
                return Err(format!(
                    "Can\u{2019}t change {}\u{2019}s settings: {e}. {BY_HAND}",
                    place.player
                ));
            }
        }
    }
    let Some((_, port)) = done.first().copied() else {
        return Err(format!(
            "No MPC-HC or MPC-BE settings were found in the registry (a portable copy keeps them in an .ini file). {BY_HAND}"
        ));
    };
    // The port the player listens on, when the user changed it there.
    let c = config::Config::load(&paths::config_file()).unwrap_or_default();
    if c.mpc.port != port
        && done.iter().all(|(_, p)| *p == port)
        && config::write_setting(&paths::config_file(), "mpc", "port", &port.to_string()).is_ok()
        && let (Some(d), Ok(c)) = (detector, config::Config::load(&paths::config_file()))
    {
        d.set(crate::detect::settings(&c));
    }
    Ok(turned_on(
        &done.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        port,
    ))
}

/// How to turn the web interface on in the player itself.
#[cfg_attr(not(windows), allow(dead_code))]
const BY_HAND: &str = "In the player: View \u{2192} Options \u{2192} Player \u{2192} Web Interface, tick \u{201c}Listen on port\u{201d} (13579).";

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn close_first(player: &str) -> String {
    format!(
        "Close {player} first, then click Turn on again: it saves its settings when it closes, which would undo the change."
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn turned_on(players: &[&str], port: u16) -> String {
    format!(
        "The web interface of {} is on from its next start (port {port}, this computer only). Open a video and Kotora shows the position.",
        players.join(" and ")
    )
}

/// Settings → Players, under MPC-HC / MPC-BE: what the web interface adds, with Turn on, and
/// the answer.
pub fn mpc_hint(s: &State) -> Option<El> {
    if s.mpc_web_live() || !(s.mpc_seen_by_window() || s.winsetup.mpc_done.is_some()) {
        return None;
    }
    let button = |label: &str| {
        if s.winsetup.mpc_busy {
            small_button("set/mpc-web-on", "Turning on\u{2026}", false, None)
        } else {
            small_button("set/mpc-web-on", label, false, Some(State::enable_mpc_web))
        }
    };
    let (line, action, tint) = match &s.winsetup.mpc_done {
        Some(Ok(done)) => (done.clone(), None, PAL.ok()),
        Some(Err(why)) => (why.clone(), Some(button("Turn on")), PAL.warn()),
        None => (
            "Seen by its window only: no position. The player\u{2019}s web interface gives the position and the pause; Kotora can turn it on for this computer only (one setting of the player).".to_string(),
            Some(button("Turn on")),
            PAL.st_completed,
        ),
    };
    let mut r = row()
        .key("set/mpc-web")
        .gap(10.0)
        .ml(19.0)
        .mb(4.0)
        .px(10.0)
        .py(8.0)
        .center_y()
        .radius(theme::R_MD)
        .fill(tint.alpha(0.08))
        .border(1.0, tint.alpha(0.25))
        .child(
            text(line)
                .key("set/mpc-web/text")
                .font(theme::LABEL)
                .color(PAL.dim)
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        );
    if let Some(a) = action {
        r = r.child(a);
    }
    Some(r)
}

/// Settings → VLC web interface: the password field with Save.
pub fn vlc_field(s: &State) -> El {
    let w = &s.winsetup;
    let hint = if s.prefs.vlc_set_up {
        "Saved. Type a new one to change it"
    } else {
        "The Lua HTTP password you set in VLC"
    };
    let input = row()
        .key("set/vlc-password/box")
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .max_w(360.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(
            text_input(&w.vlc_draft, TextStyle::new(12.0, 400.0).mono())
                .key("set/vlc-password/input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(
                    if w.vlc_draft.text.is_empty() {
                        hint
                    } else {
                        ""
                    },
                    PAL.faint,
                )
                .on_edit(|s: &mut State| &mut s.winsetup.vlc_draft)
                .on_input_key(crate::ui::field_page_key)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    s.save_vlc_password();
                }),
        );
    let save = if w.vlc_draft.text.trim().is_empty() {
        small_button("set/vlc-password/save", "Save", false, None)
    } else {
        small_button(
            "set/vlc-password/save",
            "Save",
            true,
            Some(State::save_vlc_password),
        )
    };
    row()
        .key("set/vlc-password")
        .gap(8.0)
        .center_y()
        .child(input)
        .child(save.h(32.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precise::PlayerState;

    fn mpc(via: &'static str, led: Led) -> PlayerState {
        PlayerState {
            switch: Switch::Mpc,
            via,
            line: "x".into(),
            led,
        }
    }

    #[test]
    fn the_mpc_hint_shows_while_only_the_window_is_seen() {
        let mut s = State::fake();
        s.players = vec![mpc(VIA_PROCESS, Led::Playing), mpc(VIA_WEB, Led::Off)];
        assert!(s.mpc_seen_by_window() && mpc_hint(&s).is_some());
        // The web interface answers: nothing to turn on.
        s.players = vec![mpc(VIA_PROCESS, Led::Playing), mpc(VIA_WEB, Led::Playing)];
        assert!(!s.mpc_seen_by_window() && mpc_hint(&s).is_none());
        // Not running: nothing, until there is an answer to show.
        s.players = vec![mpc(VIA_PROCESS, Led::Off), mpc(VIA_WEB, Led::Off)];
        assert!(mpc_hint(&s).is_none());
        // "Close it first" stays on screen after the player closed, with the button.
        s.mpc_web_answer(Err(close_first("MPC-HC")));
        assert!(mpc_hint(&s).is_some() && !s.winsetup.mpc_busy);
        s.mpc_web_answer(Ok(turned_on(&["MPC-HC"], 13579)));
        assert!(mpc_hint(&s).is_some());
        // Switched off in Settings: not offered.
        let mut s = State::fake();
        s.players = vec![mpc(VIA_PROCESS, Led::Playing)];
        s.prefs.players.mpc = false;
        assert!(mpc_hint(&s).is_none());
        assert_eq!(VIA_WEB, kotora_detect::Via::MpcHttp.label());
    }

    #[test]
    fn windows_now_playing_spells_out_the_rule() {
        let mut s = State::fake();
        s.prefs.players.process_minutes = 20;
        let mut n = s.now.clone().unwrap();
        n.player = "mpv.net".into();
        n.switch = Switch::Mpv;
        let rule = crate::ui::no_position_rule(&s, &n, true);
        assert!(rule.starts_with("mpv.net shows this file"), "{rule}");
        assert!(
            rule.contains("open for 20 minutes")
                && rule.contains("always asks first")
                && rule.ends_with("turn on precise detection (Settings \u{2192} Detection)."),
            "{rule}"
        );
        n.player = "PotPlayer".into();
        n.switch = Switch::PotPlayer;
        s.prefs.players.process_minutes = 1;
        let rule = crate::ui::no_position_rule(&s, &n, true);
        assert!(
            rule.contains("open for 1 minute,") && rule.ends_with("always asks first."),
            "{rule}"
        );
        // The other systems keep their words.
        assert!(
            crate::ui::no_position_rule(&s, &n, false)
                .starts_with("Kotora sees the file the player has open")
        );
    }

    #[test]
    fn the_vlc_password_is_saved_once_typed() {
        let mut s = State::fake();
        s.prefs.vlc_set_up = false;
        s.save_vlc_password();
        assert!(!s.prefs.vlc_set_up, "an empty field saves nothing");
        s.winsetup.vlc_draft.text = "  secret  ".into();
        s.save_vlc_password();
        assert!(s.prefs.vlc_set_up && s.winsetup.vlc_saved && s.winsetup.vlc_draft.text.is_empty());
    }
}
