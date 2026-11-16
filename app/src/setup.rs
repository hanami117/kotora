//! The setup wizard: the service choice, then Welcome, sign in, done.
//!
//! Kotora has its own MyAnimeList and AniList Client IDs built in (`core::service`), so there is
//! nothing to register or paste; someone with a client of their own sets its ID in Settings →
//! Account. The Create and Paste steps stay for a service without a built-in ID (none today).
//!
//! The choice ("Which service do you use?") is an unnumbered intro, only for someone who has not
//! chosen yet and has no Client ID or token (`Config::needs_service_choice`); the numbered steps
//! are the same for both services, with each one's own words. Settings → Account switches
//! services later.
//!
//! It shows at first run (no Client ID: every list page shows it) and again from Settings →
//! Accounts → "Set up again". Each step has the indicator ("Step 2 of 5") and Back / Next.
//!
//! Nothing here talks to the network itself: Save is `clientid.rs`, Sign in is `online.rs`. A
//! saved ID moves the wizard to the sign-in step, and a sign-in to the last step.

use rune::{El, TextStyle, Tx, col, el, icon, row, spacer, text};

use kotora_core::service::ServiceId;

use crate::clientid::{API_PAGE, copy_button, id_field, token_field};
use crate::icons;
use crate::model::{Auth, Page, State};
use crate::online;
use crate::theme::{self, PAL};
use crate::ui::{alert, button, ghost};

/// The wizard's steps, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    /// "Which service do you use?" (not numbered, in no list of [`Step::all`]).
    Service,
    #[default]
    Welcome,
    Create,
    Paste,
    SignIn,
    Done,
}

impl Step {
    /// Registering your own client: Welcome, Create, Paste, Sign in, Done.
    const OWN_ID: [Step; 5] = [Step::Welcome, Step::Create, Step::Paste, Step::SignIn, Step::Done];
    /// A service with a Client ID built in (AniList): no Create or Paste.
    const BUILT_IN: [Step; 3] = [Step::Welcome, Step::SignIn, Step::Done];

    /// The numbered steps for `svc`: a service with a built-in Client ID skips registering one.
    pub fn all(svc: ServiceId) -> &'static [Step] {
        if svc.built_in_client_id().is_some() { &Step::BUILT_IN } else { &Step::OWN_ID }
    }

    /// 1 to the last; 0 for the service choice.
    pub fn number(self, svc: ServiceId) -> usize {
        Step::all(svc).iter().position(|s| *s == self).map_or(0, |i| i + 1)
    }

    fn next(self, svc: ServiceId) -> Step {
        Step::all(svc).get(self.number(svc)).copied().unwrap_or(Step::Done)
    }

    fn prev(self, svc: ServiceId) -> Step {
        Step::all(svc)[self.number(svc).saturating_sub(2)]
    }
}

/// What went wrong with the last sign-in, in the wizard's words (`None`: nothing, or cancelled).
fn sign_in_problem(svc: ServiceId, e: &str) -> Option<(String, String)> {
    let name = svc.title();
    if e.contains("cancelled") {
        return None;
    }
    if e.contains("timed out") {
        return Some((format!("No answer from {name}"), format!("Nothing came back from your browser within 5 minutes. Try again, and press Allow (Approve on AniList) on {name}'s page.")));
    }
    if e.contains("refused") || e.contains("denied") {
        return Some((
            format!("{name} said no"),
            if svc.built_in_client_id().is_some() {
                "The sign-in was denied. Try again and approve it, so Kotora can read and update your list. If it keeps failing and you use your own client ID (Settings \u{2192} Account), check that its redirect URL is exactly http://127.0.0.1:47321/callback.".to_string()
            } else {
                format!("The sign-in was denied. Try again and allow it, so Kotora can read and update your list. If it keeps failing, check that your {}'s redirect URL is exactly the one from step 2.", if svc == ServiceId::AniList { "client" } else { "app" })
            },
        ));
    }
    Some(("Sign-in failed".into(), e.to_string()))
}

