//! Opens a file in a player ("Play next episode"). Settings → Library → Open with: the system's
//! default app, or mpv, IINA (macOS) or VLC when installed. `auto` is the player Kotora last saw
//! playing, else the system's.
//!
//! macOS goes through `open` (`open -a IINA FILE`); a Homebrew mpv with no app bundle is started
//! itself. Linux starts the player's binary, or `xdg-open`. Kotora never waits on the player: a
//! short thread reaps it.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    System,
    Mpv,
    Iina,
    Vlc,
}

impl Player {
    /// The choices on this system, in the Settings order.
    pub fn here() -> &'static [Player] {
        if cfg!(target_os = "macos") { &[Player::System, Player::Mpv, Player::Iina, Player::Vlc] } else { &[Player::System, Player::Mpv, Player::Vlc] }
    }

    /// The `[library] open_with` value.
    pub fn key(self) -> &'static str {
        match self {
            Player::System => "default",
            Player::Mpv => "mpv",
            Player::Iina => "iina",
            Player::Vlc => "vlc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Player::System => "System default",
            Player::Mpv => "mpv",
            Player::Iina => "IINA",
            Player::Vlc => "VLC",
        }
    }

    pub fn from_key(k: &str) -> Option<Player> {
        Player::here().iter().copied().find(|p| p.key() == k)
    }

    /// The player detection reported ("IINA", "mpv", "VLC media player"); `None` for others.
    pub fn from_seen(name: &str) -> Option<Player> {
        let n = name.to_lowercase();
        // IINA is built on mpv: its name first.
        if n.contains("iina") {
            cfg!(target_os = "macos").then_some(Player::Iina)
        } else if n.contains("vlc") {
            Some(Player::Vlc)
        } else if n.contains("mpv") && !n.contains("mpc") {
            Some(Player::Mpv)
        } else {
            None
        }
    }
}

/// Which player opens the file: the setting, or for `auto` the last one seen when installed,
/// else the system's.
pub fn resolve(open_with: &str, last_seen: &str, installed: &[Player]) -> Player {
    let ok = |p: Player| installed.contains(&p);
    match Player::from_key(open_with).filter(|p| ok(*p)) {
        Some(p) => p,
        None => Player::from_key(last_seen).filter(|p| *p != Player::System && ok(*p)).unwrap_or(Player::System),
    }
}

/// Where a player is, if installed: an app bundle (macOS) or a binary on `PATH`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub mpv_app: bool,
    pub mpv_bin: Option<PathBuf>,
    pub iina: bool,
    pub vlc_app: bool,
    pub vlc_bin: Option<PathBuf>,
}

