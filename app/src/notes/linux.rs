//! Linux: `org.freedesktop.Notifications` on the session bus (zbus 5 blocking, the copy the
//! detection's MPRIS already builds).
//!
//! One thread, `kotora-notify`, owns the connection and does every call; a second one listens
//! for `ActionInvoked` and `NotificationClosed`. The server's capabilities are read once:
//! - `actions`: the buttons are sent (plus `default`, a click on the notification, which shows
//!   Kotora). Without it the notification is the old one, with "Open Kotora, or run kot
//!   confirm." in the text.
//! - `body-markup`: the body is escaped (a title with `&` stays as it is).
//!
//! Each of our ids maps to the server's number, sent as `replaces_id`, so a notification about
//! the same episode replaces the last one. With no server nothing happens and nothing is printed.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use zbus::blocking::Connection;
use zbus::zvariant::Value;

use super::{Act, Backend, Kind, Note, Support};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

enum Cmd {
    Show(Note),
    Withdraw(String),
}

/// Our ids and the server's numbers, both ways (the listener reads it too).
#[derive(Default)]
struct Ids {
    by_note: HashMap<String, u32>,
    by_server: HashMap<u32, String>,
}

impl Ids {
    fn set(&mut self, note: &str, server: u32) {
        if let Some(old) = self.by_note.insert(note.to_string(), server) {
            self.by_server.remove(&old);
        }
        self.by_server.insert(server, note.to_string());
    }

    fn forget_server(&mut self, server: u32) {
        if let Some(note) = self.by_server.remove(&server) {
            self.by_note.remove(&note);
        }
    }
}

pub struct Sys {
    tx: Mutex<Sender<Cmd>>,
}

/// What the server can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Caps {
    pub actions: bool,
    pub markup: bool,
}

impl Caps {
    fn from(list: &[String]) -> Caps {
        Caps { actions: list.iter().any(|c| c == "actions"), markup: list.iter().any(|c| c == "body-markup") }
    }
}

/// The arguments of `Notify`, before they go on the bus.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Request {
    pub replaces_id: u32,
    pub summary: String,
    pub body: String,
    /// Key, label, key, label…
    pub actions: Vec<String>,
    /// `urgency` (0 low, 1 normal) and `transient`.
    pub urgency: u8,
    pub transient: bool,
}

/// Builds the call for `note`; `replaces` is the server's number of the last one (0: none).
pub(super) fn build(note: &Note, replaces: u32, caps: Caps) -> Request {
    let mut actions = Vec::new();
    if caps.actions {
        for (a, label) in note.kind.actions() {
            actions.push(a.key().to_string());
            actions.push((*label).to_string());
        }
        // A click on the notification shows Kotora (not drawn as a button).
        actions.push(Act::Open.key().to_string());
        actions.push("Open Kotora".to_string());
    }
    let body = note.text(caps.actions);
    let body = if caps.markup { escape(&body) } else { body };
    // "Detected" is news, not a question: low urgency and kept out of the history.
    let quiet = note.kind == Kind::Detected;
    Request { replaces_id: replaces, summary: note.summary.clone(), body, actions, urgency: if quiet { 0 } else { 1 }, transient: quiet }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

impl Sys {
    pub fn start(on_action: Arc<dyn Fn(String, Act) + Send + Sync>, on_support: Arc<dyn Fn(Support) + Send + Sync>) -> Sys {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let _ = std::thread::Builder::new().name("kotora-notify".into()).spawn(move || {
            let ids = Arc::new(Mutex::new(Ids::default()));
            // Connected at once, to tell Settings whether buttons work; again after a failure.
            let mut bus: Option<(Connection, Caps)> = connect(&ids, &on_action, &on_support);
            for cmd in rx {
                if bus.is_none() {
                    bus = connect(&ids, &on_action, &on_support);
                }
                let Some((conn, caps)) = &bus else { continue };
                let ok = match cmd {
                    Cmd::Show(note) => {
                        let replaces = ids.lock().map(|i| i.by_note.get(&note.id).copied().unwrap_or(0)).unwrap_or(0);
                        match notify(conn, &build(&note, replaces, *caps)) {
                            Ok(server) => {
                                if let Ok(mut i) = ids.lock() {
                                    i.set(&note.id, server);
                                }
                                true
                            }
                            Err(_) => false,
                        }
                    }
                    Cmd::Withdraw(id) => {
                        let server = ids.lock().ok().and_then(|mut i| {
                            let n = i.by_note.get(&id).copied()?;
                            i.forget_server(n);
                            Some(n)
                        });
                        server.is_none_or(|n| conn.call_method(Some(DEST), PATH, Some(DEST), "CloseNotification", &(n,)).is_ok())
                    }
                };
                if !ok {
                    // The server went away: a fresh connection next time.
                    bus = None;
                }
            }
        });
        Sys { tx: Mutex::new(tx) }
    }

    fn send(&self, cmd: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(cmd);
        }
    }
}

impl Backend for Sys {
    fn show(&self, note: &Note) {
        self.send(Cmd::Show(note.clone()));
    }

