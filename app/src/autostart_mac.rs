//! macOS start at login: the app bundle as a login item through `SMAppService.mainApp`
//! (ServiceManagement, macOS 13+, the app's minimum). It is listed in System Settings → General
//! → Login Items → "Open at Login". Works for the ad-hoc signed bundle. Only a built
//! `Kotora.app` can be one: `cargo run` has no bundle, and a copy on a disk image or in App
//! Translocation (opened from Downloads with the quarantine flag) would point at a path that
//! goes away. Same approach as Uzumaki's `login_mac.rs`.
//!
//! A login item gets no arguments, so `--hidden` can't be passed. `watch_launch` tells the app
//! whether this launch is the login one: the `kAEOpenApplication` Apple event that starts a login
//! item carries `keyAEPropData = keyAELaunchedAsLogInItem`, readable while AppKit posts
//! `NSApplicationDidFinishLaunchingNotification`.

use std::cell::Cell;
use std::path::Path;
use std::process::{Command, Stdio};

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSError, NSString};

use super::{APP_ID, Login, Migration};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

/// `SMAppServiceStatus`.
const NOT_REGISTERED: isize = 0;
const ENABLED: isize = 1;
const REQUIRES_APPROVAL: isize = 2;

const fn fourcc(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

/// Why this copy can't be a login item, from its bundle path.
fn unavailable_for(bundle: &str) -> Option<&'static str> {
    if !bundle.trim_end_matches('/').ends_with(".app") {
        Some("Only in the app (Kotora.app). This copy runs from a build folder.")
    } else if bundle.starts_with("/Volumes/") || bundle.contains("/AppTranslocation/") {
        Some("Move Kotora to the Applications folder first.")
    } else {
        None
    }
}

/// What the login item's status number means for the switch.
fn from_status(st: isize) -> Login {
    Login {
        on: matches!(st, ENABLED | REQUIRES_APPROVAL),
        needs_approval: st == REQUIRES_APPROVAL,
        ..Login::default()
    }
}

/// A second copy for tests never touches the real login item.
fn test_instance() -> bool {
    std::env::var_os("KOTORA_TEST_INSTANCE").is_some_and(|v| !v.is_empty())
}

fn bundle_path() -> String {
    objc2_foundation::NSBundle::mainBundle()
        .bundlePath()
        .to_string()
}

fn unavailable_now() -> Option<&'static str> {
    if test_instance() {
        return Some("Off in test instances.");
    }
    unavailable_for(&bundle_path())
}

fn service() -> Option<Retained<AnyObject>> {
    let cls = AnyClass::get(c"SMAppService")?;
    // SAFETY: `+[SMAppService mainAppService]` returns the shared instance (or nil).
    unsafe { msg_send![cls, mainAppService] }
}

fn status(svc: &AnyObject) -> isize {
    // SAFETY: `status` is an NSInteger property.
    unsafe { msg_send![svc, status] }
}

/// The login item's state.
pub fn read() -> Login {
    if let Some(why) = unavailable_now() {
        return Login::unavailable(why);
    }
    let Some(svc) = service() else {
        return Login::unavailable("Needs macOS 13 or later.");
    };
    from_status(status(&svc))
}

/// Registers or unregisters the login item.
pub fn set(on: bool) -> Result<Login, String> {
    if let Some(why) = unavailable_now() {
        return Err(why.to_string());
    }
    let svc = service().ok_or("needs macOS 13 or later")?;
    let st = status(&svc);
    let result: Result<(), Retained<NSError>> = if on {
        if matches!(st, ENABLED | REQUIRES_APPROVAL) {
            Ok(())
        } else {
            // SAFETY: `-registerAndReturnError:` returns BOOL with an NSError out-parameter.
            unsafe { msg_send![&*svc, registerAndReturnError: _] }
        }
    } else if st == NOT_REGISTERED {
        Ok(())
    } else {
        // SAFETY: as above.
        unsafe { msg_send![&*svc, unregisterAndReturnError: _] }
    };
    match result {
        Ok(()) => Ok(from_status(status(&svc))),
        Err(e) => Err(e.localizedDescription().to_string()),
    }
}

/// Opens System Settings → General → Login Items (to approve a waiting item).
pub fn open_login_items() {
    if let Some(cls) = AnyClass::get(c"SMAppService") {
        // SAFETY: a class method with no arguments and no result.
        unsafe {
            let _: () = msg_send![cls, openSystemSettingsLoginItems];
        }
    }
}