impl Found {
    pub fn installed(&self) -> Vec<Player> {
        let mut out = vec![Player::System];
        if self.mpv_app || self.mpv_bin.is_some() {
            out.push(Player::Mpv);
        }
        if self.iina && cfg!(target_os = "macos") {
            out.push(Player::Iina);
        }
        if self.vlc_app || self.vlc_bin.is_some() {
            out.push(Player::Vlc);
        }
        out
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    // A GUI start on macOS has a short PATH: Homebrew's folders too.
    let extra = if cfg!(target_os = "macos") { vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")] } else { vec![] };
    std::env::split_paths(&path).chain(extra).map(|d| d.join(name)).find(|p| is_exec(p))
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn app_bundle(name: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [Some(PathBuf::from("/Applications")), home.map(|h| h.join("Applications"))].into_iter().flatten().any(|d| d.join(format!("{name}.app")).is_dir())
}

/// Looks for the players (a few `stat`s; run off the UI thread).
pub fn find() -> Found {
    let mac = cfg!(target_os = "macos");
    Found {
        mpv_app: mac && app_bundle("mpv"),
        mpv_bin: on_path("mpv"),
        iina: mac && app_bundle("IINA"),
        vlc_app: mac && app_bundle("VLC"),
        vlc_bin: if mac { None } else { on_path("vlc") },
    }
}

/// The command that opens `file` in `p`.
pub fn command(p: Player, file: &str, found: &Found, mac: bool) -> (String, Vec<String>) {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mpv_bin = |bin: &Path| (bin.to_string_lossy().into_owned(), s(&["--player-operation-mode=pseudo-gui", "--", file]));
    match (p, mac) {
        (Player::System, true) => ("open".into(), s(&["--", file])),
        (Player::System, false) => ("xdg-open".into(), s(&[file])),
        (Player::Iina, _) => ("open".into(), s(&["-a", "IINA", "--", file])),
        (Player::Vlc, true) => ("open".into(), s(&["-a", "VLC", "--", file])),
        (Player::Vlc, false) => (found.vlc_bin.as_ref().map_or("vlc".into(), |b| b.to_string_lossy().into_owned()), s(&["--", file])),
        (Player::Mpv, true) if found.mpv_app || found.mpv_bin.is_none() => ("open".into(), s(&["-a", "mpv", "--", file])),
        (Player::Mpv, _) => mpv_bin(found.mpv_bin.as_deref().unwrap_or(Path::new("mpv"))),
    }
}

/// Starts the player on `file` and returns at once. An error is in plain words.
pub fn open(p: Player, file: &str, found: &Found) -> Result<(), String> {
    // Scenes and tests never start a player.
    if rune::is_headless() || cfg!(test) {
        return Ok(());
    }
    let (prog, args) = command(p, file, found, cfg!(target_os = "macos"));
    let mut child = std::process::Command::new(&prog)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Can't start {}: {e}", p.label()))?;
    let _ = std::thread::Builder::new().name("kotora-player".into()).spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_last_player_seen_when_installed() {
        let all = [Player::System, Player::Mpv, Player::Vlc];
        assert_eq!(resolve("auto", "vlc", &all), Player::Vlc);
        assert_eq!(resolve("auto", "", &all), Player::System);
        assert_eq!(resolve("auto", "mpv", &[Player::System]), Player::System, "not installed any more");
        assert_eq!(resolve("mpv", "vlc", &all), Player::Mpv, "a pick wins");
        assert_eq!(resolve("vlc", "mpv", &[Player::System, Player::Mpv]), Player::Mpv, "a pick that's gone: auto");
        assert_eq!(resolve("default", "mpv", &all), Player::System);
    }

    #[test]
    fn names_seen_map_to_players() {
        assert_eq!(Player::from_seen("mpv"), Some(Player::Mpv));
        assert_eq!(Player::from_seen("VLC media player"), Some(Player::Vlc));
        assert_eq!(Player::from_seen("mpc-qt"), None);
        assert_eq!(Player::from_seen("Haruna"), None);
        assert_eq!(Player::from_seen("IINA"), if cfg!(target_os = "macos") { Some(Player::Iina) } else { None });
    }

    #[test]
    fn commands_per_platform() {
        let f = "/m/a/Show - 07.mkv";
        let none = Found::default();
        let cmd = |p, found: &Found, mac| {
            let (prog, args) = command(p, f, found, mac);
            format!("{prog} {}", args.join(" "))
        };
        assert_eq!(cmd(Player::System, &none, true), "open -- /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::Iina, &none, true), "open -a IINA -- /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::Vlc, &none, true), "open -a VLC -- /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::Mpv, &Found { mpv_app: true, ..none.clone() }, true), "open -a mpv -- /m/a/Show - 07.mkv");
        let brew = Found { mpv_bin: Some("/opt/homebrew/bin/mpv".into()), ..none.clone() };
        assert_eq!(cmd(Player::Mpv, &brew, true), "/opt/homebrew/bin/mpv --player-operation-mode=pseudo-gui -- /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::System, &none, false), "xdg-open /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::Vlc, &Found { vlc_bin: Some("/usr/bin/vlc".into()), ..none.clone() }, false), "/usr/bin/vlc -- /m/a/Show - 07.mkv");
        assert_eq!(cmd(Player::Mpv, &Found { mpv_bin: Some("/usr/bin/mpv".into()), ..none }, false), "/usr/bin/mpv --player-operation-mode=pseudo-gui -- /m/a/Show - 07.mkv");
    }

    #[test]
    fn installed_lists_what_was_found() {
        let f = Found { mpv_bin: Some("/usr/bin/mpv".into()), ..Found::default() };
        assert_eq!(f.installed(), [Player::System, Player::Mpv]);
    }
}
