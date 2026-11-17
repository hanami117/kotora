//! The manga list pages' Grid and Large views: the anime ones (`grid.rs`: sizes, rows of cards,
//! badges) with a manga's progress by the agreed rule ([`crate::manga::shows`]: "Ch 64 / 88",
//! "Vol 3 / 9", or "Ch 119 · Vol 11" when both are read). `+1` shows on hover, always on Reading
//! (as on Watching); it steps chapters, or volumes when only volumes show. A press opens the
//! details page, a right-click the manga's menu.

use kotora_core::Status;
use kotora_core::config::ListView;
use rune::{El, GrowthSpec, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::grid::{DENSE, LARGE, badge, fit, row_key, size};
use crate::icons;
use crate::look;
use crate::manga::{MEntry, Run, Unit, cover_id, stub};
use crate::model::{Page, State};
use crate::theme::{self, PAL};

/// The progress under a poster, by the rule: one count with its total, or both counts.
pub fn progress_text(e: &MEntry) -> String {
    let total = |n: Option<u32>| n.map_or("\u{2013}".to_string(), |t| t.to_string());
    match e.shows() {
        (true, true) => format!("Ch {} \u{b7} Vol {}", e.chapters, e.volumes),
        (false, true) => format!("Vol {} / {}", e.volumes, total(e.volumes_total)),
        _ => format!("Ch {} / {}", e.chapters, total(e.chapters_total)),
    }
}

/// Whether `+1` applies (a next chapter, or volume, exists).
fn has_next(s: &State, e: &MEntry) -> bool {
    let at_end = match e.plus_unit() {
        Unit::Chapters => e.chapters_total.is_some_and(|n| e.chapters >= n),
        Unit::Volumes => e.volumes_total.is_some_and(|n| e.volumes >= n),
    };
    s.has_list() && !at_end
}

fn status_color(e: &MEntry) -> rune::Color {
    if e.rereading { PAL.st_completed } else { PAL.status(e.status) }
}

/// The badges at a poster's top-left: reread or the run, a change waiting.
fn badges(e: &MEntry) -> El {
    let mut c = col().key("badges").absolute(6.0, 6.0).gap(4.0).items_start();
    if e.rereading {
        c = c.child(badge("Rereading", Some(PAL.st_completed)));
    } else if e.run == Run::Publishing {
        c = c.child(badge("Publishing", Some(PAL.airing)));
    } else if e.run == Run::Hiatus {
        c = c.child(badge("On hiatus", Some(PAL.st_dropped)));
    }
    if let Some(w) = e.waiting {
        c = c.child(badge(w, Some(PAL.warn())));
    }
    c
}

fn score(s: &State, e: &MEntry) -> El {
    match e.score {
        Some(v) => crate::scores::poster_mark(s.score_format(), v),
        None => el(),
    }
}

/// `+1` on a poster: a pill of the page colour, orange under the pointer.
fn plus_on_poster(id: u32, u: Unit) -> El {
    el().key("plus")
        .role("button")
        .label(if u == Unit::Chapters { "One more chapter" } else { "One more volume" })
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
            if let Err(m) = s.manga_plus_one(id) {
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
            if let Some(e) = s.manga.entries.iter().find(|e| e.id == id) {
                let st = stub(e);
                s.open_details(st);
                s.manga_toggle_editor(id);
            }
        })
        .child(icon(&icons::PENCIL, 15.0).color(PAL.text))
}

/// Under the pointer: a shade, Edit, and `+1` unless it is always there.
fn hover_layer(s: &State, e: &MEntry, plus: bool) -> El {
    let shade = PAL.bg;
    col()
        .key("hover")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .linear_gradient(std::f32::consts::PI, [(0.0, shade.alpha(0.0)), (0.55, shade.alpha(0.0)), (1.0, shade.alpha(0.75))])
        .group_opacity(0.0, 1.0)
        .padding(8.0)
        .child(spacer())
        .child(row().gap(6.0).center_y().child(edit_on_poster(e.id)).child(spacer()).child(if plus && has_next(s, e) { plus_on_poster(e.id, e.plus_unit()) } else { el() }))
}

/// A card's press (the details page) and right-click (the menu).
fn pressable(c: El, e: &MEntry) -> El {
    let id = e.id;
    let st = stub(e);
    c.role("button")
        .label(format!("About {}", e.title))
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
            tx.instant();
            s.open_manga_ctx(id, crate::ctxmenu::At::Point(p.x, p.y));
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_details(st.clone());
        })
}

