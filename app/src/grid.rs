//! The list pages' other two views: a dense grid of posters (badges on the poster, title,
//! "Ep x / y" and the score under it; edit and +1 on hover) and large cards (bigger posters, a
//! progress bar and +1 on each card). The Watching page keeps +1 visible on every poster. The
//! switch sits in the list's toolbar and each list keeps its own view (`[views]` in config.toml).
//!
//! - **Size:** posters have one size per view ([`DENSE`], [`LARGE`]), so covers stay at display
//!   size (`art.rs`) whatever the window; spare width goes into the gaps.
//! - **Performance:** a virtual list of rows, each row a line of cards (Rune has no virtual grid):
//!   only the rows on screen and two more each way are built.
//! - **Keyboard:** the arrows move a selection (outlined), Enter opens its details, Escape drops
//!   it. The list's own shortcuts (Ctrl F, Ctrl K, Ctrl , and Ctrl or Alt 1 to 5, `[keys] pages`) are as
//!   in the table.

use std::cell::Cell;

use kotora_core::Status;
use kotora_core::config::ListView;
use rune::{El, GrowthSpec, Icon, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::details::Stub;
use crate::icons;
use crate::look;
use crate::model::{Air, Entry, Page, State};
use crate::theme::{self, PAL};

/// A view's card: poster width and height, the least gap, and the gap's most before cards
/// line up left instead of spreading.
#[derive(Clone, Copy)]
pub(crate) struct Size {
    pub(crate) w: f32,
    pub(crate) poster_h: f32,
    gap: f32,
    max_gap: f32,
    /// The whole card's height with the space under it (a virtual row's height).
    pub(crate) row_h: f32,
}

/// Grid: 128 x 192 posters.
pub(crate) const DENSE: Size = Size { w: 128.0, poster_h: 192.0, gap: 16.0, max_gap: 36.0, row_h: 192.0 + 6.0 + 34.0 + 4.0 + 16.0 + 24.0 };
/// Large: 184 x 262 posters on a card with a progress bar and +1.
pub(crate) const LARGE: Size = Size { w: 184.0, poster_h: 262.0, gap: 20.0, max_gap: 44.0, row_h: 262.0 + 12.0 + 38.0 + 10.0 + 5.0 + 10.0 + 26.0 + 12.0 + 2.0 + 20.0 };

pub(crate) fn size(v: ListView) -> Size {
    if v == ListView::Large { LARGE } else { DENSE }
}

/// The grid's selection (keyboard) and the cards per row it was last drawn with.
#[derive(Default)]
pub struct Grid {
    /// The selected entry's media id.
    pub sel: Option<u32>,
    /// Cards per row at the last draw (the arrows need it; set by the view).
    pub(crate) per_row: Cell<usize>,
}

/// Cards per row in `width` (the main area's), and the gap between them.
pub(crate) fn fit(width: f32, z: Size) -> (usize, f32) {
    let avail = width - 2.0 * theme::PAGE_PAD;
    let n = (((avail + z.gap) / (z.w + z.gap)).floor() as usize).max(1);
    let gap = if n > 1 { ((avail - n as f32 * z.w) / (n - 1) as f32).clamp(z.gap, z.max_gap) } else { z.gap };
    (n, gap)
}

impl State {
    /// The view of list page `st`.
    pub fn view_of(&self, st: Status) -> ListView {
        self.prefs.views.get(st)
    }

    /// The switch in the toolbar: this list's view changes and `config.toml` keeps it.
    pub fn set_view(&mut self, st: Status, v: ListView) {
        if self.view_of(st) == v {
            return;
        }
        self.prefs.views.set(st, v);
        self.grid.sel = None;
        crate::detect::save_setting(self, "views", kotora_core::config::Views::key(st), kotora_core::config::quote(v.key()));
    }

    /// The grid's keys: arrows move the selection, Enter opens it, Escape drops it. Returns
    /// whether the key was used.
    pub fn grid_key(&mut self, k: &KeyEvent, tx: &mut Tx) -> bool {
        let Page::List(st) = self.page else { return false };
        if self.view_of(st) == ListView::List || self.searching() || self.rows.is_empty() || k.mods.cmd || k.mods.alt || k.mods.ctrl {
            return false;
        }
        let per = self.grid.per_row.get().max(1);
        let n = self.rows.len();
        let at = self.grid.sel.and_then(|id| self.rows.iter().position(|&k| self.entries[k].id == id));
        let next = match (&k.key, at) {
            (Key::Escape, Some(_)) => {
                self.grid.sel = None;
                return true;
            }
            (Key::Enter, Some(i)) => {
                tx.instant();
                let e = &self.entries[self.rows[i]];
                self.open_details(Stub::from_entry(e));
                return true;
            }
            // + and - on the selected card: one episode more or less (as +1 and the editor).
            (Key::Text(t), Some(i)) if matches!(t.as_str(), "+" | "=" | "-") => {
                tx.instant();
                let id = self.entries[self.rows[i]].id;
                let r = if t.as_str() == "-" {
                    match self.entries.iter().find(|e| e.id == id).map(|e| e.progress) {
                        Some(p) if p > 0 => self.edit(id, kotora_core::Change { progress: Some(p - 1), ..Default::default() }).map(|_| ()),
                        _ => Ok(()),
                    }
                } else {
                    self.plus_one_or_ask(id)
                };
                if let Err(m) = r {
                    self.error = Some(m);
                }
                return true;
            }
            (Key::Right | Key::Left | Key::Up | Key::Down, None) => 0,
            (Key::Right, Some(i)) => (i + 1).min(n - 1),
            (Key::Left, Some(i)) => i.saturating_sub(1),
            (Key::Down, Some(i)) => (i + per).min(n - 1),
            (Key::Up, Some(i)) => i.checked_sub(per).unwrap_or(i),
            _ => return false,
        };
        self.grid.sel = Some(self.entries[self.rows[next]].id);
        tx.scroll_into_view(row_key(next / per));
        true
    }
}

/// A grid row's key: its index. A resize changes how many cards a row holds, not which rows
/// there are, so the rows stay and only their cards move (at once, see `ui::main`).
pub(crate) fn row_key(r: usize) -> String {
    format!("cards/{r}")
}

/// The toolbar's segmented switch: List, Grid, Large (icons only on narrow windows).
pub fn switcher(s: &State, st: Status, labels: bool) -> El {
    // A manga page keeps its own views (`[manga_views]`).
    let manga = matches!(s.page, Page::Manga(_));
    let now = if manga { s.manga_view_of(st) } else { s.view_of(st) };
    let mut r = row().key("views").h(32.0).padding(2.0).gap(2.0).center_y().radius(theme::R_SM).fill(PAL.field).border(1.0, PAL.border);
    for v in ListView::ALL {
        let on = v == now;
        let ic: &'static Icon = match v {
            ListView::List => &icons::VIEW_LIST,
            ListView::Grid => &icons::VIEW_GRID,
            ListView::Large => &icons::VIEW_LARGE,
        };
        let seg = row()
            .key(format!("view/{}", v.key()))
            .role("button")
            .label(format!("{} view", v.label()))
            .focusable(true)
            .h(26.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(theme::R_XS)
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                if manga { s.set_manga_view(st, v) } else { s.set_view(st, v) }
            })
            .child(icon(ic, 15.0).color(if on { PAL.accent_hi } else { PAL.dim }))
            .child(if labels { text(v.label()).font(theme::LABEL_BTN).color(if on { PAL.text } else { PAL.dim }) } else { el() });
        r = r.child(if on { seg.fill(PAL.raised).border(1.0, PAL.border_strong) } else { seg });
    }
    r
}

