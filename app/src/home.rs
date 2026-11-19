//! Home: the list at a glance, in shelves. Continue watching (wide cards: the next episode's +1,
//! and Play when the file is in the library folders), with Manga on Continue reading (the same
//! cards for manga: +1 is the next chapter, or volume), Airing this week (the next episode of each
//! airing show on the list, soonest first, with its day and time), and Plan to watch. Each shelf scrolls
//! sideways and links to its full page ("See all").
//!
//! Only reads `State`: the order is made here from at most a few hundred entries, which costs
//! less than one list row's layout. Cards keep one size each, so covers stay at display size.

use kotora_core::Status;
use rune::{El, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::details::Stub;
use crate::icons;
use crate::library::next_for;
use crate::look;
use crate::model::{Entry, Page, State};
use crate::theme::{self, PAL};
use kotora_core::library::Next;

/// Cards a shelf holds at most; "See all" opens the rest.
const SHELF_MAX: usize = 20;
const WIDE_W: f32 = 320.0;
const WIDE_H: f32 = 132.0;
const SMALL_W: f32 = 116.0;
const SMALL_H: f32 = 174.0;
/// Title box and the line under the poster.
const SMALL_TITLE_H: f32 = 34.0;
const SMALL_LINE_H: f32 = 16.0;
const SMALL_GAP: f32 = 6.0;
/// A poster card's whole height (the shelf's height: a shorter shelf clips the last line's
/// descenders, the "p" of "Ep").
const SMALL_CARD_H: f32 = SMALL_H + SMALL_GAP + SMALL_TITLE_H + SMALL_GAP + SMALL_LINE_H;

/// Continue watching: Watching (and rewatches) with a next episode, last updated first.
fn continuing(s: &State) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = s
        .entries
        .iter()
        .filter(|e| e.on_page(Status::Watching) && e.episodes.is_none_or(|n| e.progress < n))
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    v
}

/// Airing this week: the next episode of each airing show on the list (not dropped) within
/// seven days, soonest first: (entry, episode, unix time).
fn airing_week(s: &State) -> Vec<(&Entry, u32, i64)> {
    let now = s.clock();
    let mut v: Vec<(&Entry, u32, i64)> = s
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                Status::Watching | Status::PlanToWatch | Status::OnHold
            ) || e.rewatching
        })
        .filter_map(|e| {
            let (ep, at) = e.timetable?.with_progress(e.progress, now).next(now)?;
            (at - now < kotora_core::airing::WEEK).then_some((e, ep, at))
        })
        .collect();
    v.sort_by_key(|x| x.2);
    v
}

/// Continue reading: Reading (and rereads) with a next chapter or volume, last updated first.
fn reading(s: &State) -> Vec<&crate::manga::MEntry> {
    let mut v: Vec<&crate::manga::MEntry> = s
        .manga
        .entries
        .iter()
        .filter(|e| e.on_page(Status::Watching) && !e.at_end(e.plus_unit()))
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    v
}

/// A Continue reading card: as Continue watching's, `+1` for the next chapter (or volume), the
/// counts by the agreed rule; a press opens the manga's page, a right-click its menu.
fn manga_card(s: &State, e: &crate::manga::MEntry) -> El {
    use crate::manga::{Unit, cover_id};
    let id = e.id;
    let cid = cover_id(id);
    let (word, next) = match e.plus_unit() {
        Unit::Chapters => ("Ch", e.chapters + 1),
        Unit::Volumes => ("Vol", e.volumes + 1),
    };
    let st = if e.rereading {
        PAL.st_completed
    } else {
        PAL.status(e.status)
    };
    let plus = row()
        .key("plus")
        .role("button")
        .label(format!(
            "+1, {} {next}",
            if word == "Ch" { "chapter" } else { "volume" }
        ))
        .focusable(true)
        .h(28.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.bg.alpha(0.7))
        .border(1.0, PAL.border_strong)
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Err(m) = s.manga_plus_one(id) {
                s.error = Some(m);
            }
        })
        .child(
            text(format!("+1  {word} {next}"))
                .font(theme::LABEL_BTN)
                .color(PAL.text)
                .group_color(PAL.on_accent),
        );
    let stub = crate::manga::stub(e);
    el().key(format!("read/{id}"))
        .role("button")
        .label(format!("About {}", e.title))
        .size(WIDE_W, WIDE_H)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .cursor(rune::Cursor::Pointer)
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_manga_ctx(id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_details(stub.clone());
        })
        .child(look::backdrop(
            s,
            cid,
            e.cover_url.as_deref(),
            look::card_scrim(),
        ))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(10.0)
                .gap(12.0)
                .child(look::poster(
                    s,
                    cid,
                    &e.title,
                    e.cover_url.as_deref(),
                    74.0,
                    111.0,
                    theme::R_SM,
                ))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .py(2.0)
                        .child(
                            text(e.title.clone())
                                .font(TextStyle::new(14.0, 600.0))
                                .color(PAL.text)
                                .wrap()
                                .max_lines(2),
                        )
                        .child(
                            text(crate::mgrid::progress_text(e))
                                .font(theme::LABEL_NUM)
                                .color(PAL.dim)
                                .truncate(),
                        )
                        .child(spacer())
                        .child(
                            row()
                                .gap(8.0)
                                .center_y()
                                .child(col().grow(1.0).child(look::bar(
                                    e.fraction(),
                                    st,
                                    4.0,
                                    PAL.text.alpha(0.16),
                                )))
                                .child(plus),
                        ),
                ),
        )
}

