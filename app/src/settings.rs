//! Settings (Cmd+, / Ctrl+,): the sections on the left (a row of tabs on top in a narrow window),
//! the chosen section's rows on the right, and a search field in the top bar that finds rows in
//! every section.
//!
//! Each row is a [`Row`]: the sections and the search results build the same ones (`row_el`), in
//! a keyed wrapper that a search jump scrolls to and tints for a moment. The search matches a
//! static table of titles, notes and synonyms ([`SPECS`]), folded once into one lower-case string
//! per row. The section opened last is kept in `config.toml` (`[display] settings_section`).

use std::sync::LazyLock;

use kotora_core::config;
use kotora_core::service::ServiceId;
use kotora_detect::Switch;
use rune::{El, Icon, Key, KeyEvent, ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Auth, State};
use crate::now::THRESHOLDS;
use crate::theme::{self, PAL};
use crate::ui::{cell, field_hint, hex, kbd, led_color, service_row, small_button, switch, switch_if};

// ------------------------------------------------------------------------------------ sections

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    General,
    Appearance,
    Account,
    Detection,
    Library,
    Notifications,
    About,
}

impl Section {
    pub const ALL: [Section; 7] = [Section::General, Section::Appearance, Section::Account, Section::Detection, Section::Library, Section::Notifications, Section::About];

    /// The `config.toml` value, also the end of the section's keys.
    pub fn key(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Appearance => "appearance",
            Section::Account => "account",
            Section::Detection => "detection",
            Section::Library => "library",
            Section::Notifications => "notifications",
            Section::About => "about",
        }
    }

    pub fn from_key(k: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.key() == k)
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Account => "Account",
            Section::Detection => "Detection",
            Section::Library => "Library",
            Section::Notifications => "Notifications",
            Section::About => "About",
        }
    }

    fn icon(self) -> &'static Icon {
        match self {
            Section::General => &icons::GEAR,
            Section::Appearance => &icons::CONTRAST,
            Section::Account => &icons::USER,
            Section::Detection => &icons::EYE,
            Section::Library => &icons::FOLDER,
            Section::Notifications => &icons::BELL,
            Section::About => &icons::INFO,
        }
    }

    /// The section's groups: a heading (or none) over its rows, in order.
    fn groups(self) -> Vec<(Option<&'static str>, Vec<Row>)> {
        match self {
            Section::General => vec![(Some("Startup"), vec![Row::StartAtLogin, Row::StartHidden]), (Some("Updates"), vec![Row::CheckUpdates, Row::CheckNow]), (Some("Keyboard"), vec![Row::PageKeys])],
            Section::Appearance => vec![(None, vec![Row::Theme, Row::Titles, Row::Intro])],
            Section::Account => vec![(Some("Service"), vec![Row::Mal, Row::AniList]), (Some("Sign-in"), vec![Row::ClientId, Row::Token])],
            Section::Detection => {
                let mut players: Vec<Row> = Switch::here().iter().map(|sw| Row::Player(*sw)).collect();
                if kotora_detect::VLC_WEB {
                    players.push(Row::VlcWeb);
                }
                vec![
                    (Some("Players"), players),
                    (Some("Recording episodes"), vec![Row::Auto, Row::Threshold, Row::KeepHigher]),
                    (Some("Players without setup"), vec![Row::ProcessScan, Row::ProcessMinutes]),
                ]
            }
            Section::Library => vec![(None, vec![Row::Folders, Row::OpenWith])],
            Section::Notifications => vec![
                (Some("While you watch"), vec![Row::NotifyDetected, Row::NotifyOffer, Row::NotifyRecorded]),
                (Some("Airing"), vec![Row::NotifyStarted, Row::NotifyEpisode]),
            ],
            Section::About => vec![(None, vec![Row::About, Row::Licences, Row::Links])],
        }
    }
}

/// One row of a section, as search finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    StartAtLogin,
    StartHidden,
    CheckUpdates,
    CheckNow,
    /// The modifier with 1 to 5 for the list pages.
    PageKeys,
    Theme,
    Titles,
    Intro,
    Mal,
    ClientId,
    Token,
    AniList,
    Player(Switch),
    /// macOS: VLC is read through its web interface.
    VlcWeb,
    Auto,
    Threshold,
    KeepHigher,
    ProcessScan,
    ProcessMinutes,
    Folders,
    OpenWith,
    NotifyDetected,
    NotifyOffer,
    NotifyRecorded,
    NotifyStarted,
    NotifyEpisode,
    About,
    Licences,
    Links,
}

impl Row {
    /// The key of the row's wrapper (a search jump scrolls to it and tints it).
    pub fn key(self) -> String {
        let slug = match self {
            Row::Player(sw) => return format!("set/row/player-{}", sw.config_key()),
            Row::StartAtLogin => "start-at-login",
            Row::StartHidden => "start-hidden",
            Row::CheckUpdates => "check-updates",
            Row::CheckNow => "check-now",
            Row::PageKeys => "page-keys",
            Row::Theme => "theme",
            Row::Titles => "titles",
            Row::Intro => "intro",
            Row::Mal => "mal",
            Row::ClientId => "client-id",
            Row::Token => "token",
            Row::AniList => "anilist",
            Row::VlcWeb => "vlc-web",
            Row::Auto => "auto",
            Row::Threshold => "threshold",
            Row::KeepHigher => "keep-higher",
            Row::ProcessScan => "process-scan",
            Row::ProcessMinutes => "process-minutes",
            Row::Folders => "folders",
            Row::OpenWith => "open-with",
            Row::NotifyDetected => "notify-detected",
            Row::NotifyOffer => "notify-offer",
            Row::NotifyRecorded => "notify-recorded",
            Row::NotifyStarted => "notify-started",
            Row::NotifyEpisode => "notify-episode",
            Row::About => "about",
            Row::Licences => "licences",
            Row::Links => "links",
        };
        format!("set/row/{slug}")
    }
}

// -------------------------------------------------------------------------------------- search

struct Spec {
    row: Row,
    section: Section,
    title: &'static str,
    note: &'static str,
    /// Words people may type that the title and note don't hold.
    words: &'static str,
}

const fn spec(row: Row, section: Section, title: &'static str, note: &'static str, words: &'static str) -> Spec {
    Spec { row, section, title, note, words }
}

