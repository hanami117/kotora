//! Started airing and New episode notifications (Settings → Notifications).
//!
//! A thread (`kotora-airing-notes`, never headless) looks at the saved list once a minute and
//! right after a list sync or a details refresh: core [`airnotes::pass`] says what is due, marks
//! it announced in the store, and the UI thread shows it if its switch is on. It reads the
//! database only: no MyAnimeList request of its own, so the rate budget is untouched. It runs
//! while only the tray is there (the app keeps running without its window).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use kotora_core::airnotes::{self, Due, What};
use kotora_core::library::Next;
use kotora_core::store::Store;
use kotora_core::{Status, paths};

use crate::details::Stub;
use crate::model::State;
use crate::notes::{Act, Kind, Note};

const SERVICE: &str = "mal";
/// How often the thread looks without being asked: the estimate is to the minute.
const EVERY: Duration = Duration::from_secs(60);
/// Note ids remembered for clicks (the newest).
const KEEP_IDS: usize = 64;

/// Kept in the state.
#[derive(Default)]
pub struct AirNotes {
    tx: Option<Sender<()>>,
    /// Shown notification ids and their anime, for a click or Play.
    ids: Vec<(String, u32)>,
}

/// At start (never headless).
pub fn start(s: &mut State) {
    if rune::is_headless() {
        return;
    }
    let Some(h) = s.handle.clone() else { return };
    let (tx, rx) = mpsc::channel();
    s.airnotes.tx = Some(tx);
    let _ = std::thread::Builder::new().name("kotora-airing-notes".into()).spawn(move || run(&h, &rx));
}

fn run(h: &crate::model::Handle, rx: &Receiver<()>) {
    let mut store: Option<Store> = None;
    loop {
        if store.is_none() {
            store = Store::open(&paths::db_file()).ok();
        }
        if let Some(st) = store.as_mut() {
            match airnotes::pass(st, SERVICE, kotora_core::time::now_unix()) {
                Ok(due) if !due.is_empty() => h.update_if(move |s, _| s.air_notes(&due)),
                Ok(_) => {}
                Err(e) => eprintln!("kotora: airing notifications: {e}"),
            }
        }
        match rx.recv_timeout(EVERY) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Several kicks at once are one look.
        while rx.try_recv().is_ok() {}
    }
}

impl State {
    /// The saved list changed (a sync, a details refresh): look again now.
    pub fn kick_air_notes(&self) {
        if let Some(tx) = &self.airnotes.tx {
            let _ = tx.send(());
        }
    }

    /// Shows what is due, under its switch. Never visible in the window: returns false.
    pub fn air_notes(&mut self, due: &[Due]) -> bool {
        for d in due {
            let Some(e) = self.entries.iter().find(|e| e.id == d.media_id) else { continue };
            let title = e.title.clone();
            let note = match d.what {
                What::Started if self.prefs.notify.started_airing && e.status == Status::PlanToWatch => {
                    Note { id: format!("kotora-{}-started", e.id), kind: Kind::Started, summary: format!("{title} started airing"), body: "It\u{2019}s on your Plan to watch list.".into(), tail: None }
                }
                // The window may know a newer progress than the saved list.
                What::Episode(n) if self.prefs.notify.new_episode && e.status == Status::Watching && e.progress < n => {
                    let play = matches!(crate::library::next_for(self, e), Some(Next::Ready { .. }));
                    let body = format!("Estimated from the broadcast time. You\u{2019}re at episode {}.", e.progress);
                    Note {
                        // The same id as the offer for that episode: one replaces the other.
                        id: format!("kotora-{}-ep{n}", e.id),
                        kind: if play { Kind::AiredPlay } else { Kind::Aired },
                        summary: format!("{title} \u{b7} Ep {n} is out"),
                        body,
                        tail: play.then(|| "Play it from Kotora\u{2019}s list.".to_string()),
                    }
                }
                _ => continue,
            };
            self.airnotes.ids.retain(|(id, _)| *id != note.id);
            self.airnotes.ids.push((note.id.clone(), e.id));
            if self.airnotes.ids.len() > KEEP_IDS {
                self.airnotes.ids.remove(0);
            }
            self.notes.show(note);
        }
        false
    }

    /// The anime of a Started airing or New episode notification.
    pub fn air_note_media(&self, id: &str) -> Option<u32> {
        self.airnotes.ids.iter().find(|(i, _)| i == id).map(|(_, m)| *m)
    }

