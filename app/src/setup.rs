//! The setup wizard (Stage 5): Welcome, create a MyAnimeList Client ID, paste it, sign in, done.
//! It shows at first run (no Client ID: every list page shows it) and again from Settings →
//! Accounts → "Set up again". Each step has the indicator ("Step 2 of 5") and Back / Next.
//!
//! Nothing here talks to the network itself: Save is `clientid.rs`, Sign in is `online.rs`. A
//! saved ID moves the wizard to the sign-in step, and a sign-in to the last step.

use rune::{El, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::clientid::{API_PAGE, copy_button, id_field};
use crate::icons;
use crate::model::{Auth, Page, State};
use crate::online;
use crate::theme::{self, PAL};
use crate::ui::{alert, button, ghost};

/// The wizard's steps, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Welcome,
    Create,
    Paste,
    SignIn,
    Done,
}

impl Step {
    pub const ALL: [Step; 5] = [Step::Welcome, Step::Create, Step::Paste, Step::SignIn, Step::Done];

    /// 1 to 5.
    pub fn number(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }

    fn next(self) -> Step {
        Step::ALL.get(self.number()).copied().unwrap_or(Step::Done)
    }

    fn prev(self) -> Step {
        Step::ALL[self.number().saturating_sub(2)]
    }
}

/// What went wrong with the last sign-in, in the wizard's words (`None`: nothing, or cancelled).
fn sign_in_problem(e: &str) -> Option<(&'static str, String)> {
    if e.contains("cancelled") {
        return None;
    }
    if e.contains("timed out") {
        return Some(("No answer from MyAnimeList", "Nothing came back from your browser within 5 minutes. Try again, and press Allow on MyAnimeList's page.".into()));
    }
    if e.contains("refused") || e.contains("denied") {
        return Some((
            "MyAnimeList said no",
            "The sign-in was denied. Try again and press Allow, so Kotora can read and update your list. If it keeps failing, check that your app's redirect URL is exactly the one from step 2.".into(),
        ));
    }
    Some(("Sign-in failed", e.to_string()))
}

impl State {
    /// Settings → Accounts → "Set up again": the wizard from the start.
    pub fn setup_again(&mut self) {
        if self.page != Page::Setup {
            self.setup_back = self.page;
        }
        self.setup = Step::Welcome;
        self.client_id.error = None;
        self.go(Page::Setup);
    }

    /// Whether the wizard can be closed: only once a Client ID is saved (without one, every
    /// list page shows it anyway).
    pub fn setup_closable(&self) -> bool {
        self.auth != Auth::NoClientId
    }

    /// Close, or "Open my list" on the last step.
    pub fn setup_close(&mut self) {
        let back = match self.setup_back {
            Page::Setup => Page::List(kotora_core::Status::Watching),
            p if self.setup == Step::Done => p.min_list(),
            p => p,
        };
        self.setup = Step::Welcome;
        self.go(back);
    }

    pub fn setup_next(&mut self) {
        if self.setup == Step::Done {
            self.setup_close();
            return;
        }
        if self.setup_can_go_on() {
            self.setup = self.setup.next();
            if self.setup == Step::Paste && self.client_id.hint.is_none() {
                self.client_id.draft.focused = true;
            }
        }
    }

    /// "Check config.toml again": for an ID written into the file by hand.
    pub fn setup_recheck(&mut self) {
        online::recheck_config(self);
        if self.auth != Auth::NoClientId {
            self.client_id.hint = crate::clientid::saved_hint();
            self.client_id.error = None;
            if self.setup == Step::Paste {
                self.setup = Step::SignIn;
            }
        }
    }

    pub fn setup_prev(&mut self) {
        self.setup = self.setup.prev();
    }

    /// Next is enabled: a saved ID to leave Paste, an account to leave Sign in.
    pub fn setup_can_go_on(&self) -> bool {
        match self.setup {
            Step::Paste => self.client_id.hint.is_some() && self.auth != Auth::NoClientId,
            Step::SignIn => self.account.is_some() && self.auth == Auth::SignedIn,
            _ => true,
        }
    }
}

impl Page {
    /// The page to land on after setup: a list page, or the page "Set up again" came from.
    fn min_list(self) -> Page {
        match self {
            Page::Settings => Page::Settings,
            _ => Page::List(kotora_core::Status::Watching),
        }
    }
}

