//! Line icons on a 24 grid, 1.6 stroke, round caps (design-spec §5), from `mockup/kotora.html`.
//! Tinted with `.color()`.

use rune::Icon;

macro_rules! line_icon {
    ($id:ident, $name:literal, $body:literal) => {
        pub static $id: Icon = Icon {
            name: concat!("kot-", $name),
            view: 24.0,
            body: $body,
        };
    };
}

line_icon!(SEARCH, "search", r#"<circle cx="11" cy="11" r="6.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M16 16l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(DOWN, "down", r#"<path d="M6 9l6 6 6-6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(PLAY, "play", r#"<circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M10 8.5v7l5.5-3.5z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#);
line_icon!(SYNC, "sync", r#"<path d="M4 12a8 8 0 0 1 13.7-5.6L20 9M20 4v5h-5M20 12a8 8 0 0 1-13.7 5.6L4 15M4 20v-5h5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(SLIDERS, "sliders", r#"<path d="M4 7h10M18 7h2M4 17h2M10 17h10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="16" cy="7" r="2" fill="none" stroke="currentColor" stroke-width="1.6"/><circle cx="8" cy="17" r="2" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(LOCK, "lock", r#"<rect x="5" y="10.5" width="14" height="10" rx="2" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