/// The cards of list page `st` (its rows after the chip and the sort), `width` the main area's.
pub fn cards(s: &State, st: Status, width: f32) -> El {
    let v = s.view_of(st);
    let z = size(v);
    let (per, gap) = fit(width, z);
    s.grid.per_row.set(per);
    let rows = s.rows.len().div_ceil(per);
    col().key(format!("grid-scroll/{}", st.slug())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
        col().key("grid").growth(GrowthSpec::default()).virtual_rows(
            rows,
            z.row_h,
            move |_: &State, r| row_key(r).into(),
            move |s: &State, r| {
                let mut line = row().w_full().h(z.row_h).px(theme::PAGE_PAD).pt(if r == 0 { 4.0 } else { 0.0 }).gap(gap).items_start();
                for &k in s.rows.iter().skip(r * per).take(per) {
                    let e = &s.entries[k];
                    line = line.child(if v == ListView::Large { card_large(s, e, st) } else { card_dense(s, e, st) });
                }
                line
            },
        ),
    )
}

// ---------------------------------------------------------------------------------- pieces

/// A pill on a poster: the page colour, nearly opaque, so it reads on any cover in both themes.
pub(crate) fn badge(label: &str, dot: Option<rune::Color>) -> El {
    let mut b = row().h(20.0).px(7.0).gap(5.0).center_y().radius(10.0).fill(PAL.bg.alpha(0.86));
    if let Some(c) = dot {
        b = b.child(el().size(6.0, 6.0).radius(3.0).fill(c));
    }
    b.child(text(label.to_string()).font(theme::BADGE).color(PAL.text))
}

