//! Remove from list: the context menu's "Remove from list…", the details page's button and
//! `kot remove` ask first (a bar on top of the page, like "Mark anyway?"), then the entry leaves
//! the list at once and its removal waits in the sync queue (core `Store::remove_from_list`).
//! The toast's Undo, or Discard on the queue page, puts it back while the removal hasn't left.
//! If the entry changed on the service after ours, the queue shows a conflict instead of
//! deleting it.

use rune::{El, TextStyle, col, el, row, text};

use crate::library::{Toast, Undo};
use crate::model::{Entry, State};
use crate::queue::Cmd;
use crate::theme::{self, PAL};
use crate::ui::{button, mix};

/// How long the toast with Undo stays.
const UNDO_SECS: f64 = 8.0;

impl State {
    /// Asks before removing (the bar on top of the page).
    pub fn ask_remove(&mut self, id: u32) {
        self.ctx = None;
        if self.entries.iter().any(|e| e.id == id) {
            self.confirm_remove = Some(id);
        }
    }

    /// The question the bar and `kot remove` ask.
    pub fn remove_question(&self, id: u32) -> Option<String> {
        let e = self.entries.iter().find(|e| e.id == id)?;
        Some(format!("Remove {} from your list? It goes from your {} list too, with its progress and score.", e.title, self.service.title()))
    }

    /// Removes the entry now: off the list at once, its removal queued, a toast with Undo.
    pub fn remove_entry(&mut self, id: u32) -> Result<(), String> {
        self.confirm_remove = None;
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let i = self.entries.iter().position(|e| e.id == id).ok_or("That anime is not on your list.")?;
        let e = self.entries.remove(i);
        if self.editor.is_some_and(|ed| ed.id == id) {
            self.editor = None;
        }
        crate::queue::send(self, Cmd::Remove { media_id: id });
        self.recheck_now();
        self.refresh();
        let text = format!("Removed {} from your list", e.title);
        self.toast = Some(Toast { text, warn: false, until: std::time::Instant::now() + std::time::Duration::from_secs_f64(UNDO_SECS), undo: Some(Undo::Remove(Box::new(e))) });
        Ok(())
    }

    /// Undo of a removal: the entry is back at once; the worker discards the waiting removal
    /// (if it already left, the queue says so and the entry goes again).
    pub fn undo_remove(&mut self, e: Entry) {
        let id = e.id;
        if self.entries.iter().any(|x| x.id == id) {
            return;
        }
        self.entries.push(e);
        crate::queue::send(self, Cmd::UndoRemove { media_id: id });
        self.recheck_now();
        self.refresh();
    }
}

/// "Remove … from your list?": Remove or Cancel (Escape).
pub fn bar(s: &State, id: u32) -> Option<El> {
    let question = s.remove_question(id)?;
    Some(
        row()
            .key("remove")
            .mx(theme::PAGE_PAD)
            .mt(14.0)
            .px(14.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .radius(theme::R_LG)
            .fill(mix(PAL.bg, PAL.st_dropped, 0.10))
            .border(1.0, PAL.st_dropped.alpha(0.45))
            .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.st_dropped))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(text(question).key("remove/question").font(theme::BODY).color(PAL.text).wrap())
                    .child(text("Undo stays possible until the change is sent; the sync queue shows it.").font(theme::LABEL).color(PAL.dim).wrap()),
            )
            .child(text("Remove?").font(TextStyle::new(14.0, 600.0)).color(PAL.cream))
            .child(button("remove/yes", "Remove", true, Some(|s: &mut State| {
                if let Some(id) = s.confirm_remove
                    && let Err(m) = s.remove_entry(id)
                {
                    s.error = Some(m);
                }
            })))
            .child(button("remove/cancel", "Cancel", false, Some(|s: &mut State| s.confirm_remove = None))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(s: &State, id: u32) -> Option<&Entry> {
        s.entries.iter().find(|e| e.id == id)
    }

    #[test]
    fn remove_asks_then_takes_it_off_and_undo_puts_it_back() {
        let mut s = State::fake();
        let id = s.entries[2].id;
        let title = s.entries[2].title.clone();
        s.open_ctx(id, crate::ctxmenu::At::Point(10.0, 10.0));
        s.ask_remove(id);
        assert_eq!((s.ctx.is_none(), s.confirm_remove), (true, Some(id)), "the menu closes, the bar asks");
        assert!(s.remove_question(id).unwrap().starts_with(&format!("Remove {title} from your list?")));
        assert!(titled(&s, id).is_some(), "nothing goes before the answer");
        s.remove_entry(id).unwrap();
        assert!(titled(&s, id).is_none() && s.confirm_remove.is_none());
        assert!(!s.rows.iter().any(|&k| s.entries[k].id == id), "gone from the page");
        assert!(s.toast.as_ref().is_some_and(|t| t.text == format!("Removed {title} from your list") && matches!(t.undo, Some(Undo::Remove(_)))));
        s.toast_undo();
        let e = titled(&s, id).unwrap();
        assert_eq!(e.title, title);
        assert!(s.rows.iter().any(|&k| s.entries[k].id == id), "back on the page");
        assert!(s.remove_entry(9_999_999).is_err());
    }

    #[test]
    fn removing_the_entry_being_edited_closes_the_editor() {
        let mut s = State::fake();
        s.toggle_editor(0);
        s.remove_entry(0).unwrap();
        assert!(s.editor.is_none());
    }
}