/// Every row but the players (`index` adds those for this system), in section order.
const SPECS: &[Spec] = &[
    spec(Row::StartAtLogin, Section::General, "Start Kotora when I log in", "Kotora keeps detecting episodes from the moment you log in.", "login log in logon autostart auto start startup boot launch open at login login items session automatically"),
    spec(Row::StartHidden, Section::General, "Start in the background", "At login, show only the tray icon, no window.", "background hidden minimised minimized tray menu bar silent quiet no window autostart startup login"),
    spec(Row::CheckUpdates, Section::General, "Check for updates", crate::update::CHECK_NOTE, "update updates upgrade new version release kotora.moe privacy"),
    spec(Row::CheckNow, Section::General, "Check now", "When Kotora last checked for a new version.", "update updates upgrade new version release last checked"),
    spec(Row::PageKeys, Section::General, "List page shortcuts", "A modifier key with 1 to 5 opens Watching, Plan to watch, Completed, On hold and Dropped.", "keyboard shortcut shortcuts keys hotkey hotkeys keybinding keybindings bindings chord ctrl control alt option cmd command modifier numbers digits 1 2 3 4 5 pages lists navigate go"),
    spec(Row::Theme, Section::Appearance, "Theme", "System follows your desktop's light or dark setting.", "dark mode light mode night colour color appearance system"),
    spec(Row::Titles, Section::Appearance, "Titles", "Romaji, English or Japanese titles everywhere.", "title language romaji english japanese names kot"),
    spec(Row::Intro, Section::Appearance, "Startup animation", "Claw marks when Kotora opens.", "intro splash launch claw motion animation reduce motion"),
    spec(Row::Mal, Section::Account, "MyAnimeList", "Signed in, or Sign in. Or switch to it.", "mal myanimelist account service sign in sign out log in login logout user switch use"),
    spec(Row::ClientId, Section::Account, "Client ID", "The ID of your own MyAnimeList API app, or your own AniList API client instead of Kotora\u{2019}s. Set up again.", "client id api key apiconfig developer setup wizard set up again"),
    spec(Row::Token, Section::Account, "Sign-in token", "Kept in a file only you can read. Signing out deletes it.", "token security privacy password file keyring"),
    spec(Row::AniList, Section::Account, "AniList", "Signed in, or Sign in. Or switch to it.", "anilist al account service sign in sign out log in login logout user switch use"),
    spec(Row::VlcWeb, Section::Detection, "VLC web interface", if cfg!(windows) { "VLC reports through its web interface, with a password." } else { "VLC on the Mac reports through its web interface, with a password." }, "vlc web interface http password lua config.toml"),
    spec(Row::Auto, Section::Detection, "Update automatically", "Record confident matches without asking.", "auto automatic ask before updating ask first confirm record"),
    spec(Row::Threshold, Section::Detection, "Offer the update at", "Share of the episode watched.", "threshold percent percentage watched progress 70 80 90 100"),
    spec(Row::KeepHigher, Section::Detection, "Keep the higher progress in conflicts", "When only the progress differs and yours is higher, Kotora sends yours.", "conflict conflicts sync queue progress"),
    spec(Row::ProcessScan, Section::Detection, if cfg!(windows) { "Find players without setup" } else { "Find players by their open files" }, "No setup, but no position or pause.", "process open files proc lsof scan no setup window title command line zero config"),
    spec(Row::ProcessMinutes, Section::Detection, "Count a file after it stays open", "Minutes, when the episode's length is unknown.", "minutes process time open files duration"),
    spec(Row::Folders, Section::Library, "Folders", crate::library::FOLDERS_NOTE, "library folder folders directory files disk drive rescan scan play next episode"),
    spec(Row::OpenWith, Section::Library, "Open with", crate::library::OPEN_WITH_NOTE, "player play mpv iina vlc default automatic open"),
    spec(Row::NotifyDetected, Section::Notifications, "Episode detected", "When a recognised episode starts playing.", "notification notifications alert popup detected"),
    spec(Row::NotifyOffer, Section::Notifications, "Update offered", "Update to 6? with Update and Not now.", "notification notifications alert popup offer ask"),
    spec(Row::NotifyRecorded, Section::Notifications, "Episode recorded", "Recorded, with Undo.", "notification notifications alert popup recorded undo"),
    spec(Row::NotifyStarted, Section::Notifications, "Started airing", "When a show on Plan to watch starts airing.", "notification notifications alert popup airing premiere plan to watch"),
    spec(Row::NotifyEpisode, Section::Notifications, "New episode", "When the next episode of a show you're watching is out.", "notification notifications alert popup new episode aired out broadcast"),
    spec(Row::About, Section::About, "About Kotora", "Version, and a tribute to Taiga.", "version about taiga credits thanks"),
    spec(Row::Licences, Section::About, "Licences", "GPL-3.0-or-later, anitomy, anime-relations, Noto Sans JP.", "licence license licences licenses gpl mpl cc0 ofl anitomy anime-relations noto copyright"),
    spec(Row::Links, Section::About, "Links", "kotora.moe and the source code.", "website site github source code links kotora.moe"),
];

/// A player row's search words beyond its name.
fn player_words(sw: Switch) -> &'static str {
    match sw {
        Switch::Mpv => "mpv ipc socket precise detection position mpv.conf player",
        Switch::Iina => "iina precise detection ipc socket position mpv.conf player mac",
        Switch::Vlc => "vlc player mpris web interface",
        Switch::MpcQt => "mpc-qt mpc qt mpris player",
        Switch::OtherMpris => "mpris haruna celluloid smplayer other players",
        Switch::Mpc => "mpc-hc mpc-be media player classic web interface position player",
        Switch::PotPlayer => "potplayer pot player daum position",
    }
}

/// A row as search sees it.
pub struct Entry {
    pub row: Row,
    pub section: Section,
    /// Section, title, note and synonyms, folded.
    hay: String,
}

/// Lower case, accents dropped from the Latin letters people type with them.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ý' | 'ÿ' => 'y',
            '\u{2019}' => '\'',
            c => c,
        })
        .collect()
}

/// This system's rows (the players of `Switch::here`), in section order.
fn index() -> Vec<Entry> {
    let mut v: Vec<Entry> = Vec::new();
    for section in Section::ALL {
        if section == Section::Detection {
            for sw in Switch::here() {
                v.push(Entry { row: Row::Player(*sw), section, hay: fold(&format!("{} {} {}", section.title(), sw.label(), player_words(*sw))) });
            }
        }
        for s in SPECS.iter().filter(|s| s.section == section) {
            if s.row == Row::VlcWeb && !kotora_detect::VLC_WEB {
                continue;
            }
            v.push(Entry { row: s.row, section, hay: fold(&format!("{} {} {} {}", section.title(), s.title, s.note, s.words)) });
        }
    }
    v
}

pub static INDEX: LazyLock<Vec<Entry>> = LazyLock::new(index);

/// The rows (indices into [`INDEX`]) holding every word of `query`, case and accents aside, in
/// section order. An empty query finds nothing.
pub fn find(query: &str) -> Vec<usize> {
    let q = fold(query);
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    INDEX.iter().enumerate().filter(|(_, e)| words.iter().all(|w| e.hay.contains(w))).map(|(i, _)| i).collect()
}

// ------------------------------------------------------------------------------------- state

/// How long a row a search jumped to stays tinted.
const FLASH: f64 = 1.4;

