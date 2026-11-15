//! "Precise mode" for mpv: its IPC socket, set in the user's `mpv.conf`. Without it Kotora still
//! sees mpv through the process source, but without position or pause; with it mpv reports both.
//!
//! Kotora never changes `mpv.conf` on its own. The GUI shows [`check`]'s answer and calls
//! [`enable`] only when the user clicks for it. [`enable`] only appends (never rewrites the
//! file), never adds the option twice, and leaves an option the user set alone.
//!
//! Where mpv reads its config (mpv(1), FILES): `$MPV_HOME/mpv.conf` when `MPV_HOME` is set,
//! else `$XDG_CONFIG_HOME/mpv/mpv.conf`, else `~/.config/mpv/mpv.conf` (also on macOS). The
//! old `~/.mpv/` folder is not looked at.
//!
//! In `mpv.conf`, options before the first `[profile]` line, or in a `[default]` section, apply
//! to every run; options in other profiles only when that profile is used. So [`check`] only
//! counts the former, and [`enable`] appends `[default]` first when the file has profiles.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The option.
pub const OPTION: &str = "input-ipc-server";

/// The environment that decides where `mpv.conf` is. [`Env::current`] reads the process's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    pub mpv_home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    pub home: Option<OsString>,
}

impl Env {
    pub fn current() -> Env {
        let get = |k| std::env::var_os(k).filter(|v| !v.is_empty());
        Env { mpv_home: get("MPV_HOME"), xdg_config_home: get("XDG_CONFIG_HOME"), home: get("HOME") }
    }

    /// mpv's config folder.
    pub fn config_dir(&self) -> Option<PathBuf> {
        if let Some(d) = &self.mpv_home {
            return Some(PathBuf::from(d));
        }
        if let Some(d) = self.xdg_config_home.as_ref().filter(|d| Path::new(d).is_absolute()) {
            return Some(Path::new(d).join("mpv"));
        }
        Some(Path::new(self.home.as_ref()?).join(".config").join("mpv"))
    }

    /// `mpv.conf`'s path (it may not exist).
    pub fn conf_file(&self) -> Option<PathBuf> {
        Some(self.config_dir()?.join("mpv.conf"))
    }

    /// mpv's path prefixes: `~~/x` (config folder), `~/x` (home).
    pub fn expand(&self, value: &str) -> PathBuf {
        if let Some(rest) = value.strip_prefix("~~/")
            && let Some(d) = self.config_dir()
        {
            return d.join(rest);
        }
        if let Some(rest) = value.strip_prefix("~/")
            && let Some(h) = &self.home
        {
            return Path::new(h).join(rest);
        }
        PathBuf::from(value)
    }
}

/// What [`check`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// `mpv.conf`'s path.
    pub file: PathBuf,
    /// The file exists.
    pub exists: bool,
    /// The socket path `mpv.conf` sets for every run (expanded), if any. An empty value turns
    /// the socket off and counts as none.
    pub socket: Option<PathBuf>,
}

impl Check {
    /// The socket is set and Kotora watches it (`sockets`: `Settings::mpv_sockets`).
    pub fn watched(&self, sockets: &[PathBuf]) -> bool {
        self.socket.as_ref().is_some_and(|s| sockets.contains(s))
    }
}

/// Reads the user's `mpv.conf` (from the process environment).
pub fn check() -> std::io::Result<Check> {
    let env = Env::current();
    let file = env.conf_file().ok_or_else(no_home)?;
    check_file(&file, &env)
}

/// [`check`] for a given file and environment.
pub fn check_file(file: &Path, env: &Env) -> std::io::Result<Check> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let socket = text.as_deref().and_then(find).filter(|v| !v.is_empty()).map(|v| env.expand(&v));
    Ok(Check { file: file.to_path_buf(), exists: text.is_some(), socket })
}

/// The last value of the option for every run (mpv keeps the last one), unquoted.
fn find(text: &str) -> Option<String> {
    let mut global = true;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[') {
            global = name.trim_end().trim_end_matches(']').trim() == "default";
            continue;
        }
        if !global {
            continue;
        }
        let line = line.strip_prefix("--").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() == OPTION {
            found = Some(unquote(v.trim()));
        }
    }
    found
}