fn plan(s: &State) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = s
        .entries
        .iter()
        .filter(|e| e.status == Status::PlanToWatch)
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    v
}

pub fn page(s: &State) -> El {
    let cont = continuing(s);
    let air = airing_week(s);
    let plan = plan(s);
    let mut body = col().gap(26.0).pt(4.0).pb(theme::PAGE_PAD);
    body = body.child(shelf(
        "continue",
        "Continue watching",
        cont.len(),
        Some(Page::List(Status::Watching)),
        WIDE_H,
        cont.iter()
            .take(SHELF_MAX)
            .map(|e| wide_card(s, e))
            .collect(),
        "Nothing to continue: start something from Plan to watch.",
    ));
    // Manga on: what is being read, the same way (nothing when Manga is off).
    if s.manga.enabled {
        let reading = reading(s);
        body = body.child(shelf(
            "reading",
            "Continue reading",
            reading.len(),
            Some(Page::Manga(Status::Watching)),
            WIDE_H,
            reading
                .iter()
                .take(SHELF_MAX)
                .map(|e| manga_card(s, e))
                .collect(),
            "Nothing to continue: start something from Plan to read.",
        ));
    }
    let now = s.clock();
    let today = s.airing.clock(now).day;
    body = body.child(shelf(
        "airing",
        "Airing this week",
        air.len(),
        Some(Page::Airing),
        SMALL_CARD_H,
        air.iter()
            .take(SHELF_MAX)
            .map(|(e, ep, at)| {
                small_card(
                    s,
                    e,
                    Some((format!("Ep {ep}"), s.airing.clock(*at).day == today)),
                    s.airing.when(*at, now),
                )
            })
            .collect(),
        "None of your shows airs in the next seven days.",
    ));
    body = body.child(shelf(
        "plan",
        "Plan to watch",
        plan.len(),
        Some(Page::List(Status::PlanToWatch)),
        SMALL_CARD_H,
        plan.iter()
            .take(SHELF_MAX)
            .map(|e| {
                small_card(
                    s,
                    e,
                    None,
                    e.episodes.map_or_else(
                        || e.kind.to_string(),
                        |n| format!("{} \u{b7} {n} ep", e.kind),
                    ),
                )
            })
            .collect(),
        "Your Plan to watch list is empty.",
    ));
    col()
        .key("page/home")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Home", None, false))
        .child(
            col()
                .key("home-scroll")
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0)
                .child(body),
        )
}

/// A shelf: heading with its count and "See all", then the cards in a row that scrolls sideways.
#[allow(clippy::too_many_arguments)]
fn shelf(
    key: &str,
    title: &str,
    n: usize,
    all: Option<Page>,
    h: f32,
    cards: Vec<El>,
    none: &str,
) -> El {
    let mut head = row()
        .px(theme::PAGE_PAD)
        .pb(10.0)
        .gap(8.0)
        .items_end()
        .child(
            text(title.to_string())
                .font(TextStyle::new(16.0, 600.0))
                .color(PAL.text),
        )
        .child(
            text(n.to_string())
                .font(theme::NUM)
                .color(PAL.faint)
                .pb(1.0),
        )
        .child(spacer());
    if let Some(page) = all {
        head = head.child(
            row()
                .key(format!("home/{key}/all"))
                .role("link")
                .label(format!("See all: {title}"))
                .focusable(true)
                .h(26.0)
                .px(8.0)
                .gap(4.0)
                .center_y()
                .radius(theme::R_SM)
                .hover(PAL.hover())
                .press(PAL.press())
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(move |s: &mut State, tx: &mut Tx| crate::ui::go_to(s, page, tx))
                .child(text("See all").font(theme::LABEL_BTN).color(PAL.dim))
                .child(icon(&icons::ARROW, 13.0).color(PAL.dim)),
        );
    }
    let body = if cards.is_empty() {
        row()
            .px(theme::PAGE_PAD)
            .child(text(none.to_string()).font(theme::LABEL).color(PAL.faint))
    } else {
        let mut r = row().gap(14.0).items_start().px(theme::PAGE_PAD);
        for c in cards {
            r = r.child(c);
        }
        el().key(format!("home/{key}/scroll"))
            .w_full()
            .h(h)
            .scroll_x()
            .child(r)
    };
    col().key(format!("home/{key}")).child(head).child(body)
}

