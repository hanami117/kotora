//! Desktop notifications (Settings → Notifications), one switch per event:
//! - **Episode detected**: a recognised episode starts playing ("Title · Ep 6"). Off by default.
//! - **Update offered**: "Update Title to episode 6?", with **Update** and **Not now**. On.
//! - **Episode recorded**: "Recorded · Ep 6", with **Undo**. On.
//!
//! The buttons act directly, through the same paths as the banner and `kot confirm` /
//! `kot dismiss` ([`State::note_action`](crate::model::State)). Never spam: one notification
//! per event, and every notification about one episode has the same id, so the next one
//! replaces the last instead of stacking.
//!
//! The system part is small and behind `cfg`:
//! - `linux.rs`: `org.freedesktop.Notifications` over D-Bus, with actions when the server has the
//!   `actions` capability (KDE, GNOME, most others); else the text says how to answer.
//! - `mac.rs`: `UNUserNotificationCenter` with categories and actions inside Kotora.app (asks
//!   for permission once, at the first notification); outside a bundle, `osascript` as before,
//!   without buttons.
//!
//! Never headless: the scenes and tests have no backend and only log what would be shown.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;

use std::sync::Arc;

use crate::model::Handle;

/// What a notification is about. Each has its own switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Detected,
    /// "Update to N?" (Update, Not now).
    Offer,
    /// "Watching it again?" (Start rewatch, Not now); under the Offer switch.
    Rewatch,
    /// "Recorded" (Undo).
    Recorded,
}

impl Kind {
    /// The macOS category identifier (each category has its buttons).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn category(self) -> &'static str {
        match self {
            Kind::Detected => "detected",
            Kind::Offer => "offer",
            Kind::Rewatch => "rewatch",
            Kind::Recorded => "recorded",
        }
    }

    /// The buttons, in order.
    pub fn actions(self) -> &'static [(Act, &'static str)] {
        match self {
            Kind::Detected => &[],
            Kind::Offer => &[(Act::Update, "Update"), (Act::NotNow, "Not now")],
            Kind::Rewatch => &[(Act::Update, "Start rewatch"), (Act::NotNow, "Not now")],
            Kind::Recorded => &[(Act::Undo, "Undo")],
        }
    }
}

/// A button, or a click on the notification itself ([`Act::Open`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Update,
    NotNow,
    Undo,
    /// The notification was clicked: show Kotora.
    Open,
}

impl Act {
    /// The action key (D-Bus) or action identifier (macOS).
    pub fn key(self) -> &'static str {
        match self {
            Act::Update => "update",
            Act::NotNow => "not-now",
            Act::Undo => "undo",
            // The freedesktop spec's name for a click on the body.
            Act::Open => "default",
        }
    }

    pub fn from_key(key: &str) -> Option<Act> {
        [Act::Update, Act::NotNow, Act::Undo, Act::Open].into_iter().find(|a| a.key() == key)
    }
}

/// One notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The same for every notification about one episode: a new one replaces the last.
    pub id: String,
    pub kind: Kind,
    pub summary: String,
    pub body: String,
    /// Added to the body where the system shows no buttons: how to answer instead.
    pub tail: Option<String>,
}

impl Note {
    /// The body as shown: with buttons as is, without them with the tail.
    pub fn text(&self, buttons: bool) -> String {
        match (&self.tail, buttons && !self.kind.actions().is_empty()) {
            (Some(t), false) if self.body.is_empty() => t.clone(),
            (Some(t), false) => format!("{} {t}", self.body),
            _ => self.body.clone(),
        }
    }
}

/// What the system offers, for Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Support {
    /// Not known yet (or headless).
    #[default]
    Unknown,
    /// Notifications with buttons.
    Buttons,
    /// Linux: the notification server shows no buttons.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NoButtons,
    /// macOS: not running from Kotora.app, so no buttons.
    Outside,
    /// macOS: notifications are turned off for Kotora in System Settings.
    Denied,
}

/// The system side. Called on the UI thread: it only hands the work to its own thread.
pub trait Backend: Send + Sync {
    /// Shows `note`, replacing one with the same id.
    fn show(&self, note: &Note);
    /// Takes the notification with this id away, if it is still there.
    fn withdraw(&self, id: &str);
    /// Asks for permission where the system needs it (macOS), when a switch is turned on.
    fn prepare(&self) {}
}

