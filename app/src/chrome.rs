//! macOS: the system's own parts of the app follow Settings → Appearance → Theme. The traffic
//! lights, the title bar, native panels (the library's Choose…), tooltips and AppKit menus take
//! `NSApplication.appearance`: Aqua for Light, DarkAqua for Dark, nil (follow macOS) for System.
//! Set at start before the window shows, and on every theme change. The native window's
//! background (`paint_window_bg`) is repainted from the same palette, so they always agree.
//!
//! Rune's `system_dark` on macOS is the window's effective appearance, so a forced one reads
//! back as the same theme, and System (nil) follows macOS again at once. Linux has nothing to do.

use kotora_core::config::Theme;

/// The `NSAppearanceName` for a theme; `None`: follow the system (nil).
#[cfg(any(target_os = "macos", test))]
pub fn appearance_name(theme: Theme) -> Option<&'static str> {
    match theme {
        Theme::System => None,
        Theme::Light => Some("NSAppearanceNameAqua"),
        Theme::Dark => Some("NSAppearanceNameDarkAqua"),
    }
}

/// Puts the theme on the app's chrome (macOS, UI thread). Safe to call again with the same one.
#[cfg(target_os = "macos")]
pub fn follow(theme: Theme) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, class, msg_send};
    use objc2_foundation::NSString;

    if rune::is_headless() || MainThreadMarker::new().is_none() {
        return;
    }
    CURRENT.set(Some(theme));
    // SAFETY: main thread (checked); NSApplication exists (winit made it before the app's init).
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let appearance: Option<Retained<AnyObject>> = match appearance_name(theme) {
            Some(name) => msg_send![class!(NSAppearance), appearanceNamed: &*NSString::from_str(name)],
            None => None,
        };
        let _: () = msg_send![&*app, setAppearance: appearance.as_deref()];
    }
    // Forced: the palette is known now, so the window's background follows before the next
    // frame. System waits for the window's appearance (the next frame's `theme::apply`).
    if theme != Theme::System {
        crate::theme::apply(theme == Theme::Light);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn follow(_theme: Theme) {}

#[cfg(target_os = "macos")]
thread_local! {
    /// The theme last put on the chrome, for a window made later (`on_window`).
    static CURRENT: std::cell::Cell<Option<Theme>> = const { std::cell::Cell::new(None) };
}

/// A native window was made: the chrome's theme again, and its palette for the background
/// (with System, from macOS's appearance now, so the first paint is already right).
#[cfg(target_os = "macos")]
pub fn on_window() {
    let theme = CURRENT.get().unwrap_or_default();
    follow(theme);
    if theme == Theme::System
        && let Some(light) = system_light()
    {
        crate::theme::apply(light);
    }
}

/// Whether the app's effective appearance is light (with nil: macOS's own setting).
#[cfg(target_os = "macos")]
fn system_light() -> Option<bool> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, class, msg_send};
    use objc2_foundation::{NSArray, NSString};

    MainThreadMarker::new()?;
    let (aqua, dark) = (NSString::from_str("NSAppearanceNameAqua"), NSString::from_str("NSAppearanceNameDarkAqua"));
    let names = NSArray::from_slice(&[&*aqua, &*dark]);
    // SAFETY: main thread (checked); NSApplication exists once a window does.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let eff: Option<Retained<AnyObject>> = msg_send![&*app, effectiveAppearance];
        let best: Option<Retained<NSString>> = msg_send![&*eff?, bestMatchFromAppearancesWithNames: &*names];
        Some(best?.to_string() == aqua.to_string())
    }
}

/// macOS: the folder picker (Settings → Library → Choose…) as an `NSOpenPanel` in Kotora's own
/// process, so it takes the app's appearance (an `osascript` dialog is another app and follows
/// macOS only). A sheet on the window when there is one; it never blocks the UI thread.
/// `done` runs on the UI thread with the chosen path, or None when cancelled.
#[cfg(target_os = "macos")]
pub fn pick_folder(prompt: &str, done: impl Fn(Option<String>) + 'static) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{MainThreadMarker, class, msg_send};
    use objc2_foundation::NSString;

    MainThreadMarker::new().ok_or("Choose\u{2026} works from the window only.")?;
    // SAFETY: main thread (checked); the panel is kept alive by the block until it ends.
    unsafe {
        let panel: Retained<AnyObject> = msg_send![class!(NSOpenPanel), openPanel];
        let _: () = msg_send![&*panel, setCanChooseDirectories: Bool::YES];
        let _: () = msg_send![&*panel, setCanChooseFiles: Bool::NO];
        let _: () = msg_send![&*panel, setAllowsMultipleSelection: Bool::NO];
        let _: () = msg_send![&*panel, setCanCreateDirectories: Bool::NO];
        let _: () = msg_send![&*panel, setMessage: &*NSString::from_str(prompt)];
        let _: () = msg_send![&*panel, setPrompt: &*NSString::from_str("Choose")];
        let kept = panel.clone();
        let handler = RcBlock::new(move |response: isize| {
            // NSModalResponseOK
            let path = (response == 1).then(|| {
                let url: Option<Retained<AnyObject>> = msg_send![&*kept, URL];
                let path: Option<Retained<NSString>> = url.and_then(|u| msg_send![&*u, path]);
                path.map(|p| p.to_string())
            });
            done(path.flatten());
        });
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let window: Option<Retained<AnyObject>> = msg_send![&*app, mainWindow];
        match window {
            Some(w) => {
                let _: () = msg_send![&*panel, beginSheetModalForWindow: &*w, completionHandler: &*handler];
            }
            None => {
                let _: () = msg_send![&*app, activateIgnoringOtherApps: Bool::YES];
                let _: () = msg_send![&*panel, beginWithCompletionHandler: &*handler];
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forced_theme_forces_the_appearance_and_system_follows_macos() {
        assert_eq!(appearance_name(Theme::Light), Some("NSAppearanceNameAqua"));
        assert_eq!(appearance_name(Theme::Dark), Some("NSAppearanceNameDarkAqua"));
        assert_eq!(appearance_name(Theme::System), None, "nil: follow macOS");
        // The palette agrees with the forced appearance whatever macOS says.
        for dark_mac in [true, false] {
            assert!(Theme::Light.is_light(dark_mac));
            assert!(!Theme::Dark.is_light(dark_mac));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_appearance_names_are_appkits() {
        // NSAppearanceNameAqua's value is its own name (AppKit headers); check against AppKit.
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        for t in [Theme::Light, Theme::Dark] {
            let name = NSString::from_str(appearance_name(t).unwrap());
            let a: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSAppearance), appearanceNamed: &*name] };
            let a = a.expect("AppKit knows the name");
            let back: Retained<NSString> = unsafe { msg_send![&*a, name] };
            assert_eq!(back.to_string(), name.to_string());
        }
    }
}
