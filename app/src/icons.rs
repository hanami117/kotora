//! Line icons on a 24 grid, 1.6 stroke, round caps (design-spec §5).
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
line_icon!(LEFT, "left", r#"<path d="M14.5 6l-6 6 6 6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(RIGHT, "right", r#"<path d="M9.5 6l6 6-6 6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(CHECK, "check", r#"<path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(ALERT, "alert", r#"<path d="M12 4l9 16H3z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M12 10v4.5M12 17.4v.2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(FILE, "file", r#"<path d="M6 3.5h8l4 4V20.5H6z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M14 3.5v4h4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#);
line_icon!(EXTERNAL, "external", r#"<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(COPY, "copy", r#"<rect x="8.5" y="8.5" width="11" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#);
line_icon!(CALENDAR, "calendar", r#"<rect x="4" y="5.5" width="16" height="14.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M4 10h16M8.5 3.5v4M15.5 3.5v4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(ARROW, "arrow", r#"<path d="M5 12h14M13 6l6 6-6 6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(DOWNLOAD, "download", r#"<path d="M12 4v11M7 10.5l5 5 5-5M5 19.5h14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
line_icon!(CLOSE, "close", r#"<path d="M6.5 6.5l11 11M17.5 6.5l-11 11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(VIEW_LIST, "view-list", r#"<path d="M8.5 6.5h11M8.5 12h11M8.5 17.5h11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="4.75" cy="6.5" r="1.1" fill="currentColor"/><circle cx="4.75" cy="12" r="1.1" fill="currentColor"/><circle cx="4.75" cy="17.5" r="1.1" fill="currentColor"/>"#);
line_icon!(VIEW_GRID, "view-grid", r#"<rect x="3.5" y="4" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="9.75" y="4" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="16" y="4" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="3.5" y="13.5" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="9.75" y="13.5" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="16" y="13.5" width="4.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(VIEW_LARGE, "view-large", r#"<rect x="4" y="3.5" width="7" height="17" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><rect x="13" y="3.5" width="7" height="17" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M4 15.5h7M13 15.5h7" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(PENCIL, "pencil", r#"<path d="M4.5 19.5h4l10-10-4-4-10 10z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M12.5 7.5l4 4" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(HOME, "home", r#"<path d="M4 11l8-6.5 8 6.5M6 9.5V19.5h4.5v-5h3v5H18V9.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#);
pub static STAR: Icon = Icon {
    name: "kot-star",
    view: 24.0,
    body: r#"<path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.8z" fill="currentColor"/>"#,
};

// Settings sections.
line_icon!(GEAR, "gear", r#"<circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.6"/><circle cx="12" cy="12" r="6.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M12 2.8v2.7M12 18.5v2.7M2.8 12h2.7M18.5 12h2.7M5.5 5.5l1.9 1.9M16.6 16.6l1.9 1.9M5.5 18.5l1.9-1.9M16.6 7.4l1.9-1.9" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(CONTRAST, "contrast", r#"<circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M12 3.5a8.5 8.5 0 0 1 0 17z" fill="currentColor"/>"#);
line_icon!(USER, "user", r#"<circle cx="12" cy="8.5" r="3.7" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M4.8 20a7.2 7.2 0 0 1 14.4 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(EYE, "eye", r#"<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(FOLDER, "folder", r#"<path d="M3.5 7a1.5 1.5 0 0 1 1.5-1.5h4.4l2 2.5H19a1.5 1.5 0 0 1 1.5 1.5v8.5A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#);
line_icon!(BOOK, "book", r#"<path d="M12 6.5C10 5 7 4.5 4 5v13c3-.5 6 0 8 1.5 2-1.5 5-2 8-1.5V5c-3-.5-6 0-8 1.5z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M12 6.5v13" fill="none" stroke="currentColor" stroke-width="1.6"/>"#);
line_icon!(BELL, "bell", r#"<path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M10 20.5a2 2 0 0 0 4 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(INFO, "info", r#"<circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M12 11v5.5M12 7.8v.2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);

line_icon!(HEART, "heart", r#"<path d="M12 19.5S4 14.8 4 9.3A4.3 4.3 0 0 1 12 7a4.3 4.3 0 0 1 8 2.3c0 5.5-8 10.2-8 10.2z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#);

// Score smileys (AniList's 3-point format).
line_icon!(FACE_SAD, "face-sad", r#"<circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><circle cx="9" cy="10" r="1.1" fill="currentColor"/><circle cx="15" cy="10" r="1.1" fill="currentColor"/><path d="M8.5 16.2a4.5 4.5 0 0 1 7 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(FACE_NEUTRAL, "face-neutral", r#"<circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><circle cx="9" cy="10" r="1.1" fill="currentColor"/><circle cx="15" cy="10" r="1.1" fill="currentColor"/><path d="M8.8 15h6.4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
line_icon!(FACE_HAPPY, "face-happy", r#"<circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="1.6"/><circle cx="9" cy="10" r="1.1" fill="currentColor"/><circle cx="15" cy="10" r="1.1" fill="currentColor"/><path d="M8.5 14a4.5 4.5 0 0 0 7 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#);
