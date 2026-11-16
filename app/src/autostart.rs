//! Settings → Startup: "Start Kotora when I log in" and "Start in the background".
//!
//! The switch shows what the system holds (`read`), not a saved bool:
//! - Linux: an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/io.github.hanami117.kotora.desktop`.
//!   Its existence is the switch, so removing it by hand turns the switch off.
//! - macOS (`autostart_mac.rs`): the app itself as a login item (`SMAppService.mainApp`), listed
//!   in System Settings → General → Login Items → "Open at Login", as Uzumaki's is. Only a built
//!   `Kotora.app` can be one. A login item gets no arguments, so the background start is decided
//!   at launch (`watch_launch`). Before 2026-11-16 Kotora wrote a LaunchAgent plist instead;
//!   `migrate_old_entry` moves that over once.
//!
//! "In the background" is `[startup] background` in `config.toml`: the login start shows only the
//! tray or menu bar icon, no window. Linux adds `--hidden` to the entry; macOS reads the setting at
//! launch. Starting Kotora by hand always opens the window.
//!
//! The Linux entry runs the binary that wrote it (`current_exe`); a second copy for tests
//! (`KOTORA_TEST_INSTANCE`) has a scratch `XDG_CONFIG_HOME`. On macOS a test instance never
//! touches the login item (`read` says unavailable). Nothing here runs headless.

use std::path::{Path, PathBuf};

use crate::APP_ID;

#[cfg(target_os = "macos")]
#[path = "autostart_mac.rs"]
mod imp;
#[cfg(target_os = "macos")]
pub use imp::{migrate_old_entry, open_login_items, watch_launch};

/// What the system says about starting at login.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Login {
    /// Why this copy can't start at login (shown under the disabled switch), or None.
    pub unavailable: Option<&'static str>,
    /// Starts at login (Linux: the autostart entry exists; macOS: the login item is registered,
    /// approved or not).
    pub on: bool,
    /// macOS: registered, but waiting for the user's OK in System Settings → Login Items.
    pub needs_approval: bool,
}

impl Login {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn unavailable(why: &'static str) -> Self {
        Login { unavailable: Some(why), ..Login::default() }
    }

    /// The switch can be used.
    pub fn available(&self) -> bool {
        self.unavailable.is_none()
    }
}

/// The state of the system's login entry.
pub fn read() -> Login {
    #[cfg(target_os = "macos")]
    {
        imp::read()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Login { on: entry_path().is_file(), ..Login::default() }
    }
}

/// Turns the login start on or off; `hidden` adds `--hidden` where the system's entry takes
/// arguments (Linux). Returns what the system says afterwards.
pub fn set(on: bool, hidden: bool) -> Result<Login, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = hidden;
        imp::set(on)
    }
    #[cfg(not(target_os = "macos"))]
    {
        set_entry(on, hidden)?;
        Ok(read())
    }
}

/// Where the Linux autostart entry lives.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn entry_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"));
    base.join("autostart").join(format!("{APP_ID}.desktop"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Writes (on) or removes (off) the Linux autostart entry.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn set_entry(on: bool, hidden: bool) -> Result<(), String> {
    let path = entry_path();
    if !on {
        return remove(&path);
    }
    let exe = std::env::current_exe().map_err(|e| format!("can't find Kotora's own path: {e}"))?;
    write(&path, &desktop_text(&exe, hidden))
}

/// Removes a file; one that isn't there is fine.
fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("can't remove {}: {e}", path.display())),
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    // Written beside it, then renamed: a login never sees half a file.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

/// An XDG autostart desktop entry. `Exec` quoting follows the Desktop Entry spec.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn desktop_text(exe: &Path, hidden: bool) -> String {
    let exe = exe.to_string_lossy();
    let needs_quotes = exe.chars().any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c));
    let exe = if needs_quotes {
        // Inside quotes `"`, `` ` ``, `$` and `\` take a backslash; then `\` is doubled again for
        // the key file's own escapes.
        let inner: String = exe.chars().flat_map(|c| if "\"`$\\".contains(c) { vec!['\\', c] } else { vec![c] }).collect();
        format!("\"{}\"", inner.replace('\\', "\\\\"))
    } else {
        exe.replace('%', "%%")
    };
    let args = if hidden { " --hidden" } else { "" };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Kotora\n\
         Comment=Anime tracker\n\
         Exec={exe}{args}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

// ---- the old macOS LaunchAgent ----

/// The LaunchAgent plist Kotora wrote before it used `SMAppService` (macOS, until 2026-11-16).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn old_launch_agent_path() -> PathBuf {
    home().join("Library/LaunchAgents").join(format!("{APP_ID}.plist"))
}

/// What is at the old LaunchAgent's path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum OldEntry {
    /// Written by Kotora (its Label is our app id).
    Ours,
    /// Some other file at that path: left alone.
    Foreign,
}

