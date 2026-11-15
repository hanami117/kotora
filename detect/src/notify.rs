//! Desktop notifications, sent from a short thread so nobody waits.
//! - Linux: `org.freedesktop.Notifications.Notify` on the session bus (zbus 5, as MPRIS).
//! - macOS: `osascript`'s `display notification`, text passed as arguments (no quoting).
//!   Without an app bundle the system may show it under "Script Editor"; a bundle (v0.2's DMG) can
//!   use the native API.

/// Shows `summary` and `body`. Never blocks the caller.
pub fn send(summary: &str, body: &str) {
    let (summary, body) = (summary.to_string(), body.to_string());
    let _ = std::thread::Builder::new().name("kotora-notify".into()).spawn(move || {
        if let Err(e) = show(&summary, &body) {
            eprintln!("kotora: notification failed: {e}");
        }
    });
}

#[cfg(target_os = "linux")]
fn show(summary: &str, body: &str) -> Result<(), String> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let hints: HashMap<&str, Value> = HashMap::from([("desktop-entry", Value::new("io.github.hanami117.kotora"))]);
    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("Kotora", 0u32, "io.github.hanami117.kotora", summary, body, Vec::<&str>::new(), hints, -1i32),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn show(summary: &str, body: &str) -> Result<(), String> {
    let st = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", "on run argv", "-e", "display notification (item 2 of argv) with title \"Kotora\" subtitle (item 1 of argv)", "-e", "end run", summary, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err(format!("osascript: {st}")) }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn show(_summary: &str, _body: &str) -> Result<(), String> {
    Ok(())
}