// ---------------------------------------------------------------------------------------- view

/// "Step 2 of 5" and five bars, the done ones orange.
fn indicator(step: Step) -> El {
    let n = step.number();
    let mut bars = row().gap(4.0);
    for i in 1..=Step::ALL.len() {
        bars = bars.child(el().w(28.0).h(4.0).radius(2.0).fill(if i <= n { PAL.accent } else { PAL.border }));
    }
    row()
        .key("setup/indicator")
        .gap(12.0)
        .center_y()
        .child(text(format!("Step {n} of {}", Step::ALL.len())).font(theme::LABEL_NUM).color(PAL.dim))
        .child(bars)
}

fn para(t: &str) -> El {
    text(t.to_string()).font(theme::BODY).color(PAL.dim).wrap()
}

fn title(t: &str) -> El {
    text(t.to_string()).font(TextStyle::new(22.0, 600.0)).color(PAL.text).wrap().pb(2.0)
}

/// A button that opens a web page in the browser.
fn open_button(key: &str, label: &str, url: &'static str) -> El {
    row()
        .key(key.to_string())
        .role("link")
        .label(label.to_string())
        .focusable(true)
        .h(32.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.raised)
        .border(1.0, PAL.border_strong)
        .hover(PAL.hover())
        .press(PAL.press())
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut State| online::open_link(s, url))
        .child(text(label.to_string()).font(theme::LABEL_BTN).color(PAL.text))
        .child(icon(&icons::EXTERNAL, 14.0).color(PAL.dim))
}

fn welcome(s: &State) -> El {
    let mut c = col().gap(12.0).w_full().min_w(0.0);
    if let Some(img) = &s.logo_big {
        c = c.child(el().size(56.0, 56.0).radius(theme::R_LG).pixels(img.pixels()));
    }
    c.child(text("Welcome to Kotora").font(TextStyle::new(28.0, 600.0)).color(PAL.text).wrap())
        .child(para("Kotora keeps your MyAnimeList list up to date while you watch. It notices the episode playing in your own video player and asks before it records it."))
        .child(para("You need two things: a MyAnimeList account, and a free \u{201c}Client ID\u{201d}. MyAnimeList asks every app to register itself; it takes about 2 minutes, and the next steps show you how."))
        .child(text("Kotora is a tribute to Taiga, the Windows anime tracker. It is not affiliated with MyAnimeList.").font(theme::LABEL).color(PAL.faint).wrap())
}

/// One line of the form guide: the field's name, what to put, and a note.
fn form_row(field: &str, value: El, note: &str) -> El {
    let mut c = col().gap(2.0).grow(1.0).min_w(0.0).child(value);
    if !note.is_empty() {
        c = c.child(text(note.to_string()).font(theme::LABEL).color(PAL.faint).wrap());
    }
    row()
        .key(format!("setup/form/{}", field.to_lowercase().replace([' ', '/'], "-")))
        .gap(12.0)
        .py(7.0)
        .items_start()
        .child(text(field.to_string()).font(theme::LABEL).color(PAL.dim).w(170.0).pt(1.0))
        .child(c)
}

fn value(t: &str) -> El {
    text(t.to_string()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).wrap()
}

fn mono(t: &str) -> El {
    el().min_w(0.0).px(8.0).py(3.0).radius(theme::R_XS).fill(PAL.field).border(1.0, PAL.border).child(text(t.to_string()).font(TextStyle::new(12.0, 400.0).mono()).color(PAL.text).truncate())
}

fn numbered(n: u32, body: El) -> El {
    row()
        .gap(10.0)
        .items_start()
        .child(
            el().size(20.0, 20.0)
                .radius(10.0)
                .fill(PAL.accent.alpha(0.14))
                .center()
                .child(text(n.to_string()).font(theme::BADGE).color(PAL.accent_hi)),
        )
        .child(col().grow(1.0).min_w(0.0).gap(8.0).child(body))
}

