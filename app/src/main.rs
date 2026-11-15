//! Kotora: a native anime tracker for Linux and macOS (Rune UI).
//!
//! Stage 2: the window with the sidebar and the list page, the saved list in SQLite, MyAnimeList
//! sign-in and a read-only sync, the `kot` socket and single instance. Headless scenes use fake data.
//! No edits to the MAL account and no detection yet.
//!
//! Headless checks (never run the app windowed from an agent):
//! `kotora --snap /tmp/out.png --scene list --size 1280x800 --scale 1` (`--list-scenes` lists them).

mod ctl;
mod icons;
mod model;
mod online;
mod scenes;
mod theme;
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
    s.images = Some(cache);
}

/// No tray yet: closing the window quits.
fn on_close(_s: &mut State, tx: &mut Tx) {
    tx.quit();
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
    // clear of the logo. Linux keeps the desktop's frame and is not touched.
    #[cfg(target_os = "macos")]
    let app = app
        .chrome(rune::Chrome { traffic_lights: Some(theme::TRAFFIC_LIGHTS), ..rune::Chrome::DEFAULT })
        .standard_menus();

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
    let handle = app.handle();
    // `kot` talks to this socket; it goes away with `_ctl` when the app ends.
    let _ctl = ctl::start(handle.clone());
    let app = app.init(move || {
        let mut s = State::start(online::initial_auth());
        s.handle = Some(handle);
        s.window = !hidden;
        // The saved list first, then a sync when signed in (workers, off the UI thread).
        online::start(&mut s);
        s
    });
    app.run();
}
