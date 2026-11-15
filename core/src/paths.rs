//! Where Kotora keeps things: XDG paths on Linux and macOS (DESIGN.md §5.6).

use std::path::PathBuf;

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