fn create(s: &State) -> El {
    let redirect = kotora_core::mal::REDIRECT_URI;
    let copy = copy_button("setup/copy-redirect", s.client_id.copied, |s: &mut State, tx: &mut Tx| {
        tx.copy(kotora_core::mal::REDIRECT_URI);
        s.client_id.copied = true;
    });
    let form = col()
        .key("setup/form")
        .px(14.0)
        .py(6.0)
        .radius(theme::R_MD)
        .fill(PAL.bg)
        .border(1.0, PAL.border)
        .child(form_row("App Name", value("Anything unique, for example \u{201c}Kotora (yourname)\u{201d}"), ""))
        .child(form_row("App Type", value("other"), ""))
        .child(form_row("App Description", value("One line, for example \u{201c}Personal anime tracker\u{201d}"), ""))
        .child(form_row(
            "App Redirect URL",
            row().w_full().min_w(0.0).gap(8.0).center_y().flex_wrap().child(mono(redirect)).child(copy),
            "Must match exactly, or the sign-in fails.",
        ))
        .child(form_row("Homepage URL", row().child(mono("https://kotora.moe")), ""))
        .child(form_row("Commercial / Non-Commercial", value("Non-Commercial"), ""))
        .child(form_row("Name / Company Name", value("Your name or nickname"), ""))
        .child(form_row("Purpose of Use", value("hobbyist"), "Or whichever option says hobby or personal use."));
    col()
        .w_full()
        .min_w(0.0)
        .gap(14.0)
        .child(title("Create your Client ID"))
        .child(numbered(
            1,
            col()
                .gap(8.0)
                .child(para("Sign in to myanimelist.net in your browser first. Then open the API page and press \u{201c}Create ID\u{201d}."))
                .child(row().child(open_button("setup/open-api", "Open the API page", API_PAGE))),
        ))
        .child(numbered(2, col().gap(8.0).child(para("Fill in the form. If a field is named a little differently, pick the closest; only the redirect URL must be exact.")).child(form)))
        .child(numbered(3, para("Accept the license agreement and press Submit.")))
        .child(numbered(4, para("Back on the API page, press \u{201c}Edit\u{201d} next to your app and copy the Client ID.")))
        .child(alert("setup/no-secret", PAL.warn(), "You do NOT need the Client Secret. Never paste it here.".into(), None).mx(0.0).mb(0.0))
}

fn paste(s: &State) -> El {
    let mut c = col()
        .w_full()
        .min_w(0.0)
        .gap(12.0)
        .child(title("Paste the Client ID"))
        .child(para("It is 32 characters long, digits and the letters a to f. Kotora keeps it in its settings file (config.toml), which only you can read."))
        .child(id_field(s, "setup/cid", 380.0));
    if let Some(h) = &s.client_id.hint
        && s.auth != Auth::NoClientId
    {
        c = c.child(
            row()
                .key("setup/cid-saved")
                .gap(6.0)
                .center_y()
                .child(icon(&icons::CHECK, 14.0).color(PAL.ok()))
                .child(text(format!("Client ID saved (ending in {h}).")).font(theme::LABEL).color(PAL.ok())),
        );
    }
    c.child(text("Not the Client Secret: Kotora refuses anything that looks like one.").font(theme::LABEL).color(PAL.faint).wrap())
        .child(row().child(ghost("setup/recheck", "I put it in config.toml myself: check again", State::setup_recheck).h(26.0).px(0.0)))
}