impl State {
    /// Settings → Accounts → "Set up again": the wizard from the start.
    pub fn setup_again(&mut self) {
        if self.page != Page::Setup {
            self.setup_back = self.page;
        }
        self.setup = self.setup_first();
        self.client_id.error = None;
        self.go(Page::Setup);
    }

    /// The wizard's first screen: the service choice while it is due, else Welcome.
    pub fn setup_first(&self) -> Step {
        if self.setup_service { Step::Service } else { Step::Welcome }
    }

    /// A card of the first screen: remember the choice in `[account] service` (a worker writes
    /// the file; tests and scenes have no handle, so nothing is written), make it the active
    /// service, and go on.
    pub fn choose_service(&mut self, id: ServiceId) {
        if self.setup != Step::Service {
            return;
        }
        self.service = id;
        if self.handle.is_some() {
            crate::account::set_active(id);
            // The choice is made (its file write may still be on its way): no first-run state.
            self.auth = online::auth_for(id);
            self.client_id.hint = crate::clientid::saved_hint(id);
            // A built-in Client ID means the list pages no longer show the wizard by themselves.
            if self.page != Page::Setup {
                self.setup_back = self.page;
                self.page = Page::Setup;
            }
            std::thread::spawn(move || {
                if let Err(e) = kotora_core::config::save_service(&kotora_core::paths::config_file(), id.name()) {
                    eprintln!("kotora: {e}");
                }
            });
        }
        self.setup = Step::Welcome;
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
        // Done with the first run: "Set up again" starts at Welcome from now on.
        self.setup_service = false;
        self.setup = Step::Welcome;
        self.go(back);
    }

    pub fn setup_next(&mut self) {
        if self.setup == Step::Service {
            self.choose_service(self.service);
            return;
        }
        if self.setup == Step::Done {
            self.setup_close();
            return;
        }
        if self.setup_can_go_on() {
            self.setup = self.setup.next(self.service);
            if self.setup == Step::Paste && self.client_id.hint.is_none() {
                self.client_id.draft.focused = true;
            }
            // First run: the wizard was the only page; from the sign-in step on, signing in works
            // (the service has a built-in Client ID).
            if self.setup == Step::SignIn && self.auth == Auth::NoClientId && self.service.built_in_client_id().is_some() {
                self.auth = Auth::SignedOut;
            }
        }
    }

    /// "Check config.toml again": for an ID written into the file by hand.
    pub fn setup_recheck(&mut self) {
        online::recheck_config(self);
        if self.auth != Auth::NoClientId {
            self.client_id.hint = crate::clientid::saved_hint(self.service);
            self.client_id.error = None;
            if self.setup == Step::Paste {
                self.setup = Step::SignIn;
            }
        }
    }

