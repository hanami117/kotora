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
        // Windows: mpv reads `%APPDATA%\mpv\mpv.conf`, which is the XDG rule with APPDATA as
        // the config home; the home folder is USERPROFILE.
        #[cfg(windows)]
        return Env { mpv_home: get("MPV_HOME"), xdg_config_home: get("APPDATA"), home: get("USERPROFILE") };
        #[cfg(not(windows))]
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
    // A byte order mark (Windows editors) is not part of the first line.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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

// ------------------------------------------------------------------------------------ Windows
//
// On Windows the mpv family has several homes, so "the user's mpv.conf" depends on which player
// runs and where it was started from:
//
// - **mpv.net** (`mpvnet.exe`): `%MPVNET_HOME%`, else `portable_config` beside the program,
//   else `%APPDATA%\mpv.net` (its manual, "Config folder");
// - **mpv** (`mpv.exe`): `%MPV_HOME%`, else `portable_config` beside the program (also how
//   Scoop installs it: `scoop\apps\mpv\current\portable_config`), else `%APPDATA%\mpv`.
//
// A fresh install has no `mpv.conf` at all, so [`win_enable`] creates it. An existing file is
// copied to `mpv.conf.kotora-backup` first, and then only appended to. This part is plain file
// code, so it is tested on every system; only the app on Windows calls it.

/// The environment that decides where the mpv family reads its config on Windows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WinEnv {
    pub appdata: Option<PathBuf>,
    pub mpv_home: Option<PathBuf>,
    pub mpvnet_home: Option<PathBuf>,
}

impl WinEnv {
    pub fn current() -> WinEnv {
        let get = |k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        WinEnv { appdata: get("APPDATA"), mpv_home: get("MPV_HOME"), mpvnet_home: get("MPVNET_HOME") }
    }

    /// The `mpv.conf` a player reads. `net`: mpv.net; `exe`: the running program, when known.
    pub fn conf(&self, net: bool, exe: Option<&Path>) -> Option<PathBuf> {
        let (home, folder) = if net { (&self.mpvnet_home, "mpv.net") } else { (&self.mpv_home, "mpv") };
        if let Some(h) = home {
            return Some(h.join("mpv.conf"));
        }
        if let Some(portable) = exe.and_then(Path::parent).map(|d| d.join("portable_config"))
            && portable.is_dir()
        {
            return Some(portable.join("mpv.conf"));
        }
        Some(self.appdata.as_ref()?.join(folder).join("mpv.conf"))
    }

    /// The files precise mode is about. With players running (`(mpv.net?, program)`): theirs.
    /// With none: the config folders that exist, and mpv's own when there is none yet.
    pub fn targets(&self, running: &[(bool, PathBuf)]) -> Vec<Target> {
        fn add(out: &mut Vec<Target>, net: bool, file: Option<PathBuf>) {
            if let Some(file) = file
                && !out.iter().any(|t| t.file == file)
            {
                out.push(Target { player: if net { "mpv.net" } else { "mpv" }, file });
            }
        }
        let mut out: Vec<Target> = Vec::new();
        for (net, exe) in running {
            add(&mut out, *net, self.conf(*net, Some(exe)));
        }
        if running.is_empty() {
            for net in [true, false] {
                let file = self.conf(net, None);
                if file.as_ref().and_then(|f| f.parent()).is_some_and(Path::is_dir) {
                    add(&mut out, net, file);
                }
            }
        }
        if out.is_empty() {
            add(&mut out, false, self.conf(false, None));
        }
        out
    }
}

/// One `mpv.conf` and the player that reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// "mpv" or "mpv.net".
    pub player: &'static str,
    pub file: PathBuf,
}

/// The pipe an `input-ipc-server` value names on Windows: mpv puts `\\.\pipe\` in front of a
/// value that doesn't start with it.
pub fn win_pipe(value: &str) -> PathBuf {
    const PREFIX: &str = r"\\.\pipe\";
    if value.starts_with(PREFIX) { PathBuf::from(value) } else { PathBuf::from(format!("{PREFIX}{value}")) }
}

