//! Where Kotora keeps things: XDG paths on Linux and macOS, the user's AppData folders on Windows.
//!
//! Windows (the rule Rune's `AppDirs` uses): settings in `%APPDATA%\kotora`, the database, the
//! token file and the app's own state in `%LOCALAPPDATA%\kotora`, covers in
//! `%LOCALAPPDATA%\kotora\Cache`, logs in `%LOCALAPPDATA%\kotora\Logs`. An absolute `XDG_*`
//! variable still wins there, so a test instance can have scratch folders on every system.

use std::ffi::OsString;
use std::path::PathBuf;

/// The user's home folder: `HOME`, on Windows `USERPROFILE`. `None` when it is unset or not an
/// absolute path, so nothing is ever written relative to the working directory by mistake.
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Which of the Windows folders.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Config,
    Data,
    Cache,
    State,
    Logs,
}

/// The Windows rule, with the environment passed in (tests run it on every system). `temp` is
/// the last resort when neither the AppData variables nor `USERPROFILE` are set: a real folder,
/// never the working directory.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_dir(kind: Kind, env: &dyn Fn(&str) -> Option<OsString>, temp: PathBuf) -> PathBuf {
    let abs = |var: &str| env(var).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    let xdg = match kind {
        Kind::Config => "XDG_CONFIG_HOME",
        Kind::Data => "XDG_DATA_HOME",
        Kind::Cache => "XDG_CACHE_HOME",
        Kind::State | Kind::Logs => "XDG_STATE_HOME",
    };
    if let Some(p) = abs(xdg) {
        return match kind {
            Kind::Logs => p.join("kotora").join("Logs"),
            _ => p.join("kotora"),
        };
    }
    let (var, under_profile) = match kind {
        Kind::Config => ("APPDATA", "AppData\\Roaming"),
        _ => ("LOCALAPPDATA", "AppData\\Local"),
    };
    let base = abs(var).or_else(|| abs("USERPROFILE").map(|h| h.join(under_profile))).unwrap_or_else(|| temp.join("kotora-profile"));
    let dir = base.join("kotora");
    match kind {
        Kind::Cache => dir.join("Cache"),
        Kind::Logs => dir.join("Logs"),
        _ => dir,
    }
}

#[cfg(windows)]
fn win(kind: Kind) -> PathBuf {
    windows_dir(kind, &|v| std::env::var_os(v), std::env::temp_dir())
}

#[cfg(windows)]
fn xdg(var: &str, _home_rel: &str) -> PathBuf {
    win(match var {
        "XDG_CONFIG_HOME" => Kind::Config,
        "XDG_CACHE_HOME" => Kind::Cache,
        _ => Kind::Data,
    })
}

#[cfg(not(windows))]
fn xdg(var: &str, home_rel: &str) -> PathBuf {
    match std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(p) => p,
        None => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(home_rel),
    }
    .join("kotora")
}

/// `$XDG_CONFIG_HOME/kotora` (`~/.config/kotora`): `config.toml`.
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_DATA_HOME/kotora` (`~/.local/share/kotora`): `kotora.db`, `tokens.json`.
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_CACHE_HOME/kotora` (`~/.cache/kotora`): downloaded covers. Safe to delete.
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_STATE_HOME/kotora`, else `~/.local/state/kotora` (on macOS too): what the app
/// remembers by itself, apart from the settings. Windows: `%LOCALAPPDATA%\kotora`.
pub fn state_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return Some(win(Kind::State));
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(base.join("kotora"))
    }
}

/// Windows only: `%LOCALAPPDATA%\kotora\Logs`, where the app writes `kotora.log` when it has
/// no console.
#[cfg(windows)]
pub fn log_dir() -> PathBuf {
    win(Kind::Logs)
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn db_file() -> PathBuf {
    data_dir().join("kotora.db")
}

pub fn token_file() -> PathBuf {
    data_dir().join("tokens.json")
}

/// Creates a directory (and parents) readable only by the user.
pub fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)
}

/// `KOTORA_OFFLINE=1`: no network at all; the app shows the saved list.
pub fn offline_forced() -> bool {
    std::env::var_os("KOTORA_OFFLINE").is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(kind: Kind, vars: &[(&str, &str)]) -> String {
        let env = |v: &str| vars.iter().find(|(k, _)| *k == v).map(|(_, val)| OsString::from(val));
        windows_dir(kind, &env, PathBuf::from(if cfg!(windows) { "C:\\Temp" } else { "/tmp" })).to_string_lossy().replace('/', "\\")
    }

    /// The Windows rule. Absolute paths are judged by the system the test runs on, so the
    /// inputs are absolute on both kinds.
    #[test]
    fn windows_folders() {
        let (roaming, local, profile) = if cfg!(windows) { ("C:\\U\\me\\AppData\\Roaming", "C:\\U\\me\\AppData\\Local", "C:\\U\\me") } else { ("/U/me/AppData/Roaming", "/U/me/AppData/Local", "/U/me") };
        let w = |p: &str| p.replace('/', "\\");
        let vars = [("APPDATA", roaming), ("LOCALAPPDATA", local), ("USERPROFILE", profile)];
        assert_eq!(dir(Kind::Config, &vars), format!("{}\\kotora", w(roaming)));
        assert_eq!(dir(Kind::Data, &vars), format!("{}\\kotora", w(local)));
        assert_eq!(dir(Kind::State, &vars), format!("{}\\kotora", w(local)));
        assert_eq!(dir(Kind::Cache, &vars), format!("{}\\kotora\\Cache", w(local)));
        assert_eq!(dir(Kind::Logs, &vars), format!("{}\\kotora\\Logs", w(local)));
        // Only the profile is known.
        let vars = [("USERPROFILE", profile)];
        assert_eq!(dir(Kind::Config, &vars), format!("{}\\AppData\\Roaming\\kotora", w(profile)));
        assert_eq!(dir(Kind::Data, &vars), format!("{}\\AppData\\Local\\kotora", w(profile)));
        // Nothing is set, or only relative paths: a folder under temp, never the working directory.
        for vars in [&[][..], &[("APPDATA", "AppData"), ("LOCALAPPDATA", ""), ("USERPROFILE", "me")][..]] {
            for kind in [Kind::Config, Kind::Data, Kind::Cache, Kind::State, Kind::Logs] {
                let d = dir(kind, vars);
                assert!(d.contains("kotora-profile\\kotora"), "{d}");
                assert!(PathBuf::from(d.replace('\\', std::path::MAIN_SEPARATOR_STR)).is_absolute() || !cfg!(windows));
            }
        }
        // A scratch XDG folder wins (test instances).
        let vars = [("XDG_DATA_HOME", profile), ("LOCALAPPDATA", local)];
        assert_eq!(dir(Kind::Data, &vars), format!("{}\\kotora", w(profile)));
        assert_eq!(dir(Kind::Cache, &vars), format!("{}\\kotora\\Cache", w(local)));
    }
}
