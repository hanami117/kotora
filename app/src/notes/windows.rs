//! Windows: toast notifications (WinRT `Windows.UI.Notifications`, through the `windows` crate
//! Rune already builds).
//!
//! One thread, `kotora-notify`, owns the notifier and does every call. A toast is a small XML
//! text ([`toast_xml`]): the summary, the body, and the kind's buttons. The buttons and a click
//! on the toast come back through the toast's `Activated` event **while Kotora runs** (it runs
//! in the notification area whenever a toast is up). Kotora has no COM activator, so a toast
//! left in the notification centre after Kotora quit does nothing when clicked; the ones left
//! from the last run are cleared at start for that reason.
//!
//! Every toast carries Kotora's app id (`win::set_app_id`, registered with its name and icon
//! by `win::register_app_id`) and the note's id as its tag, so a new toast about the same
//! episode replaces the last one, in the notification centre too.
//!
//! Windows itself may hold a toast back (Do not disturb, or a full-screen video with "when
//! using an app in full-screen mode" on, which is Windows' default): it then goes straight to
//! the notification centre. Kotora's banner shows either way.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{NotificationSetting, ToastActivatedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier};
use windows::core::{HSTRING, IInspectable, Interface};

use super::{Act, Backend, Kind, Note, Support};

/// Every toast of Kotora's is in this group (a tag is unique inside its group).
const GROUP: &str = "kotora";

enum Cmd {
    Show(Note),
    Withdraw(String),
    Prepare,
}

pub struct Sys {
    tx: Mutex<Sender<Cmd>>,
}

/// The button's label on Windows: the offer is a question ("Update … to episode 6?"), so its
/// first button answers it with Yes; the others are the same words as everywhere.
pub(super) fn label(kind: Kind, act: Act, default: &'static str) -> &'static str {
    match (kind, act) {
        (Kind::Offer | Kind::ReadOffer, Act::Update) => "Yes",
        _ => default,
    }
}

/// The toast's tag: the note's id (Windows allows 64 characters; a longer one is cut to a
/// stable short form).
pub(super) fn tag(id: &str) -> String {
    let id = id.strip_prefix("kotora-").unwrap_or(id);
    if id.chars().count() <= 64 {
        return id.to_string();
    }
    // FNV-1a: stable across runs, which a tag must be.
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{}-{hash:016x}", id.chars().take(40).collect::<String>())
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 has no control characters but tab and the line ends.
            c if c.is_control() && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// The toast: the summary in bold, the body under it, the kind's buttons. A click on the toast
/// itself comes back as `default` (show Kotora); a button as its action key.
pub(super) fn toast_xml(note: &Note) -> String {
    let mut xml = format!("<toast launch=\"{}\" activationType=\"foreground\"><visual><binding template=\"ToastGeneric\"><text>{}</text>", Act::Open.key(), escape(&note.summary));
    let body = note.text(true);
    if !body.is_empty() {
        xml.push_str(&format!("<text>{}</text>", escape(&body)));
    }
    xml.push_str("</binding></visual>");
    let actions = note.kind.actions();
    if !actions.is_empty() {
        xml.push_str("<actions>");
        for (act, default) in actions {
            xml.push_str(&format!("<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>", escape(label(note.kind, *act, default)), act.key()));
        }
        xml.push_str("</actions>");
    }
    // "Detected" is news, not a question: no sound.
    if note.kind == Kind::Detected {
        xml.push_str("<audio silent=\"true\"/>");
    }
    xml.push_str("</toast>");
    xml
}

/// What Windows' own switches say (Settings → System → Notifications).
fn support(notifier: &ToastNotifier) -> Support {
    match notifier.Setting() {
        Ok(NotificationSetting::Enabled) => Support::Buttons,
        Ok(_) => Support::Denied,
        // Not known (an old Windows refuses the question for an unpackaged app): assume on.
        Err(_) => Support::Buttons,
    }
}

type OnAction = Arc<dyn Fn(String, Act) + Send + Sync>;

/// Builds the toast for `note`, with its events wired to `on_action`.
fn toast(note: &Note, on_action: &OnAction) -> windows::core::Result<ToastNotification> {
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast_xml(note)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(tag(&note.id)))?;
    toast.SetGroup(&HSTRING::from(GROUP))?;
    // Gone after a restart of Windows: its buttons would be dead (older Windows 10: no such
    // setting, and the toast shows all the same).
    let _ = toast.SetExpiresOnReboot(true);
    let (id, on_action) = (note.id.clone(), on_action.clone());
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
        // A button gives its `arguments`; the toast itself its `launch` text.
        let key = args.as_ref().and_then(|a| a.cast::<ToastActivatedEventArgs>().ok()).and_then(|a| a.Arguments().ok()).map(|a| a.to_string()).unwrap_or_default();
        on_action(id.clone(), Act::from_key(&key).unwrap_or(Act::Open));
        Ok(())
    }))?;
    Ok(toast)
}