/// The pipe a Windows `mpv.conf` sets for every run, if any (`None` also for a missing file).
pub fn win_check(file: &Path) -> std::io::Result<Option<PathBuf>> {
    match std::fs::read(file) {
        Ok(bytes) => Ok(find(&String::from_utf8_lossy(&bytes)).filter(|v| !v.is_empty()).map(|v| win_pipe(&v))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// What [`win_enable`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WinEnabled {
    Wrote {
        file: PathBuf,
        /// The lines appended.
        text: String,
        /// The file didn't exist.
        created: bool,
        /// The copy of the file as it was (none for a new or empty file).
        backup: Option<PathBuf>,
    },
    /// The file already sets a pipe; nothing was written.
    AlreadySet { file: PathBuf, pipe: PathBuf },
}

/// Turns on mpv's IPC pipe in a Windows `mpv.conf`: creates the folder and file when missing,
/// keeps a copy of an existing file, then appends the option. Never twice, and an option the
/// user set is left alone. The player must restart to use it.
pub fn win_enable(file: &Path, pipe: &Path) -> std::io::Result<WinEnabled> {
    let old = match std::fs::read(file) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let old_text = String::from_utf8_lossy(old.as_deref().unwrap_or_default()).into_owned();
    if let Some(v) = find(&old_text).filter(|v| !v.is_empty()) {
        return Ok(WinEnabled::AlreadySet { file: file.to_path_buf(), pipe: win_pipe(&v) });
    }
    let pipe = pipe.to_str().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "the pipe name is not UTF-8"))?;
    // The file's own line ends.
    let eol = if old_text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut text = String::new();
    if !old_text.is_empty() && !old_text.ends_with('\n') {
        text.push_str(eol);
    }
    if !old_text.is_empty() {
        text.push_str(eol);
    }
    text.push_str("# Added by Kotora: lets Kotora see what the player plays (position and pause).");
    text.push_str(eol);
    if old_text.lines().any(|l| l.trim_start().starts_with('[')) {
        text.push_str("[default]");
        text.push_str(eol);
    }
    text.push_str(&format!("{OPTION}={pipe}{eol}"));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let backup = match &old {
        Some(bytes) if !bytes.is_empty() => Some(write_backup(file, bytes)?),
        _ => None,
    };
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    f.write_all(text.as_bytes())?;
    f.flush()?;
    Ok(WinEnabled::Wrote { file: file.to_path_buf(), text, created: old.is_none(), backup })
}

