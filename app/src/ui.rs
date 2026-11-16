//! The view: sidebar, list page with the inline editor, Now Playing (page and banner), sync queue
//! and Settings, as in
//! `mockup/kotora.html` (design-spec §4).

use std::f32::consts::PI;

use kotora_core::Status;
use rune::{Color, Cx, El, GrowthSpec, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text, text_input};

use crate::icons;
use kotora_core::queue::{Op, OpState};

use crate::model::{Air, Auth, Editor, Entry, Filter, Page, Sort, State, remote_text};
use crate::now::{Now, THRESHOLDS, clock_text};
use kotora_core::recognize::Found;
use kotora_detect::{Led, Switch};
use crate::online;
use crate::queue::Cmd;
use crate::theme::{self, PAL};

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    // The palette first: Settings → Appearance, or the desktop's preference, which Rune follows
    // live (macOS appearance; the XDG portal's color-scheme on Linux; dark when unknown; `--theme`
    // headless).
    theme::apply(s.prefs.theme.is_light(cx.system_dark()));
    #[cfg(target_os = "macos")]
    crate::menu_bar::sync();
    // The type column hides below 1100 pt (design-spec §3).
    let wide = cx.layout_size("root").is_none_or(|[w, _]| w >= theme::NARROW_W);
    // macOS draws the traffic lights over the window's top-left (Rune's full-size content), so the
    // sidebar keeps a band clear for them. None in full screen; Linux draws its own frame.
    s.covers.set_scale(cx.env().scale);
    let chrome_top = if cfg!(target_os = "macos") && cx.titlebar_inset() > 0.0 { theme::TITLEBAR_BAND } else { 0.0 };
    // A window resize (the window's size changed since the last build) reflows the pages:
    // posters change rows, columns and the sidebar card change size. That lands at once, as in
    // Finder: no fading rows, no cards sliding up. Changes the user makes (filter, sort, view)
    // keep their own transitions.
    let size = cx.layout_size("root").unwrap_or([0.0, 0.0]);
    let resized = s.main_size.replace(size) != size;
    let root = row()
        .key("root")
        .w_full()
        .h_full()
        .fill(PAL.bg);
    let root = if resized { root.instant_changes() } else { root };
    root
        .child(sidebar(s, chrome_top, cx.layout_size("root").map_or(800.0, |[_, h]| h)))
        .child(el().w(1.0).h_full().fill(PAL.border))
        .child(main(s, wide, cx.layout_size("main").unwrap_or([1056.0, 800.0])))
        // Menus last, so they draw over everything.
        .children(s.sort_menu.then(|| sort_menu(s)))
        .children(s.toast.as_ref().map(crate::library::toast))
}

/// Workers report through `update_if`. The one timer is the position of a playing file, only
/// while a window shows it (`now::tick`); at rest `poll` has no change and asks for no wake.
pub fn poll(s: &mut State, _t: f64, tx: &mut Tx) -> bool {
    // Both run: each may ask for its own wake.
    // The tray's line follows picks made in the window too (cheap: a string compare).
    s.sync_tray();
    let now = crate::now::tick(s, tx);
    let airing = crate::airing::tick(s, tx);
    let toast = crate::library::toast_tick(s, tx);
    now || airing || toast
}

/// Ctrl K (Cmd K on macOS) opens the search page from anywhere, as does Ctrl F off the list
/// pages; on a list page Ctrl F jumps to its filter box. Escape closes the inline editor, a
/// menu, the details page or the search page.
pub fn on_key(s: &mut State, k: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if k.is_command('k') || (k.is_command('f') && !matches!(s.page, Page::List(_))) {
        tx.instant();
        s.open_search();
        return s.page == Page::Search;
    }
    if k.is_command('f') && matches!(s.page, Page::List(_)) && s.has_list() {
        s.query.focused = true;
        return true;
    }
    if k.key == rune::Key::Escape && s.ask.is_some() {
        s.ask = None;
        return true;
    }
    if k.key == rune::Key::Escape && s.editor.is_some() {
        s.editor = None;
        return true;
    }
    if k.key == rune::Key::Escape && s.online.menu.is_some() {
        s.online.menu = None;
        return true;
    }
    if k.key == rune::Key::Escape && s.page == Page::Details {
        s.details_back();
        return true;
    }
    if k.key == rune::Key::Escape && s.page == Page::Setup && s.setup_closable() {
        tx.instant();
        s.setup_close();
        return true;
    }
    if k.key == rune::Key::Escape && s.page == Page::Search {
        tx.instant();
        s.close_search();
        return true;
    }
    // P (or Cmd/Ctrl+Return) plays the selected entry's next episode, as a middle-click does.
    let p = matches!(&k.key, rune::Key::Text(t) if t.eq_ignore_ascii_case("p")) && !(k.mods.cmd || k.mods.ctrl || k.mods.alt);
    if (p || (k.key == rune::Key::Enter && k.mods.cmd)) && let Some(id) = s.play_target() {
        s.play_entry(id);
        return true;
    }
    s.grid_key(k, tx)
}

// ------------------------------------------------------------------------------------ sidebar

/// Opens a page as its sidebar item does (also the macOS Go menu).
pub(crate) fn go_to(s: &mut State, page: Page, tx: &mut Tx) {
    // A new page replaces the old one at once; rows don't swim.
    tx.instant();
    match page {
        Page::Search => s.open_search(),
        Page::Airing => s.open_airing(),
        _ => s.go(page),
    }
}

/// `chrome_top`: the traffic-light band's height (macOS), or 0 where the system draws the frame.
/// `height`: the window's, for the playing card's size.
fn sidebar(s: &State, chrome_top: f32, height: f32) -> El {
    let side = col().key("sidebar").w(theme::SIDEBAR_W).h_full().fill(PAL.side).px(10.0).pb(12.0);
    let side = if chrome_top > 0.0 {
        // The band drags the window like a title bar; the brand sits just under it.
        side.child(el().key("sb-titlebar").w_full().h(chrome_top).window_drag()).child(el().h(4.0))
    } else {
        side.pt(14.0)
    };
    let mut side = side.child(brand(s));
    if let Some((name, service)) = &s.account {
        side = side.child(account(s, name, service, s.lit_page() == Page::Profile));
    }
    let lit = s.lit_page();
    // The pages scroll on their own when the window is too short for them; the playing card
    // and the sync line stay at the bottom.
    let mut nav = col().w_full();
    if s.has_list() {
        nav = nav.child(nav_item("home", NavMark::Icon(&icons::HOME), "Home", None, lit == Page::Home, Page::Home));
    }
    if s.has_list() {
        let kbd = if cfg!(target_os = "macos") { "\u{2318} K" } else { "Ctrl K" };
        nav = nav.child(nav_item("search", NavMark::Icon(&icons::SEARCH), "Search", Some(NavEnd::Kbd(kbd)), lit == Page::Search, Page::Search).mb(6.0));
    }
    nav = nav.child(nav_heading("My list"));
    for st in Status::ALL {
        let count = (!s.loading && s.has_list()).then(|| s.count(st).to_string());
        nav = nav.child(nav_item(
            st.slug(),
            NavMark::Dot(PAL.status(st)),
            st.label(),
            count.map(NavEnd::Count),
            lit == Page::List(st),
            Page::List(st),
        ));
    }
    nav = nav.child(nav_heading("Kotora").mt(12.0));
    nav = nav
        .child(nav_item("now-playing", NavMark::Icon(&icons::PLAY), "Now Playing", None, lit == Page::NowPlaying, Page::NowPlaying))
        .child(nav_item("airing", NavMark::Icon(&icons::CALENDAR), "Airing", None, lit == Page::Airing, Page::Airing))
        .child(nav_item(
            "queue",
            NavMark::Icon(&icons::SYNC),
            "Sync queue",
            (s.queue > 0).then(|| NavEnd::Badge(s.queue.to_string())),
            lit == Page::Queue,
            Page::Queue,
        ))
        .child(nav_item("settings", NavMark::Icon(&icons::SLIDERS), "Settings", None, lit == Page::Settings, Page::Settings));
    side = side.child(col().key("sb-nav").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(nav.pb(8.0)));
    if let Some(now) = &s.now {
        side = side.child(now_mini(s, now, height >= 760.0));
    }
    let (dot, line) = if s.syncing {
        (PAL.faint, "Syncing…".to_string())
    } else if s.offline && s.queue > 0 {
        (PAL.warn(), format!("Offline · {} changes waiting", s.queue))
    } else if s.offline {
        (PAL.warn(), "Offline · showing your saved list".to_string())
    } else if let Some(at) = s.synced_at {
        (PAL.ok(), format!("Synced {}", sync_age(s.clock() - at)))
    } else if s.has_list() {
        (PAL.faint, "Not synced yet".to_string())
    } else {
        (PAL.faint, "Not signed in".to_string())
    };
    side.child(
        row()
            .key("syncline")
            .gap(6.0)
            .center_y()
            .pt(10.0)
            .px(8.0)
            .child(el().size(6.0, 6.0).radius(3.0).fill(dot))
            .child(text(line).font(theme::SMALL).color(PAL.faint)),
    )
}

/// The sidebar's now-playing card: the poster with the text beside it (player, two lines of
/// title, episode, how far in), the cover's blurred tint behind. Opens Now Playing. Short
/// windows (`tall` false) get a lower card: a smaller poster and one line of title.
fn now_mini(s: &State, now: &Now, tall: bool) -> El {
    use crate::look;
    let (id, title) = now_title(s, now);
    let url = now_cover_url(s, now);
    let entry = now.media_id().and_then(|id| s.entries.iter().find(|e| e.id == id));
    let episode = match (now.episode(), entry.and_then(|e| e.episodes)) {
        (Some(ep), Some(total)) => format!("Episode {ep} of {total}"),
        (Some(ep), None) => format!("Episode {ep}"),
        _ => String::new(),
    };
    let player = if now.playing { now.player.clone() } else { format!("{} \u{b7} paused", now.player) };
    let tail = match now.fraction() {
        Some(f) => row()
            .key("np-mini/progress")
            .gap(6.0)
            .center_y()
            .child(col().grow(1.0).child(look::bar(f, PAL.accent, 4.0, PAL.text.alpha(0.16))))
            .child(text(format!("{:.0}%", f * 100.0)).font(theme::LABEL_NUM).color(PAL.dim)),
        None => text(if now.reached { "Counts as watched" } else { "Open in the player" }).font(theme::SMALL).color(PAL.dim),
    };
    el().key("np-mini")
        .role("button")
        .label(format!("Now playing: {title}"))
        .w_full()
        .h(if tall { 124.0 } else { 76.0 })
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .press(PAL.press())
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.go(Page::NowPlaying);
        })
        .child(look::backdrop(s, id, url, look::card_scrim()))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(if tall { 10.0 } else { 8.0 })
                .gap(10.0)
                .child(if tall { look::poster(s, id, &title, url, 68.0, 102.0, theme::R_SM) } else { look::poster(s, id, &title, url, 40.0, 60.0, theme::R_XS) }.shadow(4.0, 10.0, Color::BLACK.alpha(0.35)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(3.0)
                        .child(row().gap(6.0).center_y().child(look::live_dot(now.playing, 6.0)).child(text(player.to_uppercase()).font(theme::CAPTION).color(PAL.dim).truncate()))
                        .child(text(title).font(TextStyle::new(13.0, 600.0)).color(PAL.text).wrap().max_lines(if tall { 2 } else { 1 }))
                        .child(if tall { text(episode).font(theme::SMALL).color(PAL.dim).truncate() } else { el() })
                        .child(spacer())
                        .child(tail),
                ),
        )
}

