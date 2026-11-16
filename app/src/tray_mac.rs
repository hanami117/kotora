//! The macOS menu bar icon: an `NSStatusItem` whose menu has the detection line (disabled, also
//! the tooltip), Show Kotora and Quit Kotora. Rune has no Mac tray, so this talks to AppKit
//! itself, as Uzumaki's `tray_mac.rs` does.
//!
//! AppKit objects live on the main thread. `TrayCtl` is built and used on the UI thread (the
//! app's init and its update handlers run there, and Rune's UI thread is the main thread on
//! macOS); `MainThreadMarker::new()` fails anywhere else and `start` then returns None.
//!
//! The icon is a template image (black with alpha) cut from the app icon's tiger head, so macOS
//! tints it for light and dark menu bars.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSObject, NSSize, NSString};

use crate::model::Handle;

/// Menu bar icon height in points; the image is twice that in pixels for Retina.
const ICON_PT: f64 = 18.0;
const ICON_PX: u32 = 36;

const SHOW: isize = 1;
const QUIT: isize = 2;

struct Ivars {
    handle: Handle,
}

define_class!(
    // The target of every menu item; the item's tag says which one was chosen.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KotoraTrayTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        #[unsafe(method(trayAction:))]
        fn tray_action(&self, item: &NSMenuItem) {
            let handle = &self.ivars().handle;
            match item.tag() {
                SHOW => super::show_window(handle),
                QUIT => handle.quit(),
                _ => {}
            }
        }
    }
);

pub struct TrayCtl {
    item: Retained<NSStatusItem>,
    target: Retained<Target>,
    mtm: MainThreadMarker,
    /// The detection line, or empty.
    line: String,
}

impl TrayCtl {
    /// None when not on the main thread (AppKit would crash there).
    pub fn start(handle: Handle) -> Option<Self> {
        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("kotora: no menu bar icon: not on the main thread");
            return None;
        };
        let target = mtm.alloc::<Target>().set_ivars(Ivars { handle });
        let target: Retained<Target> = unsafe { msg_send![super(target), init] };
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            button.setToolTip(Some(&NSString::from_str("Kotora")));
            match template_image() {
                Some(image) => button.setImage(Some(&image)),
                None => button.setTitle(&NSString::from_str("K")), // the icon failed to decode
            }
        }
        let ctl = Self { item, target, mtm, line: String::new() };
        ctl.rebuild();
        Some(ctl)
    }

    /// The detection line ("Title · Ep 7") as the menu's first item and the tooltip.
    pub fn set_now(&mut self, line: &str) {
        if line == self.line {
            return;
        }
        self.line = line.to_string();
        if let Some(button) = self.item.button(self.mtm) {
            button.setToolTip(Some(&NSString::from_str(if line.is_empty() { "Kotora" } else { line })));
        }
        self.rebuild();
    }

    fn rebuild(&self) {
        let menu = NSMenu::new(self.mtm);
        // Without this AppKit would grey and enable items by itself; be explicit.
        menu.setAutoenablesItems(false);
        if !self.line.is_empty() {
            let title = NSMenuItem::new(self.mtm);
            title.setTitle(&NSString::from_str(&self.line));
            title.setEnabled(false);
            menu.addItem(&title);
            menu.addItem(&NSMenuItem::separatorItem(self.mtm));
        }
        self.add(&menu, "Show Kotora", SHOW);
        menu.addItem(&NSMenuItem::separatorItem(self.mtm));
        self.add(&menu, "Quit Kotora", QUIT);
        self.item.setMenu(Some(&menu));
    }

    fn add(&self, menu: &NSMenu, title: &str, tag: isize) {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(self.mtm.alloc::<NSMenuItem>(), &NSString::from_str(title), Some(sel!(trayAction:)), &NSString::new())
        };
        item.setTag(tag);
        let target: &AnyObject = &self.target;
        unsafe { item.setTarget(Some(target)) };
        menu.addItem(&item);
    }
}

impl Drop for TrayCtl {
    /// Removes the icon from the menu bar.
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

fn template_image() -> Option<Retained<NSImage>> {
    let (w, h, rgba) = template_pixels()?;
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))?;
    // The pixels are for 2x, so the point size is half and it stays sharp on Retina.
    image.setSize(NSSize::new(ICON_PT * f64::from(w) / f64::from(h), ICON_PT));
    image.setTemplate(true);
    Some(image)
}

/// The 128 px app icon (the desktop entry's).
const ICON_PNG: &[u8] = include_bytes!("../../assets/icons/hicolor/128x128/apps/io.github.hanami117.kotora.png");

fn template_pixels() -> Option<(u32, u32, Vec<u8>)> {
    let src = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png).ok()?.to_rgba8();
    Some(template_from(&src))
}

/// The tiger as black with alpha: warm pixels (the orange head and ears, the cream muzzle, the
/// wooden sword) count, the blue tile and the dark stripes don't, so the stripes show as cuts.
/// Warmth is red minus blue: the tile is about -40, the stripes about 25, the head about 200.
fn template_from(src: &image::RgbaImage) -> (u32, u32, Vec<u8>) {
    const COLD: f32 = 40.0;
    const WARM: f32 = 110.0;
    let mut mask = image::RgbaImage::new(src.width(), src.height());
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in src.enumerate_pixels() {
        let warm = ((f32::from(p[0]) - f32::from(p[2]) - COLD) / (WARM - COLD)).clamp(0.0, 1.0);
        let a = (warm * f32::from(p[3])).round() as u8;
        mask.put_pixel(x, y, image::Rgba([0, 0, 0, a]));
        if a > 40 {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x0 > x1 {
        return (1, 1, vec![0; 4]); // nothing warm found
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let cropped = image::imageops::crop_imm(&mask, x0, y0, w, h).to_image();
    // A square canvas so the head isn't stretched, then the menu bar size.
    let side = w.max(h);
    let mut square = image::RgbaImage::new(side, side);
    image::imageops::replace(&mut square, &cropped, i64::from((side - w) / 2), i64::from((side - h) / 2));
    let out = image::imageops::resize(&square, ICON_PX, ICON_PX, image::imageops::FilterType::Lanczos3);
    (ICON_PX, ICON_PX, out.into_raw())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_template_is_a_black_tiger_without_the_tile() {
        let (w, h, rgba) = super::template_pixels().expect("the icon decodes");
        assert_eq!((w, h), (super::ICON_PX, super::ICON_PX));
        assert!(rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]), "black only; macOS tints it");
        let alpha = |x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize];
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert!(alpha(x, y) < 40, "clear corner at {x},{y}");
        }
        let solid = rgba.chunks(4).filter(|p| p[3] > 200).count();
        let clear = rgba.chunks(4).filter(|p| p[3] < 20).count();
        assert!(solid > 300, "a head, got {solid} solid pixels");
        assert!(clear > 150, "and not a filled square, got {clear} clear pixels");
        if let Some(path) = std::env::var_os("KOTORA_DUMP_TRAY") {
            image::RgbaImage::from_raw(w, h, rgba).unwrap().save(path).unwrap();
        }
    }
}