    pub fn setup_prev(&mut self) {
        self.setup = if self.setup == Step::Welcome && self.setup_service { Step::Service } else { self.setup.prev(self.service) };
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
fn indicator(step: Step, svc: ServiceId) -> El {
    let n = step.number(svc);
    let total = Step::all(svc).len();
    let mut bars = row().gap(4.0);
    for i in 1..=total {
        bars = bars.child(el().w(28.0).h(4.0).radius(2.0).fill(if i <= n { PAL.accent } else { PAL.border }));
    }
    row()
        .key("setup/indicator")
        .gap(12.0)
        .center_y()
        .child(text(format!("Step {n} of {total}")).font(theme::LABEL_NUM).color(PAL.dim))
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

fn logo(s: &State) -> El {
    match &s.logo_big {
        Some(img) => el().size(56.0, 56.0).radius(theme::R_LG).pixels(img.pixels()),
        None => el().size(56.0, 56.0),
    }
}

/// The first screen: "Which service do you use?" with a card for each service; Return picks
/// MyAnimeList. `width`: the main area's, to put the cards side by side or one under the other.
fn service(s: &State, width: f32) -> El {
    // The wizard card's inner width: its 680 max, less the page's and the card's padding.
    let inner = (width - 2.0 * theme::PAGE_PAD).min(680.0) - 2.0 * 24.0 - 2.0;
    let side_by_side = inner >= 520.0;
    let mal = service_tile("setup/service-mal", "MyAnimeList", "MAL", rune::color::hex("#2e51a2").expect("hex"), "Sync your MyAnimeList list.", ServiceId::Mal);
    let ani = service_tile("setup/service-anilist", "AniList", "AL", rune::color::hex("#1d3550").expect("hex"), "Sync your AniList anime list.", ServiceId::AniList);
    let cards = if side_by_side {
        let w = ((inner - 12.0) / 2.0).floor();
        row().key("setup/services").gap(12.0).focus_group().child(mal.w(w)).child(ani.w(w))
    } else {
        col().key("setup/services").gap(12.0).focus_group().child(mal.w_full()).child(ani.w_full())
    };
    col()
        .w_full()
        .min_w(0.0)
        .gap(14.0)
        .child(logo(s))
        .child(title("Which service do you use?"))
        .child(para("Kotora keeps your list up to date while you watch. Pick the service your list lives on."))
        .child(cards)
}

/// One service card: a button (Tab or the arrows focus it, Return or a click goes on).
/// MyAnimeList's is the default button.
fn service_tile(key: &str, name: &str, short: &str, badge: rune::Color, line: &str, id: ServiceId) -> El {
    let live = true;
    let mark = el()
        .size(44.0, 44.0)
        .radius(theme::R_MD)
        .fill(badge)
        .center()
        .child(text(short.to_string()).font(TextStyle::new(15.0, 700.0)).color(if live { rune::Color::WHITE } else { PAL.dim }));
    let mut top = row().gap(12.0).center_y().child(mark).child(spacer());
    if !live {
        top = top.child(
            el().px(8.0)
                .py(3.0)
                .radius(10.0)
                .border(1.0, PAL.border_strong)
                .child(text("Coming soon").font(theme::BADGE).color(PAL.dim)),
        );
    }
    let mut body = col()
        .gap(4.0)
        .child(text(name.to_string()).font(TextStyle::new(17.0, 600.0)).color(PAL.text))
        .child(text(line.to_string()).font(theme::BODY).color(PAL.dim).wrap());
    if live {
        body = body.child(
            row()
                .gap(6.0)
                .center_y()
                .pt(8.0)
                .child(text("Continue").font(theme::LABEL_BTN).color(PAL.accent_hi))
                .child(icon(&icons::ARROW, 14.0).color(PAL.accent_hi)),
        );
    }
    let tile = col().key(key.to_string()).role("button").gap(14.0).padding(18.0).radius(theme::R_MD).fill(PAL.bg).border(1.0, PAL.border_strong).child(top).child(body);
    let tile = tile
        .label(format!("{name}. {line} Continue."))
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.choose_service(id);
        });
    if id == ServiceId::Mal { tile.default_button() } else { tile }
}

fn welcome(s: &State) -> El {
    let mut c = col().gap(12.0).w_full().min_w(0.0);
    // After the service choice the logo and "Welcome" were already shown.
    if !s.setup_service {
        c = c.child(logo(s));
    }
    let name = s.service.title();
    let register = match s.service {
        ServiceId::Mal => "MyAnimeList asks every app to register itself",
        ServiceId::AniList => "AniList asks every app to register itself as an \u{201c}API client\u{201d}",
    };
    c.child(text(if s.setup_service { format!("Set up {name}") } else { "Welcome to Kotora".to_string() }).font(TextStyle::new(28.0, 600.0)).color(PAL.text).wrap())
        .child(para(&format!("Kotora keeps your {name} list up to date while you watch. It notices the episode playing in your own video player and asks before it records it.")))
        .child(if s.service.built_in_client_id().is_some() {
            para(&format!("All you need is a {name} account. Kotora signs you in through {name}'s own page in your browser: one click to approve, then you are back here."))
        } else {
            para(&format!("You need two things: a {name} account, and a free \u{201c}Client ID\u{201d}. {register}; it takes about 2 minutes, and the next steps show you how."))
        })
        .child(text(format!("Kotora is a tribute to Taiga, the Windows anime tracker. It is not affiliated with {name}.")).font(theme::LABEL).color(PAL.faint).wrap())
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
    if s.service == ServiceId::AniList {
        return create_anilist(s);
    }
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

/// AniList: register an API client (a name and the redirect URL; no secret is ever needed).
fn create_anilist(s: &State) -> El {
    let redirect = kotora_core::anilist::REDIRECT_URI;
    let copy = copy_button("setup/copy-redirect", s.client_id.copied, |s: &mut State, tx: &mut Tx| {
        tx.copy(kotora_core::anilist::REDIRECT_URI);
        s.client_id.copied = true;
    });
    let form = col()
        .key("setup/form")
        .px(14.0)
        .py(6.0)
        .radius(theme::R_MD)
        .fill(PAL.bg)
        .border(1.0, PAL.border)
        .child(form_row("Name", value("Anything, for example \u{201c}Kotora (yourname)\u{201d}"), ""))
        .child(form_row(
            "Redirect URL",
            row().w_full().min_w(0.0).gap(8.0).center_y().flex_wrap().child(mono(redirect)).child(copy),
            "Must match exactly, or the sign-in can\u{2019}t come back to Kotora.",
        ));
    col()
        .w_full()
        .min_w(0.0)
        .gap(14.0)
        .child(title("Create your Client ID"))
        .child(numbered(
            1,
            col()
                .gap(8.0)
                .child(para("Sign in to anilist.co in your browser first. Then open Settings, Developer, and press \u{201c}Create New Client\u{201d}."))
                .child(row().child(open_button("setup/open-api", "Open the Developer page", kotora_core::anilist::DEVELOPER_PAGE))),
        ))
        .child(numbered(2, col().gap(8.0).child(para("Fill in the form:")).child(form)))
        .child(numbered(3, para("Press Save. AniList lists your client with its ID, a number such as 12345.")))
        .child(alert("setup/no-secret", PAL.warn(), "You do NOT need the Client Secret. Never paste it here.".into(), None).mx(0.0).mb(0.0))
}

fn paste(s: &State) -> El {
    let what = match s.service {
        ServiceId::Mal => "It is 32 characters long, digits and the letters a to f.",
        ServiceId::AniList => "It is a number, such as 12345 (the ID, not the Secret).",
    };
    let mut c = col()
        .w_full()
        .min_w(0.0)
        .gap(12.0)
        .child(title("Paste the Client ID"))
        .child(para(&format!("{what} Kotora keeps it in its settings file (config.toml), which only you can read.")))
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
    let name = s.service.title();
    let allow = if s.service == ServiceId::AniList { "Approve" } else { "Allow" };
    let mut c = col()
        .w_full()
        .min_w(0.0)
        .gap(12.0)
        .child(title(&format!("Sign in to {name}")))
        .child(para(&format!("Kotora opens {name}'s own sign-in page in your browser. Sign in there if you need to, press {allow}, then come back here.")))
        .child(
            row()
                .gap(8.0)
                .items_start()
                .child(icon(&icons::LOCK, 14.0).color(PAL.st_completed).mt(2.0))
                .child(text(format!("Kotora never sees your password. {name} gives it a key for your list, kept in a file only you can read.")).font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0)),
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
                    .child(text(format!("Waiting for {name} in your browser\u{2026}")).font(theme::BODY_MED).color(PAL.text).wrap().grow(1.0).min_w(0.0))
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
        if s.service == ServiceId::AniList {
            // If the page can't come back to Kotora (a busy port, a blocked loopback), the address
            // it ended on holds the token; a client registered with the PIN page shows it there.
            wait = wait
                .child(text("The page didn\u{2019}t come back here? Paste the address your browser ended on, or the token AniList showed:").font(theme::LABEL).color(PAL.dim).wrap())
                .child(token_field(s, "setup/token"));
        }
        return c.child(wait);
    }
    if let Some((head, why)) = s.error.as_deref().and_then(|e| sign_in_problem(s.service, e)) {
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
    if s.service == ServiceId::AniList && s.error.as_deref().and_then(|e| sign_in_problem(s.service, e)).is_some() {
        c = c.child(text("Or paste the address your browser ended on, or the token AniList showed:").font(theme::LABEL).color(PAL.dim).wrap()).child(token_field(s, "setup/token"));
    }
    let label = if s.error.as_deref().and_then(|e| sign_in_problem(s.service, e)).is_some() { "Try again".to_string() } else { format!("Sign in with {name}") };
    c.child(row().mt(4.0).child(button("setup/sign-in", &label, true, Some(online::sign_in)).h(36.0).px(16.0)))
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
                                    crate::settings::open(s, crate::settings::Section::Detection);
                                }),
                        ),
                ),
        )
}