/// "just now", "2 min ago", "3 h ago", "2 days ago".
pub(crate) fn sync_age(secs: i64) -> String {
    match secs.max(0) {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn brand(s: &State) -> El {
    let logo = el().size(28.0, 28.0).radius(theme::R_SM);
    let logo = match &s.logo {
        Some(img) => logo.pixels(img.pixels()),
        None => logo.fill(PAL.raised),
    };
    row()
        .key("brand")
        .gap(10.0)
        .center_y()
        .pt(2.0)
        .px(8.0)
        .pb(14.0)
        .child(logo)
        .child(text("Kotora").font(rune::TextStyle::new(15.0, 600.0).tracking(0.2)).color(PAL.text))
        .child(spacer())
        .child(text(env!("CARGO_PKG_VERSION").rsplit_once('.').map_or("", |(v, _)| v)).font(theme::SMALL).color(PAL.faint))
}

/// The cover cache's id for the profile picture (no anime has it).
pub(crate) const AVATAR_ID: u32 = u32::MAX;

/// The account card; it opens the Profile page (`on`: that page is open).
fn account(s: &State, name: &str, service: &str, on: bool) -> El {
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    // The profile picture, round, over the initial (shown while it loads, offline or with none).
    let mut face = el()
        .size(24.0, 24.0)
        .radius(12.0)
        .linear_gradient(160.0 * PI / 180.0, [(0.0, hex("#2e51a2")), (1.0, hex("#1d3170"))])
        .center()
        // The fill is MAL's blue in both themes, so the letter stays light.
        .child(text(initial).font(rune::TextStyle::new(10.0, 600.0)).color(hex("#edf0f7")));
    if let Some(img) = s.covers.get(AVATAR_ID, s.avatar.as_deref(), 24.0, 24.0) {
        face = face.child(el().key("avatar-img").absolute(0.0, 0.0).size(24.0, 24.0).radius(12.0).pixels(img.pixels()));
    }
    row()
        .key("account")
        .role("button")
        .label(format!("Profile: {name}"))
        .focusable(true)
        .gap(8.0)
        .padding(8.0)
        .mb(14.0)
        .center_y()
        .radius(theme::R_MD)
        .fill(if on { PAL.raised } else { PAL.surface })
        .border(1.0, if on { PAL.border_strong } else { PAL.border })
        .hover(PAL.hover())
        .press(PAL.press())
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(|s: &mut State, tx: &mut Tx| go_to(s, Page::Profile, tx))
        .child(face)
        .child(
            col()
                .min_w(0.0)
                .child(text(name.to_string()).font(theme::BODY).color(PAL.text).truncate())
                .child(text(service.to_string()).font(theme::SMALL).color(PAL.faint)),
        )
}

fn nav_heading(label: &str) -> El {
    text(label.to_uppercase()).font(theme::CAPTION).color(PAL.faint).py(6.0).px(8.0)
}

enum NavMark {
    Dot(Color),
    Icon(&'static rune::Icon),
}

enum NavEnd {
    Count(String),
    Badge(String),
    /// A keyboard shortcut ("Ctrl K").
    Kbd(&'static str),
}

fn nav_item(key: &str, mark: NavMark, label: &str, end: Option<NavEnd>, on: bool, page: Page) -> El {
    let fg = if on { PAL.text } else { PAL.dim };
    let mark = match mark {
        NavMark::Dot(c) => el().size(8.0, 8.0).radius(4.0).mx(4.0).fill(c),
        NavMark::Icon(i) => icon(i, 16.0).color(fg),
    };
    let weight = if on { 500.0 } else { 400.0 };
    let mut item = row()
        .key(format!("nav/{key}"))
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(theme::NAV_H)
        .px(8.0)
        .gap(10.0)
        .center_y()
        .radius(theme::R_SM)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| go_to(s, page, tx))
        .child(mark)
        .child(text(label.to_string()).font(rune::TextStyle::new(13.0, weight)).color(fg))
        .child(spacer());
    if on {
        item = item.fill(PAL.raised);
    }
    match end {
        Some(NavEnd::Count(n)) => item.child(text(n).font(theme::LABEL_NUM).color(if on { PAL.text } else { PAL.faint })),
        Some(NavEnd::Badge(n)) => item.child(
            el().min_w(18.0).h(18.0).px(5.0).radius(9.0).fill(PAL.accent).center().child(text(n).font(theme::BADGE).color(PAL.on_accent)),
        ),
        Some(NavEnd::Kbd(k)) => item.child(kbd(k)),
        None => item,
    }
}

/// A shortcut hint: small text in an outlined box.
pub(crate) fn kbd(label: &str) -> El {
    el().px(5.0).py(1.0).radius(4.0).border(1.0, PAL.border_strong).child(text(label.to_string()).font(TextStyle::new(10.0, 500.0)).color(PAL.faint))
}

// --------------------------------------------------------------------------------------- main

/// `width`: the main area's, from the last layout (the Airing grid's cards per row).
fn main(s: &State, wide: bool, size: [f32; 2]) -> El {
    let width = size[0];
    let page = match s.page {
        Page::Setup => crate::setup::page(s),
        // First run: no Client ID, so the wizard is the only thing a list page can show.
        Page::List(_) if s.auth == Auth::NoClientId => crate::setup::page(s),
        Page::List(_) if !s.has_list() => welcome(s),
        Page::List(st) => list_page(s, st, wide, width),
        Page::Home if !s.has_list() => welcome(s),
        Page::Home => crate::home::page(s),
        Page::Profile if s.account.is_none() => welcome(s),
        Page::Profile => crate::profile::page(s, wide),
        Page::NowPlaying => now_page(s, wide, size),
        Page::Queue => queue_page(s),
        Page::Settings => settings(s, wide),
        Page::Details => crate::details::page(s, wide),
        Page::Search if !s.has_list() => welcome(s),
        Page::Search => search_view(s, wide),
        Page::Airing if !s.has_list() => welcome(s),
        Page::Airing => crate::airing::page(s, width),
    };
    let mut m = col().key("main").grow(1.0).min_w(0.0).h_full();
    // The Now Playing banner sits on top of every page (design-spec §4).
    if s.banner() {
        m = m.child(banner(s));
    } else if s.precise_banner() {
        m = m.child(crate::precise::banner(s));
    }
    // Now Playing: turn on precise detection for the followed player (once per player).
    if s.page == Page::NowPlaying
        && let Some(p) = crate::precise::prompt(s)
    {
        m = m.child(p);
    }
    if let Some(a) = &s.ask {
        m = m.child(ask_bar(a));
    }
    // A newer Kotora is out (`update.rs`): quiet, under the rest.
    if let Some(u) = crate::update::banner(s) {
        m = m.child(u);
    }
    m.child(col().grow(1.0).min_h(0.0).w_full().child(page))
}

/// "Mark anyway?": an edit by hand (`+1`, the editor) of an episode that hasn't aired yet by the
/// estimate waits here. Escape or Cancel drops it.
fn ask_bar(a: &crate::model::Ask) -> El {
    row()
        .key("ask")
        .mx(theme::PAGE_PAD)
        .mt(14.0)
        .px(14.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(theme::R_LG)
        .fill(mix(PAL.bg, PAL.warn(), 0.10))
        .border(1.0, PAL.warn().alpha(0.45))
        .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.warn()))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(text(a.question.clone()).key("ask/question").font(theme::BODY).color(PAL.text).wrap())
                .child(text("Times are estimated from MyAnimeList's weekly slot.").font(theme::LABEL).color(PAL.dim)),
        )
        .child(text("Mark anyway?").font(TextStyle::new(14.0, 600.0)).color(PAL.cream))
        .child(button("ask/anyway", "Mark anyway", true, Some(|s: &mut State| {
            if let Err(m) = s.mark_anyway() {
                s.error = Some(m);
            }
        })))
        .child(button("ask/cancel", "Cancel", false, Some(|s: &mut State| s.ask = None)))
}

/// The page's top bar: title, count, and (on the list) search and sort.
pub(crate) fn top(s: &State, title: &str, count: Option<usize>, tools: bool) -> El {
    top_with(s, title, count, None, tools)
}

/// A list page's top bar with a note after the count ("+ 1 rewatching").
fn top_extra(s: &State, title: &str, count: usize, extra: String) -> El {
    top_with(s, title, Some(count), Some(extra), true)
}

fn top_with(s: &State, title: &str, count: Option<usize>, extra: Option<String>, tools: bool) -> El {
    let mut t = row().key("top").gap(12.0).center_y().px(theme::PAGE_PAD).pt(18.0).pb(14.0).child(
        row()
            .gap(6.0)
            .items_end()
            .child(text(title.to_string()).font(theme::TITLE).color(PAL.text))
            .child(match count {
                Some(n) => text(n.to_string()).font(theme::TITLE_NUM).color(PAL.faint),
                None => el(),
            })
            .child(match extra {
                Some(x) => text(x).key("top/extra").font(theme::LABEL).color(PAL.faint).pb(3.0),
                None => el(),
            }),
    );
    if tools {
        t = t.child(spacer()).child(search_box(s)).child(sort_select(s));
        if s.auth == Auth::SignedIn {
            t = t.child(refresh_button(s.syncing));
        }
    }
    t
}

fn search_box(s: &State) -> El {
    let kbd = if cfg!(target_os = "macos") { "\u{2318} F" } else { "Ctrl F" };
    row()
        .key("search")
        .w(260.0)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .child(icon(&icons::SEARCH, 16.0).color(PAL.faint))
        .child(
            text_input(&s.query, theme::BODY)
                .key("search-input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder("Filter your list", PAL.faint)
                .on_edit(|s: &mut State| &mut s.query)
                .on_input_change(|s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    s.search_changed();
                })
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    // A bare MyAnimeList id opens its page; otherwise Enter only leaves the box.
                    if s.search_submit() {
                        tx.instant();
                    }
                    tx.blur();
                }),
        )
        .child(self::kbd(kbd))
}

/// The Sort button: opens the Sort menu ([`sort_menu`]), by a press, Space, Return or Down.
fn sort_select(s: &State) -> El {
    let (sort, open) = (s.sort, s.sort_menu);
    let toggle = |s: &mut State, tx: &mut Tx| {
        tx.instant();
        s.sort_menu = !s.sort_menu;
    };
    let b = row()
        .key("sort")
        .role("button")
        .label(format!("Sort: {}, {}", sort.label(), sort.directions()[usize::from(s.sort_rev)]))
        .focusable(true)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(if open { PAL.raised } else { PAL.field })
        .border(1.0, if open { PAL.border_strong } else { PAL.border })
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(toggle)
        .on_key(move |s: &mut State, k: &rune::KeyEvent, tx: &mut Tx| {
            if matches!(k.key, rune::Key::Enter | rune::Key::Down) && !s.sort_menu {
                toggle(s, tx);
                return true;
            }
            false
        })
        .child(text("Sort").font(theme::BODY).color(PAL.faint))
        .child(text(sort.label()).font(theme::BODY_MED).color(PAL.text));
    // A reversed order shows as an arrow (the words are in the label and the menu).
    let b = if s.sort_rev { b.child(text("\u{2191}").key("sort/rev").font(theme::BODY_MED).color(PAL.accent_hi)) } else { b };
    b.child(icon(&icons::DOWN, 14.0).color(PAL.text))
}

/// The Sort menu, under the Sort button: the orders, then the two directions, a check on each
/// one in force. It holds the keyboard while open (arrows move, Return or Space picks, Escape
/// or a press outside closes it, and the focus goes back to the button). Mounted at the
/// window's root so nothing clips it.
fn sort_menu(s: &State) -> El {
    let item = |key: String, label: &str, on: bool, pick: Box<dyn Fn(&mut State)>| {
        row()
            .key(key)
            .role("menuitemradio")
            .label(label.to_string())
            .checked(on)
            .focusable(true)
            .focus_selected(on)
            .focus_highlight(PAL.hover())
            .h(30.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(theme::R_SM)
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                pick(s);
                s.sort_menu = false;
            })
            .child(el().size(14.0, 14.0).center().child(if on { icon(&icons::CHECK, 14.0).color(PAL.accent_hi) } else { el() }))
            .child(text(label.to_string()).font(if on { theme::BODY_MED } else { theme::BODY }).color(PAL.text))
    };
    let heading = |t: &str| text(t.to_uppercase()).font(theme::CAPTION).color(PAL.faint).px(8.0).pt(6.0).pb(2.0);
    let mut m = col()
        .key("sort-menu")
        .role("menu")
        .label("Sort")
        .focus_group()
        .w(220.0)
        .padding(4.0)
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border_strong)
        .shadow(8.0, 24.0, Color::BLACK.alpha(if PAL.light { 0.14 } else { 0.4 }))
        .anchored("sort", rune::Side::Bottom, rune::Align::End, 6.0)
        .modal(true, |s: &mut State| s.sort_menu = false)
        .focus_return("sort")
        .child(heading("Sort by"));
    for o in Sort::ALL {
        m = m.child(item(format!("sort/{}", o.slug()), o.label(), s.sort == o, Box::new(move |s: &mut State| s.set_sort(o))));
    }
    m = m.child(el().w_full().h(1.0).my(4.0).fill(PAL.border)).child(heading("Order"));
    for (i, label) in s.sort.directions().into_iter().enumerate() {
        let rev = i == 1;
        m = m.child(item(format!("sort/dir/{i}"), label, s.sort_rev == rev, Box::new(move |s: &mut State| s.set_sort_rev(rev))));
    }
    m
}

fn refresh_button(syncing: bool) -> El {
    el().key("refresh")
        .role("button")
        .label("Refresh")
        .focusable(true)
        .size(32.0, 32.0)
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            online::refresh(s);
        })
        .child(icon(&icons::SYNC, 16.0).color(if syncing { PAL.faint } else { PAL.dim }))
}

fn list_page(s: &State, st: Status, wide: bool, width: f32) -> El {
    if crate::search::shows(s) && !s.loading {
        return search_page(s, st, wide);
    }
    let count = s.count(st);
    // The Watching page shows rewatches too; the sidebar counts MAL's status only, so the page
    // says how many more it shows.
    let rewatches = if st == Status::Watching { s.rewatch_count() } else { 0 };
    let mut head = top(s, st.label(), (!s.loading).then_some(count), true);
    if rewatches > 0 && !s.loading {
        head = top_extra(s, st.label(), count, format!("+ {rewatches} rewatching"));
    }
    let count = count + rewatches;
    // The view switch ends the top bar; a narrow window has no room there, so it ends the chips.
    if !s.loading && wide {
        head = head.child(crate::grid::switcher(s, st, true));
    }
    let mut page = col().key(format!("page/{}", st.slug())).w_full().h_full().child(head);
    if let Some(a) = status_alert(s) {
        page = page.child(a);
    }
    if s.loading {
        return page.child(loading());
    }
    if count == 0 {
        return page.child(empty(st));
    }
    page = page.child(if wide { chips(s, st) } else { chips(s, st).child(spacer()).child(crate::grid::switcher(s, st, false).mt(-3.0)) });
    if s.rows.is_empty() {
        return page.child(no_match(s));
    }
    if s.view_of(st) != kotora_core::config::ListView::List {
        return page.child(crate::grid::cards(s, st, width));
    }
    let n = s.rows.len();
    page.child(header(wide, s.sort)).child(
        col().key(format!("rows-scroll/{}", st.slug())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
            col().key("rows").growth(GrowthSpec::default()).virtual_rows(
                n,
                theme::ROW_H,
                move |s: &State, i| {
                    // `i` is an index into the shown rows; the key is the media id.
                    s.rows.get(i).map_or_else(|| format!("gone-{i}").into(), |&k| s.entries[k].id.to_string().into())
                },
                move |s: &State, i| match s.rows.get(i) {
                    Some(&k) => list_row(s, &s.entries[k], wide),
                    None => el(),
                },
            ),
        ),
    )
}

