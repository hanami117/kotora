//! macOS: `UNUserNotificationCenter` inside Kotora.app (`io.github.hanami117.kotora`), through
//! objc2 messages (no extra crate; the framework is linked here).
//!
//! - At start: the delegate (button clicks, and "show while Kotora is in front") and the
//!   categories with their buttons. The permission state is read without asking.
//! - Permission is asked once, nicely: at the first notification (or when a Notifications
//!   switch is turned on), never at launch. The system remembers the answer; Settings says when
//!   notifications are off for Kotora.
//! - Each episode has one request identifier, so its next notification replaces the last.
//!
//! A bare binary (`cargo run`, `target/release/kotora`) has no bundle identifier and the
//! framework would raise an exception: there, `osascript`'s `display notification` as before,
//! without buttons (text passed as arguments, never quoted into the script).

use std::sync::{Arc, OnceLock};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::{NSArray, NSString};

use super::{Act, Backend, Kind, Note, Support};

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// `UNAuthorizationOptionAlert`: banners only, no sound, no badge.
const AUTH_ALERT: usize = 1 << 2;
/// `UNNotificationPresentationOptionList | Banner`: shown while Kotora is in front too, as on
/// Linux.
const PRESENT_LIST_BANNER: usize = (1 << 3) | (1 << 4);
/// `UNAuthorizationStatusDenied`.
const STATUS_DENIED: isize = 1;
/// The identifier of a click on the notification itself.
const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";

type OnAction = Arc<dyn Fn(String, Act) + Send + Sync>;
type OnSupport = Arc<dyn Fn(Support) + Send + Sync>;

/// The delegate's way back to the app (one per process).
static ON_ACTION: OnceLock<OnAction> = OnceLock::new();

pub fn start(on_action: OnAction, on_support: OnSupport) -> Box<dyn Backend> {
    if !bundled() {
        on_support(Support::Outside);
        return Box::new(Script);
    }
    let _ = ON_ACTION.set(on_action);
    install();
    read_status(on_support.clone());
    Box::new(Center { on_support })
}

/// Runs from a `.app` with a bundle identifier.
fn bundled() -> bool {
    let Some(class) = AnyClass::get(c"NSBundle") else { return false };
    // SAFETY: no-argument getters on the main bundle.
    unsafe {
        let bundle: Option<Retained<AnyObject>> = msg_send![class, mainBundle];
        let Some(bundle) = bundle else { return false };
        let id: Option<Retained<NSString>> = msg_send![&*bundle, bundleIdentifier];
        let path: Option<Retained<NSString>> = msg_send![&*bundle, bundlePath];
        id.is_some() && path.is_some_and(|p| p.to_string().ends_with(".app"))
    }
}

fn center() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"UNUserNotificationCenter")?;
    // SAFETY: a class getter; only called inside a bundle (checked in `start`).
    unsafe { msg_send![class, currentNotificationCenter] }
}

/// The delegate and the categories, once.
fn install() {
    let Some(center) = center() else { return };
    // SAFETY: the delegate class's methods match the protocol's selectors (see below); the
    // center keeps its delegate weakly, so this one is never released.
    unsafe {
        let delegate: Retained<AnyObject> = msg_send![delegate_class(), new];
        let _: () = msg_send![&*center, setDelegate: &*delegate];
        std::mem::forget(delegate);
    }
    let (Some(action_class), Some(category_class), Some(set_class)) = (AnyClass::get(c"UNNotificationAction"), AnyClass::get(c"UNNotificationCategory"), AnyClass::get(c"NSSet")) else {
        return;
    };
    let mut categories: Vec<Retained<AnyObject>> = Vec::new();
    for kind in [Kind::Detected, Kind::Offer, Kind::Rewatch, Kind::Recorded, Kind::Started, Kind::Aired, Kind::AiredPlay, Kind::Chapter] {
        // SAFETY: the documented factory methods, with NSString / NSArray arguments and option
        // masks of 0 (no foreground launch, no authentication, not destructive).
        unsafe {
            let actions: Vec<Retained<AnyObject>> = kind
                .actions()
                .iter()
                .map(|(a, label)| {
                    let id = NSString::from_str(a.key());
                    let title = NSString::from_str(label);
                    msg_send![action_class, actionWithIdentifier: &*id, title: &*title, options: 0usize]
                })
                .collect();
            let actions = NSArray::from_retained_slice(&actions);
            let intents: Retained<NSArray<AnyObject>> = NSArray::new();
            let id = NSString::from_str(kind.category());
            let category: Retained<AnyObject> = msg_send![category_class, categoryWithIdentifier: &*id, actions: &*actions, intentIdentifiers: &*intents, options: 0usize];
            categories.push(category);
        }
    }
    // SAFETY: an NSSet of the categories above.
    unsafe {
        let array = NSArray::from_retained_slice(&categories);
        let set: Retained<AnyObject> = msg_send![set_class, setWithArray: &*array];
        let _: () = msg_send![&*center, setNotificationCategories: &*set];
    }
}