    fn withdraw(&self, id: &str) {
        self.send(Cmd::Withdraw(id.to_string()));
    }
}

/// Connects, reads the capabilities, says what Settings should show, and starts the listener.
fn connect(ids: &Arc<Mutex<Ids>>, on_action: &Arc<dyn Fn(String, Act) + Send + Sync>, on_support: &Arc<dyn Fn(Support) + Send + Sync>) -> Option<(Connection, Caps)> {
    let conn = Connection::session().ok()?;
    let caps = conn
        .call_method(Some(DEST), PATH, Some(DEST), "GetCapabilities", &())
        .ok()
        .and_then(|r| r.body().deserialize::<Vec<String>>().ok())
        .map(|l| Caps::from(&l))?;
    on_support(if caps.actions { Support::Buttons } else { Support::NoButtons });
    if caps.actions {
        listen(conn.clone(), ids.clone(), on_action.clone());
    }
    Some((conn, caps))
}

fn notify(conn: &Connection, r: &Request) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, Value<'_>> = HashMap::from([("desktop-entry", Value::from(crate::APP_ID)), ("urgency", Value::from(r.urgency))]);
    if r.transient {
        hints.insert("transient", Value::from(true));
    }
    let actions: Vec<&str> = r.actions.iter().map(String::as_str).collect();
    let reply = conn.call_method(
        Some(DEST),
        PATH,
        Some(DEST),
        "Notify",
        &("Kotora", r.replaces_id, crate::APP_ID, r.summary.as_str(), r.body.as_str(), actions, hints, -1i32),
    )?;
    reply.body().deserialize::<u32>()
}

/// The listener: a button (or the notification) clicked, or a notification gone.
fn listen(conn: Connection, ids: Arc<Mutex<Ids>>, on_action: Arc<dyn Fn(String, Act) + Send + Sync>) {
    let _ = std::thread::Builder::new().name("kotora-notify-actions".into()).spawn(move || {
        let Ok(rule) = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(DEST).and_then(|b| b.path(PATH)).map(|b| b.build()) else { return };
        let Ok(iter) = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None) else { return };
        for msg in iter.flatten() {
            let header = msg.header();
            match header.member().map(|m| m.as_str()) {
                Some("ActionInvoked") => {
                    let Ok((server, key)) = msg.body().deserialize::<(u32, String)>() else { continue };
                    // Only ours: other apps' notifications share the server.
                    let id = ids.lock().ok().and_then(|i| i.by_server.get(&server).cloned());
                    if let (Some(id), Some(act)) = (id, Act::from_key(&key)) {
                        on_action(id, act);
                    }
                }
                Some("NotificationClosed") => {
                    if let Ok((server, _reason)) = msg.body().deserialize::<(u32, u32)>()
                        && let Ok(mut i) = ids.lock()
                    {
                        i.forget_server(server);
                    }
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: Kind) -> Note {
        Note { id: "kotora-7-6".into(), kind, summary: "Update Kaze & Mori to episode 6?".into(), body: "Skips episode 5.".into(), tail: Some("Open Kotora, or run kot confirm.".into()) }
    }

    #[test]
    fn with_actions_the_buttons_and_a_click_are_sent() {
        let caps = Caps::from(&["actions".into(), "body".into()]);
        let r = build(&note(Kind::Offer), 12, caps);
        assert_eq!(r.actions, ["update", "Update", "not-now", "Not now", "default", "Open Kotora"]);
        assert_eq!(r.body, "Skips episode 5.");
        assert_eq!((r.replaces_id, r.urgency, r.transient), (12, 1, false));
        let r = build(&note(Kind::Recorded), 0, caps);
        assert_eq!(r.actions, ["undo", "Undo", "default", "Open Kotora"]);
    }

    #[test]
    fn without_actions_it_is_the_old_notification() {
        let r = build(&note(Kind::Offer), 0, Caps::from(&["body".into()]));
        assert!(r.actions.is_empty());
        assert_eq!(r.body, "Skips episode 5. Open Kotora, or run kot confirm.");
    }

    #[test]
    fn markup_servers_get_an_escaped_body_and_detected_is_quiet() {
        let caps = Caps::from(&["actions".into(), "body-markup".into()]);
        let n = Note { body: "Kaze & Mori <TV>".into(), ..note(Kind::Detected) };
        let r = build(&n, 0, caps);
        assert_eq!(r.body, "Kaze &amp; Mori &lt;TV&gt; Open Kotora, or run kot confirm.", "no buttons for Detected: the tail stays");
        assert_eq!(r.summary, "Update Kaze & Mori to episode 6?", "the summary is never markup");
        assert_eq!((r.urgency, r.transient), (0, true));
        assert_eq!(r.actions, ["default", "Open Kotora"]);
    }

    #[test]
    fn ids_map_both_ways_and_a_replacement_drops_the_old_number() {
        let mut i = Ids::default();
        i.set("a", 3);
        i.set("a", 4);
        assert_eq!(i.by_note.get("a"), Some(&4));
        assert!(!i.by_server.contains_key(&3));
        i.forget_server(4);
        assert!(i.by_note.is_empty() && i.by_server.is_empty());
    }
}