/// The wizard page.
pub fn page(s: &State, width: f32) -> El {
    let step = s.setup;
    let body = match step {
        Step::Service => service(s, width),
        Step::Welcome => welcome(s),
        Step::Create => create(s),
        Step::Paste => paste(s),
        Step::SignIn => sign_in(s),
        Step::Done => done(s),
    };
    let mut head = row().gap(12.0).center_y();
    if step != Step::Service {
        head = head.child(indicator(step, s.service));
    }
    head = head.child(spacer());
    if s.setup_closable() && step != Step::Done {
        head = head.child(ghost("setup/close", "Close", State::setup_close).h(26.0));
    }
    let mut foot = row().key("setup/foot").gap(8.0).center_y();
    if (step != Step::Welcome || s.setup_service) && step != Step::Done {
        foot = foot.child(button("setup/back", "Back", false, Some(State::setup_prev)));
    }
    foot = foot.child(spacer());
    let (label, can) = match step {
        Step::Service | Step::Welcome => ("Start", true),
        Step::Done => ("Open my list", true),
        _ => ("Next", s.setup_can_go_on()),
    };
    foot = foot.child(button("setup/next", label, true, can.then_some(State::setup_next as fn(&mut State))));
    let mut card = col()
        .key(if step == Step::Service { "setup/service".to_string() } else { format!("setup/{}", step.number(s.service)) })
        .w_full()
        .min_w(0.0)
        .max_w(680.0)
        .gap(20.0)
        .padding(24.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border);
    // The service choice has a head only when there is something to close, and no Back or Next:
    // a card goes on.
    if step != Step::Service || s.setup_closable() {
        card = card.child(head);
    }
    card = card.child(body.w_full().min_w(0.0));
    if step != Step::Service {
        card = card.child(el().w_full().h(1.0).fill(PAL.border)).child(foot);
    }
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
        assert!(!s.setup_closable(), "first run: nothing else to show");
        // Kotora's own MyAnimeList Client ID is built in: Welcome, Sign in, Done.
        assert_eq!(Step::all(ServiceId::Mal), [Step::Welcome, Step::SignIn, Step::Done]);
        s.setup_next();
        assert_eq!(s.setup, Step::SignIn);
        assert_eq!(s.auth, Auth::SignedOut, "signing in works from here");
        assert!(!s.setup_can_go_on(), "not signed in yet");
        s.setup_prev();
        assert_eq!(s.setup, Step::Welcome);
        s.setup_next();
        // A secret's shape is still refused for someone's own ID (Settings).
        s.client_id.draft.replace_text("0123456789abcdef".repeat(4));
        s.save_client_id();
        assert!(s.client_id.error.as_deref().is_some_and(|e| e.contains("Client Secret")));
        // Signing in: the link shows; a denial explains and offers Try again.
        s.auth = Auth::SigningIn;
        s.apply(Msg::SignInUrl { url: "https://myanimelist.net/v1/oauth2/authorize?x".into(), opened: false });
        assert!(s.sign_in_link.is_some());
        s.apply(Msg::SignInFailed("MyAnimeList refused the sign-in (access_denied)".into()));
        assert!(s.sign_in_link.is_none() && sign_in_problem(ServiceId::Mal, s.error.as_deref().unwrap()).is_some_and(|(h, _)| h.contains("said no")));
        assert!(sign_in_problem(ServiceId::Mal, "sign-in cancelled").is_none());
        assert!(sign_in_problem(ServiceId::Mal, "sign-in timed out").is_some_and(|(h, _)| h.contains("No answer")));
        s.auth = Auth::SigningIn;
        s.apply(Msg::SignedIn { user: "demo_user".into(), picture: None });
        assert_eq!(s.setup, Step::Done);
        s.setup_next();
        assert_eq!(s.page, Page::List(kotora_core::Status::Watching));
    }

    #[test]
    fn the_service_choice_comes_first_only_when_due() {
        // Existing users (a Client ID or a token) never get the flag, so the wizard starts at
        // Welcome, also from "Set up again".
        let mut s = State::fake();
        assert!(!s.setup_service);
        s.setup_again();
        assert_eq!(s.setup, Step::Welcome);
        s.setup_close();

        // First run, nothing chosen: the choice, then Welcome with a way back.
        let mut s = first_run();
        s.setup_service = true;
        s.setup = s.setup_first();
        assert_eq!((s.setup, s.setup.number(s.service)), (Step::Service, 0));
        assert!(s.handle.is_none(), "tests and scenes never write the config file");
        s.choose_service(ServiceId::Mal);
        assert_eq!(s.setup, Step::Welcome);
        s.setup_prev();
        assert_eq!(s.setup, Step::Service, "Back from Welcome");
        s.setup_next();
        s.setup_next();
        assert_eq!(s.setup, Step::SignIn);
        s.setup_prev();
        s.setup_prev();
        assert_eq!(s.setup, Step::Service, "Sign in, Welcome, then the choice");
        s.choose_service(ServiceId::Mal);
        s.setup_next();
        // Other steps ignore the choice.
        s.choose_service(ServiceId::AniList);
        assert_eq!((s.setup, s.service), (Step::SignIn, ServiceId::Mal));

        // Once the wizard is closed the choice is behind us.
        s.auth = Auth::SignedOut;
        s.setup_close();
        s.setup_again();
        assert_eq!(s.setup, Step::Welcome);
    }

    #[test]
    fn the_anilist_card_goes_straight_to_sign_in() {
        let mut s = first_run();
        s.setup_service = true;
        s.setup = s.setup_first();
        s.choose_service(ServiceId::AniList);
        assert_eq!((s.setup, s.service), (Step::Welcome, ServiceId::AniList));
        // Kotora's own Client ID is built in: three steps, no Create or Paste.
        assert_eq!(Step::all(ServiceId::AniList), [Step::Welcome, Step::SignIn, Step::Done]);
        assert_eq!(Step::all(ServiceId::Mal).len(), 3, "MyAnimeList's is built in too (0.1.3)");
        assert_eq!((Step::Welcome.number(ServiceId::AniList), Step::SignIn.number(ServiceId::AniList), Step::Done.number(ServiceId::AniList)), (1, 2, 3));
        s.setup_next();
        assert_eq!(s.setup, Step::SignIn);
        s.setup_prev();
        assert_eq!(s.setup, Step::Welcome);
        s.setup_next();
        // The words are AniList's.
        assert!(sign_in_problem(ServiceId::AniList, "AniList refused the sign-in (access_denied)").is_some_and(|(h, _)| h == "AniList said no"));
        // A bad paste says why and keeps waiting.
        s.auth = Auth::SigningIn;
        s.client_id.token.replace_text("not a token".to_string());
        crate::online::sign_in_with_token(&mut s);
        assert!(s.client_id.token_error.as_deref().is_some_and(|e| e.contains("isn't an AniList token")));
        assert_eq!(s.auth, Auth::SigningIn);
        s.apply(Msg::SignedIn { user: "demo_user".into(), picture: None });
        assert_eq!((s.setup, s.account.as_ref().map(|a| a.1)), (Step::Done, Some("AniList")));
    }

    #[test]
    fn a_built_in_id_means_no_missing_client_id() {
        // Without a saved ID, both services (built in) are signed out, not "no Client ID".
        let cfg = kotora_core::config::Config::default();
        assert!(cfg.has_client_id(ServiceId::AniList) && cfg.has_client_id(ServiceId::Mal));
        let mut s = State::fake();
        s.service = ServiceId::AniList;
        s.auth = Auth::SignedOut;
        s.setup = Step::SignIn;
        s.go(Page::Setup);
        assert!(s.setup_closable(), "nothing to register first");
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
