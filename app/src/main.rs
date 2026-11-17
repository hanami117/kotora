//! Kotora: a native anime and manga tracker for Linux, macOS and Windows (Rune UI).
//!
//! Stages 1–3: the window with the sidebar and the list page, the saved list in SQLite, MyAnimeList
//! sign-in and sync, edits (inline editor, `+1`, `kot`) through the sync queue, the queue page, the
//! `kot` socket and single instance; stage 4: detection (mpv IPC, MPRIS, VLC's web interface),
//! recognition, the Now Playing page and banner. Headless scenes use fake data.
//!
//! Headless checks (never run the app windowed from an agent):
//! `kotora --snap /tmp/out.png --scene list --size 1280x800 --scale 1` (`--list-scenes` lists them).

// Windows: a windowed program (no console window); `win::attach_output` gives its text a place.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod account;
mod airing;
mod airnotes;
mod art;
mod chrome;
mod autostart;
mod clientid;
mod crashlog;
mod ctl;
mod ctxmenu;
mod detect;
mod details;
mod discord;
mod grid;
mod home;
mod icons;
mod intro;
mod kanji;
mod library;
mod manga;
mod mgrid;
mod volfill;
mod look;
#[cfg(target_os = "macos")]
mod iina;
#[cfg(target_os = "macos")]
mod menu_bar;
mod mirror;
mod model;
mod notes;
mod now;
mod online;
mod play;
mod precise;
mod profile;
mod publishing;
mod queue;
mod remove;
mod scenes;
mod scores;
mod search;
mod settings;
mod setup;
mod theme;
mod transfer;
mod tray;
mod ui;
mod startup;
mod update;
#[cfg(windows)]
mod win;
mod winsetup;
mod winsize;

use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

use rune::{App, AppSpec, ImageCache, Tx, Waker};

use crate::model::State;

/// Wayland app_id, desktop entry, icon name and macOS bundle id.
pub const APP_ID: &str = "io.github.hanami117.kotora";

/// The C2 icon (`assets/icons/`), drawn at sidebar size.
const LOGO: &[u8] = include_bytes!("../../assets/icons/io.github.hanami117.kotora.svg");

/// When `main` started, for the first-frame time.
static START: OnceLock<Instant> = OnceLock::new();

fn setup(s: &mut State, wake: Waker) {
    let cache = ImageCache::new(wake);
    // Rasterised once at 28 pt and 2x; smaller scales use its mips.
    let logo = cache.svg(LOGO, 28.0, 2.0);
    let logo_big = cache.svg(LOGO, 56.0, 2.0);
    if rune::is_headless() {
        // A snap is taken at once: wait for the few-ms rasters so the logo is in it.
        let _ = logo.wait();
        let _ = logo_big.wait();
    }
    s.logo = Some(logo);
    s.logo_big = Some(logo_big);
    s.covers.attach(cache);
}

/// Closing the window hides it to the tray (detection keeps working); without a tray it quits.
/// The tray's Quit, ⌘Q and `kot quit` quit. On macOS Rune also runs this when the app ends
/// (⌘Q, the Dock's Quit); the loop is already exiting then, so hiding still quits.
fn on_close(s: &mut State, tx: &mut Tx) {
    s.remember_page();
    if !s.hide_on_close() {
        crate::discord::shutdown();
        tx.quit();
    }
}

/// macOS: the Dock icon clicked, or Kotora opened again from the Finder.
#[cfg(target_os = "macos")]
fn on_reopen(s: &mut State, has_visible_windows: bool, _tx: &mut Tx) -> bool {
    s.reopen(has_visible_windows)
}

#[cfg(target_os = "macos")]
thread_local! {
    /// The native window, kept (on the UI thread) to repaint its background on a theme change.
    static WINDOW: std::cell::RefCell<Option<rune::WindowHandle>> = const { std::cell::RefCell::new(None) };
}

/// macOS: the window shows; its native background takes the palette's (as in Uzumaki), so a
/// resize never flashes the other theme's colour at the edges.
#[cfg(target_os = "macos")]
fn on_window(handle: &rune::WindowHandle) {
    WINDOW.with(|w| *w.borrow_mut() = Some(handle.clone()));
    // The chrome's appearance (traffic lights, panels, menus) and the palette first.
    chrome::on_window();
    paint_window_bg(handle);
}