/// The badges at a poster's top-left: airing slot or rewatch, a new episode, a change waiting.
fn badges(e: &Entry) -> El {
    let mut c = col().key("badges").absolute(6.0, 6.0).gap(4.0).items_start();
    if e.rewatching {
        c = c.child(badge("Rewatching", Some(PAL.st_completed)));
    } else if e.air == Air::Airing {
        c = c.child(badge(e.airing_note.unwrap_or("Airing"), Some(PAL.airing)));
    }
    if let Some(n) = e.new_episode {
        c = c.child(row().h(20.0).px(7.0).center_y().radius(10.0).fill(PAL.accent).child(text(format!("Ep {n} out")).font(theme::BADGE).color(PAL.on_accent)));
    }
    if let Some(w) = e.waiting {
        c = c.child(badge(w, Some(PAL.warn())));
    }
    c
}

fn ep_text(e: &Entry) -> String {
    format!("Ep {} / {}", e.progress, e.episodes.map_or("?".to_string(), |n| n.to_string()))
}

fn score(s: &State, e: &Entry) -> El {
    match e.score {
        Some(v) => crate::scores::poster_mark(s.score_format(), v),
        None => el(),
    }
}

fn status_color(e: &Entry) -> rune::Color {
    if e.rewatching { PAL.st_completed } else { PAL.status(e.status) }
}

/// Whether +1 applies (a next episode exists).
fn has_next(s: &State, e: &Entry) -> bool {
    s.has_list() && e.episodes.is_none_or(|n| e.progress < n)
}

/// `+1` on a poster: a pill of the page colour, orange under the pointer.
fn plus_on_poster(id: u32) -> El {
    el().key("plus")
        .role("button")
        .label("+1")
        .focusable(true)
        .h(28.0)
        .px(11.0)
        .radius(14.0)
        .fill(PAL.bg.alpha(0.88))
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Err(m) = s.plus_one_or_ask(id) {
                s.error = Some(m);
            }
        })
        .child(text("+1").font(TextStyle::new(13.0, 600.0)).color(PAL.text).group_color(PAL.on_accent))
}

