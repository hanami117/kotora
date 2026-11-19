//! The right-click menu of an entry (list rows, Grid and Large posters, Home cards): Move to ▸,
//! Score ▸, +1 episode, Play next episode, Edit…, Open details, Open on MyAnimeList, Remove from
//! list… (asks first: `remove.rs`).
//!
//! Built like the Sort menu: anchored (at the pointer, or under the selected entry when the
//! Menu key or Shift+F10 opens it), mounted at the window's root, modal (Escape or a press
//! outside closes it) and a menu focus group (arrows, Return, Space). A submenu opens on hover,
//! a press, Right or Return; it is a child of the menu, so the pointer can move between both. One
//! opened from the keyboard takes the focus (its own modal); Left or Escape goes back.
//!
//! Every change goes through `State::edit_or_ask`, as the editor's Save does (the sync queue,
//! the "hasn't aired yet" question), and the toast offers Undo, which puts the old values back
//! (the queue drops a change that hasn't left).

use kotora_core::{Change, Status};
use rune::{Align, El, Side, Tx, col, el, icon, row, spacer, text};

use crate::details::Stub;
use crate::icons;
use crate::library::{Toast, Undo};
use crate::model::{Page, State};
use crate::theme::{self, PAL};

/// Where the menu opens: a window point (a right-click) or under a keyed element (the keys).
#[derive(Clone, Debug, PartialEq)]
pub enum At {
    Point(f32, f32),
    Under(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    Move,
    Score,
}

/// The open menu: whose, where, and the submenu (`keys`: opened from the keyboard, so it holds
/// the focus).
#[derive(Clone, Debug, PartialEq)]
pub struct Ctx {
    pub id: u32,
    /// Anime or manga (the manga pages' rows and posters).
    pub kind: kotora_core::Kind,
    pub at: At,
    pub sub: Option<(Sub, bool)>,
}

/// How long a toast with Undo stays.
const UNDO_SECS: f64 = 6.0;

impl State {
    pub fn open_ctx(&mut self, id: u32, at: At) {
        self.sort_menu = false;
        self.airing.menu = false;
        self.ctx = Some(Ctx {
            id,
            kind: kotora_core::Kind::Anime,
            at,
            sub: None,
        });
    }

    /// The menu of a manga (its row or poster).
    pub fn open_manga_ctx(&mut self, id: u32, at: At) {
        self.sort_menu = false;
        self.ctx = Some(Ctx {
            id,
            kind: kotora_core::Kind::Manga,
            at,
            sub: None,
        });
        self.want_resume(id);
    }

    /// The Menu key, Shift+F10: the menu of the selected entry (as P plays it), under it.
    pub fn ctx_key(&mut self) -> bool {
        let Some(id) = self.play_target() else {
            return false;
        };
        let under = match self.page {
            Page::List(st) if self.view_of(st) != kotora_core::config::ListView::List => {
                format!("card/{id}")
            }
            Page::List(_) => "ed/progress".to_string(),
            _ => "det-mine".to_string(),
        };
        self.open_ctx(id, At::Under(under));
        true
    }

    /// A change from the menu: as the editor's Save, then a toast with Undo (unless it waits for
    /// "Mark anyway").
    fn ctx_edit(&mut self, id: u32, change: Change, done: String) {
        self.ctx = None;
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let old = e.values();
        let change = change.without_same(&old);
        if change == Change::default() {
            return;
        }
        if let Err(m) = self.edit_or_ask(id, change, false) {
            self.error = Some(m);
            return;
        }
        if self.ask.is_some() {
            return;
        }
        let back = Change {
            status: Some(old.status),
            progress: Some(old.progress),
            score: Some(old.score),
            rewatching: Some(old.rewatching),
            times_rewatched: Some(old.times_rewatched),
            volumes: None,
        };
        self.toast = Some(Toast {
            text: done,
            warn: false,
            until: std::time::Instant::now() + std::time::Duration::from_secs_f64(UNDO_SECS),
            undo: Some(Undo::Edit(id, back)),
        });
    }

    /// Move to: the new status; Completed also sets the last episode, as the editor and MAL do.
    pub fn ctx_move(&mut self, id: u32, st: Status) {
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let change = Change::to_status(st, e.episodes);
        let done = format!("Moved {} to {}", e.title, st.label());
        self.ctx_edit(id, change, done);
    }