/// The list page while its filter box holds a query: the list's matches (every status), then a
/// line that takes the query to the search page (`search.rs`).
fn search_page(s: &State, st: Status, wide: bool) -> El {
    let page = col().key("page/filter").w_full().h_full().child(top(s, "Your list", Some(s.rows.len()), true));
    results(s, page, format!("filter-scroll/{}", st.slug()), wide)
}

/// The search results under `page`: the id line, the list's matches and MyAnimeList's (or, on
/// the list page, the line to the search page).
fn results(s: &State, mut page: El, scroll_key: String, wide: bool) -> El {
    use crate::search::{SRow, layout};
    if let Some(a) = status_alert(s) {
        page = page.child(a);
    }
    let n = layout(s).len();
    page.child(header(wide, s.sort)).child(
        col().key(scroll_key).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
            col().key("rows").growth(GrowthSpec::default()).virtual_rows(
                n,
                theme::ROW_H,
                |s: &State, i| layout(s).get(i).map_or_else(|| format!("gone-{i}"), |r| crate::search::key(s, *r)).into(),
                move |s: &State, i| match layout(s).get(i) {
                    Some(SRow::List(k)) => s.rows.get(*k).map_or_else(el, |&k| list_row(s, &s.entries[k], wide)),
                    Some(r) => crate::search::line(s, *r),
                    None => el(),
                },
            ),
        ),
    )
}

/// The search page (the sidebar's Search, Ctrl K): a wide field, then the list's matches and
/// MyAnimeList's results, as the list page showed them before.
fn search_view(s: &State, wide: bool) -> El {
    let field = row()
        .key("find")
        .role("textbox")
        .h(40.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(icon(&icons::SEARCH, 18.0).color(PAL.faint))
        .child(
            text_input(&s.find, TextStyle::new(15.0, 400.0))
                .key("find-input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder("Search your list and MyAnimeList, or paste a MyAnimeList link", PAL.faint)
                .on_edit(|s: &mut State| &mut s.find)
                // Escape goes back from the field too (the field would only let go of the focus).
                .on_input_key(|s: &mut State, k: &rune::KeyEvent, tx: &mut Tx| {
                    if k.key != rune::Key::Escape || s.online.menu.is_some() {
                        return false;
                    }
                    tx.instant();
                    tx.blur();
                    s.close_search();
                    true
                })
                .on_input_change(|s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    s.search_changed();
                })
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    if s.search_submit() {
                        tx.instant();
                    }
                    tx.blur();
                }),
        )
        .child(kbd("Esc"));
    let count = s.searching().then(|| s.rows.len() + s.new_hits().len());
    let page = col()
        .key("page/search")
        .w_full()
        .h_full()
        .child(top(s, "Search", count, false))
        .child(row().w_full().px(theme::PAGE_PAD).pb(14.0).child(field.grow(1.0).max_w(720.0)));
    if !s.searching() {
        return page.child(search_hint());
    }
    results(s, page, "search-scroll".into(), wide)
}

/// The empty search page: what it finds.
fn search_hint() -> El {
    let line = |t: &str| text(t.to_string()).font(theme::LABEL).color(PAL.dim).wrap();
    col()
        .key("search-hint")
        .mx(theme::PAGE_PAD)
        .max_w(720.0)
        .gap(6.0)
        .padding(16.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(text("Find any anime").font(theme::SECTION).color(PAL.text))
        .child(line("Titles in English, Japanese or romaji search your list first, then MyAnimeList (3 letters or more)."))
        .child(line("A MyAnimeList link or id (such as 41590) opens that anime's page, on your list or not."))
        .child(text("Esc goes back. The box on a list page filters that list.").font(theme::LABEL).color(PAL.faint).wrap().mt(4.0))
}

fn chips(s: &State, st: Status) -> El {
    let mut r = row().key("chips").gap(6.0).px(theme::PAGE_PAD).pb(10.0);
    for f in Filter::ALL {
        let n = s.entries.iter().filter(|e| e.on_page(st) && f.keeps(e)).count();
        if n == 0 && f != Filter::All {
            continue;
        }
        let on = s.filter == f;
        let mut chip = row()
            .key(format!("chip/{}", f.label()))
            .role("button")
            .label(f.label())
            .focusable(true)
            .h(26.0)
            .px(10.0)
            .gap(6.0)
            .center_y()
            .radius(13.0)
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.set_filter(f);
            })
            .child(text(format!("{} {n}", f.label())).font(theme::LABEL_NUM).color(if on { PAL.accent_hi } else { PAL.dim }));
        chip = if on { chip.fill(PAL.accent.alpha(0.14)) } else { chip.border(1.0, PAL.border) };
        r = r.child(chip);
    }
    r
}

/// Column widths shared by the header and the rows.
fn columns(cover: El, title: El, progress: El, score: El, kind: El, updated: El, wide: bool) -> El {
    let r = row()
        .w_full()
        .gap(theme::COL_GAP)
        .center_y()
        .child(cover)
        .child(title.grow(1.0).min_w(0.0))
        .child(progress.w(theme::COL_PROGRESS))
        .child(score.w(theme::COL_SCORE));
    let r = if wide { r.child(kind.w(theme::COL_TYPE)) } else { r };
    r.child(updated.w(theme::COL_UPDATED))
}

fn header(wide: bool, sort: Sort) -> El {
    let h = |s: &str, sorted: bool| text(s.to_uppercase()).font(theme::CAPTION).color(if sorted { PAL.dim } else { PAL.faint });
    col()
        .key("thead")
        .w_full()
        .child(
            columns(
                el().w(34.0),
                h("Title", sort == Sort::Title),
                h("Progress", sort == Sort::Progress),
                h("Score", sort == Sort::Score),
                h("Type", false).key("thead/type"),
                row().gap(4.0).center_y().child(h("Updated", sort == Sort::Updated)).child(icon(&icons::DOWN, 10.0).color(PAL.dim)),
                wide,
            )
            .h(30.0)
            .px(theme::PAGE_PAD),
        )
        .child(el().w_full().h(1.0).fill(PAL.border))
}

fn list_row(s: &State, e: &Entry, wide: bool) -> El {
    let now = s.clock();
    let id = e.id;
    let status_c = PAL.status(e.status);

    // The tag line fades out at the title column's end instead of running into the progress.
    let mut sub = row().w_full().min_w(0.0).gap(8.0).center_y().clip_fade(0.0, 0.0, 16.0, 0.0);
    if s.searching() {
        sub = sub.child(tag(e.status.label(), PAL.status(e.status), true));
    }
    if e.air == Air::Airing {
        sub = sub.child(tag("Airing", PAL.airing, true));
    }
    if e.rewatching {
        sub = sub.child(tag("Rewatching", PAL.st_completed, false));
    }
    if e.waiting.is_some() {
        sub = sub.child(tag("Waiting to sync", PAL.warn(), false));
    }
    if !e.note().is_empty() {
        sub = sub.child(text(e.note()).font(theme::LABEL).color(PAL.faint));
    }
    if let Some(n) = e.new_episode {
        sub = sub.child(tag_tinted(&format!("Ep {n} out"), PAL.accent_hi, PAL.tint(PAL.accent, 0.14)));
    }
    // The title opens the details page; the rest of the row opens the editor.
    let name = text(e.title.clone()).font(theme::ROW_TITLE).color(PAL.text).group_color(PAL.accent_hi).truncate();
    let title = col().gap(2.0).child(crate::details::title_link(name, crate::details::Stub::from_entry(e))).child(sub);

    let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
    // `+1` while there is a next episode; the space stays so the columns line up.
    let plus = s.has_list() && e.episodes.is_none_or(|n| e.progress < n);
    let progress = row()
        .gap(10.0)
        .center_y()
        .child(
            row()
                .grow(1.0)
                .h(4.0)
                .radius(2.0)
                .clip_rounded(2.0)
                .fill(PAL.border)
                .child(el().grow(e.fraction()).h_full().radius(2.0).fill(status_c))
                .child(el().grow(1.0 - e.fraction())),
        )
        .child(
            row()
                .w(54.0)
                .justify_end()
                .child(text(e.progress.to_string()).font(theme::NUM_MED).color(PAL.text))
                .child(text(format!(" / {total}")).font(theme::NUM).color(PAL.dim)),
        )
        .children(crate::library::row_button(s, e))
        .child(if plus { plus_button(id) } else { el().size(38.0, 26.0) });
    let score = match e.score {
        Some(n) => text(n.to_string()).font(theme::NUM_MED).color(PAL.text),
        None => text("\u{2013}").font(theme::BODY).color(PAL.faint),
    };
    let open = s.editor.filter(|ed| ed.id == id);
    let line = columns(
        row_cover(s, e),
        title,
        progress,
        score,
        text(e.kind).font(theme::LABEL).color(PAL.dim),
        text(kotora_core::time::age(e.updated_at, now)).font(theme::LABEL).color(PAL.dim),
        wide,
    )
    .key("line")
    .h(theme::ROW_H)
    .px(theme::PAGE_PAD)
    .role("button")
    .label(format!("Edit {}", e.title))
    .hover(PAL.hover())
    // A press on the row opens its editor (`+1` keeps its own press); the middle button plays
    // the next episode.
    .pointer_buttons([rune::PointerButton::Middle])
    .on_press_tx(move |s: &mut State, tx: &mut Tx| {
        tx.instant();
        if tx.button() == rune::PointerButton::Middle {
            return s.play_entry(id);
        }
        s.toggle_editor(id);
    });
    let mut c = col().w_full().child(if open.is_some() { line.fill(PAL.surface) } else { line });
    if let Some(ed) = open {
        c = c.child(editor(e, &ed));
    }
    c.child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
}

/// `+1`: optimistic and at once (design-spec §6), no counting animation.
pub(crate) fn plus_button(id: u32) -> El {
    el().key("plus")
        .role("button")
        .label("+1")
        .focusable(true)
        .size(38.0, 26.0)
        .radius(theme::R_SM)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        // On orange the label turns dark (white or dim on orange fails contrast).
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Err(m) = s.plus_one_or_ask(id) {
                s.error = Some(m);
            }
        })
        .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent))
}

// -------------------------------------------------------------------------------- inline editor

/// A labelled group in the editor.
pub(crate) fn field(label: &str, body: El) -> El {
    col().gap(6.0).child(text(label.to_uppercase()).font(theme::CAPTION).color(PAL.faint)).child(body)
}

/// A small pressable cell (status chips, score cells, stepper buttons).
pub(crate) fn cell(key: String, label: String, on: bool, f: impl Fn(&mut State) + 'static) -> El {
    let c = row()
        .key(key)
        .role("button")
        .label(label.clone())
        .focusable(true)
        .h(28.0)
        .min_w(28.0)
        .px(8.0)
        .gap(6.0)
        .center()
        .radius(theme::R_SM)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s);
        });
    if on { c.fill(PAL.accent.alpha(0.14)).border(1.0, PAL.accent.alpha(0.5)) } else { c.fill(PAL.field).border(1.0, PAL.border) }
}

/// Under an open row: status, progress and score, then Cancel / Save. Nothing is queued until
/// Save, and then as one change.
pub(crate) fn editor(e: &Entry, ed: &Editor) -> El {
    let mut statuses = row().flex_wrap().gap(6.0);
    for st in Status::ALL {
        let on = ed.status == st;
        statuses = statuses.child(
            cell(format!("ed/status/{}", st.slug()), st.label().into(), on, move |s| s.editor_status(st))
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                .child(text(st.label()).font(theme::LABEL).color(if on { PAL.text } else { PAL.dim })),
        );
    }
    statuses = statuses.child(rewatch_cell(e));
    let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
    let stepper = row()
        .key("ed/progress")
        .h(32.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(step_button("ed/minus", "\u{2212}", -1))
        .child(
            row()
                .min_w(76.0)
                .center()
                .child(text(ed.progress.to_string()).font(theme::NUM_MED).color(PAL.text))
                .child(text(format!(" / {total}")).font(theme::NUM).color(PAL.dim)),
        )
        .child(step_button("ed/plus", "+", 1));
    let mut scores = row().gap(4.0).child(
        cell("ed/score/none".into(), "No score".into(), ed.score.is_none(), |s| s.editor_score(None))
            .child(text("\u{2013}").font(theme::LABEL).color(PAL.dim)),
    );
    for n in 1..=10u8 {
        let on = ed.score == Some(n);
        scores = scores.child(
            cell(format!("ed/score/{n}"), format!("Score {n}"), on, move |s| s.editor_score(Some(n)))
                .px(0.0)
                .child(text(n.to_string()).font(theme::LABEL_NUM).color(if on { PAL.accent_hi } else { PAL.dim })),
        );
    }
    col()
        .key("editor")
        .mx(theme::PAGE_PAD)
        .mb(10.0)
        .py(12.0)
        .pl(62.0)
        .pr(14.0)
        .gap(14.0)
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border_strong)
        .child(field("Status", statuses))
        .child(
            // In a narrow window the buttons wrap to a line of their own, on the right.
            row()
                .flex_wrap()
                .gap(24.0)
                .items_end()
                .child(field("Progress", stepper))
                .child(field("Score", scores))
                .child(
                    row()
                        .grow(1.0)
                        .gap(8.0)
                        .justify_end()
                        .child(button("ed/cancel", "Cancel", false, Some(|s: &mut State| s.editor = None)))
                        .child(button(
                            "ed/save",
                            "Save",
                            true,
                            Some(|s: &mut State| {
                                if let Err(m) = s.save_editor() {
                                    s.error = Some(m);
                                }
                            }),
                        )),
                ),
        )
}

