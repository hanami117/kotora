//! Windows: start at login through the current user's Run key. See `autostart.rs`.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Kotora` = `"<kotora.exe>"` or
//! `"<kotora.exe>" --hidden`. Windows keeps its own switch for every entry in
//! `…\Explorer\StartupApproved\Run` (Settings → Apps → Startup, Task Manager → Startup apps);
//! both are read, and turning the switch on here clears a "disabled" left there.

use super::{Login, run_command, startup_approved};
use crate::win::reg;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// The value's name (what Windows' Startup list shows until it reads the program's own name).
const NAME: &str = "Kotora";

/// A second copy for tests shares the user's registry: it never touches the entry.
fn test_instance() -> bool {
    std::env::var_os("KOTORA_TEST_INSTANCE").is_some_and(|v| !v.is_empty())
}

pub fn read() -> Login {
    if test_instance() {
        return Login::unavailable("Off in a test copy.");
    }
    read_at(RUN, APPROVED, NAME)
}

pub fn set(on: bool, hidden: bool) -> Result<Login, String> {
    if test_instance() {
        return Err("off in a test copy".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("can't find Kotora's own path: {e}"))?;
    set_at(
        RUN,
        APPROVED,
        NAME,
        on.then(|| run_command(&exe.to_string_lossy(), hidden)),
    )?;
    Ok(read())
}

fn read_at(run: &str, approved: &str, name: &str) -> Login {
    let entry = reg::get_string(run, name)
        .ok()
        .flatten()
        .is_some_and(|v| !v.trim().is_empty());
    let allowed = startup_approved(reg::get_binary(approved, name).ok().flatten().as_deref());
    Login {
        on: entry && allowed,
        ..Login::default()
    }
}

/// `Some(command)`: writes the entry and clears Windows' "disabled" for it. `None`: removes both.
fn set_at(run: &str, approved: &str, name: &str, command: Option<String>) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("the registry refused: {e}");
    match command {
        Some(c) => reg::set_string(run, name, &c).map_err(failed)?,
        None => reg::delete_value(run, name).map_err(failed)?,
    }
    // Best effort: without this value the entry counts as on.
    let _ = reg::delete_value(approved, name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real calls on a scratch key of this test (never the user's Run key).
    #[test]
    fn the_entry_is_written_read_and_removed() {
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY, RegCloseKey, RegCreateKeyExW,
            RegDeleteTreeW, RegSetValueExW,
        };
        let wide = |s: &str| {
            s.encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let root = format!(r"Software\KotoraTest-login-{}", std::process::id());
        let (run, approved) = (
            format!(r"{root}\Run"),
            format!(r"{root}\StartupApproved\Run"),
        );
        assert!(!read_at(&run, &approved, NAME).on);
        let cmd = run_command(r"C:\Users\A B\Kotora\kotora.exe", true);
        set_at(&run, &approved, NAME, Some(cmd.clone())).unwrap();
        assert!(read_at(&run, &approved, NAME).on);
        assert_eq!(reg::get_string(&run, NAME).unwrap(), Some(cmd.clone()));
        // The user switches it off in Task Manager: off here too, the entry itself stays.
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: plain registry calls on the scratch key; strings are NUL-terminated.
        unsafe {
            assert_eq!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&approved).as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut h,
                    std::ptr::null_mut()
                ),
                0
            );
            let off = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            assert_eq!(
                RegSetValueExW(
                    h,
                    wide(NAME).as_ptr(),
                    0,
                    REG_BINARY,
                    off.as_ptr(),
                    off.len() as u32
                ),
                0
            );
            RegCloseKey(h);
        }
        assert!(!read_at(&run, &approved, NAME).on);
        assert_eq!(reg::get_string(&run, NAME).unwrap(), Some(cmd));
        // Turned on again in Kotora: on in Windows too, without `--hidden` this time.
        set_at(
            &run,
            &approved,
            NAME,
            Some(run_command(r"C:\Users\A B\Kotora\kotora.exe", false)),
        )
        .unwrap();
        assert!(read_at(&run, &approved, NAME).on);
        assert_eq!(
            reg::get_string(&run, NAME).unwrap().as_deref(),
            Some(r#""C:\Users\A B\Kotora\kotora.exe""#)
        );
        // Off: gone, and off twice is fine.
        set_at(&run, &approved, NAME, None).unwrap();
        set_at(&run, &approved, NAME, None).unwrap();
        assert!(!read_at(&run, &approved, NAME).on);
        assert_eq!(reg::get_string(&run, NAME).unwrap(), None);
        // SAFETY: deletes the scratch key made above.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&root).as_ptr()) };
    }
}
