//! The user's own MyAnimeList Client ID (advanced): Kotora has its own built in (`core::service`),
//! so sign-in needs none. Settings → Account has "Use my own client ID…" with a field, and "Use
//! Kotora's built-in ID" to go back; nobody edits `config.toml` by hand.
//!
//! Save checks the ID on the UI thread (`config::check_client_id`, instant), then writes
//! `config.toml` on a short thread (comments kept, 0600) and re-reads the sign-in state, so Sign in
//! works at once. Headless runs never write: there is no app handle.

use kotora_core::config::{self, Config};
use kotora_core::paths;
use rune::{El, TextStyle, Tx, col, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Auth, Msg, State};
use crate::online;
use crate::theme::{self, PAL};
use crate::ui::{button, field_hint, small_button};

/// The Client ID field and what it shows.
#[derive(Default)]
pub struct ClientIdForm {
    pub draft: rune::TextEdit,
    /// What was wrong with the last Save, in plain words.
    pub error: Option<String>,
    /// Settings: the field is open (Change was pressed).
    pub editing: bool,
    /// The file is being written.
    pub saving: bool,
    /// The saved ID's last four characters (Settings shows only those).
    pub hint: Option<String>,
    /// The sign-in link was copied (the wizard's "browser didn't open" line).
    pub link_copied: bool,
    /// Saved in this session (the Welcome page says so above Sign in).
    pub saved: bool,
}

/// The saved ID's last four characters, at start.
pub fn saved_hint() -> Option<String> {
    Config::load(&paths::config_file()).ok()?.mal_client_id.map(|id| config::client_id_hint(&id))
}

impl State {
    /// Save (the button, Enter in the field, or `kot config client-id`): check, then write.
    pub fn save_client_id(&mut self) {
        let raw = self.client_id.draft.text.clone();
        if let Err(e) = config::check_client_id(&raw) {
            self.client_id.error = Some(e);
            return;
        }
        self.client_id.error = None;
        let Some(h) = self.handle.clone() else { return };
        self.client_id.saving = true;
        std::thread::spawn(move || {
            let msg = match config::save_client_id(&paths::config_file(), &raw) {
                Ok(id) => Msg::ClientIdSaved(Ok((config::client_id_hint(&id), online::initial_auth()))),
                Err(e) => Msg::ClientIdSaved(Err(format!("Can't save it: {e}"))),
            };
            h.update_if(move |s, _| s.apply(msg));
        });
    }

    /// The writer's answer.
    pub fn client_id_saved(&mut self, r: Result<(String, Auth), String>) -> bool {
        let f = &mut self.client_id;
        f.saving = false;
        match r {
            Ok((hint, auth)) => {
                f.hint = Some(hint);
                f.error = None;
                f.editing = false;
                f.saved = true;
                f.draft.clear();
                // Sign in is enabled at once; a running sign-in or a signed-in account stays.
                if self.auth == Auth::NoClientId {
                    self.auth = auth;
                    self.error = None;
                }
            }
            Err(e) => f.error = Some(e),
        }
        true
    }

    /// "Use Kotora's built-in ID": blank the saved one. A sign-in made with the old ID is asked
    /// for again when it runs out.
    fn use_built_in_client_id(&mut self) {
        let Some(h) = self.handle.clone() else { return };
        self.client_id.saving = true;
        std::thread::spawn(move || {
            let msg = Msg::ClientIdCleared(config::clear_client_id(&paths::config_file()).map_err(|e| format!("Can't save it: {e}")));
            h.update_if(move |s, _| s.apply(msg));
        });
    }

    /// The writer's answer: no saved ID any more.
    pub fn client_id_cleared(&mut self, r: Result<(), String>) -> bool {
        let f = &mut self.client_id;
        f.saving = false;
        match r {
            Ok(()) => {
                f.hint = None;
                f.error = None;
                f.editing = false;
            }
            Err(e) => f.error = Some(e),
        }
        true
    }

    fn edit_client_id(&mut self) {
        self.client_id.editing = true;
        self.client_id.error = None;
        self.client_id.draft.clear();
        self.client_id.draft.focused = true;
    }

    fn cancel_client_id(&mut self) {
        self.client_id.editing = false;
        self.client_id.error = None;
        self.client_id.draft.clear();
    }
}

/// The field with its Save button. `key` scopes the parts (Welcome and Settings both have one).
pub(crate) fn id_field(s: &State, key: &str, wide: f32) -> El {
    let f = &s.client_id;
    let border = if f.error.is_some() { PAL.status(kotora_core::Status::Dropped).alpha(0.7) } else { PAL.border_strong };
    let input = row()
        .key(format!("{key}/box"))
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .max_w(wide)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, border)
        .child(
            text_input(&f.draft, TextStyle::new(12.0, 400.0).mono())
                .key(format!("{key}/input"))
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&f.draft, "Paste the Client ID here"), PAL.faint)
                .on_edit(|s: &mut State| &mut s.client_id.draft)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.client_id.error = None)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    s.save_client_id();
                }),
        );
    let save = if f.saving { button(&format!("{key}/save"), "Saving\u{2026}", true, None) } else { button(&format!("{key}/save"), "Save", true, Some(State::save_client_id)) };
    // The box shrinks with its card (up to `wide`), so a narrow window never pushes Save out.
    let mut c = col().w_full().min_w(0.0).gap(6.0).child(row().w_full().min_w(0.0).gap(8.0).center_y().child(input).child(save));
    if let Some(e) = &f.error {
        c = c.child(text(e.clone()).key(format!("{key}/error")).font(theme::LABEL).color(PAL.status(kotora_core::Status::Dropped)).wrap().max_w(wide + 80.0).min_w(0.0));
    }
    c
}