/// After the statuses: "Rewatch" for a completed entry, "Stop rewatching" during one. Acts at
/// once (no Save), like `+1`.
pub(crate) fn rewatch_cell(e: &Entry) -> El {
    let id = e.id;
    if e.rewatching {
        return cell("ed/stop-rewatch".into(), "Stop rewatching".into(), false, move |s| {
            if let Err(m) = s.stop_rewatch(id) {
                s.error = Some(m);
            }
        })
        .ml(10.0)
        .child(text("Stop rewatching").font(theme::LABEL).color(PAL.dim));
    }
    if e.status != Status::Completed {
        return el();
    }
    cell("ed/rewatch".into(), "Rewatch from episode 1".into(), false, move |s| {
        if let Err(m) = s.start_rewatch(id) {
            s.error = Some(m);
        }
    })
    .ml(10.0)
    .child(icon(&icons::SYNC, 13.0).color(PAL.st_completed))
    .child(text("Rewatch").font(theme::LABEL).color(PAL.text))
}

pub(crate) fn step_button(key: &str, label: &str, delta: i32) -> El {
    el().key(key.to_string())
        .role("button")
        .label(if delta < 0 { "One less" } else { "One more" })
        .focusable(true)
        .size(30.0, 30.0)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.editor_step(delta);
        })
        .child(text(label.to_string()).font(TextStyle::new(15.0, 500.0)).color(PAL.dim))
}

pub(crate) fn tag(label: &str, c: Color, dot: bool) -> El {
    let mut t = row().h(18.0).px(6.0).gap(4.0).center_y().radius(theme::R_XS).fill(PAL.tint(c, 0.12));
    if dot {
        t = t.child(el().size(5.0, 5.0).radius(2.5).fill(c));
    }
    t.child(text(label.to_string()).font(theme::CAPTION.tracking(0.0)).color(c))
}

pub(crate) fn tag_tinted(label: &str, fg: Color, bg: Color) -> El {
    row().h(18.0).px(6.0).center_y().radius(theme::R_XS).fill(bg).child(text(label.to_string()).font(theme::CAPTION.tracking(0.0)).color(fg))
}

/// Cover pairs from the mockup; an id picks one (design-spec §3: covers while loading or missing).
pub(crate) const TILES: [(&str, &str); 10] = [
    ("#e0794a", "#5b2a6e"),
    ("#3b7dd8", "#1b2b5c"),
    ("#d9b44a", "#7a3b1e"),
    ("#41b39a", "#173f47"),
    ("#e04a6a", "#2a1340"),
    ("#8a7ae0", "#2a2160"),
    ("#c96b3c", "#3b1f1a"),
    ("#5fa0c8", "#1d3346"),
    ("#d27ab0", "#40204a"),
    ("#7fae5a", "#22381f"),
];

pub(crate) fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}

/// The generated tile: a 160° gradient and the title's initials.
pub(crate) fn cover(id: u32, title: &str, w: f32, h: f32, size: f32) -> El {
    let (a, b) = TILES[id as usize % TILES.len()];
    el().size(w, h)
        .radius(theme::R_XS)
        .linear_gradient(160.0 * PI / 180.0, [(0.0, hex(a)), (1.0, hex(b))])
        .border(1.0, Color::WHITE.alpha(0.06))
        .center()
        .child(text(initials(title)).font(rune::TextStyle::new(size, 600.0).tracking(0.3)).color(Color::WHITE.alpha(0.88)))
}

/// A row's cover: the generated tile, with the real cover over it once it is in memory (a
/// cover still in Rune's decoder paints nothing, so the tile shows until then).
pub(crate) fn row_cover(s: &State, e: &Entry) -> El {
    let (w, h) = (34.0, 48.0);
    let tile = cover(e.id, &e.title, w, h, 12.0);
    match s.covers.get(e.id, e.cover_url.as_deref(), w, h) {
        Some(img) => tile.child(el().key("cover-img").absolute(0.0, 0.0).size(w, h).radius(theme::R_XS).pixels(img.pixels())),
        None => tile,
    }
}

/// The cover address of a queued change: its entry's, if the entry is on the list. A change
/// for something not on the list (an add still waiting) has none, so it keeps the tile.
pub(crate) fn op_cover_url(s: &State, media_id: u32) -> Option<&str> {
    s.entries.iter().find(|e| e.id == media_id).and_then(|e| e.cover_url.as_deref())
}

/// A queued change's poster: the tile, with the real cover over it once it is in memory.
fn op_cover(s: &State, op: &Op, title: &str) -> El {
    let (w, h) = (34.0, 48.0);
    let tile = cover(op.media_id, title, w, h, 12.0);
    match s.covers.get(op.media_id, op_cover_url(s, op.media_id), w, h) {
        Some(img) => tile.child(el().key("cover-img").absolute(0.0, 0.0).size(w, h).radius(theme::R_XS).pixels(img.pixels())),
        None => tile,
    }
}

/// Small words the initials skip (English and romaji particles).
const SMALL_WORDS: [&str; 10] = ["the", "of", "to", "no", "on", "a", "an", "and", "in", "wa"];

/// Two letters: the first letters of the first two words that aren't small words.
pub fn initials(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().filter(|w| !SMALL_WORDS.iter().any(|s| w.eq_ignore_ascii_case(s))).collect();
    let words = if words.is_empty() { title.split_whitespace().collect() } else { words };
    words.iter().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).flat_map(char::to_uppercase).collect()
}

// ------------------------------------------------------------------------------------- states

fn empty(st: Status) -> El {
    let help = match st {
        Status::Watching => "Anime you're watching on MyAnimeList show up here.",
        Status::PlanToWatch => "Anime you plan to watch on MyAnimeList show up here.",
        Status::Completed => "Anime you finish on MyAnimeList show up here.",
        Status::OnHold => "Anime you put on hold on MyAnimeList show up here.",
        Status::Dropped => "Entries you drop on MyAnimeList show up here.",
    };
    col()
        .key("empty")
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(4.0)
        .pb(60.0)
        .child(text(format!("No anime in {}", st.label())).font(theme::SECTION).color(PAL.text))
        .child(text(help).font(theme::LABEL).color(PAL.dim))
}

/// The first sync with nothing saved: four skeleton rows. Static for now (the shimmer would be the
/// one looping animation; it waits until loading is real, so nothing ticks at rest).
fn loading() -> El {
    let mut c = col().key("loading").px(theme::PAGE_PAD).pt(4.0).gap(8.0).child(text("Loading your list…").font(theme::LABEL).color(PAL.faint).pb(2.0));
    for i in 0..4 {
        c = c.child(el().key(format!("skel/{i}")).w_full().h(44.0).radius(theme::R_MD).fill(PAL.raised));
    }
    c
}

// ------------------------------------------------------------------------------------ queue

/// A button whose press needs a value (an op id): `button` takes plain fns only.
pub(crate) fn action(key: String, label: &str, primary: bool, f: impl Fn(&mut State, &mut Tx) + 'static) -> El {
    let (fg, fill) = if primary { (PAL.on_accent, PAL.accent) } else { (PAL.text, PAL.raised) };
    let b = el()
        .key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(26.0)
        .px(10.0)
        .radius(theme::R_SM)
        .fill(fill)
        .center()
        .hover(if primary { Color::BLACK.alpha(0.08) } else { PAL.hover() })
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s, tx);
        })
        .child(text(label.to_string()).font(theme::LABEL_BTN).color(fg));
    if primary { b } else { b.border(1.0, PAL.border_strong) }
}

