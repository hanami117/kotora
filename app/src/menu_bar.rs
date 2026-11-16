//! The macOS menu bar (Rune's native menus, macOS only).
//!
//! Rune and winit install most of it: winit's app menu (Services, Hide ⌘H, Hide Others, Show
//! All, Quit ⌘Q) and Rune's standard Edit menu (Undo … Select All, keys to text fields first) and
//! Window menu (Minimize ⌘M, Close ⌘W), from `App::standard_menus` in `main.rs`. This module adds
//! Kotora's own items: About Kotora and Settings… ⌘, in the app menu, and a Go menu with the
//! sidebar's pages. `tidy` then fixes the app menu with AppKit: one About (ours), and Hide and
//! Quit named "Kotora" (winit names them after the process, "kotora").
//!
//! Every item with a key is `keys_first`: the press goes through Rune first (a text field, then
//! `ui::on_key`), as without the menu, and the item runs only if nothing took the key. ⌘K and ⌘F
//! are `ui::on_key`'s own keys; ⌘, and ⌘1–⌘5 only exist here.
//!
//! Same pattern as Uzumaki's `ui/menu_bar.rs` and `menu_bar_mac.rs`, smaller: Kotora's items
//! never change, so they are set once at start (`App::menu_item`) instead of synced each frame.

use std::cell::Cell;

use kotora_core::Status;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{NSDictionary, NSString};
use rune::{App, KeyPlatform, MenuItem, Shortcut, Tx};

use crate::model::{Page, State};

/// One item Kotora adds. `menu` None is the app menu; `title` "-" is a separator.
#[derive(Clone, Copy)]
pub struct Entry {
    pub menu: Option<&'static str>,
    pub id: &'static str,
    pub title: &'static str,
    /// Rune chord syntax ("mod" is ⌘).
    pub chord: Option<&'static str>,
    pub action: fn(&mut State, &mut Tx),
}

const fn item(menu: Option<&'static str>, id: &'static str, title: &'static str, chord: Option<&'static str>, action: fn(&mut State, &mut Tx)) -> Entry {
    Entry { menu, id, title, chord, action }
}

const fn sep(menu: Option<&'static str>) -> Entry {
    Entry { menu, id: "", title: "-", chord: None, action: |_, _| {} }
}

const GO: Option<&str> = Some("Go");

/// Every item Kotora adds, in menu order (Rune keeps the order within a menu).
pub const ENTRIES: &[Entry] = &[
    item(None, "app.about", "About Kotora", None, |_, _| about()),
    sep(None),
    item(None, "app.settings", "Settings\u{2026}", Some("mod+comma"), |s, tx| go(s, tx, Page::Settings)),
    sep(None),
    item(GO, "go.search", "Search", Some("mod+k"), |s, tx| go(s, tx, Page::Search)),
    item(GO, "go.find", "Find in List", Some("mod+f"), find),
    sep(GO),
    item(GO, "go.watching", "Watching", Some("mod+1"), |s, tx| go(s, tx, Page::List(Status::Watching))),
    item(GO, "go.plan", "Plan to Watch", Some("mod+2"), |s, tx| go(s, tx, Page::List(Status::PlanToWatch))),
    item(GO, "go.completed", "Completed", Some("mod+3"), |s, tx| go(s, tx, Page::List(Status::Completed))),
    item(GO, "go.on_hold", "On Hold", Some("mod+4"), |s, tx| go(s, tx, Page::List(Status::OnHold))),
    item(GO, "go.dropped", "Dropped", Some("mod+5"), |s, tx| go(s, tx, Page::List(Status::Dropped))),
    sep(GO),
    item(GO, "go.now_playing", "Now Playing", None, |s, tx| go(s, tx, Page::NowPlaying)),
    item(GO, "go.airing", "Airing", None, |s, tx| go(s, tx, Page::Airing)),
    item(GO, "go.queue", "Sync Queue", None, |s, tx| go(s, tx, Page::Queue)),
];

/// The sidebar's way to a page, so a menu pick and a click do the same.
fn go(s: &mut State, tx: &mut Tx, page: Page) {
    crate::ui::go_to(s, page, tx);
}

/// ⌘F as `ui::on_key` has it: the list's filter box on a list page, else the search page. Picking
/// the item does the same as the key.
fn find(s: &mut State, tx: &mut Tx) {
    let key = rune::KeyEvent { key: rune::Key::Command('f'), mods: rune::Mods { cmd: true, ..Default::default() } };
    crate::ui::on_key(s, &key, tx);
}

fn shortcut(chord: &str) -> Option<Shortcut> {
    Shortcut::parse_on(chord, KeyPlatform::Mac).ok()
}

/// Adds the items to the app (before `run`).
pub fn install(mut app: App<State>) -> App<State> {
    for e in ENTRIES {
        let mut m = if e.title == "-" { MenuItem::separator() } else { MenuItem::new(e.title, e.action).id(e.id).keys_first() };
        if let Some(menu) = e.menu {
            m = m.menu(menu);
        }
        if let Some(sc) = e.chord.and_then(shortcut) {
            m = m.key_equivalent(sc);
        }
        app = app.menu_item(m);
    }
    app
}

thread_local! {
    /// `tidy` has nothing left to do.
    static TIDY_DONE: Cell<bool> = const { Cell::new(false) };
}

