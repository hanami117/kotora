//! The view: sidebar and list page, as in `mockup/kotora.html` (design-spec §4).

use std::f32::consts::PI;

use kotora_core::Status;
use rune::{Color, Cx, El, GrowthSpec, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Air, Auth, Entry, Filter, Page, Sort, State};
use crate::online;
use crate::theme::{self, PAL};

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    // The type column hides below 1100 pt (design-spec §3).
    let wide = cx.layout_size("root").is_none_or(|[w, _]| w >= theme::NARROW_W);
    // macOS draws the traffic lights over the window's top-left (Rune's full-size content), so the
    // sidebar keeps a band clear for them. None in full screen; Linux draws its own frame.
    s.covers.set_scale(cx.env().scale);
    let chrome_top = if cfg!(target_os = "macos") && cx.titlebar_inset() > 0.0 { theme::TITLEBAR_BAND } else { 0.0 };
    row()
        .key("root")
        .w_full()
        .h_full()
        .fill(PAL.bg)
        .child(sidebar(s, chrome_top))
        .child(el().w(1.0).h_full().fill(PAL.border))
        .child(main(s, wide))
}

/// Nothing ticks: workers report through `update_if`, so `poll` never has a visible change.
pub fn poll(_s: &mut State, _t: f64, _tx: &mut Tx) -> bool {
    false
}

/// Ctrl F (Cmd F on macOS) jumps to the search box.
pub fn on_key(s: &mut State, k: &rune::KeyEvent, _tx: &mut Tx) -> bool {
    if k.is_command('f') && matches!(s.page, Page::List(_)) && s.has_list() {
        s.query.focused = true;
        return true;
    }
    false
}

// ------------------------------------------------------------------------------------ sidebar

/// `chrome_top`: the traffic-light band's height (macOS), or 0 where the system draws the frame.
fn sidebar(s: &State, chrome_top: f32) -> El {
    let side = col().key("sidebar").w(theme::SIDEBAR_W).h_full().fill(PAL.side).px(10.0).pb(12.0);
    let side = if chrome_top > 0.0 {
        // The band drags the window like a title bar; the brand sits just under it.
        side.child(el().key("sb-titlebar").w_full().h(chrome_top).window_drag()).child(el().h(4.0))
    } else {
        side.pt(14.0)
    };
    let mut side = side.child(brand(s));
    if let Some((name, service)) = &s.account {
        side = side.child(account(name, service));
    }
    side = side.child(nav_heading("My list"));
    for st in Status::ALL {
        let count = (!s.loading && s.has_list()).then(|| s.count(st).to_string());
        side = side.child(nav_item(
            st.slug(),
            NavMark::Dot(PAL.status(st)),
            st.label(),
            count.map(NavEnd::Count),
            s.page == Page::List(st),
            Page::List(st),
        ));
    }
    side = side.child(nav_heading("Kotora").mt(12.0));
    side = side
        .child(nav_item("now-playing", NavMark::Icon(&icons::PLAY), "Now Playing", None, s.page == Page::NowPlaying, Page::NowPlaying))
        .child(nav_item(
            "queue",
            NavMark::Icon(&icons::SYNC),
            "Sync queue",
            (s.queue > 0).then(|| NavEnd::Badge(s.queue.to_string())),
            s.page == Page::Queue,
            Page::Queue,
        ))
        .child(nav_item("settings", NavMark::Icon(&icons::SLIDERS), "Settings", None, s.page == Page::Settings, Page::Settings))
        .child(spacer());
    if let Some(now) = &s.now {
        side = side.child(
            row()
                .key("np-mini")
                .padding(8.0)
                .gap(8.0)
                .center_y()
                .radius(theme::R_MD)
                .fill(PAL.surface)
                .border(1.0, PAL.border)
                .child(cover(now.id, &now.title, 26.0, 38.0, 9.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(now.title.clone()).font(rune::TextStyle::new(12.0, 500.0)).color(PAL.text).truncate())
                        .child(text(now.line.clone()).font(theme::SMALL).color(PAL.faint).truncate()),
                ),
        );
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

/// "just now", "2 min ago", "3 h ago", "2 days ago".
fn sync_age(secs: i64) -> String {
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

fn account(name: &str, service: &str) -> El {
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    row()
        .key("account")
        .gap(8.0)
        .padding(8.0)
        .mb(14.0)
        .center_y()
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(
            el().size(24.0, 24.0)
                .radius(12.0)
                .linear_gradient(160.0 * PI / 180.0, [(0.0, hex("#2e51a2")), (1.0, hex("#1d3170"))])
                .center()
                .child(text(initial).font(rune::TextStyle::new(10.0, 600.0)).color(PAL.text)),
        )
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
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            // A new page replaces the old one at once; rows don't swim.
            tx.instant();
            s.go(page);
        })
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
        None => item,
    }
}