/// The sync queue (design-spec §4): sign-in or offline alert, conflicts as cards, then Waiting,
/// Failed and Sent today.
fn queue_page(s: &State) -> El {
    let now = s.clock();
    let can_sync = s.auth == Auth::SignedIn && !s.syncing;
    let top = top(s, "Sync queue", Some(s.queue as usize), false)
        .child(spacer())
        .child(button("queue/sync-now", "Sync now", false, can_sync.then_some(State::sync_now as fn(&mut State))));
    let mut body = col().key("queue-body").gap(14.0).pb(theme::PAGE_PAD);
    if s.auth == Auth::Expired {
        let n = s.queue;
        body = body.child(
            alert(
                "queue/expired",
                PAL.warn(),
                format!(
                    "Sign in again. Your MyAnimeList sign-in expired. {} wait{} in the queue; nothing is lost.",
                    changes(n),
                    if n == 1 { "s" } else { "" }
                ),
                Some(button("queue/sign-in", "Sign in", true, Some(online::sign_in))),
            )
            .mb(0.0),
        );
    } else if s.offline && s.queue > 0 {
        body = body.child(
            alert(
                "queue/offline",
                PAL.warn(),
                "You are offline. Your changes are kept here and go out when MyAnimeList answers again.".into(),
                Some(button("queue/try-now", "Try now", false, Some(State::sync_now))),
            )
            .mb(0.0),
        );
    }
    if let Some(e) = &s.error
        && !(s.offline && e.starts_with("Can't reach"))
    {
        body = body.child(alert("queue/error", PAL.status(Status::Dropped), e.clone(), None).mb(0.0));
    }
    if s.ops.is_empty() {
        return col().key("page/queue").w_full().h_full().child(top).child(body).child(
            col()
                .key("queue-empty")
                .grow(1.0)
                .min_h(0.0)
                .center()
                .gap(4.0)
                .pb(60.0)
                .child(text("Nothing waiting").font(theme::SECTION).color(PAL.text))
                .child(text("Your changes go here first, then to MyAnimeList.").font(theme::LABEL).color(PAL.dim)),
        );
    }
    for op in s.ops.iter().filter(|o| o.state == OpState::Conflict) {
        body = body.child(conflict_card(s, op, s.op_title(op), now).mx(theme::PAGE_PAD));
    }
    type Group = (&'static str, &'static str, fn(OpState) -> bool);
    let groups: [Group; 3] = [
        ("waiting", "Waiting", |st| matches!(st, OpState::Queued | OpState::Sending)),
        ("failed", "Failed", |st| st == OpState::Failed),
        ("sent", "Sent today", |st| st == OpState::Done),
    ];
    let mut list = col().key("queue-list").px(16.0).py(6.0).radius(theme::R_LG).fill(PAL.surface).border(1.0, PAL.border).mx(theme::PAGE_PAD);
    let mut any = false;
    for (key, label, keep) in groups {
        let ops: Vec<&Op> = s.ops.iter().filter(|o| keep(o.state)).collect();
        if ops.is_empty() {
            continue;
        }
        any = true;
        let count = if key == "sent" { String::new() } else { format!(" {}", ops.len()) };
        list = list.child(text(format!("{}{count}", label.to_uppercase())).key(format!("qh/{key}")).font(theme::CAPTION).color(PAL.faint).pt(12.0).pb(4.0));
        for (i, op) in ops.iter().enumerate() {
            if i > 0 {
                list = list.child(el().w_full().h(1.0).fill(PAL.border));
            }
            list = list.child(queue_row(s, op, s.op_title(op), now));
        }
    }
    if any {
        body = body.child(list);
    }
    col()
        .key("page/queue")
        .w_full()
        .h_full()
        .child(top)
        .child(col().key("queue-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(body))
}

fn changes(n: u32) -> String {
    if n == 1 { "1 change".into() } else { format!("{n} changes") }
}

/// One change: cover, title, what changes (and the service's words when it failed), its state
/// or its actions, and when it was made or sent.
fn queue_row(s: &State, op: &Op, title: &str, now: i64) -> El {
    let id = op.id;
    let media = op.media_id;
    let mut line = row().gap(6.0).center_y().child(text(kotora_core::queue::describe_op(op)).font(theme::LABEL_NUM).color(PAL.text));
    if op.edits > 1 && op.state.is_open() {
        line = line.child(text(format!("({} edits merged)", op.edits)).font(theme::LABEL).color(PAL.faint));
    }
    // The service's words under the change: red when it refused, grey while it retries.
    let mut what = col().gap(2.0).grow(1.0).min_w(0.0).child(line);
    match (&op.error, op.state) {
        (Some(e), OpState::Failed) => what = what.child(text(e.clone()).font(theme::LABEL).color(PAL.status(Status::Dropped)).truncate()),
        (Some(e), OpState::Queued) => what = what.child(text(format!("{e}; Kotora tries again by itself")).font(theme::LABEL).color(PAL.faint).truncate()),
        _ => {}
    }
    let end = match op.state {
        OpState::Failed => row()
            .gap(6.0)
            .child(action(format!("q/{id}/edit"), "Edit", false, move |s, tx| {
                // The entry's page, its editor open, scrolled to it.
                if let Some(st) = s.entries.iter().find(|e| e.id == media).map(|e| e.status) {
                    s.query.replace_text("");
                    s.go(Page::List(st));
                    s.set_filter(Filter::All);
                    s.editor = None;
                    s.toggle_editor(media);
                    tx.scroll_into_view(media.to_string());
                }
            }))
            .child(action(format!("q/{id}/discard"), "Discard", false, move |s, _| s.queue_cmd(Cmd::Discard(id)))),
        OpState::Done => row().gap(4.0).center_y().child(icon(&icons::CHECK, 13.0).color(PAL.ok())).child(text("Sent").font(theme::CAPTION.tracking(0.0)).color(PAL.ok())),
        st => pill(st.label(), if st == OpState::Sending { PAL.dim } else { PAL.warn() }),
    };
    let when = op.sent_at.unwrap_or(op.edited_at);
    let r = row()
        .key(format!("q/{id}"))
        .h(58.0)
        .gap(14.0)
        .center_y()
        .child(op_cover(s, op, title))
        .child(text(title.to_string()).font(theme::BODY_MED).color(PAL.text).truncate().w(240.0))
        .child(what)
        .child(end)
        .child(text(kotora_core::time::age(when, now)).font(theme::LABEL_NUM).color(PAL.faint).w(52.0).justify_end());
    if op.state == OpState::Done { r.opacity(0.6) } else { r }
}

pub(crate) fn pill(label: &str, c: Color) -> El {
    row().h(20.0).px(8.0).center_y().radius(10.0).fill(PAL.tint(c, 0.10)).child(text(label.to_string()).font(theme::CAPTION.tracking(0.0)).color(c))
}

/// The fields of `op`'s change, as `values` has them: "Progress 7 / 24 · Score 8".
fn conflict_values(op: &Op, v: &kotora_core::Values) -> String {
    let total = op.episodes.map_or("?".to_string(), |n| n.to_string());
    let mut parts = Vec::new();
    if op.change.status.is_some() {
        parts.push(v.status.label().to_string());
    }
    if op.change.progress.is_some() {
        parts.push(format!("Progress {} / {total}", v.progress));
    }
    if op.change.score.is_some() {
        parts.push(v.score.map_or("No score".to_string(), |n| format!("Score {n}")));
    }
    parts.join(" \u{b7} ")
}

/// A conflict: the user's change and the service's entry side by side, Keep mine / Take theirs.
fn conflict_card(s: &State, op: &Op, title: &str, now: i64) -> El {
    let id = op.id;
    let side = |key: &str, head: String, big: String, act: El, on: bool| {
        col()
            .key(key.to_string())
            .grow(1.0)
            .min_w(0.0)
            .gap(6.0)
            .padding(12.0)
            .radius(theme::R_MD)
            .fill(PAL.field)
            .border(1.0, if on { PAL.accent.alpha(0.5) } else { PAL.border })
            .child(text(head).font(theme::LABEL).color(PAL.faint))
            .child(text(big).font(TextStyle::new(16.0, 500.0).features("'tnum' 1")).color(PAL.text).pb(4.0))
            .child(row().child(act))
    };
    let (theirs_head, theirs, take) = match &op.remote {
        Some(r) => (format!("MyAnimeList \u{b7} changed {}", sync_age(now - r.updated_at)), conflict_values(op, &r.values), "Take theirs"),
        None => ("MyAnimeList".to_string(), "Removed from your list".to_string(), "Remove here too"),
    };
    let aside = match &op.remote {
        Some(r) => format!("MyAnimeList has {}", remote_text(r, op.episodes)),
        None => "removed on another device".into(),
    };
    col()
        .key(format!("conflict/{id}"))
        .padding(16.0)
        .gap(10.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.accent.alpha(0.45))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(op_cover(s, op, title))
                .child(icon(&icons::ALERT, 16.0).color(PAL.accent))
                .child(text(format!("Conflict: {title}")).font(theme::SECTION).color(PAL.text).truncate().min_w(0.0))
                .child(text(aside).font(theme::LABEL).color(PAL.faint).truncate().min_w(0.0)),
        )
        .child(text("MyAnimeList changed after your edit. Kotora did not overwrite it. Pick one.").font(theme::LABEL).color(PAL.dim))
        .child(
            row()
                .gap(12.0)
                .child(side(
                    "mine",
                    format!("Yours \u{b7} edited here, {}", sync_age(now - op.edited_at)),
                    conflict_values(op, &op.mine()),
                    action(format!("conflict/{id}/keep"), "Keep mine", true, move |s, _| s.queue_cmd(Cmd::KeepMine(id))),
                    true,
                ))
                .child(side("theirs", theirs_head, theirs, action(format!("conflict/{id}/take"), take, false, move |s, _| s.queue_cmd(Cmd::TakeTheirs(id))), false)),
        )
        .child(text("When only the progress differs and yours is higher, Kotora sends yours by itself.").font(theme::LABEL).color(PAL.faint))
}

// ------------------------------------------------------------------- alerts, sign-in, settings

pub(crate) fn button(key: &str, label: &str, primary: bool, on: Option<fn(&mut State)>) -> El {
    let (fg, fill) = match (primary, on.is_some()) {
        (true, true) => (PAL.on_accent, PAL.accent),
        (_, false) => (PAL.faint, PAL.field),
        (false, true) => (PAL.text, PAL.raised),
    };
    let b = el()
        .key(key.to_string())
        .role("button")
        .label(label.to_string())
        .h(32.0)
        .px(14.0)
        .radius(theme::R_SM)
        .fill(fill)
        .center()
        .child(text(label.to_string()).font(theme::LABEL_BTN).color(fg));
    let b = if primary || on.is_none() { b } else { b.border(1.0, PAL.border_strong) };
    match on {
        Some(f) => b
            .focusable(true)
            .hover(if primary { Color::BLACK.alpha(0.08) } else { PAL.hover() })
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                f(s);
            }),
        None => b,
    }
}

/// A tinted bar with a message and an optional button (design-spec §7).
pub(crate) fn alert(key: &str, c: Color, message: String, action: Option<El>) -> El {
    let mut r = row()
        .key(key.to_string())
        .gap(12.0)
        .center_y()
        .px(14.0)
        .py(10.0)
        .mx(theme::PAGE_PAD)
        .mb(12.0)
        .radius(theme::R_MD)
        .fill(c.alpha(0.08))
        .border(1.0, c.alpha(0.3))
        .child(el().size(6.0, 6.0).radius(3.0).fill(c))
        .child(text(message).font(theme::BODY).color(PAL.text).wrap().grow(1.0).min_w(0.0));
    if let Some(a) = action {
        r = r.child(a);
    }
    r
}

/// The bar above the list: sign in again, or the last sync's error.
fn status_alert(s: &State) -> Option<El> {
    if s.auth == Auth::Expired {
        return Some(alert(
            "alert/expired",
            PAL.warn(),
            "MyAnimeList needs you to sign in again. Your saved list still works.".into(),
            Some(button("alert/sign-in", "Sign in", true, Some(online::sign_in))),
        ));
    }
    let err = s.error.as_ref()?;
    let mut msg = err.clone();
    if s.offline && let Some(at) = s.synced_at {
        msg = format!("{msg} Last synced {}.", sync_age(s.clock() - at));
    }
    Some(alert("alert/error", PAL.status(Status::Dropped), msg, Some(button("alert/retry", "Retry", false, Some(online::refresh)))))
}

/// A status page that has entries, but none match the chip or the search.
fn no_match(s: &State) -> El {
    let q = s.query.text.trim();
    let line = if q.is_empty() { "No anime match this filter.".to_string() } else { format!("Nothing matches \u{201c}{q}\u{201d}.") };
    col().key("no-match").grow(1.0).min_h(0.0).center().gap(4.0).pb(60.0).child(text(line).font(theme::SECTION).color(PAL.text))
}

/// The list pages before there is a list: no Client ID, signed out, or waiting for the browser.
fn welcome(s: &State) -> El {
    let mut c = col().key("welcome").w_full().h_full().center().gap(10.0).pb(40.0);
    if let Some(img) = &s.logo {
        c = c.child(el().size(64.0, 64.0).radius(14.0).pixels(img.pixels()));
    }
    c = c.child(text("Welcome to Kotora").font(TextStyle::new(28.0, 600.0)).color(PAL.text));
    match s.auth {
        Auth::SigningIn => {
            c = c
                .child(text("Waiting for MyAnimeList in your browser\u{2026}").font(theme::BODY).color(PAL.dim))
                .child(row().mt(6.0).child(button("cancel", "Cancel", false, Some(online::cancel_sign_in))));
        }
        _ => {
            if s.client_id.saved {
                c = c.child(
                    row()
                        .key("cid-saved")
                        .gap(6.0)
                        .center_y()
                        .child(icon(&icons::CHECK, 14.0).color(PAL.ok()))
                        .child(text("Client ID saved.").font(theme::LABEL).color(PAL.ok())),
                );
            }
            c = c
                .child(text("Sign in to MyAnimeList to see and update your list.").font(theme::BODY).color(PAL.dim))
                .child(row().mt(6.0).child(button("sign-in", "Sign in", true, Some(online::sign_in))));
        }
    }
    if let Some(e) = &s.error {
        c = c.child(text(e.clone()).font(theme::LABEL).color(PAL.status(Status::Dropped)).mt(8.0));
    }
    c
}

/// Settings (design-spec §4): Accounts, Updates, Players and About. Two columns when wide, one
/// below `theme::NARROW_W`.
fn settings(s: &State, wide: bool) -> El {
    let cards = if wide {
        col()
            .gap(16.0)
            .w_full()
            .child(
                row()
                    .gap(16.0)
                    .w_full()
                    .items_start()
                    .child(col().gap(16.0).grow(1.0).min_w(0.0).child(accounts_card(s)).child(players_card(s)).child(crate::library::settings_card(s)))
                    .child(col().gap(16.0).grow(1.0).min_w(0.0).child(updates_card(s)).child(notifications_card(s)).child(display_card(s)).child(startup_card(s)).child(about_card(s))),
            )
    } else {
        col().gap(16.0).w_full().child(accounts_card(s)).child(updates_card(s)).child(notifications_card(s)).child(display_card(s)).child(startup_card(s)).child(players_card(s)).child(crate::library::settings_card(s)).child(about_card(s))
    };
    col()
        .key("page/Settings")
        .w_full()
        .h_full()
        .child(top(s, "Settings", None, false))
        .child(
            col()
                .key("settings-scroll")
                .scroll(ScrollSpec { stick_bottom: false })
                .grow(1.0)
                .min_h(0.0)
                .child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).max_w(1120.0).child(cards)),
        )
}

pub(crate) fn card(key: &str, title: &str) -> El {
    col()
        .key(key.to_string())
        .padding(16.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(text(title.to_string()).font(theme::SECTION).color(PAL.text).pb(4.0))
}

/// A service in the Accounts card: badge, name and state, and its button.
fn service_row(key: &str, badge: &str, badge_fill: Color, name: &str, line: String, action: El, on: bool) -> El {
    let fg = if on { PAL.text } else { PAL.dim };
    row()
        .key(key.to_string())
        .gap(12.0)
        .py(10.0)
        .center_y()
        .child(
            el().size(32.0, 32.0)
                .radius(theme::R_MD)
                .fill(badge_fill)
                .center()
                // Brand fills are dark in both themes: the letters stay light.
                .child(text(badge.to_string()).font(TextStyle::new(11.0, 600.0)).color(hex(if on { "#edf0f7" } else { "#a4afc6" }))),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(name.to_string()).font(theme::BODY_MED).color(fg).truncate())
                .child(text(line).font(theme::LABEL).color(if on { PAL.dim } else { PAL.faint }).truncate()),
        )
        .child(action)
}

fn accounts_card(s: &State) -> El {
    let (line, action) = match (&s.account, s.auth) {
        (Some((name, _)), _) => (format!("Signed in as {name}"), small_button("sign-out", "Sign out", false, Some(online::sign_out))),
        (None, Auth::NoClientId) => ("Needs a Client ID first (below)".to_string(), small_button("sign-in", "Sign in", true, None)),
        (None, Auth::SigningIn) => ("Waiting for your browser\u{2026}".to_string(), small_button("cancel", "Cancel", false, Some(online::cancel_sign_in))),
        (None, _) => ("Not signed in".to_string(), small_button("sign-in", "Sign in", true, Some(online::sign_in))),
    };
    let note = row()
        .key("token-note")
        .gap(10.0)
        .mt(10.0)
        .px(12.0)
        .py(10.0)
        .items_start()
        .radius(theme::R_MD)
        .fill(PAL.st_completed.alpha(0.08))
        .border(1.0, PAL.st_completed.alpha(0.28))
        .child(icon(&icons::LOCK, 16.0).color(PAL.st_completed))
        .child(
            text("Your sign-in token is kept in a file only you can read. It is not encrypted. Signing out deletes it, your saved list and changes not yet sent.")
            .font(theme::LABEL)
            .color(PAL.text)
            .wrap()
            .grow(1.0)
            .min_w(0.0),
        );
    card("account-card", "Accounts")
        .child(service_row("acct/mal", "MAL", hex("#2e51a2"), "MyAnimeList", line, action, true))
        .child(el().w_full().h(1.0).fill(PAL.border))
        .child(crate::clientid::settings_row(s))
        .child(el().w_full().h(1.0).fill(PAL.border))
        .child(service_row("acct/anilist", "AL", hex("#1d3550"), "AniList", "Coming in v0.3".into(), small_button("anilist", "Sign in", false, None), false))
        .child(note)
}

