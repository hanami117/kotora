//! The tray icon: Rune's StatusNotifierItem tray on Linux (`tray_linux.rs`), a menu bar item of
//! our own on macOS (`tray_mac.rs`, Rune has no Mac tray). Both give `TrayCtl` the same API:
//! the current detection line, Play next (the next episodes in the library folders,
//! `library::play_menu`), Show Kotora and Quit. Picking a Play next item plays it as ▶ Play
//! does; the usual detection then offers the update.
//!
//! With a tray, closing the window only hides it: Kotora keeps running and detection keeps
//! working. Quit in the tray, ⌘Q on the Mac and `kot quit` really quit. Same design as
//! Uzumaki's `tray.rs`; never started headless.

#[cfg(not(target_os = "macos"))]
#[path = "tray_linux.rs"]
mod imp;
#[cfg(target_os = "macos")]
#[path = "tray_mac.rs"]
mod imp;

pub use imp::TrayCtl;

/// The Linux tray's menu model also builds and is tested on macOS (Rune's tray types exist
/// everywhere; only the D-Bus part is Linux's).
#[cfg(all(test, target_os = "macos"))]
#[path = "tray_linux.rs"]
#[allow(dead_code)]
mod linux_menu;

use crate::model::{Handle, State};

/// Shows the window and, on macOS, the Dock icon that closing to the menu bar hid. The Dock
/// request goes first, so the window opens into a regular app.
pub fn show_window(handle: &Handle) {
    #[cfg(target_os = "macos")]
    handle.set_dock_icon(true);
    handle.open();
    handle.update(|s: &mut State, _| {
        s.window = true;
        crate::library::refresh(s);
    });
}

/// Quit (the tray, `kot quit`): the page left open is kept first.
pub fn quit(handle: &Handle) {
    handle.update(|s: &mut State, _| {
        s.remember_page();
        crate::discord::shutdown();
    });
    handle.quit();
}

/// A tray Play next item was picked: play that anime's next episode (Settings' Open with).
pub fn play(handle: &Handle, id: u32) {
    handle.update(move |s: &mut State, _| s.play_entry(id));
}

/// The tray's first line: what plays, as the sidebar card has it ("Title · Ep 7"), or empty.
pub fn now_line(s: &State) -> String {
    let Some(now) = &s.now else { return String::new() };
    let (_, title) = crate::ui::now_title(s, now);
    match now.episode() {
        Some(ep) => format!("{title} \u{b7} Ep {ep}"),
        None => title,
    }
}

impl State {
    /// Starts the tray (live runs only, on the UI thread).
    pub fn start_tray(&mut self) {
        if rune::is_headless() {
            return;
        }
        if let Some(h) = &self.handle {
            self.tray = TrayCtl::start(h.clone());
        }
        self.sync_tray();
    }

    /// Hands the detection line to the tray; it does nothing unless the line changed.
    pub fn sync_tray(&mut self) {
        if self.tray.is_none() {
            return;
        }
        let line = now_line(self);
        let play = crate::library::play_menu(self);
        if let Some(t) = &mut self.tray {
            t.set_now(&line);
            t.set_play(play);
        }
    }

    /// `App::on_close`: true when the window should only hide (there is a tray to come back
    /// from). On macOS Rune also runs this on ⌘Q and the Dock's Quit; the app is ending then,
    /// so the Dock request below is never applied and Kotora quits.
    pub fn hide_on_close(&mut self) -> bool {
        if self.tray.is_none() {
            return false;
        }
        self.window = false;
        // macOS: no window and a menu bar icon to come back with, so the Dock icon goes too.
        #[cfg(target_os = "macos")]
        if let Some(h) = &self.handle {
            h.set_dock_icon(false);
        }
        true
    }

    /// `App::on_reopen` (macOS: the Dock icon clicked, or Kotora opened again from the Finder):
    /// with no window showing, bring back the window and the Dock icon. Rune's own default does
    /// nothing while the Dock icon is hidden.
    #[cfg(target_os = "macos")]
    pub fn reopen(&mut self, has_visible_windows: bool) -> bool {
        match (&self.handle, has_visible_windows) {
            (Some(h), false) => {
                show_window(h);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_what_plays_and_is_empty_otherwise() {
        let mut s = State::fake();
        s.now = None;
        assert_eq!(now_line(&s), "");
        let s = State::fake();
        if let Some(now) = &s.now {
            let line = now_line(&s);
            assert!(!line.is_empty());
            if let Some(ep) = now.episode() {
                assert!(line.ends_with(&format!("Ep {ep}")), "{line}");
            }
        }
    }

    #[test]
    fn without_a_tray_closing_quits() {
        let mut s = State::fake();
        assert!(!s.hide_on_close());
    }
}