fn sign_in(s: &State) -> El {
    let mut c = col()
        .w_full()
        .min_w(0.0)
        .gap(12.0)
        .child(title("Sign in to MyAnimeList"))
        .child(para("Kotora opens MyAnimeList's own sign-in page in your browser. Sign in there if you need to, press Allow, then come back here."))
        .child(
            row()
                .gap(8.0)
                .items_start()
                .child(icon(&icons::LOCK, 14.0).color(PAL.st_completed).mt(2.0))
                .child(text("Kotora never sees your password. MyAnimeList gives it a key for your list, kept in a file only you can read.").font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0)),
        );
    if let (Some((name, _)), Auth::SignedIn) = (&s.account, s.auth) {
        return c.child(
            row()
                .key("setup/signed-in")
                .gap(6.0)
                .mt(4.0)
                .center_y()
                .child(icon(&icons::CHECK, 14.0).color(PAL.ok()))
                .child(text(format!("Signed in as {name}.")).font(theme::BODY_MED).color(PAL.ok())),
        );
    }
    if s.auth == Auth::SigningIn {
        let mut wait = col()
            .key("setup/waiting")
            .w_full()
            .min_w(0.0)
            .gap(10.0)
            .padding(14.0)
            .radius(theme::R_MD)
            .fill(PAL.bg)
            .border(1.0, PAL.border)
            .child(
                row()
                    .gap(10.0)
                    .center_y()
                    .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.accent))
                    .child(text("Waiting for MyAnimeList in your browser\u{2026}").font(theme::BODY_MED).color(PAL.text).wrap().grow(1.0).min_w(0.0))
                    .child(button("setup/cancel", "Cancel", false, Some(online::cancel_sign_in)).h(26.0)),
            );
        if let Some((url, opened)) = &s.sign_in_link {
            let lead = if *opened { "Browser didn't open? Open this link in it:" } else { "Kotora couldn't open your browser. Open this link in it:" };
            let url2 = url.clone();
            wait = wait.child(text(lead).font(theme::LABEL).color(if *opened { PAL.dim } else { PAL.warn() }).wrap()).child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        el().key("setup/link")
                            .grow(1.0)
                            .min_w(0.0)
                            .px(8.0)
                            .py(5.0)
                            .radius(theme::R_XS)
                            .fill(PAL.field)
                            .border(1.0, PAL.border)
                            .child(text(url.clone()).font(TextStyle::new(11.0, 400.0).mono()).color(PAL.dim).truncate()),
                    )
                    .child(copy_button("setup/copy-link", s.client_id.link_copied, move |s: &mut State, tx: &mut Tx| {
                        tx.copy(&url2);
                        s.client_id.link_copied = true;
                    })),
            );
        }
        return c.child(wait);
    }
    if let Some((head, why)) = s.error.as_deref().and_then(sign_in_problem) {
        c = c.child(
            col()
                .key("setup/sign-in-error")
                .gap(4.0)
                .px(14.0)
                .py(12.0)
                .radius(theme::R_MD)
                .fill(PAL.err().alpha(0.08))
                .border(1.0, PAL.err().alpha(0.3))
                .child(text(head).font(theme::BODY_MED).color(PAL.err()).wrap())
                .child(text(why).font(theme::LABEL).color(PAL.text).wrap()),
        );
    }
    let label = if s.error.as_deref().and_then(sign_in_problem).is_some() { "Try again" } else { "Sign in with MyAnimeList" };
    c.child(row().mt(4.0).child(button("setup/sign-in", label, true, Some(online::sign_in)).h(36.0).px(16.0)))
}

fn done(s: &State) -> El {
    let players = if cfg!(target_os = "macos") { "mpv, IINA or VLC" } else { "mpv, VLC or mpc-qt" };
    let list = if s.loading || (s.syncing && s.entries.is_empty()) {
        "Loading your list\u{2026}".to_string()
    } else {
        match s.entries.len() {
            1 => "Your list is loaded: 1 anime.".to_string(),
            n => format!("Your list is loaded: {n} anime."),
        }
    };
    let name = s.account.as_ref().map_or(String::new(), |(n, _)| format!("Signed in as {n}. "));
    col()
        .gap(14.0)
        .child(title("You're all set"))
        .child(text(format!("{name}{list}")).key("setup/loaded").font(theme::BODY).color(PAL.text).wrap())
        .child(
            row()
                .key("setup/players-hint")
                .gap(12.0)
                .padding(14.0)
                .items_start()
                .radius(theme::R_MD)
                .fill(PAL.bg)
                .border(1.0, PAL.border)
                .child(icon(&icons::PLAY, 18.0).color(PAL.accent_hi))
                .child(
                    col()
                        .gap(6.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(format!("Play an episode in {players} and Kotora will notice it.")).font(theme::BODY_MED).color(PAL.text).wrap())
                        .child(text("It asks before it records the episode. Which players it watches, and how, is in Settings.").font(theme::LABEL).color(PAL.dim).wrap())
                        .child(
                            text("Settings \u{2192} Players")
                                .key("setup/to-players")
                                .role("link")
                                .label("Open Settings, Players")
                                .focusable(true)
                                .font(theme::LABEL_BTN)
                                .color(PAL.accent_hi)
                                .cursor(rune::Cursor::Pointer)
                                .on_press_tx(|s: &mut State, tx: &mut Tx| {
                                    tx.instant();
                                    s.setup = Step::Welcome;
                                    s.go(Page::Settings);
                                    tx.scroll_into_view("players-card");
                                }),
                        ),
                ),
        )
}