    /// A score from the menu; one that already shows as the entry's keeps the precise score.
    pub fn ctx_score(&mut self, id: u32, score: Option<kotora_core::Score>) {
        let fmt = self.score_format();
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let score = fmt.pick(e.score, score);
        let done = match score {
            Some(n) => format!("Scored {} {}", e.title, fmt.words(n)),
            None => format!("Removed the score of {}", e.title),
        };
        self.ctx_edit(
            id,
            Change {
                score: Some(score),
                ..Default::default()
            },
            done,
        );
    }

    pub fn ctx_plus(&mut self, id: u32) {
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let (old, title) = (e.values(), e.title.clone());
        self.ctx = None;
        if let Err(m) = self.plus_one_or_ask(id) {
            self.error = Some(m);
            return;
        }
        if self.ask.is_some() {
            return;
        }
        let ep = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .map_or(0, |e| e.progress);
        let back = Change {
            status: Some(old.status),
            progress: Some(old.progress),
            rewatching: Some(old.rewatching),
            times_rewatched: Some(old.times_rewatched),
            ..Default::default()
        };
        self.toast = Some(Toast {
            text: format!("Episode {ep} of {title} recorded"),
            warn: false,
            until: std::time::Instant::now() + std::time::Duration::from_secs_f64(UNDO_SECS),
            undo: Some(Undo::Edit(id, back)),
        });
    }

    /// Undo on the toast: the entry's values from before the change, or the removed entry back.
    pub fn toast_undo(&mut self) {
        let (id, back) = match self.toast.take().and_then(|t| t.undo) {
            Some(Undo::Edit(id, back)) => (id, back),
            Some(Undo::Remove(e)) => return self.undo_remove(*e),
            Some(Undo::MangaRemove(e)) => return self.undo_remove_manga(*e),
            Some(Undo::MangaEdit(id, back)) => {
                let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else {
                    return;
                };
                let change = back.without_same(&e.values());
                if let Err(m) = self.manga_edit(id, change) {
                    self.error = Some(m);
                }
                return;
            }
            None => return,
        };
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let change = back.without_same(&e.values());
        if let Err(m) = self.edit(id, change) {
            self.error = Some(m);
        }
    }

    /// Edit…: the list's own editor in the table, else the details page with its editor.
    pub fn ctx_edit_entry(&mut self, id: u32) {
        self.ctx = None;
        let in_table = matches!(self.page, Page::List(st) if self.view_of(st) == kotora_core::config::ListView::List);
        if in_table && self.rows.iter().any(|&k| self.entries[k].id == id) {
            if self.editor.is_none_or(|ed| ed.id != id) {
                self.toggle_editor(id);
            }
            return;
        }
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let stub = Stub::from_entry(e);
        self.open_details(stub);
        self.toggle_editor(id);
    }
}

fn item(
    key: String,
    label: String,
    enabled: bool,
    checked: Option<bool>,
    f: impl Fn(&mut State, &mut Tx) + 'static,
) -> El {
    let mut r = row()
        .key(key)
        .role(if checked.is_some() {
            "menuitemradio"
        } else {
            "menuitem"
        })
        .label(label.clone())
        .focusable(enabled)
        .h(28.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM);
    if let Some(on) = checked {
        r = r.checked(on).focus_selected(on);
    }
    if enabled {
        r = r
            .focus_highlight(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                f(s, tx);
            });
    }
    let mark = if checked == Some(true) {
        icon(&icons::CHECK, 14.0).color(PAL.accent_hi)
    } else {
        el()
    };
    r.child(el().size(14.0, 14.0).center().child(mark)).child(
        text(label)
            .font(if checked == Some(true) {
                theme::BODY_MED
            } else {
                theme::BODY
            })
            .color(if enabled { PAL.text } else { PAL.faint }),
    )
}

