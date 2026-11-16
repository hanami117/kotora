//! Settings → Startup: "Start Kotora when I log in" and "Start in the background".
//!
//! - Linux: an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/io.github.hanami117.kotora.desktop`.
//! - macOS: a LaunchAgent, `~/Library/LaunchAgents/io.github.hanami117.kotora.plist` (RunAtLoad).
//!   Not `SMAppService`: its main-app login item can't pass `--hidden`, and it only works from a
//!   signed bundle; a LaunchAgent works from the bundle and from a bare binary alike.
//!
//! The file existing is the "log in" switch (the system's own state, so removing it by hand
//! turns the switch off). "In the background" is `[startup] background` in `config.toml`; it
//! adds `--hidden` to the login start (the tray or menu bar icon only, no window). Starting
//! Kotora by hand always opens the window.
//!
//! The entry runs the binary that wrote it (`current_exe`). A second copy for tests
//! (`KOTORA_TEST_INSTANCE`) writes its macOS plist into its scratch config dir, never into
//! `~/Library/LaunchAgents`; on Linux the scratch `XDG_CONFIG_HOME` already keeps it apart.
//! Nothing here runs headless.

use std::path::{Path, PathBuf};

use crate::APP_ID;

/// Where the login entry lives.
pub fn entry_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        launch_agents_dir().join(format!("{APP_ID}.plist"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home().join(".config"));
        base.join("autostart").join(format!("{APP_ID}.desktop"))
    }
}

#[cfg(target_os = "macos")]
fn launch_agents_dir() -> PathBuf {
    if std::env::var_os("KOTORA_TEST_INSTANCE").is_some_and(|v| !v.is_empty()) {
        kotora_core::paths::config_dir().join("LaunchAgents")
    } else {
        home().join("Library/LaunchAgents")
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Whether Kotora starts at login (the entry exists).
pub fn enabled() -> bool {
    entry_path().is_file()
}

/// Writes (on) or removes (off) the login entry. `hidden` adds `--hidden`.
pub fn set(on: bool, hidden: bool) -> Result<(), String> {
    let path = entry_path();
    if !on {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("can't remove {}: {e}", path.display())),
        };
    }
    let exe = std::env::current_exe().map_err(|e| format!("can't find Kotora's own path: {e}"))?;
    write(&path, &entry_text(&exe, hidden))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    // Written beside it, then renamed: a login never sees half a file.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

/// The entry's text for this platform.
pub fn entry_text(exe: &Path, hidden: bool) -> String {
    #[cfg(target_os = "macos")]
    {
        plist_text(exe, hidden)
    }
    #[cfg(not(target_os = "macos"))]
    {
        desktop_text(exe, hidden)
    }
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

/// A LaunchAgent that runs Kotora once at login (no KeepAlive: Quit stays quit).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn plist_text(exe: &Path, hidden: bool) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut args = format!("        <string>{}</string>\n", esc(&exe.to_string_lossy()));
    if hidden {
        args.push_str("        <string>--hidden</string>\n");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20   <key>Label</key><string>{APP_ID}</string>\n\
         \x20   <key>ProgramArguments</key>\n\
         \x20   <array>\n\
         {args}\
         \x20   </array>\n\
         \x20   <key>RunAtLoad</key><true/>\n\
         \x20   <key>ProcessType</key><string>Interactive</string>\n\
         </dict>\n\
         </plist>\n"
    )
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

    #[test]
    fn the_launch_agent_runs_once_at_login() {
        let t = plist_text(Path::new("/Applications/Kotora.app/Contents/MacOS/kotora"), true);
        assert!(t.contains("<key>Label</key><string>io.github.hanami117.kotora</string>"));
        assert!(t.contains("<string>/Applications/Kotora.app/Contents/MacOS/kotora</string>\n        <string>--hidden</string>"), "{t}");
        assert!(t.contains("<key>RunAtLoad</key><true/>"));
        assert!(!t.contains("KeepAlive"));
        assert!(!plist_text(Path::new("/x/kotora"), false).contains("--hidden"));
        assert!(plist_text(Path::new("/a&b/kotora"), false).contains("<string>/a&amp;b/kotora</string>"));
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