// --------------------------------------------------------------------------------------- main

fn main(s: &State, wide: bool) -> El {
    let page = match s.page {
        Page::List(_) if !s.has_list() => welcome(s),
        Page::List(st) => list_page(s, st, wide),
        Page::NowPlaying => placeholder(s, "Now Playing", "Nothing is playing."),
        Page::Queue => placeholder(s, "Sync queue", "The queue comes in a later stage."),
        Page::Settings => settings(s, wide),
    };
    col().key("main").grow(1.0).min_w(0.0).h_full().child(page)
}

fn placeholder(s: &State, title: &str, line: &str) -> El {
    col()
        .key(format!("page/{title}"))
        .w_full()
        .h_full()
        .child(top(s, title, None, false))
        .child(text(line.to_string()).font(theme::BODY).color(PAL.faint).px(theme::PAGE_PAD))
}

/// The page's top bar: title, count, and (on the list) search and sort.
fn top(s: &State, title: &str, count: Option<usize>, tools: bool) -> El {
    let mut t = row().key("top").gap(12.0).center_y().px(theme::PAGE_PAD).pt(18.0).pb(14.0).child(
        row()
            .gap(6.0)
            .items_end()
            .child(text(title.to_string()).font(theme::TITLE).color(PAL.text))
            .child(match count {
                Some(n) => text(n.to_string()).font(theme::TITLE_NUM).color(PAL.faint),
                None => el(),
            }),
    );
    if tools {
        t = t.child(spacer()).child(search_box(s)).child(sort_select(s.sort));
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
                .placeholder("Search titles", PAL.faint)
                .on_edit(|s: &mut State| &mut s.query)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.refresh())
                .on_submit(|_s: &mut State, tx: &mut Tx| tx.blur()),
        )
        .child(
            el().px(5.0).py(1.0).radius(4.0).border(1.0, PAL.border_strong).child(text(kbd).font(TextStyle::new(10.0, 500.0)).color(PAL.faint)),
        )
}

/// The sort control cycles through the orders on each press.
fn sort_select(sort: Sort) -> El {
    row()
        .key("sort")
        .role("button")
        .label(format!("Sort: {}", sort.label()))
        .focusable(true)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.set_sort(s.sort.next());
        })
        .child(text("Sort").font(theme::BODY).color(PAL.faint))
        .child(text(sort.label()).font(theme::BODY_MED).color(PAL.text))
        .child(icon(&icons::DOWN, 14.0).color(PAL.text))
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