/// Called by `theme::apply` on the UI thread when the palette flipped between dark and light.
#[cfg(target_os = "macos")]
pub(crate) fn sync_window_bg() {
    WINDOW.with(|w| {
        if let Some(h) = w.borrow().as_ref() {
            paint_window_bg(h);
        }
    });
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn sync_window_bg() {}

#[cfg(target_os = "macos")]
fn paint_window_bg(handle: &rune::WindowHandle) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use rune::raw_window_handle::RawWindowHandle;

    // Rune colours are linear; NSColor takes sRGB-encoded components (Rune tags its layer sRGB).
    let bg = theme::PAL.bg;
    let [r, g, b] = [bg.r, bg.g, bg.b].map(|c| f64::from(rune::color::srgb_encode(c)));
    let _ = handle.with_raw_handle(|h| {
        let RawWindowHandle::AppKit(h) = h.as_raw() else { return };
        // SAFETY: on the UI (main) thread, with the view alive for the callback.
        unsafe {
            let view = h.ns_view.as_ptr().cast::<AnyObject>();
            let window: Option<Retained<AnyObject>> = msg_send![view, window];
            let Some(window) = window else { return };
            let color: Retained<AnyObject> = msg_send![class!(NSColor), colorWithSRGBRed: r, green: g, blue: b, alpha: 1.0f64];
            let _: () = msg_send![&*window, setBackgroundColor: &*color];
        }
    });
}

/// `KOTORA_FRAME_LOG=FILE`: appends how long the first frame took after start
/// (the lightness budget). Off unless set.
fn log_first_frame() -> impl FnMut(&rune::FrameEncoded) + 'static {
    let mut done = false;
    move |_f| {
        if done {
            return;
        }
        done = true;
        let Some(path) = std::env::var_os("KOTORA_FRAME_LOG").filter(|p| !p.is_empty()) else { return };
        let ms = START.get().map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "first frame {ms:.1} ms after start{}", if rune::is_headless() { " (headless)" } else { "" });
        }
    }
}