#[derive(Default)]
pub struct SettingsUi {
    /// The section on screen (and to reopen on).
    pub section: Section,
    /// The search field in the top bar. While it holds more than spaces, the results take the
    /// section's place.
    pub query: TextEdit,
    /// What `query` finds: indices into [`INDEX`].
    pub hits: Vec<usize>,
    /// The row a search jump landed on, tinted for `FLASH` seconds from `flash_at` (set by
    /// `poll`, which has the clock).
    pub flash: Option<Row>,
    flash_at: Option<f64>,
}

/// At start: the section opened last.
pub fn load(s: &mut State, c: &config::Config) {
    s.settings.section = Section::from_key(&c.display.settings_section).unwrap_or_default();
}

/// True while the search field holds more than spaces.
pub fn searching(s: &State) -> bool {
    !s.settings.query.text.trim().is_empty()
}

fn clear_search(s: &mut State) {
    s.settings.query.clear();
    s.settings.hits.clear();
}

/// Scenes and tests: put `q` in the search field, as typing it would.
pub fn set_query(s: &mut State, q: &str) {
    s.settings.query = TextEdit::new(q);
    s.settings.hits = find(q);
}

/// Shows `section` (leaving a search) and keeps it for the next time.
pub fn select(s: &mut State, section: Section) {
    clear_search(s);
    s.settings.flash = None;
    if s.settings.section != section {
        s.settings.section = section;
        crate::detect::save_setting(s, "display", "settings_section", config::quote(section.key()));
    }
}

/// Opens Settings on `section` (links from other pages).
pub fn open(s: &mut State, section: Section) {
    s.go(crate::model::Page::Settings);
    select(s, section);
}

/// Up or Down in the section list: the section above or below (no wrap).
fn step(s: &mut State, by: isize) -> bool {
    let at = Section::ALL.iter().position(|x| *x == s.settings.section).unwrap_or(0) as isize;
    let next = (at + by).clamp(0, Section::ALL.len() as isize - 1) as usize;
    if Section::ALL[next] == s.settings.section {
        return false;
    }
    select(s, Section::ALL[next]);
    true
}

/// The rows the search shows, as `(section, row)` in section order.
pub fn results(s: &State) -> Vec<(Section, Row)> {
    s.settings.hits.iter().filter_map(|&i| INDEX.get(i)).map(|e| (e.section, e.row)).collect()
}

/// Leaves the search for `section`, scrolled to `row`, which is tinted for a moment.
fn jump(s: &mut State, section: Section, row: Row, tx: &mut Tx) {
    tx.instant();
    select(s, section);
    s.settings.flash = Some(row);
    s.settings.flash_at = None;
    tx.scroll_center(row.key());
}

/// Enter in the search field: go to the first result.
fn jump_first(s: &mut State, tx: &mut Tx) {
    if let Some(&(section, row)) = results(s).first() {
        jump(s, section, row, tx);
        tx.blur();
    }
}

/// Ends the tint of a row a search jumped to. True when that changes the view.
pub fn poll(s: &mut State, t: f64, tx: &mut Tx) -> bool {
    let st = &mut s.settings;
    if st.flash.is_none() {
        return false;
    }
    let at = *st.flash_at.get_or_insert(t);
    if t - at >= FLASH {
        st.flash = None;
        st.flash_at = None;
        return true;
    }
    tx.wake_after(std::time::Duration::from_secs_f64(FLASH - (t - at)));
    false
}

/// Settings' keys, from `ui::on_key` while Settings shows: Cmd+F (Ctrl+F) focuses the search,
/// Escape clears it, Up and Down step through the sections. `None`: not a Settings key.
pub fn on_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> Option<bool> {
    if k.is_command('f') {
        s.settings.query.focused = true;
        return Some(true);
    }
    if k.key == Key::Escape && !s.settings.query.text.is_empty() {
        clear_search(s);
        return Some(true);
    }
    if k.mods.cmd || k.mods.ctrl || k.mods.alt || searching(s) {
        return None;
    }
    let by = match k.key {
        Key::Up => -1,
        Key::Down => 1,
        _ => return None,
    };
    tx.instant();
    Some(step(s, by))
}

/// What the list page keys do, with the chosen modifier, and that the others stay.
fn page_keys_note(keys: config::PageKeys) -> String {
    let (first, last) = (keys.hint('1'), keys.hint('5'));
    let fixed = if cfg!(target_os = "macos") { "\u{2318} , for Settings, \u{2318} K and \u{2318} F" } else { "Ctrl , for Settings, Ctrl K and Ctrl F" };
    format!("{first} to {last} open Watching, Plan to watch, Completed, On hold and Dropped, also while you type in a search box. {fixed} stay as they are.")
}

// -------------------------------------------------------------------------------------- view

/// Below this main-area width the sections are a row of tabs on top.
pub const TABS_W: f32 = 760.0;

/// The Settings page; `width`: the main area's.
pub fn page(s: &State, width: f32) -> El {
    let narrow = width < TABS_W;
    let searching = searching(s);
    let content = if searching { search_results(s) } else { section_view(s, s.settings.section) };
    let scroll = col()
        .key("settings-scroll")
        .scroll(ScrollSpec { stick_bottom: false })
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .child(col().w_full().px(if narrow { theme::PAGE_PAD } else { 8.0 }).pr(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).max_w(820.0).child(content));
    let top = row()
        .key("top")
        .gap(12.0)
        .center_y()
        .px(theme::PAGE_PAD)
        .pt(18.0)
        .pb(14.0)
        .child(text("Settings").font(theme::TITLE).color(PAL.text))
        .child(spacer())
        .child(search_box(s, narrow));
    let page = col().key("page/Settings").w_full().h_full().child(top);
    if narrow {
        page.child(tabs(s, searching)).child(scroll)
    } else {
        page.child(row().w_full().grow(1.0).min_h(0.0).items_start().child(side(s, searching)).child(scroll))
    }
}

/// The search field. Enter goes to the first result; Escape clears it.
fn search_box(s: &State, narrow: bool) -> El {
    let k = if cfg!(target_os = "macos") { "\u{2318} F" } else { "Ctrl F" };
    let field = text_input(&s.settings.query, theme::BODY)
        .key("set/search-input")
        .grow(1.0)
        .min_w(0.0)
        .single_line()
        .color(PAL.text)
        .placeholder(field_hint(&s.settings.query, "Search settings"), PAL.faint)
        .on_edit(|s: &mut State| &mut s.settings.query)
        .on_input_change(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.settings.hits = find(&s.settings.query.text);
            s.settings.flash = None;
        })
        .on_submit(jump_first)
        // The field would only drop the focus on Escape.
        .on_input_key(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
            if crate::ui::field_page_key(s, ev, tx) {
                return true;
            }
            if ev.key != Key::Escape {
                return false;
            }
            tx.instant();
            if s.settings.query.text.is_empty() {
                tx.blur();
            } else {
                clear_search(s);
            }
            true
        });
    let mut b = row()
        .key("set/search")
        .role("textbox")
        .w(if narrow { 220.0 } else { 280.0 })
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, if s.settings.query.focused { PAL.accent.alpha(0.6) } else { PAL.border })
        .child(icon(&icons::SEARCH, 16.0).color(PAL.faint))
        .child(field);
    if s.settings.query.text.is_empty() {
        b = b.child(kbd(k));
    } else {
        b = b.child(
            el().key("set/search-clear")
                .role("button")
                .label("Clear search")
                .focusable(true)
                .size(20.0, 20.0)
                .radius(10.0)
                .center()
                .hover(PAL.hover())
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(|s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    clear_search(s);
                })
                .child(icon(&icons::CLOSE, 12.0).color(PAL.dim)),
        );
    }
    b
}