/// The first `<string>` after `<key>Label</key>` in a plist's text.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plist_label(text: &str) -> Option<&str> {
    let after = &text[text.find("<key>Label</key>")? + "<key>Label</key>".len()..];
    let from = after.find("<string>")? + "<string>".len();
    let rest = &after[from..];
    Some(rest[..rest.find("</string>")?].trim())
}

/// Looks at the old LaunchAgent: None when there is none (or it can't be read).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn read_old_entry(path: &Path) -> Option<OldEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(if plist_label(&text) == Some(APP_ID) { OldEntry::Ours } else { OldEntry::Foreign })
}

/// What the one-time migration does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Migration {
    /// No old entry of ours.
    Nothing,
    /// The old entry stays: this copy can't be a login item (why), so removing it would lose the
    /// user's choice. It moves over when Kotora runs from the app.
    KeepOld(&'static str),
    /// The login item is already there: only the old entry goes.
    RemoveOld,
    /// Register the login item, then the old entry goes.
    RegisterThenRemoveOld,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn decide_migration(old: Option<OldEntry>, login: &Login) -> Migration {
    match (old, login.unavailable) {
        (Some(OldEntry::Ours), Some(why)) => Migration::KeepOld(why),
        (Some(OldEntry::Ours), None) if login.on => Migration::RemoveOld,
        (Some(OldEntry::Ours), None) => Migration::RegisterThenRemoveOld,
        _ => Migration::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_entry_runs_this_binary_hidden_when_asked() {
        let t = desktop_text(Path::new("/usr/bin/kotora"), true);
        assert!(t.starts_with("[Desktop Entry]\n"));
        assert!(t.contains("\nExec=/usr/bin/kotora --hidden\n"), "{t}");
        assert!(t.contains("\nIcon=io.github.hanami117.kotora\n"));
        assert!(desktop_text(Path::new("/usr/bin/kotora"), false).contains("\nExec=/usr/bin/kotora\n"));
    }

    #[test]
    fn odd_paths_are_quoted_for_exec() {
        let t = desktop_text(Path::new("/opt/My Apps/kotora"), false);
        assert!(t.contains("\nExec=\"/opt/My Apps/kotora\"\n"), "{t}");
        let t = desktop_text(Path::new("/opt/a$b/kotora"), false);
        assert!(t.contains("\nExec=\"/opt/a\\\\$b/kotora\"\n"), "{t}");
        assert!(desktop_text(Path::new("/opt/100%/kotora"), false).contains("Exec=/opt/100%%/kotora\n"));
    }

    const OLD_PLIST: &str = "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key><string>io.github.hanami117.kotora</string>\n    <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n";

    #[test]
    fn the_old_launch_agent_is_ours_only_by_its_label() {
        assert_eq!(plist_label(OLD_PLIST), Some(APP_ID));
        assert_eq!(plist_label("<key>Label</key>\n  <string> other.app </string>"), Some("other.app"));
        assert_eq!(plist_label("<key>RunAtLoad</key><true/>"), None);
        let dir = std::env::temp_dir().join(format!("kotora-oldagent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (ours, other, none) = (dir.join("a.plist"), dir.join("b.plist"), dir.join("c.plist"));
        std::fs::write(&ours, OLD_PLIST).unwrap();
        std::fs::write(&other, OLD_PLIST.replace(APP_ID, "com.example.thing")).unwrap();
        assert_eq!(read_old_entry(&ours), Some(OldEntry::Ours));
        assert_eq!(read_old_entry(&other), Some(OldEntry::Foreign));
        assert_eq!(read_old_entry(&none), None);
        remove(&ours).unwrap();
        remove(&ours).unwrap(); // already gone: fine
        assert!(!ours.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_migration_keeps_the_users_choice() {
        let off = Login::default();
        let on = Login { on: true, ..Login::default() };
        let waiting = Login { on: true, needs_approval: true, ..Login::default() };
        let bare = Login::unavailable("Only in the app");
        // An old agent of ours means "start at login was on".
        assert_eq!(decide_migration(Some(OldEntry::Ours), &off), Migration::RegisterThenRemoveOld);
        // Already a login item (even one waiting for approval): only the old file goes.
        assert_eq!(decide_migration(Some(OldEntry::Ours), &on), Migration::RemoveOld);
        assert_eq!(decide_migration(Some(OldEntry::Ours), &waiting), Migration::RemoveOld);
        // Can't be a login item here (cargo run, test copy): the old agent stays, nothing is lost.
        assert_eq!(decide_migration(Some(OldEntry::Ours), &bare), Migration::KeepOld("Only in the app"));
        // No agent, or one that isn't ours: nothing to do, whatever the login item says.
        for login in [&off, &on, &bare] {
            assert_eq!(decide_migration(None, login), Migration::Nothing);
            assert_eq!(decide_migration(Some(OldEntry::Foreign), login), Migration::Nothing);
        }
    }

    #[test]
    fn writing_and_removing_an_entry() {
        let dir = std::env::temp_dir().join(format!("kotora-autostart-{}", std::process::id()));
        let path = dir.join("autostart").join("x.desktop");
        write(&path, "one").unwrap();
        write(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