/// The wizard page.
pub fn page(s: &State) -> El {
    let step = s.setup;
    let body = match step {
        Step::Welcome => welcome(s),
        Step::Create => create(s),
        Step::Paste => paste(s),
        Step::SignIn => sign_in(s),
        Step::Done => done(s),
    };
    let mut head = row().gap(12.0).center_y().child(indicator(step)).child(spacer());
    if s.setup_closable() && step != Step::Done {
        head = head.child(ghost("setup/close", "Close", State::setup_close).h(26.0));
    }
    let mut foot = row().key("setup/foot").gap(8.0).center_y();
    if step != Step::Welcome && step != Step::Done {
        foot = foot.child(button("setup/back", "Back", false, Some(State::setup_prev)));
    }
    foot = foot.child(spacer());
    let (label, can) = match step {
        Step::Welcome => ("Start", true),
        Step::Done => ("Open my list", true),
        _ => ("Next", s.setup_can_go_on()),
    };
    foot = foot.child(button("setup/next", label, true, can.then_some(State::setup_next as fn(&mut State))));
    let card = col()
        .key(format!("setup/{}", step.number()))
        .w_full()
        .min_w(0.0)
        .max_w(680.0)
        .gap(20.0)
        .padding(24.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(head)
        .child(body.w_full().min_w(0.0))
        .child(el().w_full().h(1.0).fill(PAL.border))
        .child(foot);
    col().key("page/setup").w_full().h_full().child(
        col()
            .key("setup-scroll")
            .scroll(rune::ScrollSpec { stick_bottom: false })
            .grow(1.0)
            .min_h(0.0)
            .child(col().w_full().px(theme::PAGE_PAD).py(28.0).items_center().child(card)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Msg;

    fn first_run() -> State {
        let mut s = State::fake();
        s.auth = Auth::NoClientId;
        s.account = None;
        s.client_id.hint = None;
        s.entries.clear();
        s.refresh();
        s.go(Page::Setup);
        s
    }

    #[test]
    fn steps_go_forward_only_when_ready() {
        let mut s = first_run();
        assert!(!s.setup_closable(), "no Client ID: nothing else to show");
        s.setup_next();
        assert_eq!(s.setup, Step::Create);
        s.setup_next();
        assert_eq!(s.setup, Step::Paste);
        s.setup_next();
        assert_eq!(s.setup, Step::Paste, "no ID saved yet");
        // A secret's shape is refused; a good ID saved moves on by itself.
        s.client_id.draft.replace_text("0123456789abcdef".repeat(4));
        s.save_client_id();
        assert!(s.client_id.error.as_deref().is_some_and(|e| e.contains("Client Secret")));
        s.apply(Msg::ClientIdSaved(Ok(("cdef".into(), Auth::SignedOut))));
        assert_eq!(s.setup, Step::SignIn);
        assert!(!s.setup_can_go_on(), "not signed in yet");
        s.setup_prev();
        assert_eq!(s.setup, Step::Paste);
        s.setup_next();
        // Signing in: the link shows; a denial explains and offers Try again.
        s.auth = Auth::SigningIn;
        s.apply(Msg::SignInUrl { url: "https://myanimelist.net/v1/oauth2/authorize?x".into(), opened: false });
        assert!(s.sign_in_link.is_some());
        s.apply(Msg::SignInFailed("MyAnimeList refused the sign-in (access_denied)".into()));
        assert!(s.sign_in_link.is_none() && sign_in_problem(s.error.as_deref().unwrap()).is_some_and(|(h, _)| h.contains("said no")));
        assert!(sign_in_problem("sign-in cancelled").is_none());
        assert!(sign_in_problem("sign-in timed out").is_some_and(|(h, _)| h.contains("No answer")));
        s.auth = Auth::SigningIn;
        s.apply(Msg::SignedIn { user: "demo_user".into(), picture: None });
        assert_eq!(s.setup, Step::Done);
        s.setup_next();
        assert_eq!(s.page, Page::List(kotora_core::Status::Watching));
    }

    #[test]
    fn set_up_again_returns_to_settings() {
        let mut s = State::fake();
        s.go(Page::Settings);
        s.setup_again();
        assert_eq!((s.page, s.setup), (Page::Setup, Step::Welcome));
        assert!(s.setup_closable());
        s.setup_close();
        assert_eq!(s.page, Page::Settings);
    }
}