fn unquote(v: &str) -> String {
    if v.len() >= 2 && (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\'')) {
        return v[1..v.len() - 1].to_string();
    }
    // `%N%value`: mpv's length-prefixed form.
    if let Some(rest) = v.strip_prefix('%')
        && let Some((n, val)) = rest.split_once('%')
        && n.parse::<usize>().is_ok_and(|n| n == val.len())
    {
        return val.to_string();
    }
    // A comment after the value.
    v.split(" #").next().unwrap_or(v).trim_end().to_string()
}

/// What [`enable`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enabled {
    /// The lines appended to `file` (the folder and file were created when missing).
    Wrote { file: PathBuf, text: String },
    /// `mpv.conf` already sets a socket; nothing was written. The GUI should say which, and
    /// whether Kotora watches it ([`Check::watched`]).
    AlreadySet { file: PathBuf, socket: PathBuf },
}

/// Turns on mpv's IPC socket at `socket` in the user's `mpv.conf` (process environment). The
/// path to pass is the one Kotora watches: the first of `Settings::mpv_sockets`, by default
/// `$XDG_RUNTIME_DIR/mpv.sock` (`mpv::default_sockets`). mpv must restart to use it.
pub fn enable(socket: &Path) -> std::io::Result<Enabled> {
    let env = Env::current();
    let file = env.conf_file().ok_or_else(no_home)?;
    enable_file(&file, &env, socket)
}

/// [`enable`] for a given file and environment.
pub fn enable_file(file: &Path, env: &Env, socket: &Path) -> std::io::Result<Enabled> {
    let old = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if let Some(v) = find(&old).filter(|v| !v.is_empty()) {
        return Ok(Enabled::AlreadySet { file: file.to_path_buf(), socket: env.expand(&v) });
    }
    let socket = socket.to_str().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "the socket path is not UTF-8"))?;
    let mut text = String::new();
    if !old.is_empty() && !old.ends_with('\n') {
        text.push('\n');
    }
    if !old.is_empty() {
        text.push('\n');
    }
    text.push_str("# Added by Kotora: lets Kotora see what mpv plays (position and pause).\n");
    let has_profiles = old.lines().any(|l| l.trim_start().starts_with('['));
    if has_profiles {
        text.push_str("[default]\n");
    }
    text.push_str(&format!("{OPTION}={socket}\n"));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Append only: the user's lines are never rewritten. One write call.
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    f.write_all(text.as_bytes())?;
    f.flush()?;
    Ok(Enabled::Wrote { file: file.to_path_buf(), text })
}