fn about_card(s: &State) -> El {
    let logo = el().size(56.0, 56.0).radius(theme::R_LG);
    let logo = match &s.logo_big {
        Some(img) => logo.pixels(img.pixels()),
        None => logo.fill(PAL.raised),
    };
    let link = |key: &str, label: &str, url: &'static str| {
        text(label.to_string())
            .key(key.to_string())
            .role("link")
            .label(label.to_string())
            .focusable(true)
            .font(theme::LABEL)
            .color(PAL.accent_hi)
            .cursor(rune::Cursor::Pointer)
            .on_press(move |s: &mut State| online::open_link(s, url))
    };
    card("about-card", "About Kotora").child(
        row()
            .gap(16.0)
            .pt(8.0)
            .items_start()
            .child(logo)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(8.0)
                    .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(theme::LABEL_NUM).color(PAL.faint))
                    .child(
                        text("A tribute to Taiga, the Windows anime tracker. Thank you to its author and contributors. Kotora is a separate project.")
                            .font(theme::LABEL)
                            .color(PAL.dim)
                            .wrap(),
                    )
                    .child(
                        text("GPL-3.0-or-later. File names are read with anitomy (MPL-2.0, through anitomy-ng); split seasons are mapped with anime-relations (CC0), both by Taiga's author. Not affiliated with MyAnimeList.")
                            .font(theme::LABEL)
                            .color(PAL.faint)
                            .wrap(),
                    )
                    .child(
                        row()
                            .gap(14.0)
                            .child(link("link/site", "kotora.moe", "https://kotora.moe"))
                            .child(link("link/source", "Source code", "https://github.com/hanami117/kotora")),
                    ),
            ),
    )
    .child(crate::update::settings_rows(s))
}

/// The `sm` button (26 high) of the settings rows.
pub(crate) fn small_button(key: &str, label: &str, primary: bool, on: Option<fn(&mut State)>) -> El {
    button(key, label, primary, on).h(26.0).px(10.0)
}

// ------------------------------------------------------------------------------ now playing

/// The id (for the tile) and the title to show for what plays: the list entry, the search hit,
/// the parsed title, or the file name.
pub(crate) fn now_title(s: &State, n: &Now) -> (u32, String) {
    if let Some(id) = n.media_id()
        && let Some(e) = s.entries.iter().find(|e| e.id == id)
    {
        return (id, e.title.clone());
    }
    match &n.found {
        Found::NotOnList { media_id, .. } => (*media_id, s.not_on_list_title(n).unwrap_or_default()),
        _ => {
            let t = n.parsed.as_ref().map(|p| p.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| n.file.clone());
            (t.bytes().map(u32::from).sum(), t)
        }
    }
}

/// The cover URL of what plays: the list entry's, else the one found with a match that isn't
/// on the list, else an open details page's for the same anime.
pub(crate) fn now_cover_url<'a>(s: &'a State, n: &'a Now) -> Option<&'a str> {
    let id = match &n.found {
        Found::List { media_id, .. } | Found::NotOnList { media_id, .. } => *media_id,
        _ => return None,
    };
    s.entries
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.cover_url.as_deref())
        .or(n.cover_url.as_deref())
        .or_else(|| s.detail.as_ref().filter(|d| d.id() == id).and_then(|d| d.info.as_ref()).and_then(|i| i.cover_url.as_deref()))
}

/// A cover for what plays: the real one when loaded (at its own pixel size), else the tile.
fn now_cover(s: &State, n: &Now, w: f32, h: f32, size: f32) -> El {
    let (id, title) = now_title(s, n);
    let r = if w > 60.0 { theme::R_LG } else { theme::R_XS };
    let tile = cover(id, &title, w, h, size).radius(r);
    let url = now_cover_url(s, n);
    match s.covers.get(id, url, w, h) {
        Some(img) => tile.child(el().key("np-cover-img").absolute(0.0, 0.0).size(w, h).radius(r).pixels(img.pixels())),
        None => tile,
    }
}

/// Mixes `a` toward `b` by `t` (linear colours).
pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
}

/// A ghost button (no fill, dim text): "Wrong anime…".
pub(crate) fn ghost(key: &str, label: &str, f: impl Fn(&mut State) + 'static) -> El {
    el().key(key.to_string())
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(32.0)
        .px(10.0)
        .radius(theme::R_SM)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            f(s);
        })
        .child(text(label.to_string()).font(theme::LABEL_BTN).color(PAL.dim))
}

/// The banner on top of every page: the offer (Update / Not now / Wrong anime…), or an episode
/// recorded from here (Undo).
fn banner(s: &State) -> El {
    let Some(n) = &s.now else { return el() };
    let (_, title) = now_title(s, n);
    let pct = n.fraction().map(|f| format!("{:.0}% watched", f * 100.0));
    let mut b = row()
        .key("banner")
        .mx(theme::PAGE_PAD)
        .mt(14.0)
        .px(14.0)
        .py(12.0)
        .gap(14.0)
        .center_y()
        .radius(theme::R_LG)
        .linear_gradient(PI / 2.0, [(0.0, mix(PAL.bg, PAL.accent, 0.18)), (0.6, mix(PAL.bg, PAL.accent, 0.06)), (1.0, mix(PAL.bg, PAL.accent, 0.03))])
        .border(1.0, PAL.accent.alpha(0.35))
        .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.accent))
        .child(now_cover(s, n, 36.0, 52.0, 12.0));
    // In a narrow window the line fades out at its end instead of running under the buttons.
    let line = |lead: &str, bold: String, tail: String| {
        row()
            .w_full()
            .min_w(0.0)
            .clip_fade(0.0, 0.0, 16.0, 0.0)
            .child(text(lead.to_string()).font(theme::BODY).color(PAL.text))
            .child(text(bold).font(TextStyle::new(13.0, 600.0)).color(PAL.text).truncate().min_w(0.0))
            .child(text(tail).font(theme::BODY).color(PAL.text))
    };
    if let Some(r) = n.recorded {
        let finished = r.old_rewatching && s.entries.iter().any(|e| e.id == r.id && !e.rewatching);
        let how = match (r.auto, r.completed) {
            _ if r.rewatch => "Rewatch started",
            (true, _) if finished => "Updated automatically \u{b7} Rewatch finished",
            (false, _) if finished => "Updated \u{b7} Rewatch finished",
            (true, true) => "Updated automatically \u{b7} Completed",
            (false, true) => "Updated \u{b7} Completed",
            (true, false) => "Updated automatically",
            (false, false) => "Updated",
        };
        let waiting = s.entries.iter().find(|e| e.id == r.id).and_then(|e| e.waiting).unwrap_or("Sent to MyAnimeList");
        b = b
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(line("Recorded ", title, format!(" episode {}", r.episode)))
                    .child(text(format!("{how} \u{b7} {waiting}")).font(theme::LABEL).color(PAL.dim)),
            )
            .child(button("banner/undo", "Undo", false, Some(State::undo_record)))
            .child(ghost("banner/close", "Close", |s| {
                s.dismiss_now();
            }));
        return b;
    }
    if s.offer().is_none()
        && let Some((i, ep)) = s.rewatch_offer()
    {
        let e = &s.entries[i];
        let times = match e.times_rewatched {
            0 => "Completed".to_string(),
            n => format!("Completed, rewatched {}", kotora_core::queue::times(n)),
        };
        let small = [pct.as_deref(), Some(times.as_str())].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ");
        return b
            .key("banner")
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(line("Watching ", title, format!(" episode {ep} in {}", n.player)))
                    .child(text(small).font(theme::LABEL).color(PAL.dim).truncate()),
            )
            .child(text("Watching it again?").key("banner/rewatch-ask").font(TextStyle::new(14.0, 600.0)).color(PAL.cream))
            .child(action("banner/rewatch".into(), &format!("Start rewatch at episode {ep}"), true, |s, _| {
                let _ = s.confirm_offer();
            }).h(32.0).px(12.0))
            .child(button("banner/not-now", "Not now", false, Some(|s: &mut State| {
                s.dismiss_now();
            })))
            .child(ghost("banner/wrong", "Wrong anime\u{2026}", State::wrong_anime));
    }
    let ep = s.offer().map_or(0, |(_, ep)| ep);
    let total = s.offer().and_then(|(i, _)| s.entries[i].episodes).map_or("?".to_string(), |n| n.to_string());
    let matched = if n.sure() { "Matched in your list" } else { "Not sure it's this one" };
    // Skips an episode, or not aired yet by the estimate: said here, and never recorded by itself.
    // For a sure match the note takes the place of "Matched in your list".
    let note = s.offer_note();
    let matched = if note.is_some() && n.sure() { None } else { Some(matched) };
    let small = [pct.as_deref(), matched].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ");
    let small = match note {
        Some(note) => row()
            .min_w(0.0)
            .gap(6.0)
            .child(text(small).font(theme::LABEL).color(PAL.dim).truncate().min_w(0.0))
            .child(text("\u{b7}").font(theme::LABEL).color(PAL.dim))
            .child(text(note).key("banner/note").font(theme::LABEL).color(PAL.warn()).truncate().min_w(0.0)),
        None => row().min_w(0.0).child(text(small).font(theme::LABEL).color(PAL.dim).truncate()),
    };
    b.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(3.0)
            .child(line("Watching ", title, format!(" episode {ep} in {}", n.player)))
            .child(small),
    )
    .child(text(format!("Update to {ep} / {total}?")).font(TextStyle::new(14.0, 600.0).features("'tnum' 1")).color(PAL.cream))
    .child(button("banner/update", "Update", true, Some(|s: &mut State| {
        let _ = s.confirm_offer();
    })))
    .child(button("banner/not-now", "Not now", false, Some(|s: &mut State| {
        s.dismiss_now();
    })))
    .child(ghost("banner/wrong", "Wrong anime\u{2026}", State::wrong_anime))
}

pub(crate) fn led_color(led: Led) -> Color {
    match led {
        Led::Playing => PAL.ok(),
        Led::Idle => PAL.warn(),
        Led::Problem => PAL.status(Status::Dropped),
        Led::Off => PAL.faint,
    }
}

/// One player's line: dot, name, state.
fn player_line(s: &State, sw: Switch) -> El {
    let (line, led) = s.player_line(sw);
    let on = s.prefs.players.get(sw.config_key());
    row()
        .key(format!("player/{}", sw.config_key()))
        .gap(8.0)
        .py(6.0)
        .center_y()
        .child(el().size(7.0, 7.0).radius(3.5).fill(if on { led_color(led) } else { PAL.faint }))
        .child(text(sw.label().to_string()).font(theme::BODY_MED).color(PAL.text))
        .child(text(if on { line } else { "switched off in Settings".into() }).font(theme::LABEL).color(PAL.dim).truncate().grow(1.0).min_w(0.0))
}

fn players_status_card(s: &State) -> El {
    let platform = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
    let mut c = card("np-players", "Players").child(text(platform.to_string()).font(theme::CAPTION).color(PAL.faint).pb(4.0));
    for sw in Switch::here() {
        c = c.child(player_line(s, *sw));
    }
    c
}

/// A label and a value in the Now Playing card.
pub(crate) fn kv(label: &str, value: El) -> El {
    row().gap(12.0).py(3.0).items_start().child(text(label.to_string()).font(theme::LABEL).color(PAL.faint).w(110.0)).child(value.grow(1.0).min_w(0.0))
}

fn match_text(s: &State, n: &Now) -> (String, Color) {
    match &n.found {
        Found::List { media_id, sure, why } => {
            let st = s.entries.iter().find(|e| e.id == *media_id).map_or("", |e| e.status.label());
            if *sure { (format!("In your list ({st}), sure: {why}"), PAL.ok()) } else { (format!("In your list ({st}), not sure: {why}"), PAL.warn()) }
        }
        Found::NotOnList { .. } => (format!("Not on your list: {} (found on MyAnimeList)", s.not_on_list_title(n).unwrap_or_default()), PAL.dim),
        Found::Ignored => ("You marked this file as not an anime".into(), PAL.faint),
        Found::Nothing if n.recognizing => ("Looking\u{2026}".into(), PAL.faint),
        Found::Nothing => ("Not found on your list".into(), PAL.faint),
    }
}

