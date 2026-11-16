//! Home: the list at a glance, in shelves. Continue watching (wide cards: the next episode's +1,
//! and Play when the file is in the library folders), Airing this week (the next episode of each
//! airing show on the list, soonest first, with its day), and Plan to watch. Each shelf scrolls
//! sideways and links to its full page ("See all").
//!
//! Only reads `State`: the order is made here from at most a few hundred entries, which costs
//! less than one list row's layout. Cards keep one size each, so covers stay at display size.

use kotora_core::Status;
use rune::{El, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::details::Stub;
use crate::icons;
use crate::library::next_for;
use kotora_core::library::Next;
use crate::look;
use crate::model::{Entry, Page, State};
use crate::theme::{self, PAL};

/// Cards a shelf holds at most; "See all" opens the rest.
const SHELF_MAX: usize = 20;
const WIDE_W: f32 = 320.0;
const WIDE_H: f32 = 132.0;
const SMALL_W: f32 = 116.0;
const SMALL_H: f32 = 174.0;

/// Continue watching: Watching (and rewatches) with a next episode, last updated first.
fn continuing(s: &State) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = s.entries.iter().filter(|e| e.on_page(Status::Watching) && e.episodes.is_none_or(|n| e.progress < n)).collect();
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
        .filter(|e| matches!(e.status, Status::Watching | Status::PlanToWatch | Status::OnHold) || e.rewatching)
        .filter_map(|e| {
            let (ep, at) = e.timetable?.with_progress(e.progress, now).next(now)?;
            (at - now < kotora_core::airing::WEEK).then_some((e, ep, at))
        })
        .collect();
    v.sort_by_key(|x| x.2);
    v
}

fn plan(s: &State) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = s.entries.iter().filter(|e| e.status == Status::PlanToWatch).collect();
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
        cont.iter().take(SHELF_MAX).map(|e| wide_card(s, e)).collect(),
        "Nothing to continue: start something from Plan to watch.",
    ));
    let today = s.airing.clock(s.clock()).day;
    body = body.child(shelf(
        "airing",
        "Airing this week",
        air.len(),
        Some(Page::Airing),
        SMALL_H + 6.0 + 34.0 + 18.0,
        air.iter()
            .take(SHELF_MAX)
            .map(|(e, ep, at)| {
                let c = s.airing.clock(*at);
                let day = match c.day - today {
                    0 => "Today".to_string(),
                    1 => "Tomorrow".to_string(),
                    _ => kotora_core::airing::DAY_SHORT[c.weekday as usize].to_string(),
                };
                let time = format!("{:02}:{:02}", c.minute / 60, c.minute % 60);
                small_card(s, e, Some((day, c.day == today)), format!("Ep {ep} \u{b7} {time}"))
            })
            .collect(),
        "None of your shows airs in the next seven days.",
    ));
    body = body.child(shelf(
        "plan",
        "Plan to watch",
        plan.len(),
        Some(Page::List(Status::PlanToWatch)),
        SMALL_H + 6.0 + 34.0 + 18.0,
        plan.iter().take(SHELF_MAX).map(|e| small_card(s, e, None, e.episodes.map_or_else(|| e.kind.to_string(), |n| format!("{} \u{b7} {n} ep", e.kind)))).collect(),
        "Your Plan to watch list is empty.",
    ));
    col()
        .key("page/home")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Home", None, false))
        .child(col().key("home-scroll").scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(body))
}

/// A shelf: heading with its count and "See all", then the cards in a row that scrolls sideways.
#[allow(clippy::too_many_arguments)]
fn shelf(key: &str, title: &str, n: usize, all: Option<Page>, h: f32, cards: Vec<El>, none: &str) -> El {
    let mut head = row()
        .px(theme::PAGE_PAD)
        .pb(10.0)
        .gap(8.0)
        .items_end()
        .child(text(title.to_string()).font(TextStyle::new(16.0, 600.0)).color(PAL.text))
        .child(text(n.to_string()).font(theme::NUM).color(PAL.faint).pb(1.0))
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
        row().px(theme::PAGE_PAD).child(text(none.to_string()).font(theme::LABEL).color(PAL.faint))
    } else {
        let mut r = row().gap(14.0).items_start().px(theme::PAGE_PAD);
        for c in cards {
            r = r.child(c);
        }
        el().key(format!("home/{key}/scroll")).w_full().h(h).scroll_x().child(r)
    };
    col().key(format!("home/{key}")).child(head).child(body)
}

/// Continue watching: the cover's tint, the poster, the title, how far, +1 (and Play).
fn wide_card(s: &State, e: &Entry) -> El {
    let id = e.id;
    let next = e.progress + 1;
    let mut sub = format!("Ep {} / {}", e.progress, e.episodes.map_or("?".to_string(), |n| n.to_string()));
    if let Some(n) = e.airing_note {
        sub = format!("{sub} \u{b7} {n}");
    }
    let st = if e.rewatching { PAL.st_completed } else { PAL.status(e.status) };
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
        .child(text(format!("+1  Ep {next}")).font(theme::LABEL_BTN).color(PAL.text).group_color(PAL.on_accent));
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
    el().key(format!("cont/{id}"))
        .role("button")
        .label(format!("About {}", e.title))
        .size(WIDE_W, WIDE_H)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .border(1.0, PAL.border)
        .hover(PAL.hover())
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_details(stub.clone());
        })
        .child(look::backdrop(s, id, e.cover_url.as_deref(), look::card_scrim()))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .padding(10.0)
                .gap(12.0)
                .child(look::poster(s, id, &e.title, e.cover_url.as_deref(), 74.0, 111.0, theme::R_SM))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .py(2.0)
                        .child(text(e.title.clone()).font(TextStyle::new(14.0, 600.0)).color(PAL.text).wrap().max_lines(2))
                        .child(text(sub).font(theme::LABEL_NUM).color(PAL.dim).truncate())
                        .child(spacer())
                        .child(row().gap(8.0).center_y().child(col().grow(1.0).child(look::bar(e.fraction(), st, 4.0, PAL.text.alpha(0.16)))).child(play).child(plus)),
                ),
        )
}

/// A poster card (Airing this week, Plan to watch), with an optional day chip on the poster.
fn small_card(s: &State, e: &Entry, day: Option<(String, bool)>, line: String) -> El {
    let mut p = look::poster(s, e.id, &e.title, e.cover_url.as_deref(), SMALL_W, SMALL_H, theme::R_MD);
    if let Some((d, today)) = day {
        let chip = row().absolute(6.0, 6.0).h(20.0).px(8.0).center_y().radius(10.0);
        p = p.child(if today {
            chip.fill(PAL.airing).child(text(d).font(theme::BADGE).color(PAL.bg))
        } else {
            chip.fill(PAL.bg.alpha(0.86)).child(text(d).font(theme::BADGE).color(PAL.text))
        });
    }
    let stub = Stub::from_entry(e);
    col()
        .key(format!("sc/{}", e.id))
        .role("button")
        .label(format!("About {}", e.title))
        .w(SMALL_W)
        .gap(6.0)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_details(stub.clone());
        })
        .child(p)
        .child(col().h(34.0).child(text(e.title.clone()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2)))
        .child(text(line).font(theme::LABEL_NUM).color(PAL.dim).truncate())
}