/// Reads whether notifications are off for Kotora, without asking.
fn read_status(on_support: OnSupport) {
    let Some(center) = center() else { return };
    let done = RcBlock::new(move |settings: *mut AnyObject| {
        if settings.is_null() {
            return;
        }
        // SAFETY: `settings` is a UNNotificationSettings for the length of the block.
        let status: isize = unsafe { msg_send![settings, authorizationStatus] };
        on_support(if status == STATUS_DENIED { Support::Denied } else { Support::Buttons });
    });
    // SAFETY: the selector takes a `void (^)(UNNotificationSettings *)`, copied by the center.
    unsafe {
        let _: () = msg_send![&*center, getNotificationSettingsWithCompletionHandler: &*done];
    }
}

/// Inside Kotora.app.
struct Center {
    on_support: OnSupport,
}

impl Center {
    /// Asks for permission (the system prompts only the first time), then runs `then` if allowed.
    fn authorized(&self, then: impl Fn() + 'static) {
        let Some(center) = center() else { return };
        let on_support = self.on_support.clone();
        let done = RcBlock::new(move |granted: Bool, _error: *mut AnyObject| {
            let granted = granted.as_bool();
            on_support(if granted { Support::Buttons } else { Support::Denied });
            if granted {
                then();
            }
        });
        // SAFETY: (NSUInteger, void (^)(BOOL, NSError *)); the block is copied by the center.
        unsafe {
            let _: () = msg_send![&*center, requestAuthorizationWithOptions: AUTH_ALERT, completionHandler: &*done];
        }
    }
}

impl Backend for Center {
    fn show(&self, note: &Note) {
        let note = note.clone();
        self.authorized(move || add(&note));
    }

    fn withdraw(&self, id: &str) {
        let Some(center) = center() else { return };
        let id = NSString::from_str(id);
        let ids = NSArray::from_retained_slice(&[id]);
        // SAFETY: both take an NSArray of identifiers.
        unsafe {
            let _: () = msg_send![&*center, removeDeliveredNotificationsWithIdentifiers: &*ids];
            let _: () = msg_send![&*center, removePendingNotificationRequestsWithIdentifiers: &*ids];
        }
    }

    fn prepare(&self) {
        self.authorized(|| {});
    }
}

fn add(note: &Note) {
    let (Some(center), Some(content_class), Some(request_class)) = (center(), AnyClass::get(c"UNMutableNotificationContent"), AnyClass::get(c"UNNotificationRequest")) else {
        return;
    };
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: objects made here, with the argument types of the UserNotifications selectors;
        // each stays alive across its call.
        unsafe {
            let content: Retained<AnyObject> = msg_send![content_class, new];
            let title = NSString::from_str(&note.summary);
            let body = NSString::from_str(&note.text(true));
            let category = NSString::from_str(note.kind.category());
            let _: () = msg_send![&*content, setTitle: &*title];
            let _: () = msg_send![&*content, setBody: &*body];
            let _: () = msg_send![&*content, setCategoryIdentifier: &*category];
            let id = NSString::from_str(&note.id);
            let request: Retained<AnyObject> = msg_send![request_class, requestWithIdentifier: &*id, content: &*content, trigger: Option::<&AnyObject>::None];
            let done = RcBlock::new(|error: *mut AnyObject| {
                if !error.is_null() {
                    eprintln!("kotora: notification failed");
                }
            });
            let _: () = msg_send![&*center, addNotificationRequest: &*request, withCompletionHandler: &*done];
        }
    });
}

