//! Kotora: a native anime tracker for Linux and macOS (Rune UI).
//!
//! Stages 1–3: the window with the sidebar and the list page, the saved list in SQLite, MyAnimeList
//! sign-in and sync, edits (inline editor, `+1`, `kot`) through the sync queue, the queue page, the
//! `kot` socket and single instance; stage 4: detection (mpv IPC, MPRIS, VLC's web interface),
//! recognition, the Now Playing page and banner. Headless scenes use fake data.
//!
//! Headless checks (never run the app windowed from an agent):
//! `kotora --snap /tmp/out.png --scene list --size 1280x800 --scale 1` (`--list-scenes` lists them).

mod airing;
mod art;
mod clientid;
mod ctl;
mod detect;
mod details;
mod icons;
#[cfg(target_os = "macos")]
mod iina;
#[cfg(target_os = "macos")]
mod menu_bar;
mod model;
mod now;
mod online;
mod precise;
mod queue;
mod scenes;
mod search;
mod setup;
mod theme;
mod tray;
mod ui;

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
    if !s.hide_on_close() {
        tx.quit();
    }
}

/// macOS: the Dock icon clicked, or Kotora opened again from the Finder.
#[cfg(target_os = "macos")]
fn on_reopen(s: &mut State, has_visible_windows: bool, _tx: &mut Tx) -> bool {
    s.reopen(has_visible_windows)
}

/// `KOTORA_FRAME_LOG=FILE`: appends how long the first frame took after start (the lightness
/// budget, docs/ROADMAP.md). Off unless set.
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
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("kotora {}", kotora_ctl::version_line());
        return;
    }
    rune::set_app_id(APP_ID);
    let spec = AppSpec::<State> {
        scenes: scenes::SCENES,
        setup: Some(setup),
        poll: Some(ui::poll),
        on_key: Some(ui::on_key),
        min_size: Some((960.0, 600.0)),
        ..AppSpec::new("Kotora", (1280.0, 800.0), State::fake, ui::view)
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
    // `--hidden`: start with no window (`kot show` opens it).
    let hidden = std::env::args().any(|a| a == "--hidden");
    // A second copy for tests, next to the user's: no single-instance check (pair it with a
    // scratch KOTORA_CTL_SOCKET and XDG_* dirs, and KOTORA_OFFLINE=1).
    let test_instance = std::env::var_os("KOTORA_TEST_INSTANCE").is_some_and(|v| !v.is_empty());
    if !test_instance && ctl::other_copy_running(!hidden) {
        eprintln!("kotora: already running{}", if hidden { "" } else { "; showed its window" });
        return;
    }
    let app = if hidden { app.windowless() } else { app };
    let app = app.keep_running().on_close(on_close);
    #[cfg(target_os = "macos")]
    let app = app.on_reopen(on_reopen);
    let handle = app.handle();
    // macOS: a background start is a menu bar app until the window shows (no Dock icon).
    #[cfg(target_os = "macos")]
    if hidden {
        handle.set_dock_icon(false);
    }
    // `kot` talks to this socket; it goes away with `_ctl` when the app ends.
    let _ctl = ctl::start(handle.clone());
    let app = app.init(move || {
        let mut s = State::start(online::initial_auth());
        s.client_id.hint = clientid::saved_hint();
        s.covers.go_live(handle.clone(), !kotora_core::paths::offline_forced());
        s.handle = Some(handle);
        s.window = !hidden;
        // The tray or menu bar icon (UI thread: AppKit needs the main thread on macOS).
        s.start_tray();
        // The saved list first, then a sync when signed in (workers, off the UI thread).
        online::start(&mut s);
        // Players: sources and recognition on their own threads, event driven.
        detect::start(&mut s);
        s
    });
    app.run();
}