fn main() {
    START.get_or_init(Instant::now);
    #[cfg(windows)]
    win::attach_output();
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("kotora {}", kotora_ctl::version_line());
        return;
    }
    rune::set_app_id(APP_ID);
    // Windows: the same id for the taskbar and the toasts (the installer's shortcut has it too).
    #[cfg(windows)]
    if !rune::is_headless() {
        win::set_app_id();
    }
    // The window opens at `winsize::DEFAULT` cut to the screen; headless keeps 1280×800 (every
    // snap and script is made for it, and `--size` overrides it).
    let min_size = (960.0, 600.0);
    let size = if rune::is_headless() { (1280.0, 800.0) } else { winsize::initial(min_size) };
    let spec = AppSpec::<State> {
        scenes: scenes::SCENES,
        setup: Some(setup),
        poll: Some(ui::poll),
        on_key: Some(ui::on_key),
        min_size: Some(min_size),
        ..AppSpec::new("Kotora \u{5c0f}\u{864e}", size, State::fake, ui::view)
    };
    let app = App::new(spec).tab_visits_controls(true).on_frame_encoded(log_first_frame());
    // macOS: the traffic lights pinned in a band at the sidebar's top (`theme::TRAFFIC_LIGHTS`),
    // clear of the logo, and the menu bar: Rune's standard Edit and Window menus plus Kotora's
    // own items (`menu_bar.rs`). Linux keeps the desktop's frame and is not touched.
    #[cfg(target_os = "macos")]
    let app = menu_bar::install(
        app.chrome(rune::Chrome { traffic_lights: Some(theme::TRAFFIC_LIGHTS), ..rune::Chrome::DEFAULT }).standard_menus(),
    );

    // Headless (`--snap`, `--tree`, `--script`): no socket, no single-instance check, nothing
    // that touches the user's setup.
    if rune::is_headless() {
        app.run();
        return;
    }
    // A panic in a start with no terminal (`--hidden`, at login) also goes to crash.log.
    crashlog::install();
    // `--hidden`: start with no window (`kot show` opens it).
    let hidden = std::env::args().any(|a| a == "--hidden");
    // A second copy for tests, next to the user's: no single-instance check (pair it with a
    // scratch KOTORA_CTL_SOCKET and XDG_* dirs, and KOTORA_OFFLINE=1).
    let test_instance = std::env::var_os("KOTORA_TEST_INSTANCE").is_some_and(|v| !v.is_empty());
    if !test_instance && ctl::other_copy_running(!hidden) {
        eprintln!("kotora: already running{}", if hidden { "" } else { "; showed its window" });
        return;
    }
    // macOS start at login in the background: a login item gets no `--hidden`, so with both
    // switches on (Settings → Startup) Kotora starts windowless and `autostart::watch_launch`
    // opens the window at once unless macOS says this launch is the login one (as Uzumaki does).
    // Linux and Windows write `--hidden` into their login entry instead.
    #[cfg(target_os = "macos")]
    let login_check = !hidden
        && !test_instance
        && kotora_core::config::Config::load(&kotora_core::paths::config_file()).unwrap_or_default().startup.background
        && autostart::read().on;
    #[cfg(not(target_os = "macos"))]
    let login_check = false;
    let windowless = hidden || login_check;
    let app = if windowless { app.windowless() } else { app };
    let app = app.keep_running().on_close(on_close);
    #[cfg(target_os = "macos")]
    let app = app.on_reopen(on_reopen).on_window(on_window);
    // Windows: the window gets the program's icon (title bar, taskbar, Alt+Tab).
    #[cfg(windows)]
    let app = app.on_window(win::on_window);
    let handle = app.handle();
    // The startup intro's clock starts at the first frame presented (`intro::on_frame`); the
    // first-frame log keeps running beside it.
    let app = {
        let (mut log, mut intro) = (log_first_frame(), intro::on_frame(handle.clone()));
        app.on_frame_encoded(move |f| {
            log(f);
            intro(f);
        })
    };
    // macOS: a background start is a menu bar app until the window shows (no Dock icon).
    #[cfg(target_os = "macos")]
    if hidden {
        handle.set_dock_icon(false);
    }
    // `kot` talks to this socket; it goes away with `_ctl` when the app ends.
    let _ctl = ctl::start(handle.clone());
    #[cfg(target_os = "macos")]
    if login_check {
        let h = handle.clone();
        autostart::watch_launch(move |at_login| {
            eprintln!("kotora: {}", if at_login { "started at login, in the background" } else { "not a login start: opening the window" });
            h.update(move |s: &mut State, _| s.login_launch(at_login));
        });
    }
    let app = app.init(move || {
        let service = account::load();
        let mut s = State::start(online::initial_auth());
        s.service = service;
        s.client_id.hint = clientid::saved_hint(service);
        s.setup_service = s.auth == model::Auth::NoClientId && online::wants_service_choice();
        s.setup = s.setup_first();
        s.covers.go_live(handle.clone(), !kotora_core::paths::offline_forced());
        // Desktop notifications, with buttons where the system has them (`notes/`).
        s.notes = notes::Notifier::start(handle.clone());
        s.handle = Some(handle);
        s.window = !windowless;
        // The startup intro: a cold start that opens a window (a macOS launch checked for a
        // login start keeps it until that is known: `State::login_launch`).
        s.intro = intro::at_launch(intro::Launch { enabled: s.prefs.intro, hidden, test_instance, headless: false });
        // The tray or menu bar icon (UI thread: AppKit needs the main thread on macOS).
        s.start_tray();
        // The saved list first, then a sync when signed in (workers, off the UI thread).
        online::start(&mut s);
        // Players: sources and recognition on their own threads, event driven.
        detect::start(&mut s);
        library::start(&mut s);
        // macOS: the chrome follows a forced theme, before the window shows (`chrome.rs`).
        chrome::follow(s.prefs.theme);
        // Started airing and New episode notifications, from the saved list (no requests).
        airnotes::start(&mut s);
        // New chapter notifications (Manga on and the switch on only; MangaUpdates, `publishing.rs`).
        s.publishing_start();
        // macOS: the old LaunchAgent moves over to the login item, once (a worker thread; never
        // in a test instance, which has its own scratch setup).
        #[cfg(target_os = "macos")]
        if !test_instance && let Some(h) = &s.handle {
            let h = h.clone();
            let _ = std::thread::Builder::new().name("kotora-login-migrate".into()).spawn(move || {
                if let Some(login) = autostart::migrate_old_entry() {
                    h.update_if(move |s: &mut State, _| {
                        s.prefs.login = login;
                        true
                    });
                }
            });
        }
        // The daily update check (after the settings are read; `update.rs`).
        s.start_updates();
        s
    });
    app.run();
}