/// A row that opens a submenu: on hover (without the focus), on a press, Right or Return (with it).
fn sub_item(sub: Sub, label: &str, open: bool) -> El {
    let key = match sub {
        Sub::Move => "ctx/move",
        Sub::Score => "ctx/score",
    };
    let r = row()
        .key(key)
        .role("menuitem")
        .label(label.to_string())
        .expanded(open)
        .focusable(true)
        .h(28.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .focus_highlight(PAL.hover())
        .on_hover(move |s: &mut State| {
            if let Some(c) = &mut s.ctx
                && c.sub.is_none_or(|(k, keys)| k != sub && !keys)
            {
                c.sub = Some((sub, false));
            }
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Some(c) = &mut s.ctx {
                c.sub = Some((sub, true));
            }
        })
        .on_key(move |s: &mut State, k: &rune::KeyEvent, _tx: &mut Tx| {
            if matches!(k.key, rune::Key::Right | rune::Key::Enter) {
                if let Some(c) = &mut s.ctx {
                    c.sub = Some((sub, true));
                }
                return true;
            }
            false
        });
    let r = if open { r.fill(PAL.hover()) } else { r };
    r.child(el().size(14.0, 14.0))
        .child(text(label.to_string()).font(theme::BODY).color(PAL.text))
        .child(spacer())
        .child(icon(&ARROW_RIGHT, 13.0).color(PAL.faint))
}

static ARROW_RIGHT: rune::Icon = rune::Icon {
    name: "kot-chevron-right",
    view: 24.0,
    body: r#"<path d="M9 6l6 6-6 6" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

fn sep() -> El {
    el().w_full().h(1.0).my(4.0).fill(PAL.border)
}

fn surface(key: &str, w: f32) -> El {
    col()
        .key(key.to_string())
        .role("menu")
        .focus_group()
        .w(w)
        .padding(4.0)
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border_strong)
        .shadow(
            8.0,
            24.0,
            rune::Color::BLACK.alpha(if PAL.light { 0.14 } else { 0.4 }),
        )
}

/// The submenu next to its row.
fn submenu(s: &State, id: u32, kind: kotora_core::Kind, sub: Sub, keys: bool) -> El {
    let manga = kind == kotora_core::Kind::Manga;
    // The entry's status and score, of either kind.
    let e = if manga {
        s.manga
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.status, e.score))
    } else {
        s.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.status, e.score))
    };
    let (anchor, mut m) = match sub {
        Sub::Move => ("ctx/move", surface("ctx-sub/move", 170.0)),
        Sub::Score => ("ctx/score", surface("ctx-sub/score", 150.0)),
    };
    match sub {
        Sub::Move => {
            for st in Status::ALL {
                let on = e.is_some_and(|e| e.0 == st);
                let label = if manga { st.label_in(kind) } else { st.label() };
                m = m.child(item(
                    format!("ctx/move/{}", st.slug()),
                    label.to_string(),
                    true,
                    Some(on),
                    move |s, _| {
                        if manga {
                            s.manga_ctx_move(id, st)
                        } else {
                            s.ctx_move(id, st)
                        }
                    },
                ));
            }
        }
        Sub::Score => {
            let fmt = s.score_format();
            for (n, label, choice) in crate::scores::menu_choices(fmt).into_iter().rev() {
                let on = e.is_some_and(|e| crate::scores::menu_lit(fmt, e.1, choice));
                m = m.child(item(
                    format!("ctx/score/{n}"),
                    label,
                    true,
                    Some(on),
                    move |s, _| {
                        if manga {
                            s.manga_ctx_score(id, Some(choice))
                        } else {
                            s.ctx_score(id, Some(choice))
                        }
                    },
                ));
            }
            m = m.child(sep());
            let on = e.is_some_and(|e| e.1.is_none());
            m = m.child(item(
                "ctx/score/none".into(),
                "No score".into(),
                true,
                Some(on),
                move |s, _| {
                    if manga {
                        s.manga_ctx_score(id, None)
                    } else {
                        s.ctx_score(id, None)
                    }
                },
            ));
        }
    }
    let m = m.anchored(anchor, Side::Right, Align::Start, 2.0).on_key(
        |s: &mut State, k: &rune::KeyEvent, _tx: &mut Tx| {
            if k.key == rune::Key::Left {
                if let Some(c) = &mut s.ctx {
                    c.sub = None;
                }
                return true;
            }
            false
        },
    );
    // From the keyboard it takes the focus (and Escape closes only it).
    if keys {
        m.modal(true, |s: &mut State| {
            if let Some(c) = &mut s.ctx {
                c.sub = None;
            }
        })
        .focus_return(anchor)
    } else {
        m
    }
}