/// `NSString *` → String.
unsafe fn string(s: *mut AnyObject) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller passes an NSString.
    let s: &NSString = unsafe { &*s.cast::<NSString>() };
    Some(s.to_string())
}

/// What a click means: our button keys, or a click on the notification.
fn act_for(identifier: &str) -> Option<Act> {
    if identifier == DEFAULT_ACTION {
        return Some(Act::Open);
    }
    Act::from_key(identifier).filter(|a| *a != Act::Open)
}

extern "C-unwind" fn did_receive(_this: *mut AnyObject, _cmd: Sel, _center: *mut AnyObject, response: *mut AnyObject, done: &DynBlock<dyn Fn()>) {
    let got = objc2::rc::autoreleasepool(|_| {
        if response.is_null() {
            return None;
        }
        // SAFETY: a UNNotificationResponse and its documented getters.
        unsafe {
            let action = string(msg_send![response, actionIdentifier])?;
            let notification: *mut AnyObject = msg_send![response, notification];
            let request: *mut AnyObject = msg_send![notification, request];
            let id = string(msg_send![request, identifier])?;
            Some((id, action))
        }
    });
    if let Some((id, action)) = got
        && let (Some(act), Some(f)) = (act_for(&action), ON_ACTION.get())
    {
        f(id, act);
    }
    done.call(());
}

extern "C-unwind" fn will_present(_this: *mut AnyObject, _cmd: Sel, _center: *mut AnyObject, _notification: *mut AnyObject, done: &DynBlock<dyn Fn(usize)>) {
    done.call((PRESENT_LIST_BANNER,));
}

fn delegate_class() -> &'static AnyClass {
    if let Some(c) = AnyClass::get(c"KotoraNotificationDelegate") {
        return c;
    }
    let superclass = AnyClass::get(c"NSObject").expect("NSObject");
    let mut b = ClassBuilder::new(c"KotoraNotificationDelegate", superclass).expect("a new class name");
    if let Some(p) = AnyProtocol::get(c"UNUserNotificationCenterDelegate") {
        b.add_protocol(p);
    }
    // SAFETY: the signatures are the protocol's (center, response or notification, block).
    unsafe {
        b.add_method(sel!(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:), did_receive as extern "C-unwind" fn(_, _, _, _, _));
        b.add_method(sel!(userNotificationCenter:willPresentNotification:withCompletionHandler:), will_present as extern "C-unwind" fn(_, _, _, _, _));
    }
    b.register()
}

/// Outside a bundle: `osascript`, as before Kotora.app existed. No buttons, so the text says
/// how to answer.
struct Script;

impl Backend for Script {
    fn show(&self, note: &Note) {
        let (summary, body) = (note.summary.clone(), note.text(false));
        let _ = std::thread::Builder::new().name("kotora-notify".into()).spawn(move || {
            let st = std::process::Command::new("/usr/bin/osascript")
                .args(["-e", "on run argv", "-e", "display notification (item 2 of argv) with title \"Kotora\" subtitle (item 1 of argv)", "-e", "end run", &summary, &body])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if !st.is_ok_and(|s| s.success()) {
                eprintln!("kotora: notification failed");
            }
        });
    }

    /// `display notification` can't take one back.
    fn withdraw(&self, _id: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_map_to_actions() {
        assert_eq!(act_for(DEFAULT_ACTION), Some(Act::Open));
        assert_eq!(act_for("update"), Some(Act::Update));
        assert_eq!(act_for("not-now"), Some(Act::NotNow));
        assert_eq!(act_for("undo"), Some(Act::Undo));
        // Dismissing is not an answer; "default" is only the D-Bus name.
        assert_eq!(act_for("com.apple.UNNotificationDismissActionIdentifier"), None);
        assert_eq!(act_for("default"), None);
    }

    #[test]
    fn a_test_binary_is_not_bundled() {
        assert!(!bundled());
    }

    #[test]
    fn the_delegate_class_has_both_methods() {
        let c = delegate_class();
        assert!(c.responds_to(sel!(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:)));
        assert!(c.responds_to(sel!(userNotificationCenter:willPresentNotification:withCompletionHandler:)));
    }
}