/// The section list (wide windows).
fn side(s: &State, searching: bool) -> El {
    let mut c = col().key("set/sections").role("list").label("Settings sections").w(208.0).pl(theme::PAGE_PAD - 8.0).pr(8.0).pt(4.0).gap(2.0);
    for sec in Section::ALL {
        let on = !searching && sec == s.settings.section;
        let fg = if on { PAL.text } else { PAL.dim };
        let mut item = row()
            .key(format!("set/section/{}", sec.key()))
            .role("button")
            .label(sec.title())
            .focusable(true)
            .h(theme::NAV_H)
            .px(8.0)
            .gap(10.0)
            .center_y()
            .radius(theme::R_SM)
            .hover(PAL.hover())
            .press(PAL.press())
            .cursor(rune::Cursor::Pointer)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                select(s, sec);
            })
            .child(icon(sec.icon(), 16.0).color(if on { PAL.accent_hi } else { fg }))
            .child(text(sec.title()).font(TextStyle::new(13.0, if on { 500.0 } else { 400.0 })).color(fg));
        if on {
            item = item.fill(PAL.raised);
        }
        c = c.child(item);
    }
    c.child(text("\u{2191} \u{2193} to move").font(theme::SMALL).color(PAL.faint).px(8.0).pt(10.0))
}

/// The sections as a row of tabs (narrow windows).
fn tabs(s: &State, searching: bool) -> El {
    let mut r = row().key("set/sections").role("tablist").label("Settings sections").gap(2.0).padding(3.0).radius(theme::R_MD).fill(PAL.field).border(1.0, PAL.border);
    for sec in Section::ALL {
        let on = !searching && sec == s.settings.section;
        let mut t = row()
            .key(format!("set/section/{}", sec.key()))
            .role("tab")
            .label(sec.title())
            .focusable(true)
            .h(28.0)
            .px(9.0)
            .gap(6.0)
            .center()
            .radius(theme::R_SM)
            .cursor(rune::Cursor::Pointer)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                select(s, sec);
            })
            .child(text(sec.title()).font(TextStyle::new(12.0, if on { 600.0 } else { 500.0 })).color(if on { PAL.text } else { PAL.dim }));
        t = if on { t.fill(PAL.raised).border(1.0, PAL.border_strong) } else { t.hover(PAL.hover()) };
        r = r.child(t);
    }
    row().w_full().px(theme::PAGE_PAD).pb(12.0).child(r.flex_wrap())
}

/// A section: its title, then each group in a card.
fn section_view(s: &State, sec: Section) -> El {
    let mut c = col().key(format!("set/page/{}", sec.key())).w_full().gap(8.0).child(
        row()
            .gap(10.0)
            .center_y()
            .pb(4.0)
            .child(icon(sec.icon(), 18.0).color(PAL.accent_hi))
            .child(text(sec.title()).font(TextStyle::new(17.0, 600.0)).color(PAL.text)),
    );
    for (i, (head, rows)) in sec.groups().into_iter().enumerate() {
        if let Some(h) = head {
            c = c.child(group_head(h).mt(if i == 0 { 4.0 } else { 14.0 }));
        } else if i > 0 {
            c = c.child(el().h(10.0));
        }
        c = c.child(group_card(s, &format!("set/group/{}-{i}", sec.key()), &rows));
    }
    if sec == Section::Notifications
        && let Some(n) = notify_support(s)
    {
        c = c.child(n);
    }
    if sec == Section::Detection {
        c = c.child(
            text("Socket paths and VLC\u{2019}s web password are in config.toml ([players] mpv_sockets, [vlc]). Streaming sites in the browser: use MAL-Sync.")
                .key("set/detection-note")
                .font(theme::LABEL)
                .color(PAL.faint)
                .wrap()
                .mt(6.0),
        );
    }
    c
}

fn group_head(title: &str) -> El {
    text(title.to_uppercase()).font(theme::CAPTION).color(PAL.faint).px(4.0)
}

/// A card of rows with a line between each two.
fn group_card(s: &State, key: &str, rows: &[Row]) -> El {
    let mut c = col().key(key.to_string()).w_full().px(8.0).py(4.0).radius(theme::R_LG).fill(PAL.surface).border(1.0, PAL.border);
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            c = c.child(el().mx(8.0).h(1.0).fill(PAL.border));
        }
        c = c.child(item(s, *r));
    }
    c
}

/// `row` in its keyed wrapper, tinted while a search jump flashes it.
fn item(s: &State, r: Row) -> El {
    let w = col().key(r.key()).w_full().px(8.0).radius(theme::R_MD).child(row_el(s, r));
    if s.settings.flash == Some(r) { w.fill(PAL.accent.alpha(0.14)) } else { w }
}

/// The results by section, or a note when there are none.
fn search_results(s: &State) -> El {
    let found = results(s);
    let q = s.settings.query.text.trim().to_string();
    if found.is_empty() {
        return col()
            .key("set/no-results")
            .w_full()
            .pt(72.0)
            .gap(6.0)
            .items_center()
            .child(text(format!("No settings match \u{201c}{q}\u{201d}")).font(theme::SECTION).color(PAL.text).wrap())
            .child(text("Try \u{201c}theme\u{201d}, \u{201c}login\u{201d} or \u{201c}mpv\u{201d}.").font(theme::LABEL).color(PAL.dim));
    }
    let n = found.len();
    let mut c = col().key("set/results").w_full().gap(8.0).child(
        text(format!("{n} setting{} match \u{201c}{q}\u{201d} \u{b7} Return opens the first", if n == 1 { "" } else { "s" }))
            .font(theme::LABEL)
            .color(PAL.dim)
            .pb(4.0),
    );
    let mut i = 0;
    while i < found.len() {
        let sec = found[i].0;
        let rows: Vec<Row> = found[i..].iter().take_while(|(x, _)| *x == sec).map(|(_, r)| *r).collect();
        i += rows.len();
        let first = rows[0];
        c = c
            .child(
                row()
                    .key(format!("set/result-head/{}", sec.key()))
                    .role("button")
                    .label(format!("Go to {}", sec.title()))
                    .focusable(true)
                    .mt(if i == rows.len() { 0.0 } else { 10.0 })
                    .ml(-4.0)
                    .px(4.0)
                    .py(3.0)
                    .gap(7.0)
                    .center_y()
                    .radius(theme::R_SM)
                    .hover(PAL.hover())
                    .press(PAL.press())
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(move |s: &mut State, tx: &mut Tx| jump(s, sec, first, tx))
                    .child(icon(sec.icon(), 14.0).color(PAL.accent_hi))
                    .child(text(sec.title().to_uppercase()).font(theme::CAPTION).color(PAL.dim))
                    .child(icon(&icons::RIGHT, 12.0).color(PAL.faint)),
            )
            .child(group_card(s, &format!("set/result/{}", sec.key()), &rows));
    }
    c
}