/// The menu, and the zero-size point it hangs from after a right-click. Mounted at the root.
pub fn view(s: &State, c: &Ctx) -> Vec<El> {
    let id = c.id;
    let mut out = Vec::new();
    let (anchor, side) = match &c.at {
        At::Point(x, y) => {
            out.push(el().key("ctx-point").absolute(*x, *y).size(0.0, 0.0));
            ("ctx-point".to_string(), Side::Bottom)
        }
        At::Under(k) => (k.clone(), Side::Bottom),
    };
    if c.kind == kotora_core::Kind::Manga {
        let Some(m) = manga_menu(s, c) else {
            return Vec::new();
        };
        out.push(
            m.anchored(anchor, side, Align::Start, 2.0)
                .modal(true, |s: &mut State| s.ctx = None),
        );
        return out;
    }
    let Some(e) = s.entries.iter().find(|e| e.id == id) else {
        return Vec::new();
    };
    let has_next = e.episodes.is_none_or(|n| e.progress < n);
    let next = e.progress + 1;
    let open = |k: Sub| c.sub.is_some_and(|(o, _)| o == k);
    let mut m = surface("ctx-menu", 230.0)
        .label(format!("{}: actions", e.title))
        .child(
            text(e.title.clone())
                .font(theme::CAPTION.tracking(0.0))
                .color(PAL.faint)
                .truncate()
                .px(8.0)
                .pt(4.0)
                .pb(4.0),
        )
        .child(sub_item(Sub::Move, "Move to", open(Sub::Move)))
        .child(sub_item(Sub::Score, "Score", open(Sub::Score)))
        .child(sep())
        .child(item(
            "ctx/plus".into(),
            if has_next {
                format!("+1 episode (episode {next})")
            } else {
                "+1 episode".into()
            },
            has_next && s.has_list(),
            None,
            move |s, _| s.ctx_plus(id),
        ))
        .child(item(
            "ctx/play".into(),
            "Play next episode".into(),
            true,
            None,
            move |s, _| {
                s.ctx = None;
                s.play_entry(id);
            },
        ))
        .child(sep())
        .child(item(
            "ctx/edit".into(),
            "Edit\u{2026}".into(),
            true,
            None,
            move |s, _| s.ctx_edit_entry(id),
        ))
        .children(s.feeds_can_search(id).then(|| {
            item(
                "ctx/search-releases".into(),
                "Search torrents".into(),
                true,
                None,
                move |s, _| {
                    s.ctx = None;
                    s.feeds_search(id);
                },
            )
        }))
        .child(item(
            "ctx/details".into(),
            "Open details".into(),
            true,
            None,
            move |s, _| {
                s.ctx = None;
                if let Some(e) = s.entries.iter().find(|e| e.id == id) {
                    let stub = Stub::from_entry(e);
                    s.open_details(stub);
                }
            },
        ))
        .children(open_items(s, kotora_core::Kind::Anime, id))
        .child(sep())
        .child(item(
            "ctx/remove".into(),
            "Remove from list\u{2026}".into(),
            s.has_list(),
            None,
            move |s, _| s.ask_remove(id),
        ));
    if let Some((sub, keys)) = c.sub {
        m = m.child(submenu(s, id, kotora_core::Kind::Anime, sub, keys));
    }
    out.push(
        m.anchored(anchor, side, Align::Start, 2.0)
            .modal(true, |s: &mut State| s.ctx = None),
    );
    out
}

/// "Open on MyAnimeList" (`ctx/mal`), and with the mirror on "Open on AniList" (`ctx/other`)
/// too, when its id is known ([`State::service_links`]).
fn open_items(s: &State, kind: kotora_core::Kind, id: u32) -> Vec<El> {
    s.service_links(kind, id)
        .into_iter()
        .enumerate()
        .map(|(i, (svc, url))| {
            item(
                if i == 0 {
                    "ctx/mal".into()
                } else {
                    "ctx/other".into()
                },
                format!("Open on {}", svc.title()),
                true,
                None,
                move |s, _| {
                    s.ctx = None;
                    crate::online::open_url(s, url.clone());
                },
            )
        })
        .collect()
}