/// Copies the file as it was to `mpv.conf.kotora-backup` (`-2`, `-3`, … when that name is
/// taken: an older backup is never overwritten).
fn write_backup(file: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "mpv.conf".into());
    for n in 1..100 {
        let backup = file.with_file_name(if n == 1 { format!("{name}.kotora-backup") } else { format!("{name}.kotora-backup-{n}") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&backup) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.flush()?;
                return Ok(backup);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "too many mpv.conf backups"))
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
        // Absolute on every system: a drive in front on Windows.
        let abs = |p: &str| if cfg!(windows) { format!("C:{p}") } else { p.to_string() };
        let e = Env { mpv_home: None, xdg_config_home: None, home: Some(abs("/h").into()) };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from(abs("/h/.config/mpv/mpv.conf")));
        let e = Env { xdg_config_home: Some(abs("/x").into()), ..e };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from(abs("/x/mpv/mpv.conf")));
        let e2 = Env { xdg_config_home: Some("relative".into()), ..e.clone() };
        assert_eq!(e2.conf_file().unwrap(), PathBuf::from(abs("/h/.config/mpv/mpv.conf")), "a relative XDG path is ignored");
        let e = Env { mpv_home: Some(abs("/m").into()), ..e };
        assert_eq!(e.conf_file().unwrap(), PathBuf::from(abs("/m/mpv.conf")));
        assert_eq!(e.expand("~~/sock"), PathBuf::from(abs("/m/sock")));
        assert_eq!(e.expand("~/s"), PathBuf::from(abs("/h/s")));
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

    const PIPE: &str = r"\\.\pipe\mpvsocket";

    fn win_env(t: &Tmp) -> WinEnv {
        WinEnv { appdata: Some(t.0.join("Roaming")), mpv_home: None, mpvnet_home: None }
    }

    #[test]
    fn windows_finds_the_conf_of_the_player_that_runs() {
        let t = Tmp::new("win-find");
        let env = win_env(&t);
        let roaming = t.0.join("Roaming");
        // Installed players: the roaming folder, one per player.
        let mpv_exe = t.0.join("Program Files").join("mpv").join("mpv.exe");
        let net_exe = t.0.join("Program Files").join("mpv.net").join("mpvnet.exe");
        assert_eq!(env.conf(false, Some(&mpv_exe)).unwrap(), roaming.join("mpv").join("mpv.conf"));
        assert_eq!(env.conf(true, Some(&net_exe)).unwrap(), roaming.join("mpv.net").join("mpv.conf"));
        // A portable player (and Scoop's mpv): portable_config beside the program.
        let scoop = t.0.join("scoop").join("apps").join("mpv").join("current");
        std::fs::create_dir_all(scoop.join("portable_config")).unwrap();
        assert_eq!(env.conf(false, Some(&scoop.join("mpv.exe"))).unwrap(), scoop.join("portable_config").join("mpv.conf"));
        let portable_net = t.0.join("Tools").join("mpv.net");
        std::fs::create_dir_all(portable_net.join("portable_config")).unwrap();
        assert_eq!(env.conf(true, Some(&portable_net.join("mpvnet.exe"))).unwrap(), portable_net.join("portable_config").join("mpv.conf"));
        // The environment variables come first.
        let homes = WinEnv { mpv_home: Some(t.0.join("mh")), mpvnet_home: Some(t.0.join("nh")), ..env.clone() };
        assert_eq!(homes.conf(false, Some(&scoop.join("mpv.exe"))).unwrap(), t.0.join("mh").join("mpv.conf"));
        assert_eq!(homes.conf(true, Some(&net_exe)).unwrap(), t.0.join("nh").join("mpv.conf"));
        assert_eq!(WinEnv::default().conf(false, None), None);

        // Targets: the running players' files, each once.
        let running = [(true, net_exe.clone()), (false, scoop.join("mpv.exe")), (true, net_exe.clone())];
        assert_eq!(
            env.targets(&running),
            [Target { player: "mpv.net", file: roaming.join("mpv.net").join("mpv.conf") }, Target { player: "mpv", file: scoop.join("portable_config").join("mpv.conf") }]
        );
        // Nothing runs and no folder exists yet: mpv's own.
        assert_eq!(env.targets(&[]), [Target { player: "mpv", file: roaming.join("mpv").join("mpv.conf") }]);
        // Nothing runs: the folders that exist.
        std::fs::create_dir_all(roaming.join("mpv.net")).unwrap();
        assert_eq!(env.targets(&[]), [Target { player: "mpv.net", file: roaming.join("mpv.net").join("mpv.conf") }]);
        std::fs::create_dir_all(roaming.join("mpv")).unwrap();
        assert_eq!(env.targets(&[]).len(), 2);
    }

    #[test]
    fn windows_creates_a_missing_mpv_conf() {
        // mpv.net fresh from its installer: the folder may exist, the file doesn't.
        let t = Tmp::new("win-create");
        let file = win_env(&t).conf(true, None).unwrap();
        assert_eq!(win_check(&file).unwrap(), None);
        let r = win_enable(&file, Path::new(PIPE)).unwrap();
        let want = "# Added by Kotora: lets Kotora see what the player plays (position and pause).\ninput-ipc-server=\\\\.\\pipe\\mpvsocket\n";
        assert_eq!(r, WinEnabled::Wrote { file: file.clone(), text: want.into(), created: true, backup: None });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), want);
        assert_eq!(win_check(&file).unwrap(), Some(PathBuf::from(PIPE)));
        // Never twice, and no backup of our own file.
        assert_eq!(win_enable(&file, Path::new(PIPE)).unwrap(), WinEnabled::AlreadySet { file: file.clone(), pipe: PIPE.into() });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), want);
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn windows_backs_up_and_appends_to_an_existing_mpv_conf() {
        let t = Tmp::new("win-keep");
        let file = win_env(&t).conf(false, None).unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        // Windows line ends, a byte order mark, a profile at the end, no final line end, and
        // a byte that isn't UTF-8 in a comment.
        let mut mine = b"\xef\xbb\xbf# caf\xe9\r\nvo=gpu-next\r\n\r\n[anime]\r\ndeband=yes".to_vec();
        std::fs::write(&file, &mine).unwrap();
        let WinEnabled::Wrote { text, created, backup, .. } = win_enable(&file, Path::new(PIPE)).unwrap() else { panic!() };
        assert!(!created);
        assert_eq!(text, "\r\n\r\n# Added by Kotora: lets Kotora see what the player plays (position and pause).\r\n[default]\r\ninput-ipc-server=\\\\.\\pipe\\mpvsocket\r\n");
        // The backup is the file as it was, byte for byte; the file is the old bytes plus ours.
        let backup = backup.unwrap();
        assert_eq!(backup, file.with_file_name("mpv.conf.kotora-backup"));
        assert_eq!(std::fs::read(&backup).unwrap(), mine);
        mine.extend_from_slice(text.as_bytes());
        assert_eq!(std::fs::read(&file).unwrap(), mine);
        assert_eq!(win_check(&file).unwrap(), Some(PathBuf::from(PIPE)));

        // A second change of the file never overwrites the first backup.
        std::fs::write(&file, "vo=gpu\n").unwrap();
        let WinEnabled::Wrote { backup: second, .. } = win_enable(&file, Path::new(PIPE)).unwrap() else { panic!() };
        assert_eq!(second.unwrap(), file.with_file_name("mpv.conf.kotora-backup-2"));
        assert_eq!(std::fs::read(&backup).unwrap().len(), mine.len() - text.len());

        // A pipe the user set is left alone, in mpv's short form too, and on the first line
        // after a byte order mark.
        std::fs::write(&file, "\u{feff}input-ipc-server=mine\r\n").unwrap();
        assert_eq!(win_enable(&file, Path::new(PIPE)).unwrap(), WinEnabled::AlreadySet { file: file.clone(), pipe: r"\\.\pipe\mine".into() });
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "\u{feff}input-ipc-server=mine\r\n");
        // An empty file gets no backup.
        std::fs::write(&file, "").unwrap();
        let WinEnabled::Wrote { created, backup, .. } = win_enable(&file, Path::new(PIPE)).unwrap() else { panic!() };
        assert_eq!((created, backup), (false, None));
    }
}