fn no_home() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME, MPV_HOME or XDG_CONFIG_HOME: can't find mpv.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Tmp {
            let d = std::env::temp_dir().join(format!("kotora-mpvconf-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }

        fn env(&self) -> Env {
            Env { mpv_home: None, xdg_config_home: None, home: Some(self.0.clone().into_os_string()) }
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_mpv_conf_like_mpv() {
        let e = Env { mpv_home: None, xdg_config_home: None, home: Some("/h".into()) };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from("/h/.config/mpv/mpv.conf"));
        let e = Env { xdg_config_home: Some("/x".into()), ..e };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from("/x/mpv/mpv.conf"));
        let e2 = Env { xdg_config_home: Some("relative".into()), ..e.clone() };
        assert_eq!(e2.conf_file().unwrap(), PathBuf::from("/h/.config/mpv/mpv.conf"), "a relative XDG path is ignored");
        let e = Env { mpv_home: Some("/m".into()), ..e };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from("/m/mpv.conf"));
        assert_eq!(e.expand("~~/sock"), PathBuf::from("/m/sock"));
        assert_eq!(e.expand("~/s"), PathBuf::from("/h/s"));
        assert_eq!(Env::default().conf_file(), None);
    }

    #[test]
    fn reads_the_option_for_every_run_only() {
        assert_eq!(find("input-ipc-server=/tmp/a.sock\n"), Some("/tmp/a.sock".into()));
        assert_eq!(find("  --input-ipc-server = \"/tmp/b c.sock\"  \n"), Some("/tmp/b c.sock".into()));
        assert_eq!(find("input-ipc-server=%11%/tmp/x.sock"), Some("/tmp/x.sock".into()));
        assert_eq!(find("input-ipc-server=/tmp/a.sock # mine"), Some("/tmp/a.sock".into()));
        assert_eq!(find("# input-ipc-server=/tmp/a.sock\nvo=gpu\n"), None);
        assert_eq!(find("[anime]\ninput-ipc-server=/tmp/a.sock\n"), None, "a profile is not every run");
        assert_eq!(find("[anime]\nvo=gpu\n[default]\ninput-ipc-server=/tmp/a.sock\n"), Some("/tmp/a.sock".into()));
        assert_eq!(find("input-ipc-server=/a\ninput-ipc-server=/b\n"), Some("/b".into()), "the last one wins");
    }

    #[test]
    fn check_reports_a_missing_file_and_a_set_socket() {
        let t = Tmp::new("check");
        let env = t.env();
        let file = env.conf_file().unwrap();
        let c = check_file(&file, &env).unwrap();
        assert_eq!((c.exists, c.socket.as_ref()), (false, None));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "input-ipc-server=~~/socket\n").unwrap();
        let c = check_file(&file, &env).unwrap();
        assert_eq!(c.socket, Some(t.0.join(".config/mpv/socket")));
        assert!(c.watched(&[t.0.join(".config/mpv/socket")]));
        assert!(!c.watched(&[PathBuf::from("/run/user/1/mpv.sock")]));
        std::fs::write(&file, "input-ipc-server=\n").unwrap();
        assert_eq!(check_file(&file, &env).unwrap().socket, None, "an empty value turns it off");
    }

    #[test]
    fn enable_creates_the_folder_and_file() {
        let t = Tmp::new("create");
        let env = t.env();
        let file = env.conf_file().unwrap();
        let sock = Path::new("/run/user/1000/mpv.sock");
        let r = enable_file(&file, &env, sock).unwrap();
        let want = "# Added by Kotora: lets Kotora see what mpv plays (position and pause).\ninput-ipc-server=/run/user/1000/mpv.sock\n";
        assert_eq!(r, Enabled::Wrote { file: file.clone(), text: want.into() });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), want);
        assert_eq!(check_file(&file, &env).unwrap().socket.as_deref(), Some(sock));
        // Never twice.
        assert_eq!(enable_file(&file, &env, sock).unwrap(), Enabled::AlreadySet { file: file.clone(), socket: sock.into() });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), want);
    }

    #[test]
    fn enable_keeps_the_users_file_and_respects_profiles() {
        let t = Tmp::new("keep");
        let env = t.env();
        let file = env.conf_file().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        // No final newline, and a profile at the end.
        let mine = "# my config\nvo=gpu-next\n\n[anime]\ndeband=yes";
        std::fs::write(&file, mine).unwrap();
        let Enabled::Wrote { text, .. } = enable_file(&file, &env, Path::new("/tmp/k.sock")).unwrap() else { panic!() };
        let all = std::fs::read_to_string(&file).unwrap();
        assert!(all.starts_with(mine), "{all}");
        assert_eq!(all, format!("{mine}{text}"));
        assert!(text.contains("\n[default]\ninput-ipc-server=/tmp/k.sock\n"), "{text}");
        assert_eq!(find(&all), Some("/tmp/k.sock".into()));
        assert_eq!(find(&all.replace("[default]\n", "")), None, "without [default] it would be inside the profile");

        // A socket the user set (elsewhere) is left alone.
        std::fs::write(&file, "input-ipc-server=/tmp/mine.sock\n").unwrap();
        assert_eq!(enable_file(&file, &env, Path::new("/tmp/k.sock")).unwrap(), Enabled::AlreadySet { file: file.clone(), socket: "/tmp/mine.sock".into() });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "input-ipc-server=/tmp/mine.sock\n");
    }
}