/// A manga's menu: Move to ▸, Score ▸, +1 chapter (or volume), Edit…, Open details, Open on
/// MyAnimeList, Remove from list… (asks first). No Play: manga have no player.
fn manga_menu(s: &State, c: &Ctx) -> Option<El> {
    let id = c.id;
    let e = s.manga.entries.iter().find(|e| e.id == id)?;
    let open = |k: Sub| c.sub.is_some_and(|(o, _)| o == k);
    let (unit, next) = match e.plus_unit() {
        crate::manga::Unit::Chapters => ("chapter", e.chapters + 1),
        crate::manga::Unit::Volumes => ("volume", e.volumes + 1),
    };
    let at_end = match e.plus_unit() {
        crate::manga::Unit::Chapters => e.chapters_total.is_some_and(|n| e.chapters >= n),
        crate::manga::Unit::Volumes => e.volumes_total.is_some_and(|n| e.volumes >= n),
    };
    let mut m = surface("ctx-menu", 230.0)
        .label(format!("{}: actions", e.title))
        .child(
            text(e.title.clone())
                .font(theme::CAPTION.tracking(0.0))
                .color(PAL.faint)
                .truncate()
                .px(8.0)
                .pt(4.0)
                .pb(4.0),
        )
        .child(sub_item(Sub::Move, "Move to", open(Sub::Move)))
        .child(sub_item(Sub::Score, "Score", open(Sub::Score)))
        .child(sep())
        .child(item(
            "ctx/plus".into(),
            if at_end {
                format!("+1 {unit}")
            } else {
                format!("+1 {unit} ({unit} {next})")
            },
            !at_end && s.has_list(),
            None,
            move |s, _| s.manga_ctx_plus(id),
        ))
        .child(sep())
        .children(s.resume_of(id).map(|r| {
            item(
                "ctx/continue".into(),
                format!("Continue reading ({})", r.label()),
                true,
                None,
                move |s, _| s.continue_reading(id),
            )
        }))
        .child(item(
            "ctx/edit".into(),
            "Edit\u{2026}".into(),
            true,
            None,
            move |s, _| s.manga_ctx_edit_entry(id),
        ))
        .child(item(
            "ctx/details".into(),
            "Open details".into(),
            true,
            None,
            move |s, _| {
                s.ctx = None;
                if let Some(e) = s.manga.entries.iter().find(|e| e.id == id) {
                    let stub = crate::manga::stub(e);
                    s.open_details(stub);
                }
            },
        ))
        .children(open_items(s, kotora_core::Kind::Manga, id))
        .child(sep())
        .child(item(
            "ctx/remove".into(),
            "Remove from list\u{2026}".into(),
            s.has_list(),
            None,
            move |s, _| s.ask_remove_manga(id),
        ));
    if let Some((sub, keys)) = c.sub {
        m = m.child(submenu(s, id, kotora_core::Kind::Manga, sub, keys));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kotora_core::score::ten;

    fn entry(s: &State, id: u32) -> &crate::model::Entry {
        s.entries.iter().find(|e| e.id == id).unwrap()
    }

    #[test]
    fn move_score_and_plus_go_through_the_queue_and_undo() {
        let mut s = State::fake();
        let id = 2; // Mikan Street Detectives: Watching, 7 of 24.
        let before = entry(&s, id).values();
        s.open_ctx(id, At::Point(10.0, 10.0));
        s.ctx_move(id, Status::Completed);
        assert!(s.ctx.is_none(), "a pick closes the menu");
        let e = entry(&s, id);
        assert_eq!(
            (e.status, e.progress),
            (Status::Completed, 24),
            "Completed sets the last episode, as MAL does"
        );
        assert!(e.waiting.is_some(), "queued");
        assert!(
            s.toast
                .as_ref()
                .is_some_and(|t| t.undo.is_some() && t.text.contains("Completed"))
        );
        s.toast_undo();
        assert_eq!(entry(&s, id).values(), before, "Undo puts it back");
        assert!(s.toast.is_none());

        s.ctx_score(id, ten(9));
        assert_eq!(entry(&s, id).score, ten(9));
        s.ctx_score(id, None);
        assert_eq!(entry(&s, id).score, None);
        s.toast_undo();
        assert_eq!(entry(&s, id).score, ten(9), "Undo of No score");

        let p = entry(&s, id).progress;
        s.ctx_plus(id);
        assert_eq!(entry(&s, id).progress, p + 1);
        s.toast_undo();
        assert_eq!(entry(&s, id).progress, p);

        // The same status: nothing queued, no toast.
        s.toast = None;
        s.ctx_move(id, Status::Watching);
        assert!(s.toast.is_none());
    }

    #[test]
    fn edit_and_keys_open_the_right_place() {
        let mut s = State::fake();
        // The table: its own editor.
        s.ctx_edit_entry(1);
        assert_eq!(s.editor.map(|e| e.id), Some(1));
        assert_eq!(s.page, Page::List(Status::Watching));
        // The grid: the details page with the editor; the Menu key needs a selection.
        let mut s = State::fake();
        s.prefs
            .views
            .set(Status::Watching, kotora_core::config::ListView::Grid);
        assert!(!s.ctx_key());
        s.grid.sel = Some(3);
        assert!(s.ctx_key());
        assert_eq!(
            s.ctx.as_ref().map(|c| c.at.clone()),
            Some(At::Under("card/3".into()))
        );
        s.ctx_edit_entry(3);
        assert_eq!((s.page, s.editor.map(|e| e.id)), (Page::Details, Some(3)));
    }
}