/// What the notifier did (the log the tests read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    Show(Note),
    Withdraw(String),
}

/// Kept in the state.
#[derive(Default)]
pub struct Notifier {
    backend: Option<Box<dyn Backend>>,
    /// The last notification shown, so an identical one is never sent twice in a row.
    last: Option<Note>,
    /// Ids that have a notification out (to withdraw only what was shown).
    out: Vec<String>,
    pub support: Support,
    /// Everything that would have gone to the system (no backend: scenes and tests).
    pub log: Vec<Sent>,
}

impl Notifier {
    /// The real one (live runs only, never headless). Actions come back through `handle`.
    pub fn start(handle: Handle) -> Notifier {
        if rune::is_headless() {
            return Notifier::default();
        }
        let h = handle.clone();
        let on_action: Arc<dyn Fn(String, Act) + Send + Sync> = Arc::new(move |id: String, act: Act| {
            h.update_if(move |s, _| s.note_action(&id, act));
        });
        let h = handle;
        let on_support: Arc<dyn Fn(Support) + Send + Sync> = Arc::new(move |sup: Support| {
            h.update_if(move |s, _| {
                let changed = s.notes.support != sup;
                s.notes.support = sup;
                changed
            });
        });
        #[cfg(target_os = "linux")]
        let backend: Option<Box<dyn Backend>> = Some(Box::new(linux::Sys::start(on_action, on_support)));
        #[cfg(target_os = "macos")]
        let backend: Option<Box<dyn Backend>> = Some(mac::start(on_action, on_support));
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let backend: Option<Box<dyn Backend>> = {
            let _ = (on_action, on_support);
            None
        };
        Notifier { backend, ..Default::default() }
    }

    pub fn show(&mut self, note: Note) {
        if self.last.as_ref() == Some(&note) {
            return;
        }
        if let Some(b) = &self.backend {
            b.show(&note);
        } else {
            self.log.push(Sent::Show(note.clone()));
        }
        if !self.out.contains(&note.id) {
            self.out.push(note.id.clone());
        }
        self.last = Some(note);
    }

    /// Takes the notification about this episode away (answered in the window or with `kot`).
    pub fn withdraw(&mut self, id: &str) {
        let Some(i) = self.out.iter().position(|o| o == id) else { return };
        self.out.remove(i);
        if self.last.as_ref().is_some_and(|n| n.id == id) {
            self.last = None;
        }
        match &self.backend {
            Some(b) => b.withdraw(id),
            None => self.log.push(Sent::Withdraw(id.to_string())),
        }
    }

    /// A switch was turned on: ask for permission now (macOS), not at an episode.
    pub fn prepare(&self) {
        if let Some(b) = &self.backend {
            b.prepare();
        }
    }
}

// ------------------------------------------------------------------------------ the app's side

use kotora_core::recognize::Found;

use crate::model::{Page, State};

/// Where the system shows no buttons: how to answer instead.
const ANSWER: &str = "Open Kotora, or run kot confirm.";

impl State {
    /// The id of every notification about the playing episode: `kotora-<anime>-ep<episode>`.
    /// None until the file is recognised.
    pub fn note_id(&self) -> Option<String> {
        let n = self.now.as_ref()?;
        let media = match &n.found {
            Found::List { media_id, .. } | Found::NotOnList { media_id, .. } => *media_id,
            _ => return None,
        };
        Some(format!("kotora-{media}-ep{}", n.episode().unwrap_or(0)))
    }

    /// An offer ("Update to N?", "Watching it again?") under the Update offered switch.
    pub(crate) fn notify(&mut self, kind: Kind, summary: String, body: String) {
        let on = match kind {
            Kind::Detected => self.prefs.notify.detected,
            Kind::Offer | Kind::Rewatch => self.prefs.notify.offer,
            Kind::Recorded => self.prefs.notify.recorded,
        };
        let Some(id) = self.note_id().filter(|_| on) else { return };
        let tail = match kind {
            Kind::Detected => None,
            Kind::Offer | Kind::Rewatch => Some(ANSWER.to_string()),
            Kind::Recorded => Some("Undo is in Kotora's banner.".to_string()),
        };
        self.notes.show(Note { id, kind, summary, body, tail });
    }