/// From the view (the main thread on macOS): tidies the app menu until it is done. Nothing
/// headless.
pub fn sync() {
    if rune::is_headless() || TIDY_DONE.get() {
        return;
    }
    TIDY_DONE.set(tidy());
}

/// The standard About panel: the bundle's name and icon, the binary's version, the licence.
fn about() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let keys = [NSString::from_str("ApplicationName"), NSString::from_str("ApplicationVersion"), NSString::from_str("Version"), NSString::from_str("Copyright")];
    let values = [
        NSString::from_str("Kotora"),
        NSString::from_str(env!("CARGO_PKG_VERSION")),
        NSString::from_str(&format!("{}{}", kotora_ctl::BUILD, kotora_ctl::dev_suffix())),
        NSString::from_str("A tribute to Taiga, the Windows anime tracker.\nFree software under the GNU GPL, version 3 or later."),
    ];
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&NSString> = values.iter().map(|v| &**v).collect();
    let options = NSDictionary::from_slices(&keys, &values);
    // SAFETY: main thread; the keys are AppKit's NSAboutPanelOption* strings, the values NSStrings.
    unsafe {
        let _: () = objc2::msg_send![&*app, orderFrontStandardAboutPanelWithOptions: &*options];
    }
}

/// Tidies the app menu winit made; true once nothing is left to do. Cheap and idempotent.
fn tidy() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return true };
    let app = NSApplication::sharedApplication(mtm);
    let Some(root) = app.mainMenu() else { return false };
    let Some(menu) = root.itemAtIndex(0).and_then(|i| i.submenu()) else { return false };
    let ours = sel!(runeMenuAction:);
    let mut i = 0;
    while i < menu.numberOfItems() {
        let Some(item) = menu.itemAtIndex(i) else { break };
        let action = item.action();
        if action == Some(sel!(orderFrontStandardAboutPanel:)) && action != Some(ours) {
            // winit's About and the line under it.
            menu.removeItemAtIndex(i);
            if menu.itemAtIndex(i).is_some_and(|n| n.isSeparatorItem()) {
                menu.removeItemAtIndex(i);
            }
            continue;
        }
        let title = if action == Some(sel!(hide:)) {
            Some("Hide Kotora")
        } else if action == Some(sel!(terminate:)) {
            Some("Quit Kotora")
        } else {
            None
        };
        if let Some(t) = title.filter(|t| item.title().to_string() != *t) {
            item.setTitle(&NSString::from_str(t));
        }
        // A line between Services and Hide, as every Mac app has.
        if item.submenu().is_some() && item.title().to_string() == "Services" && menu.itemAtIndex(i + 1).is_some_and(|n| !n.isSeparatorItem()) {
            menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), i + 1);
        }
        i += 1;
    }
    // The Window menu becomes AppKit's windows menu (it lists the window, Bring All to Front).
    if app.windowsMenu().is_none() {
        let window: Option<objc2::rc::Retained<NSMenu>> =
            (0..root.numberOfItems()).filter_map(|i| root.itemAtIndex(i)).find(|i| i.title().to_string() == "Window").and_then(|i| i.submenu());
        match window {
            Some(m) => app.setWindowsMenu(Some(&m)),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(menu: Option<&str>) -> Vec<&'static str> {
        ENTRIES.iter().filter(|e| e.menu == menu).map(|e| e.title).collect()
    }

    #[test]
    fn the_menus_hold_the_agreed_items_in_order() {
        assert_eq!(titles(None), ["About Kotora", "-", "Settings\u{2026}", "-"]);
        assert_eq!(
            titles(GO),
            ["Search", "Find in List", "-", "Watching", "Plan to Watch", "Completed", "On Hold", "Dropped", "-", "Now Playing", "Airing", "Sync Queue"]
        );
        // Ids are unique (Rune refuses the whole menu otherwise).
        let mut ids: Vec<_> = ENTRIES.iter().filter(|e| e.title != "-").map(|e| e.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn every_key_is_a_menu_shortcut_and_settings_is_cmd_comma() {
        for e in ENTRIES.iter().filter(|e| e.chord.is_some()) {
            assert!(shortcut(e.chord.unwrap()).is_some(), "{} parses", e.id);
        }
        let settings = ENTRIES.iter().find(|e| e.id == "app.settings").unwrap();
        assert_eq!(shortcut(settings.chord.unwrap()).unwrap(), "cmd+comma".parse::<Shortcut>().unwrap());
    }

    #[test]
    fn items_go_to_their_pages() {
        let mut s = State::fake();
        let mut tx = Tx::default();
        for (id, page) in [
            ("app.settings", Page::Settings),
            ("go.search", Page::Search),
            ("go.plan", Page::List(Status::PlanToWatch)),
            ("go.dropped", Page::List(Status::Dropped)),
            ("go.now_playing", Page::NowPlaying),
            ("go.airing", Page::Airing),
            ("go.queue", Page::Queue),
            ("go.watching", Page::List(Status::Watching)),
        ] {
            (ENTRIES.iter().find(|e| e.id == id).unwrap().action)(&mut s, &mut tx);
            assert_eq!(s.page, page, "{id}");
        }
        // Find in List on a list page focuses the filter box and stays.
        (ENTRIES.iter().find(|e| e.id == "go.find").unwrap().action)(&mut s, &mut tx);
        assert_eq!(s.page, Page::List(Status::Watching));
        assert!(s.query.focused);
    }
}
