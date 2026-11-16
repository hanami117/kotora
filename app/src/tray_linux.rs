//! The system tray icon on Linux (StatusNotifierItem through Rune): the detection line (also
//! the tooltip), Show Kotora and Quit Kotora. A left click shows the window.
//!
//! The tray runs on its own thread; its events reach the UI thread through the handle, so they
//! work while the window is closed (`App::keep_running`). It shows only where a tray host runs
//! (KDE, waybar and most bars; GNOME needs the AppIndicator extension). Without one the icon
//! never shows; starting Kotora again (or `kot show`) still brings the window back.

use rune::Tray;
use rune::tray::{TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::model::Handle;

/// Tray icon side in pixels; hosts scale it.
const ICON_PX: u32 = 64;

/// The 64 px app icon (the same file the desktop entry uses).
const ICON_PNG: &[u8] = include_bytes!("../../assets/icons/hicolor/64x64/apps/io.github.hanami117.kotora.png");

pub struct TrayCtl {
    tray: Tray,
    line: String,
}

impl TrayCtl {
    /// None when D-Bus is unavailable.
    pub fn start(handle: Handle) -> Option<Self> {
        let spec = TraySpec::new("kotora", "Kotora", icon()).tooltip("Kotora").menu(menu(""));
        let on_event = move |ev: TrayEvent| match ev {
            TrayEvent::Activate { token } => {
                match token {
                    Some(t) => handle.activate(t),
                    None => handle.open(),
                }
                handle.update(|s: &mut crate::model::State, _| s.window = true);
            }
            TrayEvent::Menu(id) => match id.as_str() {
                "show" => super::show_window(&handle),
                "quit" => handle.quit(),
                _ => {}
            },
            _ => {}
        };
        match Tray::new(spec, on_event) {
            Ok(tray) => Some(Self { tray, line: String::new() }),
            Err(e) => {
                eprintln!("kotora: no tray icon: {e}");
                None
            }
        }
    }

    /// The detection line ("Title · Ep 7") as the menu's first item and the tooltip.
    pub fn set_now(&mut self, line: &str) {
        if line != self.line {
            self.line = line.to_string();
            self.tray.set_tooltip(if line.is_empty() { "Kotora".to_string() } else { line.to_string() });
            self.tray.set_menu(menu(line));
        }
    }
}

fn menu(line: &str) -> Vec<TrayItem> {
    let mut v = Vec::new();
    if !line.is_empty() {
        v.push(TrayItem::action("now", line).enabled(false));
        v.push(TrayItem::Separator);
    }
    v.extend([TrayItem::action("show", "Show Kotora"), TrayItem::Separator, TrayItem::action("quit", "Quit Kotora")]);
    v
}

fn icon() -> TrayIcon {
    TrayIcon::rgba(ICON_PX, ICON_PX, icon_pixels())
}

/// Straight RGBA rows of the icon (decoded from the PNG built into the binary).
fn icon_pixels() -> Vec<u8> {
    match image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png) {
        Ok(img) => img.resize_exact(ICON_PX, ICON_PX, image::imageops::FilterType::Lanczos3).to_rgba8().into_raw(),
        Err(_) => vec![0; (ICON_PX * ICON_PX * 4) as usize],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_decodes_at_tray_size() {
        let rgba = super::icon_pixels();
        assert_eq!(rgba.len(), (super::ICON_PX * super::ICON_PX * 4) as usize);
        assert!(rgba.chunks(4).any(|p| p[3] == 255), "not blank");
    }

    #[test]
    fn the_menu_has_the_line_only_while_something_plays() {
        use rune::tray::TrayItem;
        assert_eq!(super::menu("").len(), 3);
        let m = super::menu("Frieren \u{b7} Ep 7");
        assert_eq!(m.len(), 5);
        assert!(matches!(&m[0], TrayItem::Action { enabled: false, .. }));
    }
}