    /// Play opens the next file; anything else (a click) opens the anime's page.
    pub fn air_note_action(&mut self, media: u32, act: Act) {
        if act == Act::Play {
            match crate::library::play_next(self, media) {
                Ok(_) => return,
                Err(m) => self.error = Some(m),
            }
        }
        if let Some(e) = self.entries.iter().find(|e| e.id == media) {
            let stub = Stub::from_entry(e);
            self.open_details(stub);
        }
        if let Some(h) = &self.handle {
            crate::tray::show_window(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;
    use crate::notes::Sent;

    fn shown(s: &State) -> Vec<Note> {
        s.notes.log.iter().filter_map(|x| if let Sent::Show(n) = x { Some(n.clone()) } else { None }).collect()
    }

    /// The fake list's first Watching entry and first Plan to watch entry.
    fn ids(s: &State) -> (u32, u32, u32) {
        let w = s.entries.iter().find(|e| e.status == Status::Watching).unwrap();
        let p = s.entries.iter().find(|e| e.status == Status::PlanToWatch).unwrap().id;
        (w.id, w.progress, p)
    }

    #[test]
    fn both_switches_are_on_by_default_and_texts_name_the_show() {
        let mut s = State::fake();
        let (w, prog, p) = ids(&s);
        assert!(!s.air_notes(&[Due { media_id: p, what: What::Started }, Due { media_id: w, what: What::Episode(prog + 1) }]), "nothing in the window changes");
        let n = shown(&s);
        assert_eq!(n.len(), 2);
        let ptitle = s.entries.iter().find(|e| e.id == p).unwrap().title.clone();
        let wtitle = s.entries.iter().find(|e| e.id == w).unwrap().title.clone();
        assert_eq!((n[0].kind, n[0].summary.clone(), n[0].id.clone()), (Kind::Started, format!("{ptitle} started airing"), format!("kotora-{p}-started")));
        assert_eq!((n[1].kind, n[1].summary.clone()), (Kind::Aired, format!("{wtitle} \u{b7} Ep {} is out", prog + 1)));
        assert_eq!(n[1].id, format!("kotora-{w}-ep{}", prog + 1), "the offer's id for that episode");
    }

    #[test]
    fn switches_off_and_watched_episodes_send_nothing() {
        let mut s = State::fake();
        let (w, prog, p) = ids(&s);
        s.prefs.notify.started_airing = false;
        s.prefs.notify.new_episode = false;
        s.air_notes(&[Due { media_id: p, what: What::Started }, Due { media_id: w, what: What::Episode(prog + 1) }]);
        assert!(shown(&s).is_empty());
        let mut s = State::fake();
        s.air_notes(&[Due { media_id: w, what: What::Episode(prog) }]);
        assert!(shown(&s).is_empty(), "already watched (the window's progress)");
        // Not on the list any more, or no longer Plan to watch: nothing.
        s.air_notes(&[Due { media_id: 999_999, what: What::Started }, Due { media_id: w, what: What::Started }]);
        assert!(shown(&s).is_empty());
    }

    #[test]
    fn a_click_opens_the_details_page() {
        let mut s = State::fake();
        // Nothing plays (the playing episode's own notification acts on what plays).
        s.now = None;
        let (w, prog, _) = ids(&s);
        s.air_notes(&[Due { media_id: w, what: What::Episode(prog + 1) }]);
        let id = shown(&s)[0].id.clone();
        assert!(s.note_action(&id, Act::Open));
        assert_eq!(s.page, Page::Details);
        assert_eq!(s.detail.as_ref().map(|d| d.stub.id), Some(w));
        // An unknown id still only opens Kotora.
        let mut s = State::fake();
        s.page = Page::List(Status::Watching);
        assert!(s.note_action("kotora-1-started", Act::Open));
        assert_ne!(s.page, Page::Details);
    }

    #[test]
    fn play_shows_only_when_the_library_has_the_next_file() {
        let mut s = State::fake();
        s.now = None;
        crate::library::fake(&mut s);
        let with_file = s.entries.iter().find(|e| e.status == Status::Watching && matches!(crate::library::next_for(&s, e), Some(Next::Ready { .. }))).map(|e| (e.id, e.progress));
        let without = s.entries.iter().find(|e| e.status == Status::Watching && !matches!(crate::library::next_for(&s, e), Some(Next::Ready { .. }))).map(|e| (e.id, e.progress));
        let ((a, pa), (b, pb)) = (with_file.expect("fake library has a ready file"), without.expect("and a missing one"));
        s.air_notes(&[Due { media_id: a, what: What::Episode(pa + 1) }, Due { media_id: b, what: What::Episode(pb + 1) }]);
        let n = shown(&s);
        assert_eq!((n[0].kind, n[1].kind), (Kind::AiredPlay, Kind::Aired));
        assert_eq!(Kind::AiredPlay.actions(), &[(Act::Play, "Play")]);
        assert_eq!(n[0].text(false), format!("{} Play it from Kotora\u{2019}s list.", n[0].body), "no buttons: says where Play is");
        // Play from the notification: the library opens it (headless: no player starts).
        assert!(s.note_action(&n[0].id, Act::Play));
        assert!(s.library.note.as_deref().is_some_and(|t| t.starts_with("Opening episode")), "{:?}", s.library.note);
    }
}