impl Sys {
    pub fn start(on_action: OnAction, on_support: Arc<dyn Fn(Support) + Send + Sync>) -> Sys {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let _ = std::thread::Builder::new().name("kotora-notify".into()).spawn(move || {
            let app_id = HSTRING::from(crate::APP_ID);
            // The app id's name and icon, for a copy with no installer; then the notifier.
            crate::win::register_app_id();
            let mut notifier: Option<ToastNotifier> = None;
            let connect = |notifier: &mut Option<ToastNotifier>| {
                if notifier.is_none() {
                    match ToastNotificationManager::CreateToastNotifierWithId(&app_id) {
                        Ok(n) => {
                            on_support(support(&n));
                            *notifier = Some(n);
                        }
                        Err(e) => eprintln!("kotora: no notifications: {e}"),
                    }
                }
            };
            connect(&mut notifier);
            // Toasts left from the last run can't answer any more: take them away.
            if let Ok(history) = ToastNotificationManager::History() {
                let _ = history.ClearWithId(&app_id);
            }
            // Kept while shown: a toast's events live as long as the object does.
            let mut shown: HashMap<String, ToastNotification> = HashMap::new();
            for cmd in rx {
                connect(&mut notifier);
                let Some(n) = &notifier else { continue };
                match cmd {
                    Cmd::Show(note) => match toast(&note, &on_action).and_then(|t| n.Show(&t).map(|()| t)) {
                        Ok(t) => {
                            shown.insert(note.id, t);
                        }
                        Err(e) => {
                            eprintln!("kotora: a notification wasn't shown: {e}");
                            // The notifier may have gone stale (Explorer restarted): a fresh one next time.
                            notifier = None;
                        }
                    },
                    Cmd::Withdraw(id) => {
                        if let Some(t) = shown.remove(&id) {
                            // Off the screen, then out of the notification centre.
                            let _ = n.Hide(&t);
                            if let Ok(history) = ToastNotificationManager::History() {
                                let _ = history.RemoveGroupedTagWithId(&HSTRING::from(tag(&id)), &HSTRING::from(GROUP), &app_id);
                            }
                        }
                    }
                    Cmd::Prepare => on_support(support(n)),
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

    /// A switch was turned on in Settings: read Windows' own switch again, so the warning
    /// under the switches is current.
    fn prepare(&self) {
        self.send(Cmd::Prepare);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: Kind) -> Note {
        Note { id: "kotora-7-ep6".into(), kind, summary: "Update Kaze & Mori to episode 6?".into(), body: "Skips episode 5.".into(), tail: Some("Open Kotora, or run kot confirm.".into()) }
    }

    #[test]
    fn the_offer_asks_with_yes_and_not_now() {
        let xml = toast_xml(&note(Kind::Offer));
        assert_eq!(
            xml,
            "<toast launch=\"default\" activationType=\"foreground\"><visual><binding template=\"ToastGeneric\">\
             <text>Update Kaze &amp; Mori to episode 6?</text><text>Skips episode 5.</text></binding></visual>\
             <actions><action content=\"Yes\" arguments=\"update\" activationType=\"foreground\"/>\
             <action content=\"Not now\" arguments=\"not-now\" activationType=\"foreground\"/></actions></toast>"
        );
        // The other kinds keep the shared words.
        assert!(toast_xml(&note(Kind::Rewatch)).contains("content=\"Start rewatch\" arguments=\"update\""));
        assert!(toast_xml(&note(Kind::Recorded)).contains("content=\"Undo\" arguments=\"undo\""));
        assert!(toast_xml(&note(Kind::AiredPlay)).contains("content=\"Play\" arguments=\"play\""));
    }

    #[test]
    fn news_has_no_buttons_and_detected_is_silent() {
        let n = Note { body: String::new(), tail: None, ..note(Kind::Aired) };
        let xml = toast_xml(&n);
        assert!(!xml.contains("<actions>") && !xml.contains("<audio"), "{xml}");
        assert_eq!(xml.matches("<text>").count(), 1, "an empty body is left out");
        let xml = toast_xml(&Note { tail: None, ..note(Kind::Detected) });
        assert!(xml.ends_with("<audio silent=\"true\"/></toast>"), "{xml}");
    }

    #[test]
    fn odd_titles_stay_valid_xml() {
        let n = Note { summary: "<\"A\" & 'B'>\u{1} \u{5c0f}\u{864e}".into(), ..note(Kind::Started) };
        let xml = toast_xml(&n);
        assert!(xml.contains("<text>&lt;&quot;A&quot; &amp; &apos;B&apos;&gt; \u{5c0f}\u{864e}</text>"), "{xml}");
    }

    #[test]
    fn the_tag_is_the_id_and_a_long_one_is_cut_the_same_way_each_time() {
        assert_eq!(tag("kotora-7-ep6"), "7-ep6");
        assert_eq!(tag("kotora-170942-started"), "170942-started");
        let long = format!("kotora-{}", "x".repeat(90));
        assert_eq!(tag(&long), tag(&long));
        assert!(tag(&long).chars().count() <= 64);
        assert_ne!(tag(&long), tag(&format!("{long}y")));
    }

    /// Windows itself reads every kind's toast (the same parser the notifier uses), and a
    /// toast object with its tag, group and event can be made. Nothing is shown.
    #[test]
    fn windows_accepts_every_toast() {
        let on_action: OnAction = Arc::new(|_, _| {});
        for kind in [Kind::Detected, Kind::Offer, Kind::Rewatch, Kind::Recorded, Kind::Started, Kind::Aired, Kind::AiredPlay] {
            let n = Note { summary: "<Kaze> & \"Mori\" \u{b7} Ep 6".into(), ..note(kind) };
            let t = toast(&n, &on_action).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert_eq!(t.Tag().unwrap().to_string(), "7-ep6");
            assert_eq!(t.Group().unwrap().to_string(), GROUP);
            let texts = t.Content().unwrap().GetElementsByTagName(&HSTRING::from("text")).unwrap();
            assert_eq!(texts.Item(0).unwrap().InnerText().unwrap().to_string(), "<Kaze> & \"Mori\" \u{b7} Ep 6", "{kind:?}");
            let buttons = t.Content().unwrap().GetElementsByTagName(&HSTRING::from("action")).unwrap();
            assert_eq!(buttons.Length().unwrap() as usize, kind.actions().len(), "{kind:?}");
        }
    }
}