fn list_page(s: &State, st: Status, wide: bool) -> El {
    let count = s.count(st);
    let mut page = col().key(format!("page/{}", st.slug())).w_full().h_full().child(top(s, st.label(), (!s.loading).then_some(count), true));
    if let Some(a) = status_alert(s) {
        page = page.child(a);
    }
    if s.loading {
        return page.child(loading());
    }
    if count == 0 {
        return page.child(empty(st));
    }
    page = page.child(chips(s, st));
    if s.rows.is_empty() {
        return page.child(no_match(s));
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

fn chips(s: &State, st: Status) -> El {
    let mut r = row().key("chips").gap(6.0).px(theme::PAGE_PAD).pb(10.0);
    for f in Filter::ALL {
        let n = s.entries.iter().filter(|e| e.status == st && f.keeps(e)).count();
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
    let (now, plus) = (s.clock(), !s.read_only);
    let id = e.id;
    let status_c = PAL.status(e.status);

    let mut sub = row().gap(8.0).center_y();
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
        sub = sub.child(tag_tinted(&format!("Ep {n} out"), PAL.accent_hi, PAL.accent.alpha(0.14)));
    }
    let title = col().gap(2.0).child(text(e.title.clone()).font(theme::ROW_TITLE).color(PAL.text).truncate()).child(sub);

    let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
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
        .child(if plus { plus_button(id) } else { el() });
    let score = match e.score {
        Some(n) => text(n.to_string()).font(theme::NUM_MED).color(PAL.text),
        None => text("\u{2013}").font(theme::BODY).color(PAL.faint),
    };
    col()
        .w_full()
        .h(theme::ROW_H)
        .hover(PAL.hover())
        .child(
            columns(
                row_cover(s, e),
                title,
                progress,
                score,
                text(e.kind).font(theme::LABEL).color(PAL.dim),
                text(kotora_core::time::age(e.updated_at, now)).font(theme::LABEL).color(PAL.dim),
                wide,
            )
            .grow(1.0)
            .px(theme::PAGE_PAD),
        )
        .child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
}

/// `+1`: optimistic and at once (design-spec §6), no counting animation.
fn plus_button(id: u32) -> El {
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
            s.plus_one(id);
        })
        .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent))
}

fn tag(label: &str, c: Color, dot: bool) -> El {
    let mut t = row().h(18.0).px(6.0).gap(4.0).center_y().radius(theme::R_XS).fill(c.alpha(0.12));
    if dot {
        t = t.child(el().size(5.0, 5.0).radius(2.5).fill(c));
    }
    t.child(text(label.to_string()).font(theme::CAPTION.tracking(0.0)).color(c))
}

fn tag_tinted(label: &str, fg: Color, bg: Color) -> El {
    row().h(18.0).px(6.0).center_y().radius(theme::R_XS).fill(bg).child(text(label.to_string()).font(theme::CAPTION.tracking(0.0)).color(fg))
}

/// Cover pairs from the mockup; an id picks one (design-spec §3: covers while loading or missing).
const TILES: [(&str, &str); 10] = [
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

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}