/// Continue watching: the cover's tint, the poster, the title, how far, +1 (and Play).
fn wide_card(s: &State, e: &Entry) -> El {
    let id = e.id;
    let next = e.progress + 1;
    let mut sub = format!(
        "Ep {} / {}",
        e.progress,
        e.episodes.map_or("?".to_string(), |n| n.to_string())
    );
    if let Some(n) = e.airing_note {
        sub = format!("{sub} \u{b7} {n}");
    }
    // Episodes out and not watched ("~3 to watch", "Caught up").
    if let Some(w) = s.to_watch(e) {
        sub = format!("{sub} \u{b7} {}", w.label());
    }
    let st = if e.rewatching {
        PAL.st_completed
    } else {
        PAL.status(e.status)
    };
    let plus = row()
        .key("plus")
        .role("button")
        .label(format!("+1, episode {next}"))
        .focusable(true)
        .h(28.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.bg.alpha(0.7))
        .border(1.0, PAL.border_strong)
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Err(m) = s.plus_one_or_ask(id) {
                s.error = Some(m);
            }
        })
        .child(
            text(format!("+1  Ep {next}"))
                .font(theme::LABEL_BTN)
                .color(PAL.text)
                .group_color(PAL.on_accent),
        );
    let play = match next_for(s, e) {
        Some(Next::Ready { episode, .. }) => el()
            .key("play")
            .role("button")
            .label(format!("Play episode {episode}"))
            .focusable(true)
            .size(28.0, 28.0)
            .radius(14.0)
            .center()
            .fill(PAL.accent)
            .press(PAL.accent_lo)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                crate::library::press_play(s, id);
            })
            .child(icon(&icons::PLAY, 18.0).color(PAL.on_accent)),
        _ => el(),
    };
    let stub = Stub::from_entry(e);
    let e_id = id;
    el().key(format!("cont/{id}"))
        .role("button")
        .label(format!("About {}", e.title))
        .size(WIDE_W, WIDE_H)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .cursor(rune::Cursor::Pointer)
        // The middle button plays the next episode, as on the list.
        .pointer_buttons([rune::PointerButton::Middle])
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_ctx(e_id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if tx.button() == rune::PointerButton::Middle {
                return s.play_entry(stub.id);
            }
            s.open_details(stub.clone());
        })
        .child(look::backdrop(
            s,
            id,
            e.cover_url.as_deref(),
            look::card_scrim(),
        ))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(10.0)
                .gap(12.0)
                .child(look::poster(
                    s,
                    id,
                    &e.title,
                    e.cover_url.as_deref(),
                    74.0,
                    111.0,
                    theme::R_SM,
                ))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .py(2.0)
                        .child(
                            text(e.title.clone())
                                .font(TextStyle::new(14.0, 600.0))
                                .color(PAL.text)
                                .wrap()
                                .max_lines(2),
                        )
                        .child(text(sub).font(theme::LABEL_NUM).color(PAL.dim).truncate())
                        .child(spacer())
                        .child(
                            row()
                                .gap(8.0)
                                .center_y()
                                .child(col().grow(1.0).child(look::bar(
                                    e.fraction(),
                                    st,
                                    4.0,
                                    PAL.text.alpha(0.16),
                                )))
                                .child(play)
                                .child(plus),
                        ),
                ),
        )
}

/// A poster card (Airing this week, Plan to watch), with an optional chip on the poster (the
/// episode that airs; filled on today's).
fn small_card(s: &State, e: &Entry, chip: Option<(String, bool)>, line: String) -> El {
    let mut p = look::poster(
        s,
        e.id,
        &e.title,
        e.cover_url.as_deref(),
        SMALL_W,
        SMALL_H,
        theme::R_MD,
    );
    if let Some((d, today)) = chip {
        let chip = row()
            .absolute(6.0, 6.0)
            .h(20.0)
            .px(8.0)
            .center_y()
            .radius(10.0);
        p = p.child(if today {
            chip.fill(PAL.airing)
                .child(text(d).font(theme::BADGE).color(PAL.bg))
        } else {
            chip.fill(PAL.bg.alpha(0.86))
                .child(text(d).font(theme::BADGE).color(PAL.text))
        });
    }
    let stub = Stub::from_entry(e);
    let e_id = e.id;
    col()
        .key(format!("sc/{}", e.id))
        .role("button")
        .label(format!("About {}", e.title))
        .w(SMALL_W)
        .gap(SMALL_GAP)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        // The middle button plays the next episode, as on the list.
        .pointer_buttons([rune::PointerButton::Middle])
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_ctx(e_id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if tx.button() == rune::PointerButton::Middle {
                return s.play_entry(stub.id);
            }
            s.open_details(stub.clone());
        })
        .child(p)
        .child(
            col().h(SMALL_TITLE_H).child(
                text(e.title.clone())
                    .font(TextStyle::new(13.0, 500.0))
                    .color(PAL.text)
                    .group_color(PAL.accent_hi)
                    .wrap()
                    .max_lines(2),
            ),
        )
        .child(
            col()
                .h(SMALL_LINE_H)
                .child(text(line).font(theme::LABEL_NUM).color(PAL.dim).truncate()),
        )
}
