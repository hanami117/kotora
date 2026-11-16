//! The window's size at first launch. Kotora doesn't remember the last size or position (nor does
//! Uzumaki), so every launch opens at this: big enough for two full rows of Large posters and the
//! toolbar with a little margin (about 1280×888 pt fits them exactly), cut down to the screen's
//! usable area (not under the menu bar or the Dock), and centred by the system on open.

/// The size the layout wants.
pub const DEFAULT: (f32, f32) = (1360.0, 900.0);

/// Room kept off the usable area's height: a framed window's title bar is outside the content
/// size, and a window flush to the edges looks wrong.
const MARGIN_H: f32 = 40.0;
const MARGIN_W: f32 = 24.0;

/// `want`, cut down to the usable area `visible` (width, height) less a margin, but never below
/// `min` (the window's own minimum, which wins on a tiny screen). With no screen known, `want`.
pub fn fit(want: (f32, f32), visible: Option<(f32, f32)>, min: (f32, f32)) -> (f32, f32) {
    let Some((vw, vh)) = visible else { return want };
    ((want.0.min(vw - MARGIN_W)).max(min.0), (want.1.min(vh - MARGIN_H)).max(min.1))
}

/// The size to open at: the default fitted to the main screen's usable area (macOS; elsewhere
/// the compositor places and limits it).
pub fn initial(min: (f32, f32)) -> (f32, f32) {
    fit(DEFAULT, if rune::is_headless() { None } else { visible_screen() }, min)
}

/// The main screen's visible frame in points: its size less the menu bar and the Dock.
#[cfg(target_os = "macos")]
fn visible_screen() -> Option<(f32, f32)> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSRect;

    // SAFETY: `+[NSScreen mainScreen]` and `-visibleFrame` take no arguments and return an
    // object (or nil) and a plain struct; both are safe to ask from the main thread, where
    // `main` runs this before the event loop starts.
    let frame = unsafe {
        let screen: Option<Retained<AnyObject>> = msg_send![class!(NSScreen), mainScreen];
        let frame: NSRect = msg_send![&*screen?, visibleFrame];
        frame
    };
    let (w, h) = (frame.size.width as f32, frame.size.height as f32);
    (w > 0.0 && h > 0.0).then_some((w, h))
}

#[cfg(not(target_os = "macos"))]
fn visible_screen() -> Option<(f32, f32)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: (f32, f32) = (960.0, 600.0);

    #[test]
    fn a_big_screen_gets_the_default() {
        assert_eq!(fit(DEFAULT, Some((2560.0, 1415.0)), MIN), DEFAULT);
        assert_eq!(fit(DEFAULT, None, MIN), DEFAULT, "no screen known");
    }

    #[test]
    fn a_small_screen_cuts_it_to_the_usable_area_with_a_margin() {
        // A 13-inch MacBook's usable area (menu bar and Dock out).
        let (w, h) = fit(DEFAULT, Some((1440.0, 800.0)), MIN);
        assert_eq!(w, DEFAULT.0.min(1440.0 - MARGIN_W));
        assert_eq!(h, 800.0 - MARGIN_H);
        assert!(w <= 1440.0 && h <= 800.0);
    }

    #[test]
    fn the_windows_minimum_wins_on_a_tiny_screen() {
        assert_eq!(fit(DEFAULT, Some((800.0, 500.0)), MIN), MIN);
    }

    #[test]
    fn the_default_holds_two_large_rows_with_room_over_the_known_fit() {
        // 1280×888 pt showed two full Large rows and the toolbar exactly.
        const { assert!(DEFAULT.0 > 1280.0 && DEFAULT.1 > 888.0) };
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mac_tests {
    #[test]
    fn the_main_screens_usable_area_is_sane_or_unknown() {
        // No window is made: this only asks AppKit for the screen's frame (none in a headless CI).
        if let Some((w, h)) = super::visible_screen() {
            assert!((200.0..20000.0).contains(&w) && (200.0..20000.0).contains(&h), "{w}x{h}");
            eprintln!("visible screen {w}x{h}");
        }
    }
}