impl State {
    /// The manga grid's keys, as the anime grid's: the arrows move a selection (outlined),
    /// Enter opens its page, Escape drops it, + and - step its chapter (or volume, when only
    /// volumes show). Returns whether the key was used.
    pub fn manga_grid_key(&mut self, k: &KeyEvent, tx: &mut Tx) -> bool {
        let Page::Manga(st) = self.page else { return false };
        if self.manga_view_of(st) == ListView::List || !self.manga.query.text.trim().is_empty() || self.manga.rows.is_empty() || k.mods.cmd || k.mods.alt || k.mods.ctrl {
            return false;
        }
        let per = self.grid.per_row.get().max(1);
        let n = self.manga.rows.len();
        let at = self.manga.grid_sel.and_then(|id| self.manga.rows.iter().position(|&k| self.manga.entries[k].id == id));
        let next = match (&k.key, at) {
            (Key::Escape, Some(_)) => {
                self.manga.grid_sel = None;
                return true;
            }
            (Key::Enter, Some(i)) => {
                tx.instant();
                let st = stub(&self.manga.entries[self.manga.rows[i]]);
                self.open_details(st);
                return true;
            }
            (Key::Text(t), Some(i)) if matches!(t.as_str(), "+" | "=" | "-") => {
                tx.instant();
                let e = &self.manga.entries[self.manga.rows[i]];
                let id = e.id;
                let r = if t.as_str() == "-" {
                    let change = match e.plus_unit() {
                        Unit::Chapters if e.chapters > 0 => Some(kotora_core::Change { progress: Some(e.chapters - 1), ..Default::default() }),
                        Unit::Volumes if e.volumes > 0 => Some(kotora_core::Change { volumes: Some(e.volumes - 1), ..Default::default() }),
                        _ => None,
                    };
                    change.map_or(Ok(()), |c| self.manga_edit(id, c).map(|_| ()))
                } else {
                    self.manga_plus_one(id).map(|_| ())
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
        self.manga.grid_sel = Some(self.manga.entries[self.manga.rows[next]].id);
        tx.scroll_into_view(row_key(next / per));
        true
    }
}

/// The selection's outline (on a wrapper: the poster clips its own children).
fn outlined(s: &State, e: &MEntry, c: El, r: f32) -> El {
    if s.manga.grid_sel == Some(e.id) { el().key(format!("msel/{}", e.id)).radius(r).outline(2.0, 2.0, PAL.accent).child(c) } else { c }
}

/// Grid: the poster with its badges and a progress strip, the title and the progress under it.
fn card_dense(s: &State, e: &MEntry, st: Status) -> El {
    let z = DENSE;
    let always = st == Status::Watching && has_next(s, e);
    let mut p = look::poster(s, cover_id(e.id), &e.title, e.cover_url.as_deref(), z.w, z.poster_h, theme::R_MD)
        .clip_rounded(theme::R_MD)
        .child(badges(e))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(4.0).fill(PAL.bg.alpha(0.7)).child(el().w(z.w * e.fraction()).h_full().fill(status_color(e))))
        .child(hover_layer(s, e, !always));
    if always {
        p = p.child(el().absolute(z.w - 8.0 - 44.0, z.poster_h - 8.0 - 28.0 - 2.0).w(44.0).child(plus_on_poster(e.id, e.plus_unit())));
    }
    let c = col()
        .key(format!("card/m{}", e.id))
        .w(z.w)
        .gap(6.0)
        .child(p)
        .child(col().h(34.0).child(text(e.title.clone()).font(TextStyle::new(13.0, 500.0)).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2)))
        .child(row().h(16.0).center_y().child(text(progress_text(e)).font(theme::LABEL_NUM).color(PAL.dim).truncate().min_w(0.0)).child(spacer()).child(score(s, e)));
    outlined(s, e, pressable(c, e), theme::R_MD)
}

/// Large: the poster, two lines of title, a bar, the progress, the score and `+1`.
fn card_large(s: &State, e: &MEntry) -> El {
    let z = LARGE;
    let p = look::poster(s, cover_id(e.id), &e.title, e.cover_url.as_deref(), z.w, z.poster_h, 0.0).child(badges(e)).child(hover_layer(s, e, false));
    let plus = if has_next(s, e) { plus_on_poster(e.id, e.plus_unit()).h(26.0).px(10.0) } else { el() };
    let c = col()
        .key(format!("card/m{}", e.id))
        .w(z.w)
        .radius(theme::R_LG)
        .clip_rounded(theme::R_LG)
        .fill(PAL.surface)
        .border(1.0, PAL.border)
        .child(p)
        .child(
            col()
                .padding(12.0)
                .gap(10.0)
                .child(col().h(38.0).child(text(e.title.clone()).font(TextStyle::new(14.0, 600.0)).color(PAL.text).group_color(PAL.accent_hi).wrap().max_lines(2)))
                // The score beside the bar, so the counts ("Ch 119 · Vol 11") keep their line.
                .child(row().h(16.0).gap(8.0).center_y().child(col().grow(1.0).child(look::bar(e.fraction(), status_color(e), 5.0, PAL.border))).child(score(s, e)))
                .child(row().h(26.0).center_y().gap(8.0).child(text(progress_text(e)).font(theme::NUM_MED).color(PAL.dim).truncate().min_w(0.0)).child(spacer()).child(plus)),
        );
    outlined(s, e, pressable(c, e), theme::R_LG)
}

/// The cards of manga page `st` (its rows after the chip and the sort), `width` the main area's.
pub fn cards(s: &State, st: Status, width: f32) -> El {
    let v = s.manga_view_of(st);
    let z = size(v);
    let (per, gap) = fit(width, z);
    // The arrow keys need it (the anime grid sets it the same way).
    s.grid.per_row.set(per);
    let rows = s.manga.rows.len().div_ceil(per);
    col().key(format!("manga-grid-scroll/{}", st.slug())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
        col().key("grid").growth(GrowthSpec::default()).virtual_rows(
            rows,
            z.row_h,
            move |_: &State, r| row_key(r).into(),
            move |s: &State, r| {
                let mut line = row().w_full().h(z.row_h).px(theme::PAGE_PAD).pt(if r == 0 { 4.0 } else { 0.0 }).gap(gap).items_start();
                for &k in s.manga.rows.iter().skip(r * per).take(per) {
                    let e = &s.manga.entries[k];
                    line = line.child(if v == ListView::Large { card_large(s, e) } else { card_dense(s, e, st) });
                }
                line
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posters_show_the_counts_read() {
        let mut s = State::fake();
        crate::manga::fake(&mut s);
        let t = |id: u32| progress_text(s.manga.entries.iter().find(|e| e.id == id).unwrap());
        assert_eq!(t(1), "Ch 119 \u{b7} Vol 11", "both read");
        assert_eq!(t(2), "Ch 64 / 88");
        assert_eq!(t(6), "Vol 3 / 9", "a light novel read by volume");
        assert_eq!(t(7), "Ch 0 / 1", "nothing read: chapters");
    }
}