/// Now Playing's hero: a full-width band with the cover's blurred tint, the poster, what plays,
/// how far in and the actions. `size`: the main area's width and height.
fn now_card(s: &State, n: &Now, size: [f32; 2]) -> El {
    use crate::look;
    let (id, title) = now_title(s, n);
    let url = now_cover_url(s, n);
    let entry = n.media_id().and_then(|id| s.entries.iter().find(|e| e.id == id));
    let mut sub = Vec::new();
    match (n.episode(), entry.and_then(|e| e.episodes)) {
        (Some(ep), Some(total)) => sub.push(format!("Episode {ep} of {total}")),
        (Some(ep), None) => sub.push(format!("Episode {ep}")),
        _ => {}
    }
    if let Some(e) = entry
        && !e.kind.is_empty()
    {
        sub.push(e.kind.to_string());
    }
    let threshold = s.prefs.threshold;
    let times = match (n.position_now(), n.duration) {
        (Some(p), Some(d)) => format!("{} / {}", clock_text(p), clock_text(d)),
        (Some(p), None) => clock_text(p),
        _ => String::new(),
    };
    let frac = n.fraction().unwrap_or(0.0);
    // The bar, then the threshold's tick in cream just under it.
    let t = f32::from(threshold) / 100.0;
    let bar = col()
        .key("np-bar")
        .w_full()
        .gap(2.0)
        .child(look::bar(frac, PAL.accent, 6.0, PAL.text.alpha(0.16)))
        .child(row().w_full().h(6.0).child(el().grow(t)).child(el().w(2.0).h(6.0).radius(1.0).fill(PAL.cream)).child(el().grow(1.0 - t)));
    // The process source has no position: say so instead of an empty bar.
    let no_position = n.position.is_none() && n.via == crate::precise::VIA_PROCESS;
    let verb = if no_position {
        "Open"
    } else if n.playing {
        "Playing"
    } else {
        "Paused"
    };
    let (wide, tall) = (size[0] >= 900.0, size[1] >= 700.0);
    let (pw, ph) = if wide && tall { (180.0, 270.0) } else if wide { (150.0, 225.0) } else { (124.0, 186.0) };
    let title_font = if wide { look::HERO_TITLE } else { TextStyle::new(24.0, 600.0).tracking(-0.2) };
    let title_el = text(title.clone()).key("np-title").font(title_font).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2);
    let mut info = col()
        .grow(1.0)
        .min_w(0.0)
        .max_w(640.0)
        .gap(6.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(look::live_dot(n.playing && !no_position, 8.0))
                .child(text(format!("{verb} in {} \u{b7} {}", n.player, n.via).to_uppercase()).font(theme::CAPTION).color(PAL.dim).truncate()),
        )
        .child(match now_stub(s, n) {
            Some(stub) => crate::details::title_link(title_el, stub),
            None => title_el,
        })
        .child(text(sub.join(" \u{b7} ")).font(TextStyle::new(16.0, 500.0)).color(PAL.dim))
        .child(if no_position {
            col()
                .key("np-no-position")
                .mt(12.0)
                .gap(4.0)
                .child(text(if n.reached { "Counts as watched" } else { "No position from the player" }).font(theme::LABEL_NUM).color(PAL.text))
                .child(
                    text("Kotora sees the file the player has open, not how far in you are: the episode counts once it has been open long enough.")
                        .font(theme::LABEL)
                        .color(PAL.dim)
                        .wrap(),
                )
        } else {
            col()
                .mt(12.0)
                .child(
                    row()
                        .mb(6.0)
                        .child(text(times).font(theme::NUM_MED).color(PAL.text))
                        .child(spacer())
                        .child(text(if n.reached { "Counts as watched".to_string() } else { format!("Offer at {threshold}%") }).font(theme::LABEL).color(PAL.dim)),
                )
                .child(bar)
        });
    let mut actions = row().gap(8.0).mt(12.0).center_y();
    if n.recorded.is_none()
        && let Some((_, ep)) = s.offer()
    {
        actions = actions.child(button("np/update", &format!("Update to episode {ep}"), true, Some(|s: &mut State| {
            let _ = s.confirm_offer();
        })));
    }
    if n.recorded.is_some() {
        actions = actions.child(button("np/undo", "Undo", false, Some(State::undo_record)));
    }
    if !matches!(n.found, Found::Ignored) {
        actions = actions.child(ghost("np/wrong", "Wrong anime\u{2026}", State::wrong_anime));
    }
    info = info.child(actions);
    let scrim = if PAL.light { look::Scrim { top: 0.5, mid: 0.62, bottom: 1.0 } } else { look::Scrim { top: 0.4, mid: 0.6, bottom: 1.0 } };
    let band_h = ph + if tall { 84.0 } else { 56.0 };
    el().key("np-card")
        .w_full()
        .min_h(band_h)
        .child(look::backdrop(s, id, url, scrim))
        .child(
            row()
                .w_full()
                .min_h(band_h)
                .px(theme::PAGE_PAD + if wide { 8.0 } else { 0.0 })
                .pt(if tall { 36.0 } else { 24.0 })
                .pb(if tall { 28.0 } else { 18.0 })
                .gap(if wide { 32.0 } else { 20.0 })
                .items_end()
                .child(look::poster(s, id, &title, url, pw, ph, theme::R_LG).border(1.0, look::glass_border()).shadow(14.0, 36.0, Color::BLACK.alpha(if PAL.light { 0.25 } else { 0.55 })))
                .child(info),
        )
}

/// What Kotora made of the file: the file name, what the parser read, the match and the
/// user's progress (under the hero).
fn now_facts(s: &State, n: &Now) -> El {
    let entry = n.media_id().and_then(|id| s.entries.iter().find(|e| e.id == id));
    let mut facts = card("np-recog", "Recognised from").child(
        row()
            .gap(8.0)
            .pb(6.0)
            .center_y()
            .child(icon(&icons::FILE, 14.0).color(PAL.faint))
            .child(text(n.file.clone()).key("np-file").font(TextStyle::new(12.0, 400.0).mono()).color(PAL.dim).truncate().min_w(0.0)),
    );
    if let Some(p) = &n.parsed {
        facts = facts.child(kv("Parsed title", text(p.title.clone()).font(theme::BODY).color(PAL.text).wrap()));
        facts = facts.child(kv("Episode", text(p.episode.map_or("–".into(), |e| e.to_string())).font(theme::LABEL_NUM).color(PAL.text)));
        if let Some(season) = p.season {
            facts = facts.child(kv("Season", text(season.to_string()).font(theme::LABEL_NUM).color(PAL.text)));
        }
        let group = [p.group.as_deref(), p.resolution.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ");
        if !group.is_empty() {
            facts = facts.child(kv("Group", text(group).font(theme::BODY).color(PAL.text)));
        }
    }
    let (m, c) = match_text(s, n);
    facts = facts.child(kv("Match", text(m).key("np-match").font(theme::BODY).color(c).wrap()));
    if let Some(e) = entry {
        let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
        let after = match s.offer() {
            Some((_, ep)) => format!("{} / {total} \u{2192} {ep} after update", e.progress),
            None => format!("{} / {total}", e.progress),
        };
        facts = facts.child(kv("Your progress", text(after).font(theme::LABEL_NUM).color(PAL.text)));
    }
    facts
}

/// "Not sure: is this X episode N?" with Yes / Pick another.
fn unsure_card(s: &State, n: &Now) -> El {
    let (_, title) = now_title(s, n);
    let ep = n.episode().map(|e| format!(", episode {e}")).unwrap_or_default();
    // The season is the file's, not the entry's: "S03E07" next to a season 1 title misleads.
    let season = n.parsed.as_ref().and_then(|p| p.season).map(|se| format!(" (season {se})")).unwrap_or_default();
    let id = n.media_id().unwrap_or(0);
    card("np-unsure", "Not sure")
        .child(
            row()
                .gap(12.0)
                .pt(4.0)
                .items_start()
                .child(now_cover(s, n, 40.0, 56.0, 12.0))
                .child(text(format!("From {}{season}: is this {title}{ep}?", n.file)).font(theme::LABEL).color(PAL.text).wrap().grow(1.0).min_w(0.0)),
        )
        .child(
            row()
                .gap(8.0)
                .mt(12.0)
                .child(action("np/yes".into(), "Yes, that one", false, move |s, _| s.pick_anime(id)))
                .child(ghost("np/pick", "Pick another\u{2026}", State::wrong_anime).h(26.0)),
        )
}

/// "Wrong anime…": the closest titles on the list, and "not an anime".
fn picker_card(n: &Now) -> El {
    let mut c = card("np-picker", "Which anime is it?").child(
        text("The closest titles on your list. Kotora remembers your pick for this title.").font(theme::LABEL).color(PAL.dim).wrap().pb(6.0),
    );
    if n.candidates.is_empty() {
        c = c.child(text("Nothing on your list comes close.").font(theme::LABEL).color(PAL.faint).py(4.0));
    }
    for (id, title) in &n.candidates {
        let id = *id;
        c = c.child(
            row()
                .key(format!("pick/{id}"))
                .role("button")
                .label(title.clone())
                .focusable(true)
                .gap(10.0)
                .py(6.0)
                .px(6.0)
                .center_y()
                .radius(theme::R_SM)
                .hover(PAL.hover())
                .press(PAL.press())
                .on_press(move |s: &mut State| s.pick_anime(id))
                .child(cover(id, title, 22.0, 31.0, 8.0))
                .child(text(title.clone()).font(theme::BODY).color(PAL.text).truncate().grow(1.0).min_w(0.0)),
        );
    }
    c.child(
        row()
            .gap(8.0)
            .mt(10.0)
            .child(button("pick/none", "Not an anime", false, Some(State::ignore_file)).h(26.0).px(10.0))
            .child(ghost("pick/cancel", "Cancel", |s| {
                if let Some(n) = &mut s.now {
                    n.picking = false;
                }
            })
            .h(26.0)),
    )
}

/// What the details page needs for the playing anime, when it is known.
fn now_stub(s: &State, n: &Now) -> Option<crate::details::Stub> {
    match &n.found {
        Found::List { media_id, .. } => s.entries.iter().find(|e| e.id == *media_id).map(crate::details::Stub::from_entry),
        Found::NotOnList { media_id, .. } => Some(crate::details::Stub::bare(*media_id, &s.not_on_list_title(n).unwrap_or_default())),
        _ => None,
    }
}

fn not_on_list_card(id: u32, title: &str, relation: Option<&str>, episode: Option<u32>) -> El {
    let (say, add) = match episode {
        Some(ep) => (
            format!("MyAnimeList knows this as {title}, but it isn't on your list. Add it as Watching and record episode {ep} in one change."),
            format!("Add as Watching at episode {ep}"),
        ),
        None => (format!("MyAnimeList knows this as {title}, but it isn't on your list. Add it, and Kotora offers the episode."), "Add as Watching".to_string()),
    };
    let mut c = card("np-not-on-list", "Not on your list").child(
        text(say)
            .font(theme::LABEL)
            .color(PAL.dim)
            .wrap(),
    );
    // anime-relations moved the file here: say so, or the other title looks like a wrong match.
    if let Some(why) = relation {
        let mut why = why.to_string();
        if let Some(first) = why.get(..1) {
            why.replace_range(..1, &first.to_uppercase());
        }
        c = c.child(text(format!("{why}.")).key("np-relation").font(theme::LABEL).color(PAL.faint).wrap().mt(6.0));
    }
    c
        .child(
            row()
                .gap(8.0)
                .mt(12.0)
                .child(action("np/add".into(), &add, true, |s, _| s.add_playing()))
                .child(crate::details::details_ghost(crate::details::Stub::bare(id, title))),
        )
}

/// Nothing plays: how to let Kotora see the players.
fn now_empty() -> El {
    let help = if cfg!(target_os = "macos") {
        "mpv, IINA and VLC are found by themselves, by the file they have open. For the position too, mpv and IINA use their IPC socket (Kotora offers to turn it on; see Settings \u{2192} Players), and VLC its web interface with a password in config.toml ([vlc] password)."
    } else {
        "mpv, VLC, mpc-qt, Celluloid and Haruna are found by themselves, by the file they have open. For the position too, VLC and mpc-qt use MPRIS, and mpv its IPC socket (Kotora offers to turn it on)."
    };
    col()
        .key("np-empty")
        .gap(6.0)
        .padding(16.0)
        .radius(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(text("Nothing is playing").font(theme::SECTION).color(PAL.text))
        .child(text("Play an episode in one of the players on the right; Kotora sees it and offers to record it.").font(theme::LABEL).color(PAL.dim).wrap())
        .child(text(help).font(theme::LABEL).color(PAL.faint).wrap().mt(4.0))
        .child(text("Streaming sites in the browser: use MAL-Sync.").font(theme::LABEL).color(PAL.faint).wrap())
}

fn now_page(s: &State, wide: bool, size: [f32; 2]) -> El {
    let mut side = col().gap(16.0);
    if s.now.is_none() {
        side = side.children(crate::library::up_next_card(s));
    }
    if let Some(n) = &s.now {
        if n.picking {
            side = side.child(picker_card(n));
        } else if matches!(n.found, Found::List { sure: false, .. }) && n.recorded.is_none() {
            side = side.child(unsure_card(s, n));
        } else if let Found::NotOnList { media_id, .. } = &n.found {
            let title = s.not_on_list_title(n).unwrap_or_default();
            side = side.child(not_on_list_card(*media_id, &title, s.relation_text(n).as_deref(), n.episode().filter(|ep| *ep > 0)));
        }
    }
    side = side.child(players_status_card(s));
    let Some(n) = &s.now else {
        let body = if wide {
            row().gap(16.0).w_full().items_start().child(now_empty().grow(1.0).min_w(0.0)).child(side.w(340.0))
        } else {
            col().gap(16.0).w_full().child(now_empty()).child(side)
        };
        return col().key("page/now").w_full().h_full().child(top(s, "Now Playing", None, false)).child(
            col()
                .key("now-scroll")
                .scroll(ScrollSpec { stick_bottom: false })
                .grow(1.0)
                .min_h(0.0)
                .child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).max_w(1120.0).child(body)),
        );
    };
    // Something plays: the hero band on top (no title bar: the band says it), the cards below.
    let facts = now_facts(s, n);
    let body = if wide {
        row().gap(16.0).w_full().items_start().child(facts.grow(1.0).min_w(0.0)).child(side.w(340.0))
    } else {
        col().gap(16.0).w_full().child(facts).child(side)
    };
    col().key("page/now").w_full().h_full().child(
        col()
            .key("now-scroll")
            .scroll(ScrollSpec { stick_bottom: false })
            .grow(1.0)
            .min_h(0.0)
            .child(now_card(s, n, size))
            .child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).max_w(1120.0).child(body)),
    )
}