// ---------------------------------------------------------------------------------------- rows

/// A settings row: title and one line of help on the left, the control on the right.
fn line(title: &str, note: impl Into<String>, control: El) -> El {
    row()
        .w_full()
        .gap(16.0)
        .py(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(text(title.to_string()).font(theme::BODY_MED).color(PAL.text).wrap())
                .child(text(note.into()).font(theme::LABEL).color(PAL.dim).wrap()),
        )
        .child(control)
}

/// A row whose control is too wide for the right: title and help, the control below.
fn block(title: &str, note: impl Into<String>, control: El) -> El {
    col()
        .w_full()
        .gap(10.0)
        .py(12.0)
        .child(
            col()
                .gap(3.0)
                .child(text(title.to_string()).font(theme::BODY_MED).color(PAL.text).wrap())
                .child(text(note.into()).font(theme::LABEL).color(PAL.dim).wrap()),
        )
        .child(control)
}

/// A small status word in a pill.
fn pill(key: &str, label: &str, c: rune::Color) -> El {
    row()
        .key(key.to_string())
        .px(8.0)
        .h(22.0)
        .gap(6.0)
        .center_y()
        .radius(11.0)
        .fill(PAL.tint(c, 0.14))
        .child(el().size(6.0, 6.0).radius(3.0).fill(c))
        .child(text(label.to_string()).font(theme::LABEL).color(PAL.text))
}

fn choice<T: Copy + PartialEq + 'static>(key: &str, items: &[T], on: T, label: fn(T) -> &'static str, slug: fn(T) -> &'static str, set: fn(&mut State, T)) -> El {
    let mut r = row().key(key.to_string()).gap(6.0).flex_wrap();
    for &t in items {
        r = r.child(cell(format!("{key}/{}", slug(t)), label(t).to_string(), on == t, move |s: &mut State| set(s, t)).child(text(label(t)).font(theme::LABEL).color(PAL.text)));
    }
    r
}