    /// "Recorded · Ep 6", with Undo. With the switch off, the offer's notification goes.
    pub(crate) fn notify_recorded(&mut self, title: &str, ep: u32) {
        if self.prefs.notify.recorded {
            self.notify(Kind::Recorded, format!("Recorded \u{b7} Ep {ep}"), title.to_string());
        } else {
            self.withdraw_note();
        }
    }

    /// "Title · Ep 6" once per file, when it is recognised and plays (switch off by default).
    pub(crate) fn announce(&mut self) {
        let Some(n) = &self.now else { return };
        if n.announced || !n.playing || n.recognizing || !self.prefs.notify.detected || self.note_id().is_none() {
            return;
        }
        let (_, title) = crate::ui::now_title(self, n);
        let summary = match n.episode() {
            Some(ep) => format!("{title} \u{b7} Ep {ep}"),
            None => title,
        };
        let body = format!("Playing in {}", n.player);
        if let Some(n) = &mut self.now {
            n.announced = true;
        }
        self.notify(Kind::Detected, summary, body);
    }

    /// The playing episode's notification goes (answered in the window or with `kot`).
    pub(crate) fn withdraw_note(&mut self) {
        if let Some(id) = self.note_id() {
            self.notes.withdraw(&id);
        }
    }

    /// Update in the banner, `kot confirm`: the offer (or the rewatch) is recorded and its
    /// notification goes.
    pub fn confirm_offer(&mut self) -> Result<(String, u32), String> {
        let r = self.record(false)?;
        self.withdraw_note();
        Ok(r)
    }

    /// A notification's button (or the notification itself) was clicked. Buttons act only on
    /// the episode they are about and while it still asks; an old one opens Kotora instead.
    pub fn note_action(&mut self, id: &str, act: Act) -> bool {
        let current = self.note_id().as_deref() == Some(id);
        let recorded = self.now.as_ref().is_some_and(|n| n.recorded.is_some());
        let asks = !recorded && (self.offer().is_some() || self.rewatch_offer().is_some());
        match act {
            Act::Update if current && asks => match self.record(false) {
                Ok((title, ep)) => self.notify_recorded(&title, ep),
                Err(_) => self.open_from_note(),
            },
            Act::NotNow if current && asks => {
                self.dismiss_now();
            }
            Act::Undo if current && recorded => self.undo_record(),
            // A click, or a button about an episode that no longer asks: show what plays.
            _ => self.open_from_note(),
        }
        true
    }

    fn open_from_note(&mut self) {
        if self.now.is_some() {
            self.go(Page::NowPlaying);
        }
        if let Some(h) = &self.handle {
            crate::tray::show_window(h);
        }
    }

