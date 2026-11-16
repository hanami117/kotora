//! The Client ID of the active service (MyAnimeList, or AniList) from the app: the field of the
//! setup wizard (`setup.rs`) and a masked field in Settings, so nobody edits `config.toml` by
//! hand. Also AniList's token field (a token pasted from AniList's page).
//!
//! Save checks the ID on the UI thread (`config::check_client_id`, instant), then writes
//! `config.toml` on a short thread (comments kept, 0600) and re-reads the sign-in state, so Sign in
//! works at once. Headless runs never write: there is no app handle.

use kotora_core::config::{self, Config};
use kotora_core::paths;
use kotora_core::service::ServiceId;
use rune::{El, TextStyle, Tx, col, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Auth, Msg, State};
use crate::online;
use crate::theme::{self, PAL};
use crate::ui::{button, field_hint, small_button};

/// MyAnimeList's page where an app (and its Client ID) is registered.
pub const API_PAGE: &str = "https://myanimelist.net/apiconfig";

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
    /// The redirect URL was copied (the button says so).
    pub copied: bool,
    /// The sign-in link was copied (the wizard's "browser didn't open" line).
    pub link_copied: bool,
    /// Saved in this session (the Welcome page says so above Sign in).
    pub saved: bool,
    /// AniList: a token pasted from AniList's page, or the address the browser ended on.
    pub token: rune::TextEdit,
    /// What was wrong with the pasted token.
    pub token_error: Option<String>,
}

/// The saved ID's last four characters, for service `id`.
pub fn saved_hint(id: ServiceId) -> Option<String> {
    Config::load(&paths::config_file()).ok()?.client_id(id).map(config::client_id_hint)
}

/// Checks a pasted ID the way `id`'s IDs look.
fn check(id: ServiceId, raw: &str) -> Result<String, String> {
    match id {
        ServiceId::Mal => config::check_client_id(raw),
        ServiceId::AniList => config::check_anilist_client_id(raw),
    }
}

impl State {
    /// Save (the button, Enter in the field, or `kot config client-id`): check, then write.
    pub fn save_client_id(&mut self) {
        let raw = self.client_id.draft.text.clone();
        let service = self.service;
        if let Err(e) = check(service, &raw) {
            self.client_id.error = Some(e);
            return;
        }
        self.client_id.error = None;
        let Some(h) = self.handle.clone() else { return };
        self.client_id.saving = true;
        std::thread::spawn(move || {
            let msg = match config::save_client_id_for(&paths::config_file(), service, &raw) {
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
                // The wizard moves on to the sign-in.
                if self.setup == crate::setup::Step::Paste {
                    self.setup = crate::setup::Step::SignIn;
                }
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

/// Settings → Accounts: the ID, masked, with Change (or the field while changing).
pub fn settings_row(s: &State) -> El {
    let f = &s.client_id;
    let head = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(text(format!("{} Client ID", s.service.title())).font(theme::BODY_MED).color(PAL.text))
        .child(match &f.hint {
            Some(h) => text(format!("{}{h}", "\u{2022}".repeat(12))).key("set/cid-masked").font(TextStyle::new(12.0, 400.0).mono()).color(PAL.dim),
            None => text("Not set: Sign in needs one").font(theme::LABEL).color(PAL.warn()),
        });
    if !f.editing {
        let label = if f.hint.is_some() { "Change" } else { "Set" };
        return row()
            .key("set/cid")
            .gap(8.0)
            .py(10.0)
            .center_y()
            .child(head)
            .child(small_button("set/setup-again", "Set up again", false, Some(State::setup_again)))
            .child(small_button("set/cid-change", label, f.hint.is_none(), Some(State::edit_client_id)));
    }
    let mut c = col()
        .key("set/cid")
        .gap(8.0)
        .py(10.0)
        .child(row().gap(12.0).center_y().child(head).child(spacer()).child(small_button("set/cid-cancel", "Cancel", false, Some(State::cancel_client_id))))
        .child(id_field(s, "set/cid", 300.0));
    let note = match (s.service, s.account.is_some()) {
        (ServiceId::Mal, true) => "A new ID needs a new sign-in: MyAnimeList asks again when the current one runs out.",
        (ServiceId::Mal, false) => "From myanimelist.net/apiconfig: app type \u{201c}other\u{201d}, redirect URL http://127.0.0.1:47321/callback.",
        (ServiceId::AniList, true) => "Your sign-in keeps working; a new ID is used at the next sign-in.",
        (ServiceId::AniList, false) => "From anilist.co/settings/developer: a client with the redirect URL http://127.0.0.1:47321/callback.",
    };
    c = c.child(text(note).font(theme::LABEL).color(PAL.faint).wrap());
    c
}

/// AniList: the field for a token pasted from AniList's page, with its Use button.
pub(crate) fn token_field(s: &State, key: &str) -> El {
    let f = &s.client_id;
    let border = if f.token_error.is_some() { PAL.status(kotora_core::Status::Dropped).alpha(0.7) } else { PAL.border_strong };
    let input = row()
        .key(format!("{key}/box"))
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, border)
        .child(
            text_input(&f.token, TextStyle::new(12.0, 400.0).mono())
                .key(format!("{key}/input"))
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&f.token, "Paste the token from AniList\u{2019}s page"), PAL.faint)
                .on_edit(|s: &mut State| &mut s.client_id.token)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.client_id.token_error = None)
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    online::sign_in_with_token(s);
                }),
        );
    let mut c = col()
        .w_full()
        .min_w(0.0)
        .gap(6.0)
        .child(row().w_full().min_w(0.0).gap(8.0).center_y().child(input).child(button(&format!("{key}/use"), "Use token", false, Some(online::sign_in_with_token))));
    if let Some(e) = &f.token_error {
        c = c.child(text(e.clone()).key(format!("{key}/error")).font(theme::LABEL).color(PAL.status(kotora_core::Status::Dropped)).wrap().min_w(0.0));
    }
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