fn row_el(s: &State, r: Row) -> El {
    let p = &s.prefs;
    let tray = if cfg!(target_os = "macos") { "menu bar" } else { "tray" };
    match r {
        Row::StartAtLogin => {
            let l = &p.login;
            let note = match (l.unavailable, l.needs_approval) {
                (Some(why), _) => why.to_string(),
                (None, true) => "Waiting for your OK in System Settings, General, Login Items.".to_string(),
                _ if cfg!(target_os = "macos") => "Opens Kotora when you log in to your Mac, so it keeps detecting episodes. Listed in System Settings, General, Login Items.".to_string(),
                _ if cfg!(windows) => "Kotora keeps detecting episodes from the moment you sign in. Also listed in Windows Settings, Apps, Startup.".to_string(),
                _ => "Kotora keeps detecting episodes from the moment you log in.".to_string(),
            };
            let sw = switch_if("set/login".into(), "Start Kotora when I log in", l.on, l.available() && !p.login_busy, State::set_login);
            let control = if l.needs_approval { row().gap(12.0).center_y().child(small_button("set/login-items", "Open Login Items", false, Some(State::open_login_items))).child(sw) } else { sw };
            line("Start Kotora when I log in", note, control)
        }
        Row::StartHidden => line(
            "Start in the background",
            format!("At login, show only the {tray} icon, no window. Closing the window always keeps Kotora in the {tray}."),
            switch("set/background".into(), "Start in the background", p.background, State::set_background),
        ),
        Row::CheckUpdates => line("Check for updates", crate::update::CHECK_NOTE, switch("set/check-updates".into(), "Check for updates", p.check_updates, State::set_check_updates)),
        Row::CheckNow => line("Check now", crate::update::status_note(s), crate::update::check_button(s)).key("set/update-status"),
        Row::PageKeys => line(
            "List page shortcuts",
            page_keys_note(p.page_keys),
            choice("set/page-keys", &config::PageKeys::ALL, p.page_keys, config::PageKeys::label, config::PageKeys::key, State::set_page_keys),
        ),
        Row::Theme => line(
            "Theme",
            "System follows your desktop\u{2019}s light or dark setting, and changes with it.",
            choice("set/theme", &config::Theme::ALL, p.theme, config::Theme::label, config::Theme::key, State::set_theme),
        ),
        Row::Titles => line(
            "Titles",
            format!("The list, Now Playing, search, details, the queue and kot. A title {} doesn\u{2019}t have in that language shows in romaji.", s.service.title()),
            choice("set/titles", &config::TitleLang::ALL, p.titles, config::TitleLang::label, config::TitleLang::key, State::set_titles),
        ),
        Row::Intro => line(
            "Startup animation",
            "Claw marks when Kotora opens. Showing the window again doesn\u{2019}t play it; with Reduce Motion it is a short fade.",
            switch("set/intro".into(), "Startup animation", p.intro, State::set_intro),
        ),
        Row::Mal => account_row(s, ServiceId::Mal),
        Row::ClientId => crate::clientid::settings_row(s),
        Row::Token => row()
            .key("token-note")
            .gap(12.0)
            .py(12.0)
            .items_start()
            .child(el().size(32.0, 32.0).radius(theme::R_MD).fill(PAL.tint(PAL.st_completed, 0.14)).center().child(icon(&icons::LOCK, 16.0).color(PAL.st_completed)))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(text("Sign-in token").font(theme::BODY_MED).color(PAL.text))
                    .child(
                        text("Kept in a file only you can read. It is not encrypted. Signing out deletes it, your saved list and changes not yet sent.")
                            .font(theme::LABEL)
                            .color(PAL.dim)
                            .wrap(),
                    ),
            ),
        Row::AniList => account_row(s, ServiceId::AniList),
        Row::Player(sw) => player(s, sw),
        Row::VlcWeb if cfg!(windows) => block(
            "VLC web interface",
            "VLC is seen with no setup by its window, without a position. For the position, turn on its web interface in VLC (Tools \u{2192} Preferences, Show settings: All, Interface \u{2192} Main interfaces: tick Web; then Main interfaces \u{2192} Lua: set a Lua HTTP password), restart VLC, and type the same password here.",
            row().gap(12.0).center_y().child(crate::winsetup::vlc_field(s).grow(1.0)).child(if p.vlc_set_up { pill("set/vlc-web", "Set up", PAL.ok()) } else { pill("set/vlc-web", "Not set up", PAL.faint) }),
        ),
        Row::VlcWeb => line(
            "VLC web interface",
            if cfg!(windows) {
                "VLC reports through its web interface. Turn it on in VLC (Preferences, Show All, Interface, Main interfaces: Web; Lua: a password), then put the password in config.toml ([vlc] password) and start Kotora again."
            } else {
                "VLC on the Mac reports through its web interface. Turn it on in VLC (Preferences, Show All, Interface, Main interfaces: Web; Lua: a password), then put the password in config.toml ([vlc] password)."
            },
            if p.vlc_set_up { pill("set/vlc-web", "Set up", PAL.ok()) } else { pill("set/vlc-web", "Not set up", PAL.faint) },
        ),
        Row::Auto => {
            let now = if p.auto {
                "On: confident matches are recorded at once, with Undo in the banner. Unsure matches, and players seen by their open file only, still ask."
            } else {
                "Off: a banner and a notification ask first; nothing changes until you click Update. Off by default."
            };
            line("Update automatically", now, switch("set/auto".into(), "Update automatically", p.auto, State::set_auto))
        }
        Row::Threshold => {
            let next = THRESHOLDS[(THRESHOLDS.iter().position(|t| *t == p.threshold).unwrap_or(1) + 1) % THRESHOLDS.len()];
            let b = row()
                .key("set/threshold")
                .role("button")
                .label(format!("Offer the update at {}%, next {next}%", p.threshold))
                .focusable(true)
                .h(28.0)
                .px(10.0)
                .gap(6.0)
                .center_y()
                .radius(theme::R_SM)
                .fill(PAL.field)
                .border(1.0, PAL.border_strong)
                .hover(PAL.hover())
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut State| s.next_threshold())
                .child(text(format!("{}%", p.threshold)).font(theme::LABEL_NUM).color(PAL.text))
                .child(icon(&icons::DOWN, 14.0).color(PAL.faint));
            line("Offer the update at", "Share of the episode watched (or half, when the player stops).", b)
        }
        Row::KeepHigher => line(
            "Keep the higher progress in conflicts",
            "When only the progress differs and yours is higher, Kotora sends yours.",
            switch("set/keep-higher".into(), "Keep the higher progress in conflicts", p.keep_higher, State::set_keep_higher),
        ),
        Row::ProcessScan if cfg!(windows) => line(
            "Find players without setup",
            "By their window title and command line: mpv, mpv.net, MPC-HC, MPC-BE, VLC, PotPlayer. No position or pause (PotPlayer gives both): a file counts after it stays open long enough, and Kotora always asks.",
            switch("set/process-scan".into(), "Find players without setup", p.players.process_scan, State::set_process_scan),
        ),
        Row::ProcessScan => line(
            "Find players by their open files",
            "No setup, but no position or pause: a file counts after it stays open long enough, and Kotora always asks.",
            switch("set/process-scan".into(), "Find players by their open files", p.players.process_scan, State::set_process_scan),
        ),
        Row::ProcessMinutes => {
            let minutes = p.players.process_minutes.clamp(1, 180);
            let stepper = row()
                .key("set/process-minutes")
                .h(28.0)
                .center_y()
                .radius(theme::R_SM)
                .fill(PAL.field)
                .border(1.0, PAL.border_strong)
                .child(cell("set/process-minutes/minus".into(), "Fewer minutes".into(), false, |s| s.step_process_minutes(false)).child(text("\u{2212}").font(theme::LABEL).color(PAL.dim)))
                .child(row().min_w(64.0).center().child(text(format!("{minutes} min")).font(theme::LABEL_NUM).color(PAL.text)))
                .child(cell("set/process-minutes/plus".into(), "More minutes".into(), false, |s| s.step_process_minutes(true)).child(text("+").font(theme::LABEL).color(PAL.dim)));
            line(
                "Count a file after it stays open",
                "Open files only, when the episode\u{2019}s length is unknown (default 15 minutes).",
                if p.players.process_scan { stepper } else { stepper.opacity(0.5) },
            )
        }
        Row::Folders => block("Folders", crate::library::FOLDERS_NOTE, crate::library::folders_block(s)),
        Row::OpenWith => block("Open with", crate::library::OPEN_WITH_NOTE, crate::library::open_with_choice(s)),
        Row::NotifyDetected => notify(
            "Episode detected",
            "When a recognised episode starts playing: \u{201c}Title \u{b7} Ep 6\u{201d}. Off by default.",
            "set/notify-detected",
            p.notify.detected,
            crate::notes::Kind::Detected,
        ),
        Row::NotifyOffer => notify("Update offered", "\u{201c}Update to 6?\u{201d} with Update and Not now, which act as in the banner.", "set/notify-offer", p.notify.offer, crate::notes::Kind::Offer),
        Row::NotifyRecorded => notify("Episode recorded", "\u{201c}Recorded \u{b7} Ep 6\u{201d} with Undo.", "set/notify-recorded", p.notify.recorded, crate::notes::Kind::Recorded),
        Row::NotifyStarted => notify(
            "Started airing",
            "When a show on Plan to watch starts airing: \u{201c}Title started airing\u{201d}.",
            "set/notify-started",
            p.notify.started_airing,
            crate::notes::Kind::Started,
        ),
        Row::NotifyEpisode => notify(
            "New episode",
            "When the next episode of a show you\u{2019}re watching is out, by the estimated broadcast time: \u{201c}Title \u{b7} Ep 7 is out\u{201d}, with Play when the file is in your library.",
            "set/notify-episode",
            p.notify.new_episode,
            crate::notes::Kind::Aired,
        ),
        Row::About => about(s),
        Row::Licences => col().key("set/licences").w_full().gap(3.0).py(12.0).child(text("Licences").font(theme::BODY_MED).color(PAL.text)).child(
            text("Kotora is GPL-3.0-or-later. File names are read with anitomy (MPL-2.0, through anitomy-ng); split seasons are mapped with anime-relations (CC0), both by Taiga\u{2019}s author. \u{5c0f}\u{864e} is drawn from Noto Sans JP (OFL 1.1). Not affiliated with MyAnimeList or AniList.")
                .font(theme::LABEL)
                .color(PAL.dim)
                .wrap(),
        ),
        Row::Links => line(
            "Links",
            "The website, and the source code on GitHub.",
            row().gap(16.0).child(link("link/site", "kotora.moe", "https://kotora.moe")).child(link("link/source", "Source code", "https://github.com/hanami117/kotora")),
        ),
    }
}

fn notify(title: &str, note: &str, key: &str, on: bool, kind: crate::notes::Kind) -> El {
    line(title, note, switch(key.into(), title, on, move |s, v| s.set_notify(kind, v)))
}

