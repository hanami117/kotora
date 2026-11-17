//! The system tray icon on Linux (StatusNotifierItem through Rune): the detection line (also
//! the tooltip), Show Kotora, Quit Kotora, then Play next (a caption and one item per anime:
//! Rune's tray has no submenus). A left click shows the window.
//!
//! **The menu's shape never changes kind by position.** Rune's dbusmenu numbers the items by
//! position (index + 1) and sends only the properties that differ from the defaults (no
//! `enabled` on an enabled item, no `type` on an action). Hosts keep their item for an id they
//! already know and update only the properties sent, so when the now-playing line came and went
//! or the Play next list changed, an id that had been a separator came back as an episode with a
//! label (KDE Plasma drew it as a bold section with a line), an id that had been the disabled
//! caption stayed greyed out, and a click could land on another item than the one shown. So
//! every position always holds the same kind: the line (disabled, "Nothing playing" when idle),
//! a separator, Show, Quit, and only after them, while there are any, a separator, the disabled
//! "Play next" caption and the episodes (enabled). The episodes are the only part whose length
//! changes, and they come last. Their ids are `play:<media id>` (unique: one per anime).
//!
//! The tray runs on its own thread; its events reach the UI thread through the handle, so they
//! work while the window is closed (`App::keep_running`). It shows only where a tray host runs
//! (KDE, waybar and most bars; GNOME needs the AppIndicator extension, which reads the same
//! dbusmenu: disabled items greyed, no sections, long labels not cut). Without one the icon
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
    play: Vec<(u32, String)>,
}

impl TrayCtl {
    /// None when D-Bus is unavailable.
    pub fn start(handle: Handle) -> Option<Self> {
        let spec = TraySpec::new("kotora", "Kotora", icon()).tooltip("Kotora").menu(menu("", &[]));
        let on_event = move |ev: TrayEvent| match ev {
            TrayEvent::Activate { token } => {
                match token {
                    Some(t) => handle.activate(t),
                    None => handle.open(),
                }
                handle.update(|s: &mut crate::model::State, _| s.window = true);
            }
            TrayEvent::Menu(id) => match action_for(&id) {
                Some(Action::Show) => super::show_window(&handle),
                Some(Action::Quit) => handle.quit(),
                // The same as the Up next ▶ button (`library::play_next`), with a toast.
                Some(Action::Play(media)) => super::play(&handle, media),
                None => {}
            },
            _ => {}
        };
        match Tray::new(spec, on_event) {
            Ok(tray) => Some(Self { tray, line: String::new(), play: Vec::new() }),
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
            self.tray.set_menu(menu(line, &self.play));
        }
    }

    /// The Play next items, (media id, "Title · Ep N"); the menu is rebuilt only on a change.
    pub fn set_play(&mut self, play: Vec<(u32, String)>) {
        if play != self.play {
            self.play = play;
            self.tray.set_menu(menu(&self.line, &self.play));
        }
    }
}

/// What a menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Show,
    Quit,
    /// Play the next episode of this anime (media id).
    Play(u32),
}

/// The action of a menu id (`None`: the line, the caption, a separator).
fn action_for(id: &str) -> Option<Action> {
    match id {
        "show" => Some(Action::Show),
        "quit" => Some(Action::Quit),
        _ => id.strip_prefix("play:").and_then(|n| n.parse().ok()).map(Action::Play),
    }
}

/// The longest title in a Play next item before it is cut with an ellipsis (" · Ep N" stays).
const TITLE_MAX: usize = 42;

/// "Title · Ep N" with a long title cut at a word where it can: "Re:Zero kara Hajimeru Isekai
/// Seikatsu 3rd Season · Ep 4" becomes "Re:Zero kara Hajimeru Isekai Seikatsu… · Ep 4".
fn short_label(label: &str) -> String {
    let (title, ep) = match label.rsplit_once(" \u{b7} ") {
        Some((t, e)) => (t, format!(" \u{b7} {e}")),
        None => (label, String::new()),
    };
    if title.chars().count() <= TITLE_MAX {
        return format!("{title}{ep}");
    }
    let cut: String = title.chars().take(TITLE_MAX).collect();
    // Back to the last space when it isn't too far, so no word is cut in two.
    let cut = match cut.rfind(' ') {
        Some(i) if i >= TITLE_MAX * 2 / 3 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}\u{2026}{ep}", cut.trim_end_matches([' ', ':', '-', ',']))
}