/// Moves the old LaunchAgent (`~/Library/LaunchAgents/<app id>.plist`, written by Kotora before it
/// used `SMAppService`) over to the login item, once, keeping the user's choice: if the agent was
/// there, the login item is on. The login item is registered first, so a failure leaves the old
/// agent working. Logs what it does; returns the new state when something changed. On a worker
/// thread, never headless or in a test instance (`read` is unavailable there).
pub fn migrate_old_entry() -> Option<Login> {
    let path = super::old_launch_agent_path();
    let old = super::read_old_entry(&path);
    let login = read();
    match super::decide_migration(old, &login) {
        Migration::Nothing => None,
        Migration::KeepOld(why) => {
            eprintln!("kotora: login item: the old LaunchAgent stays for now ({why})");
            None
        }
        Migration::RemoveOld => {
            remove_old_agent(&path);
            Some(login)
        }
        Migration::RegisterThenRemoveOld => match set(true) {
            Ok(now) => {
                eprintln!("kotora: login item: registered (it replaces the old LaunchAgent)");
                remove_old_agent(&path);
                Some(now)
            }
            Err(e) => {
                eprintln!("kotora: login item: can't register ({e}); the old LaunchAgent stays");
                None
            }
        },
    }
}

/// Stops the agent if launchd has it loaded, then deletes its plist. When this very process was
/// started by that agent, stopping it would kill us: the plist goes and launchd drops the job at
/// the next login.
fn remove_old_agent(path: &Path) {
    let by_agent = std::env::var("XPC_SERVICE_NAME").is_ok_and(|v| v == APP_ID);
    if by_agent {
        eprintln!("kotora: login item: started by the old LaunchAgent, so it is not stopped now");
    } else if let Some(uid) = user_id() {
        let stopped = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{APP_ID}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if stopped {
            eprintln!("kotora: login item: stopped the old LaunchAgent");
        }
    }
    match super::remove(path) {
        Ok(()) => eprintln!("kotora: login item: removed the old LaunchAgent file"),
        Err(e) => eprintln!("kotora: login item: {e}"),
    }
}

fn user_id() -> Option<u32> {
    let out = Command::new("id").arg("-u").output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Calls `f` once, as AppKit finishes launching, with true when macOS started the app as a
/// login item. Register before the app runs (from `main`).
pub fn watch_launch(f: impl FnOnce(bool) + 'static) {
    let f = Cell::new(Some(f));
    let block = block2::RcBlock::new(move |_note: *mut AnyObject| {
        if let Some(f) = f.take() {
            f(launched_at_login());
        }
    });
    let name = NSString::from_str("NSApplicationDidFinishLaunchingNotification");
    // SAFETY: NSNotificationCenter's block observer; the centre keeps the block, and the token is
    // kept for the process's life (the notification comes once).
    unsafe {
        let center: Retained<AnyObject> = msg_send![
            AnyClass::get(c"NSNotificationCenter").expect("Foundation"),
            defaultCenter
        ];
        let nil: *mut AnyObject = std::ptr::null_mut();
        let token: Option<Retained<AnyObject>> = msg_send![&*center, addObserverForName: &*name, object: nil, queue: nil, usingBlock: &*block];
        std::mem::forget(token);
    }
}

/// The Apple event being handled is the login item's "open application" one.
fn launched_at_login() -> bool {
    // SAFETY: plain getters on NSAppleEventManager / NSAppleEventDescriptor; every object is
    // checked for nil before use.
    unsafe {
        let Some(cls) = AnyClass::get(c"NSAppleEventManager") else {
            return false;
        };
        let manager: *mut AnyObject = msg_send![cls, sharedAppleEventManager];
        if manager.is_null() {
            return false;
        }
        let event: *mut AnyObject = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return false;
        }
        let id: u32 = msg_send![event, eventID];
        if id != fourcc(b"oapp") {
            return false;
        }
        let prop: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: fourcc(b"prdt")];
        if prop.is_null() {
            return false;
        }
        let code: u32 = msg_send![prop, enumCodeValue];
        code == fourcc(b"lgit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_installed_app_bundle_can_start_at_login() {
        assert_eq!(unavailable_for("/Applications/Kotora.app"), None);
        assert_eq!(
            unavailable_for("/Users/u/work/kotora/target/macos/Kotora.app"),
            None
        );
        assert!(
            unavailable_for("/Users/u/work/kotora/target/debug").is_some(),
            "cargo run"
        );
        assert!(
            unavailable_for("/Volumes/Kotora/Kotora.app").is_some(),
            "the disk image"
        );
        assert!(
            unavailable_for("/private/var/folders/x/T/AppTranslocation/ABC/d/Kotora.app").is_some()
        );
    }

    #[test]
    fn status_numbers_map_to_the_switch() {
        assert_eq!(
            from_status(ENABLED),
            Login {
                on: true,
                ..Login::default()
            }
        );
        assert_eq!(
            from_status(REQUIRES_APPROVAL),
            Login {
                on: true,
                needs_approval: true,
                ..Login::default()
            }
        );
        assert_eq!(from_status(NOT_REGISTERED), Login::default());
        assert_eq!(
            from_status(3),
            Login::default(),
            "not found: never registered"
        );
    }

    #[test]
    fn four_char_codes() {
        assert_eq!(fourcc(b"oapp"), 0x6f61_7070);
        assert_eq!(fourcc(b"lgit"), 0x6c67_6974);
    }
}