/// Copy: copies on press (`f`), then says "Copied" with a check.
pub(crate) fn copy_button(key: &str, copied: bool, f: impl Fn(&mut State, &mut Tx) + 'static) -> El {
    row()
        .key(key.to_string())
        .role("button")
        .label(if copied { "Copied" } else { "Copy" })
        .focusable(true)
        .h(28.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s, tx);
        })
        .child(icon(if copied { &icons::CHECK } else { &icons::COPY }, 14.0).color(if copied { PAL.ok() } else { PAL.dim }))
        .child(text(if copied { "Copied" } else { "Copy" }).font(theme::LABEL_BTN).color(PAL.text))
}

/// Settings → Account: Kotora's own ID (nothing to set up), or the user's own, masked, with Change
/// and "Use Kotora's built-in ID" (or the field while changing).
pub fn settings_row(s: &State) -> El {
    let f = &s.client_id;
    let head = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(text("MyAnimeList Client ID").font(theme::BODY_MED).color(PAL.text))
        .child(match &f.hint {
            Some(h) => text(format!("{}{h}", "\u{2022}".repeat(12))).key("set/cid-masked").font(TextStyle::new(12.0, 400.0).mono()).color(PAL.dim),
            None => text("Kotora\u{2019}s own: nothing to set up").key("set/cid-built-in").font(theme::LABEL).color(PAL.dim),
        });
    if !f.editing {
        let mut r = row().key("set/cid").gap(8.0).py(10.0).center_y().child(head).child(small_button("set/setup-again", "Set up again", false, Some(State::setup_again)));
        if f.hint.is_some() {
            let built_in = if f.saving { small_button("set/cid-built-in-use", "Saving\u{2026}", false, None) } else { small_button("set/cid-built-in-use", "Use Kotora\u{2019}s built-in ID", false, Some(State::use_built_in_client_id)) };
            r = r.child(built_in).child(small_button("set/cid-change", "Change", false, Some(State::edit_client_id)));
        } else {
            // Advanced: sign-in works without this. For someone with an app of their own.
            r = r.child(small_button("set/cid-change", "Use my own client ID\u{2026}", false, Some(State::edit_client_id)));
        }
        let mut c = col().gap(4.0).child(r);
        if let Some(e) = &f.error {
            c = c.child(text(e.clone()).key("set/cid/error").font(theme::LABEL).color(PAL.status(kotora_core::Status::Dropped)).wrap());
        }
        return c.key("set/cid-wrap");
    }
    let mut c = col()
        .key("set/cid")
        .gap(8.0)
        .py(10.0)
        .child(row().gap(12.0).center_y().child(head).child(spacer()).child(small_button("set/cid-cancel", "Cancel", false, Some(State::cancel_client_id))))
        .child(id_field(s, "set/cid", 300.0));
    let note = if s.account.is_some() {
        "Advanced. A new ID needs a new sign-in: MyAnimeList asks again when the current one runs out."
    } else {
        "Advanced. From myanimelist.net/apiconfig: app type \u{201c}other\u{201d}, redirect URL http://127.0.0.1:47321/callback."
    };
    c = c.child(text(note).font(theme::LABEL).color(PAL.faint).wrap());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_id_shows_why_and_a_good_one_waits_for_the_writer() {
        let mut s = State::fake();
        s.auth = Auth::NoClientId;
        s.client_id.draft.replace_text("my secret");
        s.save_client_id();
        assert!(s.client_id.error.as_deref().is_some_and(|e| e.contains("no spaces")));
        s.client_id.draft.replace_text("0123456789abcdef0123456789abcdef");
        s.save_client_id();
        assert_eq!(s.client_id.error, None);
        // The writer answers (no handle in tests: the answer comes by hand).
        s.apply(Msg::ClientIdSaved(Ok(("cdef".into(), Auth::SignedOut))));
        assert_eq!(s.auth, Auth::SignedOut, "Sign in is enabled at once");
        assert_eq!(s.client_id.hint.as_deref(), Some("cdef"));
        assert!(s.client_id.draft.text.is_empty() && s.client_id.saved);
    }

    #[test]
    fn a_new_id_while_signed_in_keeps_the_account() {
        let mut s = State::fake();
        s.edit_client_id();
        assert!(s.client_id.editing);
        s.apply(Msg::ClientIdSaved(Ok(("abcd".into(), Auth::SignedIn))));
        assert_eq!(s.auth, Auth::SignedIn);
        assert!(!s.client_id.editing);
        s.apply(Msg::ClientIdSaved(Err("Can't save it: disk full".into())));
        assert!(s.client_id.error.is_some());
    }
}