/// The menu (see the module's note on its fixed shape). `play`: (media id, "Title · Ep N").
fn menu(line: &str, play: &[(u32, String)]) -> Vec<TrayItem> {
    let line = if line.is_empty() { "Nothing playing" } else { line };
    let mut v = vec![
        TrayItem::action("now", short_label(line)).enabled(false),
        TrayItem::Separator,
        TrayItem::action("show", "Show Kotora"),
        TrayItem::action("quit", "Quit Kotora"),
    ];
    let mut seen = std::collections::HashSet::new();
    let items: Vec<TrayItem> = play.iter().filter(|(id, _)| seen.insert(*id)).map(|(id, label)| TrayItem::action(format!("play:{id}"), short_label(label))).collect();
    if !items.is_empty() {
        v.push(TrayItem::Separator);
        v.push(TrayItem::action("play-head", "Play next").enabled(false));
        v.extend(items);
    }
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

    use super::{Action, action_for, menu, short_label};
    use rune::tray::TrayItem;

    /// Each position's kind: 's' separator, 'd' disabled, 'a' enabled action.
    fn kinds(m: &[TrayItem]) -> String {
        m.iter()
            .map(|i| match i {
                TrayItem::Separator => 's',
                TrayItem::Action { enabled: false, .. } => 'd',
                _ => 'a',
            })
            .collect()
    }

    fn ids(m: &[TrayItem]) -> Vec<String> {
        m.iter().filter_map(|i| if let TrayItem::Action { id, .. } = i { Some(id.clone()) } else { None }).collect()
    }

    fn play() -> Vec<(u32, String)> {
        vec![
            (42, "Lanterns of Kirigaoka \u{b7} Ep 3".into()),
            (7, "Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season \u{b7} Ep 4".into()),
            (9, "Black Clover 2nd Season \u{b7} Ep 1".into()),
            (42, "Lanterns of Kirigaoka \u{b7} Ep 3".into()),
        ]
    }

    /// The bug seen on KDE: ids are positions, so no position may change kind between two
    /// menus. Every menu Kotora can build is a prefix-compatible shape: "dsaa", then "sd" and
    /// only enabled episodes.
    #[test]
    fn no_position_ever_changes_kind() {
        let menus = [menu("", &[]), menu("Frieren \u{b7} Ep 7", &[]), menu("", &play()), menu("Frieren \u{b7} Ep 7", &play()[..1]), menu("x", &play())];
        let longest = menus.iter().map(|m| kinds(m)).max_by_key(String::len).unwrap();
        assert_eq!(longest, "dsaasdaaa");
        for m in &menus {
            assert!(longest.starts_with(&kinds(m)), "{}", kinds(m));
        }
        // The line is always there (greyed), even when nothing plays.
        assert!(matches!(&menu("", &[])[0], TrayItem::Action { label, enabled: false, .. } if label == "Nothing playing"));
    }

    /// Ids are unique, every enabled item does something, the disabled ones nothing, and each
    /// episode plays its own anime (once, even when listed twice).
    #[test]
    fn ids_are_unique_and_every_item_maps_to_its_action() {
        let m = menu("Frieren \u{b7} Ep 7", &play());
        let ids = ids(&m);
        let set: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(set.len(), ids.len(), "{ids:?}");
        for item in &m {
            if let TrayItem::Action { id, enabled, .. } = item {
                assert_eq!(action_for(id).is_some(), *enabled, "{id}");
            }
        }
        let plays: Vec<Action> = ids.iter().filter_map(|i| action_for(i)).filter(|a| matches!(a, Action::Play(_))).collect();
        assert_eq!(plays, [Action::Play(42), Action::Play(7), Action::Play(9)]);
        assert_eq!((action_for("show"), action_for("quit")), (Some(Action::Show), Some(Action::Quit)));
        assert_eq!((action_for("play-head"), action_for("now"), action_for("play:x")), (None, None, None));
        // No indent: labels start with the title.
        assert!(m.iter().all(|i| !matches!(i, TrayItem::Action { label, .. } if label.starts_with(' '))));
    }

    #[test]
    fn long_titles_are_cut_but_keep_the_episode() {
        let l = short_label("Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season \u{b7} Ep 4");
        assert_eq!(l, "Re:Zero kara Hajimeru Isekai Seikatsu 3rd\u{2026} \u{b7} Ep 4");
        assert_eq!(short_label("Bocchi the Rock! \u{b7} Ep 2"), "Bocchi the Rock! \u{b7} Ep 2");
        assert!(short_label(&"\u{732b}".repeat(60)).ends_with('\u{2026}'), "no episode, no space: cut anyway");
        assert!(short_label("A Title That Has Quite a Few Words in It \u{b7} Ep 12 \u{b7} Ep 13").ends_with("\u{b7} Ep 13"));
    }
}