/// Edit: the details page with the editor open.
fn edit_on_poster(id: u32) -> El {
    el().key("edit")
        .role("button")
        .label("Edit")
        .focusable(true)
        .size(28.0, 28.0)
        .radius(14.0)
        .fill(PAL.bg.alpha(0.88))
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Some(e) = s.entries.iter().find(|e| e.id == id) {
                let stub = Stub::from_entry(e);
                s.open_details(stub);
                s.toggle_editor(id);
            }
        })
        .child(icon(&icons::PENCIL, 15.0).color(PAL.text))
}

/// What shows on a poster under the pointer: a shade at the bottom, edit, and +1 unless it is
/// always there.
fn hover_layer(s: &State, e: &Entry, plus: bool) -> El {
    let shade = PAL.bg;
    col()
        .key("hover")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .linear_gradient(std::f32::consts::PI, [(0.0, shade.alpha(0.0)), (0.55, shade.alpha(0.0)), (1.0, shade.alpha(0.75))])
        .group_opacity(0.0, 1.0)
        .padding(8.0)
        .child(spacer())
        .child(row().gap(6.0).center_y().child(edit_on_poster(e.id)).child(spacer()).child(if plus && has_next(s, e) { plus_on_poster(e.id) } else { el() }))
}

/// The card as a whole opens the details page (its buttons keep their own presses); the
/// middle button plays the next episode (see `card_dense`/`card_large`: `pointer_buttons`).
fn open(e: &Entry) -> impl Fn(&mut State, &mut Tx) + 'static {
    let stub = Stub::from_entry(e);
    move |s: &mut State, tx: &mut Tx| {
        tx.instant();
        s.grid.sel = Some(stub.id);
        if tx.button() == rune::PointerButton::Middle {
            return s.play_entry(stub.id);
        }
        s.open_details(stub.clone());
    }
}

// ------------------------------------------------------------------------------------ cards

/// Grid: the poster with its badges and a progress strip, title and "Ep x / y · score" under.
fn card_dense(s: &State, e: &Entry, st: Status) -> El {
    let e_id = e.id;
    let z = DENSE;
    let always = st == Status::Watching && has_next(s, e);
    let selected = s.grid.sel == Some(e.id);
    let mut p = look::poster(s, e.id, &e.title, e.cover_url.as_deref(), z.w, z.poster_h, theme::R_MD)
        .clip_rounded(theme::R_MD)
        .child(badges(e))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(4.0).fill(PAL.bg.alpha(0.7)).child(el().w(z.w * e.fraction()).h_full().fill(status_color(e))))
        .child(hover_layer(s, e, !always));
    if always {
        p = p.child(el().absolute(z.w - 8.0 - 44.0, z.poster_h - 8.0 - 28.0 - 2.0).w(44.0).child(plus_on_poster(e.id)));
    }
    // The outline sits on a wrapper: the poster clips its own children, the outline too.
    let p = if selected { el().radius(theme::R_MD).outline(2.0, 2.0, PAL.accent).child(p) } else { p };
    col()
        .key(format!("card/{}", e.id))
        .role("button")
        .label(format!("About {}", e.title))
        .w(z.w)
        .gap(6.0)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .pointer_buttons([rune::PointerButton::Middle])
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_ctx(e_id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(open(e))
        .child(p)
        .child(col().h(34.0).child(text(e.title.clone()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2)))
        .child(row().h(16.0).center_y().child(text(ep_text(e)).font(theme::LABEL_NUM).color(PAL.dim)).child(spacer()).child(score(s, e)))
}