// --------------------------------------------------------------------- settings: updates, players

/// The 36 × 20 switch (design-spec §4): off `border_strong`, on `accent`.
pub(crate) fn switch(key: String, label: &str, on: bool, set: impl Fn(&mut State, bool) + 'static) -> El {
    switch_if(key, label, on, true, set)
}

/// A switch that can be disabled: dimmed, not focusable, and it ignores presses.
pub(crate) fn switch_if(key: String, label: &str, on: bool, enabled: bool, set: impl Fn(&mut State, bool) + 'static) -> El {
    let knob = el().size(16.0, 16.0).radius(8.0).fill(if on { PAL.on_accent } else { PAL.knob });
    let track = row()
        .key(key)
        .role("switch")
        .label(label.to_string())
        .focusable(enabled)
        .w(36.0)
        .h(20.0)
        .px(2.0)
        .center_y()
        .radius(10.0)
        .fill(if on { PAL.accent } else { PAL.border_strong });
    let track = if on { track.justify_end() } else { track }.child(knob);
    if enabled {
        track.cursor(rune::Cursor::Pointer).on_press(move |s: &mut State| set(s, !on))
    } else {
        track.opacity(0.4).cursor(rune::Cursor::NotAllowed)
    }
}

/// A settings line: title and help on the left, the control on the right.
pub(crate) fn option(title: &str, help: String, control: El) -> El {
    row()
        .gap(12.0)
        .py(10.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(title.to_string()).font(theme::BODY_MED).color(PAL.text).wrap())
                .child(text(help).font(theme::LABEL).color(PAL.dim).wrap()),
        )
        .child(control)
}

fn updates_card(s: &State) -> El {
    let p = &s.prefs;
    let next = THRESHOLDS[(THRESHOLDS.iter().position(|t| *t == p.threshold).unwrap_or(1) + 1) % THRESHOLDS.len()];
    let threshold = row()
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
        .on_press(|s: &mut State| s.next_threshold())
        .child(text(format!("{}%", p.threshold)).font(theme::LABEL_NUM).color(PAL.text))
        .child(icon(&icons::DOWN, 14.0).color(PAL.faint));
    let ask = if p.auto {
        "Off: confident matches are recorded at once, with Undo in the banner."
    } else {
        "On: a banner and a notification ask; nothing changes until you click Update."
    };
    card("updates-card", "Updates")
        .child(
            row()
                .key(if p.auto { "set/asks-off" } else { "set/asks-on" })
                .gap(8.0)
                .py(6.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(if p.auto { PAL.faint } else { PAL.ok() }))
                .child(text("Ask before updating".to_string()).font(theme::BODY_MED).color(PAL.text))
                .child(text(ask.to_string()).font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0)),
        )
        .child(option(
            "Update automatically",
            "Record confident matches without asking. Unsure matches, and players seen by their open file only, still ask. Off by default.".into(),
            switch("set/auto".into(), "Update automatically", p.auto, State::set_auto),
        ))
        .child(option("Offer the update at", "Share of the episode watched (or half, when the player stops)".into(), threshold))
        .child(option(
            "Keep the higher progress in conflicts",
            "When only the progress differs and yours is higher, Kotora sends yours.".into(),
            switch("set/keep-higher".into(), "Keep the higher progress in conflicts", p.keep_higher, State::set_keep_higher),
        ))
}

/// Settings → Notifications: one switch per event, and what the system can't do.
fn notifications_card(s: &State) -> El {
    use crate::notes::{Kind, Support};
    let p = &s.prefs.notify;
    let mut c = card("notifications-card", "Notifications")
        .child(option(
            "Episode detected",
            "When a recognised episode starts playing: \u{201c}Title \u{b7} Ep 6\u{201d}. Off by default.".into(),
            switch("set/notify-detected".into(), "Episode detected", p.detected, |s, v| s.set_notify(Kind::Detected, v)),
        ))
        .child(option(
            "Update offered",
            "\u{201c}Update to 6?\u{201d} with Update and Not now, which act as in the banner.".into(),
            switch("set/notify-offer".into(), "Update offered", p.offer, |s, v| s.set_notify(Kind::Offer, v)),
        ))
        .child(option(
            "Episode recorded",
            "\u{201c}Recorded \u{b7} Ep 6\u{201d} with Undo.".into(),
            switch("set/notify-recorded".into(), "Episode recorded", p.recorded, |s, v| s.set_notify(Kind::Recorded, v)),
        ))
        .child(option(
            "Started airing",
            "When a show on Plan to watch starts airing: \u{201c}Title started airing\u{201d}.".into(),
            switch("set/notify-started".into(), "Started airing", p.started_airing, |s, v| s.set_notify(Kind::Started, v)),
        ))
        .child(option(
            "New episode",
            "When the next episode of a show you\u{2019}re watching is out, by the estimated broadcast time: \u{201c}Title \u{b7} Ep 7 is out\u{201d}, with Play when the file is in your library.".into(),
            switch("set/notify-episode".into(), "New episode", p.new_episode, |s, v| s.set_notify(Kind::Aired, v)),
        ));
    let line = match s.notes.support {
        Support::NoButtons => Some("Your notification server shows no buttons: answer in Kotora's banner or with kot confirm."),
        Support::Outside => Some("Kotora runs outside Kotora.app, so notifications have no buttons. Open Kotora.app to get them."),
        Support::Denied => Some("Notifications are off for Kotora in System Settings \u{2192} Notifications."),
        Support::Unknown | Support::Buttons => None,
    };
    if let Some(line) = line {
        c = c.child(
            row()
                .key("set/notify-support")
                .mt(6.0)
                .px(10.0)
                .py(8.0)
                .radius(theme::R_MD)
                .fill(PAL.warn().alpha(0.08))
                .border(1.0, PAL.warn().alpha(0.3))
                .child(text(line.to_string()).font(theme::LABEL).color(PAL.dim).wrap().grow(1.0).min_w(0.0)),
        );
    }
    c
}

/// Settings → Appearance: the theme, and which title every page (and `kot`) shows.
fn display_card(s: &State) -> El {
    let mut themes = row().key("set/theme").gap(6.0);
    for t in kotora_core::config::Theme::ALL {
        themes = themes.child(
            cell(format!("set/theme/{}", t.key()), format!("Theme {}", t.label()), s.prefs.theme == t, move |s: &mut State| s.set_theme(t))
                .child(text(t.label()).font(theme::LABEL).color(PAL.text)),
        );
    }
    let mut choice = row().key("set/titles").gap(6.0);
    for lang in kotora_core::config::TitleLang::ALL {
        choice = choice.child(
            cell(format!("set/titles/{}", lang.key()), format!("Titles in {}", lang.label()), s.prefs.titles == lang, move |s: &mut State| s.set_titles(lang))
                .child(text(lang.label()).font(theme::LABEL).color(PAL.text)),
        );
    }
    card("display-card", "Appearance")
        .child(option(
            "Theme",
            "System follows your desktop\u{2019}s light or dark setting, and changes with it.".into(),
            themes,
        ))
        .child(option(
            "Titles",
            "The list, Now Playing, search, details, the queue and kot. A title MyAnimeList doesn't have in that language shows in romaji.".into(),
            choice,
        ))
}

/// Settings → Startup: start at login, and whether that start shows the window.
fn startup_card(s: &State) -> El {
    let p = &s.prefs;
    let l = &p.login;
    let tray = if cfg!(target_os = "macos") { "menu bar" } else { "tray" };
    let note = match (l.unavailable, l.needs_approval) {
        (Some(why), _) => why.to_string(),
        (None, true) => "Waiting for your OK in System Settings, General, Login Items.".to_string(),
        _ if cfg!(target_os = "macos") => {
            "Opens Kotora when you log in to your Mac, so it keeps detecting episodes. It is listed in System Settings, General, Login Items.".to_string()
        }
        _ => "Kotora keeps detecting episodes from the moment you log in.".to_string(),
    };
    let sw = switch_if("set/login".into(), "Start Kotora when I log in", l.on, l.available() && !p.login_busy, State::set_login);
    let control = if l.needs_approval {
        row().gap(12.0).center_y().child(small_button("set/login-items", "Open Login Items", false, Some(State::open_login_items))).child(sw)
    } else {
        sw
    };
    card("startup-card", "Startup")
        .child(option("Start Kotora when I log in", note, control))
        .child(option(
            "Start in the background",
            format!("At login, show only the {tray} icon, no window. Closing the window always keeps Kotora in the {tray}."),
            switch("set/background".into(), "Start in the background", p.background, State::set_background),
        ))
}

fn players_card(s: &State) -> El {
    let platform = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
    let mut c = card("players-card", "Players").child(text(platform.to_string()).font(theme::CAPTION).color(PAL.faint));
    for sw in Switch::here() {
        let sw = *sw;
        let on = s.prefs.players.get(sw.config_key());
        let (line, led) = s.player_line(sw);
        let help = match sw {
            Switch::OtherMpris => format!("Haruna, Celluloid and others that report a file \u{b7} {}", line.split(" \u{b7} ").last().unwrap_or("")),
            _ => line,
        };
        c = c.child(
            row()
                .key(format!("set/player/{}", sw.config_key()))
                .gap(12.0)
                .py(8.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(if on { led_color(led) } else { PAL.faint }))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
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
    }
    let p = &s.prefs.players;
    let minutes = p.process_minutes.clamp(1, 180);
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
    c = c
        .child(option(
            "Find players by their open files",
            "No setup, but no position or pause: a file counts after it stays open long enough, and Kotora always asks.".into(),
            switch("set/process-scan".into(), "Find players by their open files", p.process_scan, State::set_process_scan),
        ))
        .child(option(
            "Count a file after it stays open",
            "Open files only, when the episode's length is unknown (default 15 minutes).".into(),
            if p.process_scan { stepper } else { stepper.opacity(0.5) },
        ));
    c.child(
        text("Players are found by their open files with no setup; \u{201c}process\u{201d} means no position, so a file counts after it stays open a while. Socket paths and VLC's web password: config.toml ([players] mpv_sockets, [vlc]). Streaming sites in the browser: use MAL-Sync.")
            .font(theme::LABEL)
            .color(PAL.faint)
            .wrap()
            .mt(6.0),
    )
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_skip_small_words() {
        assert_eq!(initials("Lanterns of Kirigaoka"), "LK");
        assert_eq!(initials("Kumo to Kaze no Ryokan"), "KK");
        assert_eq!(initials("Ghost of the Library Stacks"), "GL");
        assert_eq!(initials("The Ninth Shrine Maiden"), "NS");
        assert_eq!(initials("Neon Ronin 2099"), "NR");
        assert_eq!(initials("The"), "T");
        assert_eq!(initials(""), "");
    }

    /// The playing anime's cover: the entry's, else the one found with a match off the list.
    #[test]
    fn op_cover_url_is_the_entrys_or_none() {
        let mut s = crate::model::State::fake();
        let id = s.entries[0].id;
        s.entries[0].cover_url = Some("https://cdn.example.invalid/x.jpg".into());
        assert_eq!(super::op_cover_url(&s, id), Some("https://cdn.example.invalid/x.jpg"));
        assert_eq!(super::op_cover_url(&s, 4_000_000), None, "not on the list: the tile");
    }

    #[test]
    fn now_cover_url_from_the_entry_or_the_match() {
        let mut s = crate::model::State::fake();
        let n = s.now.clone().unwrap();
        assert_eq!(super::now_cover_url(&s, &n), None, "the fake entry has no cover");
        s.entries.iter_mut().find(|e| e.id == 0).unwrap().cover_url = Some("https://cdn.example.invalid/0.jpg".into());
        assert_eq!(super::now_cover_url(&s, &n), Some("https://cdn.example.invalid/0.jpg"));
        let mut n = n;
        n.found = kotora_core::recognize::Found::NotOnList { media_id: 9001, title: "Velvet".into() };
        assert_eq!(super::now_cover_url(&s, &n), None);
        n.cover_url = Some("https://cdn.example.invalid/9001l.jpg".into());
        assert_eq!(super::now_cover_url(&s, &n), Some("https://cdn.example.invalid/9001l.jpg"));
        n.found = kotora_core::recognize::Found::Nothing;
        assert_eq!(super::now_cover_url(&s, &n), None, "nothing matched: the tile");
    }
}