    /// Settings → Notifications. Turning one on asks for permission now (macOS), not at an
    /// episode.
    pub fn set_notify(&mut self, kind: Kind, on: bool) {
        let key = match kind {
            Kind::Detected => {
                self.prefs.notify.detected = on;
                "detected"
            }
            Kind::Offer | Kind::Rewatch => {
                self.prefs.notify.offer = on;
                "offer"
            }
            Kind::Recorded => {
                self.prefs.notify.recorded = on;
                "recorded"
            }
        };
        crate::detect::save_setting(self, "notifications", key, on.to_string());
        if on {
            self.notes.prepare();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: Kind) -> Note {
        Note { id: "kotora-7-6".into(), kind, summary: "Update Lanterns to episode 6?".into(), body: String::new(), tail: Some("Open Kotora, or run kot confirm.".into()) }
    }

    #[test]
    fn the_tail_is_only_for_systems_without_buttons() {
        let n = note(Kind::Offer);
        assert_eq!(n.text(true), "");
        assert_eq!(n.text(false), "Open Kotora, or run kot confirm.");
        let n = Note { body: "Skips episode 5.".into(), ..note(Kind::Offer) };
        assert_eq!(n.text(false), "Skips episode 5. Open Kotora, or run kot confirm.");
        // No buttons for this kind anyway: the tail stays.
        assert_eq!(note(Kind::Detected).text(true), "Open Kotora, or run kot confirm.");
    }

    #[test]
    fn action_keys_round_trip_and_unknown_keys_are_ignored() {
        for a in [Act::Update, Act::NotNow, Act::Undo, Act::Open] {
            assert_eq!(Act::from_key(a.key()), Some(a));
        }
        assert_eq!(Act::from_key("com.apple.UNNotificationDismissActionIdentifier"), None);
        assert_eq!(Kind::Offer.actions().iter().map(|(a, _)| *a).collect::<Vec<_>>(), [Act::Update, Act::NotNow]);
        assert_eq!(Kind::Recorded.actions()[0], (Act::Undo, "Undo"));
        assert!(Kind::Detected.actions().is_empty());
    }

    #[test]
    fn the_same_note_is_never_sent_twice_in_a_row_and_only_shown_ones_are_withdrawn() {
        let mut n = Notifier::default();
        n.withdraw("kotora-7-6");
        assert!(n.log.is_empty(), "nothing out, nothing to withdraw");
        n.show(note(Kind::Offer));
        n.show(note(Kind::Offer));
        assert_eq!(n.log.len(), 1);
        n.show(note(Kind::Recorded));
        assert_eq!(n.log.len(), 2);
        n.withdraw("kotora-7-6");
        n.withdraw("kotora-7-6");
        assert_eq!(n.log.last(), Some(&Sent::Withdraw("kotora-7-6".into())));
        assert_eq!(n.log.len(), 3);
        // After a withdraw the same offer may come again (asked again after an edit).
        n.show(note(Kind::Offer));
        assert_eq!(n.log.len(), 4);
    }

    // ------------------------------------------------------------------ the app's rules

    use crate::model::State;
    use crate::now::Seen;
    use kotora_detect::{PlayState, Playing, Switch, Track, Via};

    /// Lanterns of Kirigaoka (id 0) is at 5 of 12 in the fake list.
    fn seen(file: &str, reached: bool) -> Seen {
        let playing = Playing {
            key: "mpv:/tmp/mpv.sock".into(),
            player: "mpv".into(),
            switch: Switch::Mpv,
            via: Via::MpvIpc,
            path: Some(format!("/v/{file}")),
            title: None,
            position: Some(1000.0),
            duration: Some(1420.0),
            state: PlayState::Playing,
            rate: 1.0,
            at: kotora_detect::clock(),
        };
        Seen {
            track: Track { playing, played: 900.0, reached },
            parsed: kotora_core::recognize::parse(&format!("/v/{file}")),
            found: Found::List { media_id: 0, sure: true, why: "test".into() },
            candidates: vec![],
            cover_url: None,
            relation: None,
        }
    }

    fn fresh() -> State {
        let mut s = State::fake();
        s.now = None;
        s
    }

    fn shown(s: &State) -> Vec<Note> {
        s.notes.log.iter().filter_map(|x| if let Sent::Show(n) = x { Some(n.clone()) } else { None }).collect()
    }

    fn progress(s: &State) -> u32 {
        s.entries.iter().find(|e| e.id == 0).unwrap().progress
    }

    const EP6: &str = "Lanterns of Kirigaoka - 06.mkv";

    #[test]
    fn detected_is_off_by_default_and_once_per_file_when_on() {
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, false)));
        assert!(shown(&s).is_empty(), "off by default");
        let mut s = fresh();
        s.prefs.notify.detected = true;
        s.now_seen(Some(seen(EP6, false)));
        s.now_seen(Some(seen(EP6, false)));
        let n = shown(&s);
        assert_eq!(n.len(), 1, "one per file: {n:?}");
        assert_eq!((n[0].kind, n[0].summary.as_str(), n[0].body.as_str()), (Kind::Detected, "Lanterns of Kirigaoka \u{b7} Ep 6", "Playing in mpv"));
        assert_eq!(n[0].id, "kotora-0-ep6");
        // Not recognised: nothing.
        let mut s = fresh();
        s.prefs.notify.detected = true;
        let mut v = seen("Unknown Thing - 03.mkv", false);
        v.found = Found::Nothing;
        s.now_seen(Some(v));
        assert!(shown(&s).is_empty());
    }

    #[test]
    fn the_offer_and_the_record_share_one_id_so_they_replace() {
        let mut s = fresh();
        s.prefs.notify.detected = true;
        s.now_seen(Some(seen(EP6, true)));
        let n = shown(&s);
        assert_eq!(n.iter().map(|n| n.kind).collect::<Vec<_>>(), [Kind::Detected, Kind::Offer]);
        assert_eq!(n[1].summary, "Update Lanterns of Kirigaoka to episode 6?");
        assert_eq!(n[1].text(false), "Open Kotora, or run kot confirm.", "the old text without buttons");
        // The Update button: recorded through the queue, and the Recorded note replaces it.
        assert!(s.note_action("kotora-0-ep6", Act::Update));
        assert_eq!(progress(&s), 6);
        let n = shown(&s);
        let last = n.last().unwrap();
        assert_eq!((last.kind, last.summary.as_str(), last.body.as_str()), (Kind::Recorded, "Recorded \u{b7} Ep 6", "Lanterns of Kirigaoka"));
        assert!(n.iter().all(|x| x.id == "kotora-0-ep6"), "one id per episode");
        // Undo from the notification: back to 5, and it goes.
        assert!(s.note_action("kotora-0-ep6", Act::Undo));
        assert_eq!(progress(&s), 5);
        assert_eq!(s.notes.log.last(), Some(&Sent::Withdraw("kotora-0-ep6".into())));
        assert!(!s.banner());
    }

    #[test]
    fn not_now_from_the_notification_is_the_banners_not_now() {
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, true)));
        assert!(s.banner());
        s.note_action("kotora-0-ep6", Act::NotNow);
        assert!(!s.banner() && s.now.as_ref().unwrap().dismissed);
        assert_eq!(progress(&s), 5);
    }

    #[test]
    fn auto_records_with_an_undo_notification() {
        let mut s = fresh();
        s.prefs.auto = true;
        s.now_seen(Some(seen(EP6, true)));
        assert_eq!(progress(&s), 6);
        let n = shown(&s);
        assert_eq!(n.iter().map(|n| n.kind).collect::<Vec<_>>(), [Kind::Recorded]);
        assert_eq!(n[0].text(false), "Lanterns of Kirigaoka Undo is in Kotora's banner.");
        assert_eq!(n[0].text(true), "Lanterns of Kirigaoka");
    }

    #[test]
    fn switches_off_send_nothing() {
        let mut s = fresh();
        s.prefs.notify.offer = false;
        s.now_seen(Some(seen(EP6, true)));
        assert!(shown(&s).is_empty());
        let mut s = fresh();
        s.prefs.auto = true;
        s.prefs.notify.recorded = false;
        s.now_seen(Some(seen(EP6, true)));
        assert!(shown(&s).is_empty());
        assert_eq!(progress(&s), 6, "recording itself doesn't depend on it");
    }

    #[test]
    fn answered_in_the_window_the_notification_goes() {
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, true)));
        s.confirm_offer().unwrap();
        assert_eq!(s.notes.log.last(), Some(&Sent::Withdraw("kotora-0-ep6".into())));
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, true)));
        s.dismiss_now();
        assert_eq!(s.notes.log.last(), Some(&Sent::Withdraw("kotora-0-ep6".into())));
    }

    #[test]
    fn an_old_notifications_buttons_change_nothing() {
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, true)));
        // Episode 7 plays now; episode 6's Update and Undo are stale.
        s.now_seen(Some(seen("Lanterns of Kirigaoka - 07.mkv", false)));
        s.page = crate::model::Page::List(kotora_core::Status::Watching);
        s.note_action("kotora-0-ep6", Act::Update);
        s.note_action("kotora-0-ep6", Act::Undo);
        assert_eq!(progress(&s), 5);
        assert_eq!(s.page, crate::model::Page::NowPlaying, "it opens what plays instead");
        // Update twice (two clicks, or a click after the banner's Update) records once.
        let mut s = fresh();
        s.now_seen(Some(seen(EP6, true)));
        s.note_action("kotora-0-ep6", Act::Update);
        s.note_action("kotora-0-ep6", Act::Update);
        assert_eq!(progress(&s), 6);
        assert!(s.now.as_ref().unwrap().recorded.is_some_and(|r| r.episode == 6 && r.old_progress == 5), "the second click didn't record again");
    }
}