/// Large: a card with the poster, two lines of title, a progress bar, "Ep x / y", the score and +1.
fn card_large(s: &State, e: &Entry, _st: Status) -> El {
    let e_id = e.id;
    let z = LARGE;
    let selected = s.grid.sel == Some(e.id);
    let p = look::poster(s, e.id, &e.title, e.cover_url.as_deref(), z.w, z.poster_h, 0.0).child(badges(e)).child(hover_layer(s, e, false));
    let plus = if has_next(s, e) { crate::ui::plus_button(e.id) } else { el() };
    let c = col()
        .key(format!("card/{}", e.id))
        .role("button")
        .label(format!("About {}", e.title))
        .w(z.w)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .pointer_buttons([rune::PointerButton::Middle])
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_ctx(e_id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(open(e))
        .child(p)
        .child(
            col()
                .padding(12.0)
                .gap(10.0)
                .child(col().h(38.0).child(text(e.title.clone()).font(TextStyle::new(14.0, 600.0)).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2)))
                .child(look::bar(e.fraction(), status_color(e), 5.0, PAL.border))
                .child(row().h(26.0).center_y().gap(8.0).child(text(ep_text(e)).font(theme::NUM_MED).color(PAL.dim)).child(score(s, e)).child(spacer()).child(plus)),
        );
    if selected { el().key(format!("sel/{}", e.id)).radius(theme::R_LG).outline(2.0, 2.0, PAL.accent).child(c) } else { c }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_fit_the_width_and_keep_their_size() {
        let (n, gap) = fit(1056.0, DENSE);
        assert!(n >= 6 && (DENSE.gap..=DENSE.max_gap).contains(&gap), "{n} {gap}");
        assert!(n as f32 * DENSE.w + (n - 1) as f32 * gap <= 1056.0 - 2.0 * theme::PAGE_PAD + 0.5);
        let (n, _) = fit(735.0, LARGE);
        assert!(n >= 3, "{n}");
        assert_eq!(fit(100.0, LARGE).0, 1, "never none");
    }

    #[test]
    fn arrows_move_the_selection_and_enter_opens_it() {
        let mut s = State::fake();
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.grid.per_row.set(4);
        let mut tx = Tx::default();
        let key = |k: Key| KeyEvent { key: k, mods: Default::default() };
        assert!(s.grid_key(&key(Key::Right), &mut tx));
        let first = s.entries[s.rows[0]].id;
        assert_eq!(s.grid.sel, Some(first), "the first arrow picks the first card");
        s.grid_key(&key(Key::Down), &mut tx);
        assert_eq!(s.grid.sel, Some(s.entries[s.rows[4]].id), "down: one row on");
        s.grid_key(&key(Key::Left), &mut tx);
        assert_eq!(s.grid.sel, Some(s.entries[s.rows[3]].id));
        s.grid_key(&key(Key::Up), &mut tx);
        assert_eq!(s.grid.sel, Some(s.entries[s.rows[3]].id), "no row above: stays");
        let id = s.grid.sel.unwrap();
        assert!(s.grid_key(&key(Key::Enter), &mut tx));
        assert_eq!(s.page, Page::Details);
        assert_eq!(s.detail.as_ref().map(|d| d.id()), Some(id));
        // + and - step the picked card's episode.
        let mut s = State::fake();
        s.prefs.views.set(Status::Watching, ListView::Grid);
        s.grid.per_row.set(4);
        s.grid_key(&key(Key::Right), &mut tx);
        let id = s.grid.sel.unwrap();
        let p = s.entries.iter().find(|e| e.id == id).unwrap().progress;
        s.grid_key(&key(Key::Text("-".into())), &mut tx);
        assert_eq!(s.entries.iter().find(|e| e.id == id).unwrap().progress, p - 1);
        s.grid_key(&key(Key::Text("+".into())), &mut tx);
        assert_eq!(s.entries.iter().find(|e| e.id == id).unwrap().progress, p);
        // The table view leaves the keys alone.
        let mut s = State::fake();
        assert!(!s.grid_key(&key(Key::Down), &mut tx));
    }

    #[test]
    fn each_list_keeps_its_view() {
        let mut s = State::fake();
        s.set_view(Status::Watching, ListView::Large);
        s.set_view(Status::Completed, ListView::Grid);
        assert_eq!(s.view_of(Status::Watching), ListView::Large);
        assert_eq!(s.view_of(Status::Completed), ListView::Grid);
        assert_eq!(s.view_of(Status::OnHold), ListView::List);
    }
}