/// What the system can't do with notifications, if anything.
fn notify_support(s: &State) -> Option<El> {
    use crate::notes::Support;
    let line = match s.notes.support {
        Support::NoButtons => "Your notification server shows no buttons: answer in Kotora\u{2019}s banner or with kot confirm.",
        Support::Outside => "Kotora runs outside Kotora.app, so notifications have no buttons. Open Kotora.app to get them.",
        Support::Denied if cfg!(windows) => "Notifications are off for Kotora, or for all apps, in Windows Settings \u{2192} System \u{2192} Notifications.",
        Support::Denied => "Notifications are off for Kotora in System Settings \u{2192} Notifications.",
        Support::Unknown | Support::Buttons => return None,
    };
    Some(
        row()
            .key("set/notify-support")
            .mt(6.0)
            .px(12.0)
            .py(10.0)
            .gap(10.0)
            .items_start()
            .radius(theme::R_MD)
            .fill(PAL.tint(PAL.warn(), 0.12))
            .border(1.0, PAL.warn().alpha(0.35))
            .child(icon(&icons::ALERT, 16.0).color(PAL.warn()))
            .child(text(line.to_string()).font(theme::LABEL).color(PAL.text).wrap().grow(1.0).min_w(0.0)),
    )
}

/// One player: its state light, name and line, and its switch; precise detection's hint below.
fn player(s: &State, sw: Switch) -> El {
    let on = s.prefs.players.get(sw.config_key());
    let (line, led) = s.player_line(sw);
    let help = match sw {
        Switch::OtherMpris => format!("Haruna, Celluloid and others that report a file \u{b7} {}", line.split(" \u{b7} ").last().unwrap_or("")),
        _ if !on => "Switched off: Kotora doesn\u{2019}t look for it.".to_string(),
        _ => line,
    };
    let mut c = col().key(format!("set/player/{}", sw.config_key())).w_full().child(
        row()
            .gap(12.0)
            .py(12.0)
            .center_y()
            .child(el().size(8.0, 8.0).radius(4.0).fill(if on { led_color(led) } else { PAL.faint }))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(text(sw.label().to_string()).font(theme::BODY_MED).color(PAL.text))
                    .child(text(help).font(theme::LABEL).color(PAL.dim).truncate()),
            )
            .child(switch(format!("set/switch/{}", sw.config_key()), sw.label(), on, move |s, v| s.set_player(sw, v))),
    );
    if sw == Switch::Mpv
        && let Some(h) = crate::precise::settings_hint(s)
    {
        c = c.child(h);
    }
    #[cfg(target_os = "macos")]
    if sw == Switch::Iina
        && let Some(h) = crate::iina::settings_hint(s)
    {
        c = c.child(h);
    }
    if sw == Switch::Mpc
        && let Some(h) = crate::winsetup::mpc_hint(s)
    {
        c = c.child(h);
    }
    c
}

fn link(key: &str, label: &str, url: &'static str) -> El {
    text(label.to_string())
        .key(key.to_string())
        .role("link")
        .label(label.to_string())
        .focusable(true)
        .font(theme::LABEL_BTN)
        .color(PAL.accent_hi)
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut State| crate::online::open_link(s, url))
}

fn about(s: &State) -> El {
    let logo = el().size(56.0, 56.0).radius(theme::R_LG);
    let logo = match &s.logo_big {
        Some(img) => logo.pixels(img.pixels()),
        None => logo.fill(PAL.raised),
    };
    row()
        .key("about-card")
        .gap(16.0)
        .py(14.0)
        .items_start()
        .child(logo)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .child(
                    row()
                        .gap(8.0)
                        .items_end()
                        .child(text("Kotora \u{5c0f}\u{864e}").font(TextStyle::new(17.0, 600.0)).color(PAL.text))
                        .child(text(about_version(cfg!(windows))).key("about-version").font(theme::LABEL_NUM).color(PAL.faint).pb(2.0)),
                )
                .children(cfg!(windows).then(|| text(WINDOWS_PREVIEW).key("about-preview").font(theme::LABEL).color(PAL.dim).wrap()))
                .child(
                    text("A tribute to Taiga, the Windows anime tracker. Thank you to its author and contributors. Kotora is a separate project.")
                        .font(theme::LABEL)
                        .color(PAL.dim)
                        .wrap(),
                ),
        )
}

/// "Version 0.2.1", and on Windows "Version 0.2.1 · Preview" (the installer says Preview too).
fn about_version(windows: bool) -> String {
    format!("Version {}{}", env!("CARGO_PKG_VERSION"), if windows { " \u{b7} Preview" } else { "" })
}

/// About, on Windows only.
const WINDOWS_PREVIEW: &str = "Kotora for Windows is a preview: it is new, and not signed yet. Please report what goes wrong on GitHub.";

