//! The view: sidebar and list page, as in `mockup/kotora.html` (design-spec §4).

use std::f32::consts::PI;

use kotora_core::Status;
use rune::{Color, Cx, El, GrowthSpec, ScrollSpec, Tx, col, el, icon, row, spacer, text};

use crate::icons;
use crate::model::{Entry, Filter, Page, State};
use crate::theme::{self, PAL};

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    // The type column hides below 1100 pt (design-spec §3).
    let wide = cx.layout_size("root").is_none_or(|[w, _]| w >= theme::NARROW_W);
    row()
        .key("root")
        .w_full()
        .h_full()
        .fill(PAL.bg)
        .child(sidebar(s))
        .child(el().w(1.0).h_full().fill(PAL.border))
        .child(main(s, wide))
}

/// No visible change comes from `poll` in stage 1: nothing ticks.
pub fn poll(_s: &mut State, _t: f64, _tx: &mut Tx) -> bool {
    false
}

// ------------------------------------------------------------------------------------ sidebar

fn sidebar(s: &State) -> El {
    let mut side = col().key("sidebar").w(theme::SIDEBAR_W).h_full().fill(PAL.side).pt(14.0).px(10.0).pb(12.0).child(brand(s));
    if let Some((name, service)) = &s.account {
        side = side.child(account(name, service));
    }
    side = side.child(nav_heading("My list"));
    for st in Status::ALL {
        let count = (!s.loading).then(|| s.count(st).to_string());
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
    let (dot, line) = if s.loading {
        (PAL.faint, "Syncing…".to_string())
    } else if s.offline {
        (PAL.warn(), format!("Offline · {} changes waiting", s.queue))
    } else {
        (PAL.ok(), s.synced.to_string())
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
        Page::List(st) => list_page(s, st, wide),
        Page::NowPlaying => placeholder("Now Playing", "Nothing is playing."),
        Page::Queue => placeholder("Sync queue", "The queue comes in a later stage."),
        Page::Settings => placeholder("Settings", "Settings come in a later stage."),
    };
    col().key("main").grow(1.0).min_w(0.0).h_full().child(page)
}

fn placeholder(title: &str, line: &str) -> El {
    col()
        .key(format!("page/{title}"))
        .w_full()
        .h_full()
        .child(top(title, None, false))
        .child(text(line.to_string()).font(theme::BODY).color(PAL.faint).px(theme::PAGE_PAD))
}

/// The page's top bar: title, count, and (on the list) search and sort.
fn top(title: &str, count: Option<usize>, tools: bool) -> El {
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
        t = t.child(spacer()).child(search_box()).child(sort_select());
    }
    t
}

fn search_box() -> El {
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
        .child(text("Search titles").font(theme::BODY).color(PAL.faint))
        .child(spacer())
        .child(
            el().px(5.0).py(1.0).radius(4.0).border(1.0, PAL.border_strong).child(text(kbd).font(rune::TextStyle::new(10.0, 500.0)).color(PAL.faint)),
        )
}

fn sort_select() -> El {
    row()
        .key("sort")
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .child(text("Sort").font(theme::BODY).color(PAL.faint))
        .child(text("Last updated").font(theme::BODY_MED).color(PAL.text))
        .child(icon(&icons::DOWN, 14.0).color(PAL.text))
}

fn list_page(s: &State, st: Status, wide: bool) -> El {
    let count = s.count(st);
    let page = col().key(format!("page/{}", st.slug())).w_full().h_full().child(top(st.label(), (!s.loading).then_some(count), true));
    if s.loading {
        return page.child(loading());
    }
    if count == 0 {
        return page.child(empty(st));
    }
    let rows = s.visible();
    page.child(chips(s, st))
        .child(header(wide))
        .child(
            col().key(format!("rows-scroll/{}", st.slug())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
                col().key("rows").growth(GrowthSpec::default()).virtual_rows(
                    rows.len(),
                    theme::ROW_H,
                    move |s: &State, i| {
                        // `i` is an index into the filtered rows; the key is the media id.
                        let v = s.visible();
                        v.get(i).map_or_else(|| format!("gone-{i}").into(), |&k| s.entries[k].id.to_string().into())
                    },
                    move |s: &State, i| {
                        let v = s.visible();
                        match v.get(i) {
                            Some(&k) => list_row(&s.entries[k], wide),
                            None => el(),
                        }
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
                s.filter = f;
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

fn header(wide: bool) -> El {
    let h = |s: &str, sorted: bool| text(s.to_uppercase()).font(theme::CAPTION).color(if sorted { PAL.dim } else { PAL.faint });
    col()
        .key("thead")
        .w_full()
        .child(
            columns(
                el().w(34.0),
                h("Title", false),
                h("Progress", true),
                h("Score", false),
                h("Type", false).key("thead/type"),
                row().gap(4.0).center_y().child(h("Updated", true)).child(icon(&icons::DOWN, 10.0).color(PAL.dim)),
                wide,
            )
            .h(30.0)
            .px(theme::PAGE_PAD),
        )
        .child(el().w_full().h(1.0).fill(PAL.border))
}

fn list_row(e: &Entry, wide: bool) -> El {
    let id = e.id;
    let status_c = PAL.status(e.status);

    let mut sub = row().gap(8.0).center_y();
    if e.airing.is_some() {
        sub = sub.child(tag("Airing", PAL.airing, true));
    }
    if e.rewatching {
        sub = sub.child(tag("Rewatching", PAL.st_completed, false));
    }
    if e.waiting.is_some() {
        sub = sub.child(tag("Waiting to sync", PAL.warn(), false));
    }
    let note = e.waiting.or(e.airing).unwrap_or("Finished airing");
    sub = sub.child(text(note).font(theme::LABEL).color(PAL.faint));
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
        .child(
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
                    // Optimistic and at once (design-spec §6): no counting animation.
                    tx.instant();
                    s.plus_one(id);
                })
                .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent)),
        );
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
                cover(e.id, &e.title, 34.0, 48.0, 12.0),
                title,
                progress,
                score,
                text(e.kind).font(theme::LABEL).color(PAL.dim),
                text(e.updated).font(theme::LABEL).color(PAL.dim),
                wide,
            )
            .grow(1.0)
            .px(theme::PAGE_PAD),
        )
        .child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
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