/// The generated tile: a 160° gradient and the title's initials.
fn cover(id: u32, title: &str, w: f32, h: f32, size: f32) -> El {
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
fn row_cover(s: &State, e: &Entry) -> El {
    let (w, h) = (34.0, 48.0);
    let tile = cover(e.id, &e.title, w, h, 12.0);
    match s.covers.get(e.id, e.cover_url.as_deref(), w, h) {
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

// ------------------------------------------------------------------- alerts, sign-in, settings

fn button(key: &str, label: &str, primary: bool, on: Option<fn(&mut State)>) -> El {
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
fn alert(key: &str, c: Color, message: String, action: Option<El>) -> El {
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
        .child(text(message).font(theme::BODY).color(PAL.text).grow(1.0).min_w(0.0));
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
        Auth::NoClientId => {
            c = c
                .child(text("To sign in, Kotora needs a MyAnimeList Client ID.").font(theme::BODY).color(PAL.dim))
                .child(
                    col()
                        .key("no-client-id")
                        .gap(6.0)
                        .padding(16.0)
                        .mt(6.0)
                        .radius(theme::R_MD)
                        .fill(PAL.status(Status::Dropped).alpha(0.08))
                        .border(1.0, PAL.status(Status::Dropped).alpha(0.3))
                        .child(text("Add this to ~/.config/kotora/config.toml:").font(theme::LABEL).color(PAL.dim))
                        .child(text("[mal]\nclient_id = \"your client id\"").font(TextStyle::new(12.0, 400.0).mono()).color(PAL.text))
                        .child(
                            text("Register a free ID at myanimelist.net/apiconfig: app type \u{201c}other\u{201d}, redirect URL http://127.0.0.1:47321/callback.")
                                .font(theme::LABEL)
                                .color(PAL.dim),
                        ),
                )
                .child(row().gap(8.0).mt(6.0).child(button("sign-in", "Sign in", true, None)).child(button("check-again", "Check again", false, Some(online::recheck_config))));
        }
        Auth::SigningIn => {
            c = c
                .child(text("Waiting for MyAnimeList in your browser\u{2026}").font(theme::BODY).color(PAL.dim))
                .child(row().mt(6.0).child(button("cancel", "Cancel", false, Some(online::cancel_sign_in))));
        }
        _ => {
            c = c
                .child(text("Sign in to MyAnimeList to see your list. Kotora only reads it for now.").font(theme::BODY).color(PAL.dim))
                .child(row().mt(6.0).child(button("sign-in", "Sign in", true, Some(online::sign_in))));
        }
    }
    if let Some(e) = &s.error {
        c = c.child(text(e.clone()).font(theme::LABEL).color(PAL.status(Status::Dropped)).mt(8.0));
    }
    c
}

/// Settings (design-spec §4): Accounts and About for now; Updates and Players come with detection
/// (stage 4). Two columns when wide, one below `theme::NARROW_W`.
fn settings(s: &State, wide: bool) -> El {
    let cards = if wide {
        row().gap(16.0).w_full().child(accounts_card(s).grow(1.0).min_w(0.0)).child(about_card(s).grow(1.0).min_w(0.0))
    } else {
        col().gap(16.0).w_full().child(accounts_card(s)).child(about_card(s))
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

fn card(key: &str, title: &str) -> El {
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
                .child(text(badge.to_string()).font(TextStyle::new(11.0, 600.0)).color(fg)),
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
        (None, Auth::NoClientId) => ("No Client ID in config.toml".to_string(), small_button("sign-in", "Sign in", true, None)),
        (None, Auth::SigningIn) => ("Waiting for your browser\u{2026}".to_string(), small_button("cancel", "Cancel", false, Some(online::cancel_sign_in))),
        (None, _) => ("Not signed in".to_string(), small_button("sign-in", "Sign in", true, Some(online::sign_in))),
    };
    let client_id = if s.auth == Auth::NoClientId { "not set yet (the Welcome page shows how)" } else { "set" };
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
            text(format!(
                "Your sign-in token is kept in a file only you can read. It is not encrypted. Signing out deletes it and your saved list. The Client ID comes from config.toml: {client_id}."
            ))
            .font(theme::LABEL)
            .color(PAL.text)
            .wrap()
            .grow(1.0)
            .min_w(0.0),
        );
    card("account-card", "Accounts")
        .child(service_row("acct/mal", "MAL", hex("#2e51a2"), "MyAnimeList", line, action, true))
        .child(el().w_full().h(1.0).fill(PAL.border))
        .child(service_row("acct/anilist", "AL", hex("#1d3550"), "AniList", "Coming in v0.2".into(), small_button("anilist", "Sign in", false, None), false))
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
                    .child(text("GPL-3.0-or-later. Not affiliated with MyAnimeList.").font(theme::LABEL).color(PAL.faint).wrap())
                    .child(
                        row()
                            .gap(14.0)
                            .child(link("link/site", "kotora.moe", "https://kotora.moe"))
                            .child(link("link/source", "Source code", "https://github.com/hanami117/kotora")),
                    ),
            ),
    )
}

/// The `sm` button (26 high) of the settings rows.
fn small_button(key: &str, label: &str, primary: bool, on: Option<fn(&mut State)>) -> El {
    button(key, label, primary, on).h(26.0).px(10.0)
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
}