/// One service of Settings → Account. The active one says how its sign-in stands (Sign in,
/// Sign out); the other offers "Use …", which switches services and keeps both lists.
fn account_row(s: &State, id: ServiceId) -> El {
    let (key, badge, fill) = match id {
        ServiceId::Mal => ("acct/mal", "MAL", hex("#2e51a2")),
        ServiceId::AniList => ("acct/anilist", "AL", hex("#1d3550")),
    };
    if s.service != id {
        let switch: fn(&mut State) = match id {
            ServiceId::Mal => |s| crate::online::switch_service(s, ServiceId::Mal),
            ServiceId::AniList => |s| crate::online::switch_service(s, ServiceId::AniList),
        };
        let line = format!("Not in use. Kotora keeps one list at a time: your {} list now.", s.service.title());
        return service_row(key, badge, fill, id.title(), line, small_button(&format!("{key}/use"), &format!("Use {}", id.title()), false, Some(switch)), false);
    }
    let (line, action) = match (&s.account, s.auth) {
        (Some((name, _)), _) => (format!("Signed in as {name}"), small_button("sign-out", "Sign out", false, Some(crate::online::sign_out))),
        (None, Auth::NoClientId) => ("Needs a Client ID first (below)".to_string(), small_button("sign-in", "Sign in", true, None)),
        (None, Auth::SigningIn) => ("Waiting for your browser\u{2026}".to_string(), small_button("cancel", "Cancel", false, Some(crate::online::cancel_sign_in))),
        (None, _) => ("Not signed in".to_string(), small_button("sign-in", "Sign in", true, Some(crate::online::sign_in))),
    };
    service_row(key, badge, fill, id.title(), line, action, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(q: &str) -> Vec<Row> {
        find(q).into_iter().map(|i| INDEX[i].row).collect()
    }

    fn finds(q: &str, r: Row) -> bool {
        rows(q).contains(&r)
    }

    #[test]
    fn matches_titles_notes_and_section_names() {
        assert_eq!(rows("startup animation"), vec![Row::Intro]);
        assert!(finds("claw", Row::Intro), "a word from the note");
        assert!(finds("notifications", Row::NotifyOffer), "the section name");
        assert!(finds("detection", Row::Threshold));
        assert!(rows("").is_empty(), "an empty query finds nothing");
        assert!(rows("   ").is_empty());
        assert!(rows("zzzz").is_empty());
    }

    #[test]
    fn every_word_must_match() {
        assert_eq!(rows("keep higher"), vec![Row::KeepHigher]);
        assert!(rows("theme minutes").is_empty());
    }

    #[test]
    fn synonyms() {
        for (q, r) in [
            ("login", Row::StartAtLogin),
            ("log in", Row::StartAtLogin),
            ("autostart", Row::StartAtLogin),
            ("minimised", Row::StartHidden),
            ("tray", Row::StartHidden),
            ("theme", Row::Theme),
            ("dark", Row::Theme),
            ("light", Row::Theme),
            ("mpv", Row::Player(Switch::Mpv)),
            ("precise", Row::Player(Switch::Mpv)),
            ("vlc", Row::Player(Switch::Vlc)),
            ("english", Row::Titles),
            ("keyboard", Row::PageKeys),
            ("shortcuts", Row::PageKeys),
            ("alt", Row::PageKeys),
            ("hotkey", Row::PageKeys),
            ("sign out", Row::Mal),
            ("client id", Row::ClientId),
            ("anilist", Row::AniList),
            ("threshold", Row::Threshold),
            ("percent", Row::Threshold),
            ("conflict", Row::KeepHigher),
            ("folder", Row::Folders),
            ("open with", Row::OpenWith),
            ("new episode", Row::NotifyEpisode),
            ("license", Row::Licences),
            ("licence", Row::Licences),
            ("github", Row::Links),
            ("version", Row::About),
            ("update", Row::CheckUpdates),
        ] {
            assert!(finds(q, r), "{q:?} finds {r:?}");
        }
        #[cfg(target_os = "macos")]
        {
            assert!(finds("iina", Row::Player(Switch::Iina)));
            assert!(finds("vlc password", Row::VlcWeb));
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            assert!(!finds("iina", Row::Player(Switch::Iina)), "no IINA off the Mac");
            assert!(finds("haruna", Row::Player(Switch::OtherMpris)));
            assert!(rows("vlc password").is_empty(), "the web interface is the Mac's");
        }
        #[cfg(windows)]
        {
            assert!(!finds("iina", Row::Player(Switch::Iina)), "no IINA off the Mac");
            assert!(finds("vlc password", Row::VlcWeb));
        }
    }

    #[test]
    fn case_and_accents_dont_matter() {
        assert!(finds("THEME", Row::Theme));
        assert!(finds("thème", Row::Theme));
        assert!(finds("  LÏCENCE ", Row::Licences));
    }

    #[test]
    fn every_row_is_in_its_section_once_and_findable() {
        let mut seen = Vec::new();
        for sec in Section::ALL {
            for (_, rows) in sec.groups() {
                for r in rows {
                    assert!(!seen.contains(&r), "{r:?} twice");
                    seen.push(r);
                    let e = INDEX.iter().find(|e| e.row == r).unwrap_or_else(|| panic!("{r:?} not in the search"));
                    assert_eq!(e.section, sec, "{r:?}");
                }
            }
        }
        assert_eq!(seen.len(), INDEX.len(), "the search has no row the sections don't show");
    }

    #[test]
    fn results_keep_section_order() {
        let hits = results(&{
            let mut s = State::fake();
            set_query(&mut s, "notification");
            s
        });
        assert!(hits.len() >= 5);
        let order = |x: Section| Section::ALL.iter().position(|y| *y == x).unwrap();
        assert!(hits.windows(2).all(|w| order(w[0].0) <= order(w[1].0)));
    }

    #[test]
    fn the_section_is_remembered() {
        let mut s = State::fake();
        assert_eq!(s.settings.section, Section::General, "General the first time");
        open(&mut s, Section::Detection);
        s.go(crate::model::Page::List(kotora_core::Status::Watching));
        s.go(crate::model::Page::Settings);
        assert_eq!(s.settings.section, Section::Detection, "back on the section left");
        // `config.toml` keeps it across starts.
        let c = config::Config::parse("[display]\nsettings_section = \"notifications\"\n").unwrap();
        load(&mut s, &c);
        assert_eq!(s.settings.section, Section::Notifications);
        for (k, want) in [("", Section::General), ("nonsense", Section::General), ("about", Section::About)] {
            let mut c = config::Config::default();
            c.display.settings_section = k.into();
            load(&mut s, &c);
            assert_eq!(s.settings.section, want, "{k:?}");
        }
        for sec in Section::ALL {
            assert_eq!(Section::from_key(sec.key()), Some(sec));
        }
    }

    #[test]
    fn keys_step_focus_and_clear() {
        use rune::Mods;
        let ev = |key: Key, mods: Mods| KeyEvent { key, mods };
        let mut s = State::fake();
        s.go(crate::model::Page::Settings);
        let mut tx = Tx::default();
        assert_eq!(on_key(&mut s, &ev(Key::Up, Mods::default()), &mut tx), Some(false), "already the first");
        on_key(&mut s, &ev(Key::Down, Mods::default()), &mut tx);
        on_key(&mut s, &ev(Key::Down, Mods::default()), &mut tx);
        assert_eq!(s.settings.section, Section::Account);
        for _ in 0..10 {
            on_key(&mut s, &ev(Key::Down, Mods::default()), &mut tx);
        }
        assert_eq!(s.settings.section, Section::About, "no wrap");
        // Cmd+F (Ctrl+F) focuses the field, here and through the app's keys.
        let f = if cfg!(target_os = "macos") { ev(Key::Command('f'), Mods { cmd: true, ..Default::default() }) } else { ev(Key::Command('f'), Mods { ctrl: true, ..Default::default() }) };
        assert!(crate::ui::on_key(&mut s, &f, &mut tx));
        assert!(s.settings.query.focused);
        assert_eq!(s.page, crate::model::Page::Settings, "not the search page");
        // Typing, then Escape clears; Up and Down leave the results alone.
        set_query(&mut s, "theme");
        assert_eq!(on_key(&mut s, &ev(Key::Up, Mods::default()), &mut tx), None);
        assert!(crate::ui::on_key(&mut s, &ev(Key::Escape, Mods::default()), &mut tx));
        assert!(s.settings.query.text.is_empty() && s.settings.hits.is_empty());
        assert_eq!(s.settings.section, Section::About);
    }

    #[test]
    fn enter_jumps_to_the_first_result_and_flashes_it() {
        let mut s = State::fake();
        s.go(crate::model::Page::Settings);
        set_query(&mut s, "dark");
        let mut tx = Tx::default();
        jump_first(&mut s, &mut tx);
        assert_eq!(s.settings.section, Section::Appearance);
        assert_eq!(s.settings.flash, Some(Row::Theme));
        assert!(!searching(&s));
        assert!(!poll(&mut s, 10.0, &mut tx), "the tint starts");
        assert!(poll(&mut s, 10.0 + FLASH, &mut tx), "and ends");
        assert_eq!(s.settings.flash, None);
    }
}
