//! "Precise mode" for IINA (macOS). IINA runs its own mpv inside the app, so the process
//! source sees it with no setup (process name "IINA"), but the position needs mpv's IPC socket,
//! which IINA only opens when its settings say so:
//!
//! - Settings → Advanced → "Enable advanced settings" must be on for anything below to apply.
//! - "Additional mpv options": a list of name / value pairs (`userOptions` in IINA's defaults).
//!   `input-ipc-server` = `/tmp/iina.sock` there is the documented way.
//! - "Use config directory" (`useUserDefinedConfDir`, `userDefinedConfDir`): IINA's mpv also
//!   reads `mpv.conf` from that folder, so the socket can be one line there.
//!
//! [`check`] reads IINA's defaults through `/usr/bin/defaults export` (read only, so IINA's own
//! preferences are never written). [`enable`] appends the socket to the config folder's
//! `mpv.conf` when IINA reads one (with `mpvconf`'s append-only rules); otherwise the user sets
//! the option in IINA's Settings and the GUI says how. IINA must restart either way.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::mpvconf::{self, Enabled, Env};

/// IINA's defaults domain (its bundle id).
pub const DOMAIN: &str = "com.colliderli.iina";

/// The IINA settings that matter here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// "Enable advanced settings".
    pub advanced: bool,
    /// "Use config directory".
    pub use_conf_dir: bool,
    pub conf_dir: Option<String>,
    /// "Additional mpv options", in order.
    pub options: Vec<(String, String)>,
}

/// What IINA would do about the IPC socket at its next start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setup {
    /// A socket is set: in the additional mpv options (`file` is `None`) or in the config
    /// folder's `mpv.conf`.
    Set { socket: PathBuf, file: Option<PathBuf> },
    /// IINA reads this `mpv.conf` (in its config folder) and it sets no socket: Kotora can add one.
    ConfDir { file: PathBuf },
    /// Only IINA's Settings can turn it on (advanced settings or the config folder are off).
    Manual,
}

/// Reads IINA's settings and works out the [`Setup`].
pub fn check() -> std::io::Result<Setup> {
    setup(&read_prefs()?, std::env::var_os("HOME").filter(|h| !h.is_empty()))
}

/// Adds the IPC socket at `socket` to the `mpv.conf` of IINA's config folder. Nothing is written
/// when IINA already sets a socket ([`Enabled::AlreadySet`]) or reads no `mpv.conf` (an error
/// that says what to do in IINA).
pub fn enable(socket: &Path) -> std::io::Result<Enabled> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    enable_with(&setup(&read_prefs()?, home.clone())?, home, socket)
}

/// [`enable`] for a given setup.
pub fn enable_with(setup: &Setup, home: Option<OsString>, socket: &Path) -> std::io::Result<Enabled> {
    match setup {
        Setup::Set { socket, file } => Ok(Enabled::AlreadySet { file: file.clone().unwrap_or_else(|| PathBuf::from("IINA's additional mpv options")), socket: socket.clone() }),
        Setup::ConfDir { file } => mpvconf::enable_file(file, &conf_env(file, home), socket),
        Setup::Manual => Err(std::io::Error::other("IINA reads no mpv.conf: turn on its advanced settings and add the option there")),
    }
}

/// mpv's environment for the config folder holding `file` (`~~/` means that folder).
fn conf_env(file: &Path, home: Option<OsString>) -> Env {
    Env { mpv_home: file.parent().map(|d| d.as_os_str().to_os_string()), xdg_config_home: None, home }
}

/// The [`Setup`] for these settings. Reads the config folder's `mpv.conf` when IINA uses one.
pub fn setup(p: &Prefs, home: Option<OsString>) -> std::io::Result<Setup> {
    if !p.advanced {
        return Ok(Setup::Manual);
    }
    let expand = |v: &str| Env { mpv_home: None, xdg_config_home: None, home: home.clone() }.expand(v);
    // mpv keeps the last value; IINA passes these before mpv reads its config.
    if let Some((_, v)) = p.options.iter().rev().find(|(k, _)| k.trim().trim_start_matches("--") == mpvconf::OPTION)
        && !v.trim().is_empty()
    {
        return Ok(Setup::Set { socket: expand(v.trim()), file: None });
    }
    let Some(dir) = p.conf_dir.as_deref().filter(|d| p.use_conf_dir && !d.trim().is_empty()) else { return Ok(Setup::Manual) };
    let dir = if dir == "~" { home.as_ref().map(PathBuf::from).unwrap_or_default() } else { expand(dir) };
    if !dir.is_absolute() {
        return Ok(Setup::Manual);
    }
    let file = dir.join("mpv.conf");
    let c = mpvconf::check_file(&file, &conf_env(&file, home))?;
    Ok(match c.socket {
        Some(socket) => Setup::Set { socket, file: Some(file) },
        None => Setup::ConfDir { file },
    })
}

/// IINA's settings from `defaults export` (an XML property list). No settings (IINA never ran):
/// the defaults.
pub fn read_prefs() -> std::io::Result<Prefs> {
    let out = std::process::Command::new("/usr/bin/defaults").args(["export", DOMAIN, "-"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output()?;
    if !out.status.success() {
        return Ok(Prefs::default());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// One piece of an XML property list.
#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Open(&'a str),
    Close(&'a str),
    /// `<true/>`
    Empty(&'a str),
    Text(String),
}

fn tokens(xml: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while !rest.is_empty() {
        match rest.find('<') {
            Some(0) => {
                let Some(end) = rest.find('>') else { break };
                let tag = &rest[1..end];
                rest = &rest[end + 1..];
                if tag.starts_with('?') || tag.starts_with('!') {
                    continue;
                }
                if let Some(t) = tag.strip_prefix('/') {
                    out.push(Tok::Close(name(t)));
                } else if let Some(t) = tag.strip_suffix('/') {
                    out.push(Tok::Empty(name(t)));
                } else {
                    out.push(Tok::Open(name(tag)));
                }
            }
            Some(i) => {
                out.push(Tok::Text(unescape(&rest[..i])));
                rest = &rest[i..];
            }
            None => break,
        }
    }
    out
}

fn name(tag: &str) -> &str {
    tag.split_whitespace().next().unwrap_or("")
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The settings in an exported property list. Anything unexpected is skipped.
pub fn parse(xml: &str) -> Prefs {
    let toks = tokens(xml);
    let mut p = Prefs::default();
    // The top-level dict's entries: depth 1 inside `<plist><dict>`.
    let mut depth = 0usize;
    let mut i = 0;
    let mut key: Option<String> = None;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open("plist") | Tok::Close("plist") => {}
            Tok::Open("dict" | "array") => {
                if depth == 1
                    && let Some(k) = key.take()
                    && k == "userOptions"
                {
                    let (opts, next) = options(&toks, i);
                    p.options = opts;
                    i = next;
                    continue;
                }
                depth += 1;
                key = None;
            }
            Tok::Close("dict" | "array") => depth = depth.saturating_sub(1),
            Tok::Open("key") if depth == 1 => {
                key = text_until(&toks, i + 1, "key");
            }
            Tok::Empty(b @ ("true" | "false")) if depth == 1 => {
                let v = *b == "true";
                match key.take().as_deref() {
                    Some("enableAdvancedSettings") => p.advanced = v,
                    Some("useUserDefinedConfDir") => p.use_conf_dir = v,
                    _ => {}
                }
            }
            Tok::Open(kind @ ("integer" | "string")) if depth == 1 => {
                let v = text_until(&toks, i + 1, kind).unwrap_or_default();
                match (key.take().as_deref(), *kind) {
                    (Some("enableAdvancedSettings"), "integer") => p.advanced = v.trim() != "0",
                    (Some("useUserDefinedConfDir"), "integer") => p.use_conf_dir = v.trim() != "0",
                    (Some("userDefinedConfDir"), "string") => p.conf_dir = Some(v),
                    _ => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
    p
}

/// The text inside `<kind>…</kind>` starting at `i` (empty element: "").
fn text_until(toks: &[Tok], i: usize, kind: &str) -> Option<String> {
    match toks.get(i)? {
        Tok::Text(t) => Some(t.clone()),
        Tok::Close(k) if *k == kind => Some(String::new()),
        _ => None,
    }
}

/// `userOptions`: an array of `[name, value]` string arrays, from `toks[start]` (its `<array>`).
/// Returns the pairs and the index after the closing `</array>`.
fn options(toks: &[Tok], start: usize) -> (Vec<(String, String)>, usize) {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut strings: Vec<String> = Vec::new();
    let mut i = start;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open("array" | "dict") => {
                depth += 1;
                strings.clear();
            }
            Tok::Close("array" | "dict") => {
                if depth == 2 && strings.len() >= 2 {
                    out.push((strings[0].clone(), strings[1].clone()));
                }
                strings.clear();
                depth -= 1;
                if depth == 0 {
                    return (out, i + 1);
                }
            }
            Tok::Open("string") if depth == 2 => strings.push(text_until(toks, i + 1, "string").unwrap_or_default()),
            _ => {}
        }
        i += 1;
    }
    (out, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made up, in the shape `defaults export com.colliderli.iina -` prints.
    const EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSWindow Frame MainWindow</key>
	<string>100 100 800 600 0 0 1440 900 </string>
	<key>enableAdvancedSettings</key>
	<true/>
	<key>inputConfigs</key>
	<dict>
		<key>enableAdvancedSettings</key>
		<false/>
		<key>userDefinedConfDir</key>
		<string>/nowhere/inner</string>
	</dict>
	<key>recentDocuments</key>
	<array>
		<data>AAAA</data>
	</array>
	<key>useUserDefinedConfDir</key>
	<integer>1</integer>
	<key>userDefinedConfDir</key>
	<string>~/.config/iina &amp; mpv</string>
	<key>userOptions</key>
	<array>
		<array>
			<string>sub-auto</string>
			<string>fuzzy</string>
		</array>
		<array>
			<string>input-ipc-server</string>
			<string>/tmp/iina.sock</string>
		</array>
	</array>
	<key>volume</key>
	<integer>80</integer>
</dict>
</plist>
"#;

    #[test]
    fn reads_the_settings_from_an_export() {
        let p = parse(EXPORT);
        assert!(p.advanced && p.use_conf_dir);
        assert_eq!(p.conf_dir.as_deref(), Some("~/.config/iina & mpv"));
        assert_eq!(p.options, [("sub-auto".to_string(), "fuzzy".to_string()), ("input-ipc-server".to_string(), "/tmp/iina.sock".to_string())]);
        // Nothing set (IINA never ran, or a fresh install).
        assert_eq!(parse("<plist version=\"1.0\"><dict/></plist>"), Prefs::default());
        assert_eq!(parse(""), Prefs::default());
        assert_eq!(unescape("a &lt;b&gt; &#233;&#x3042; & c"), "a <b> \u{e9}\u{3042} & c");
    }

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Tmp {
            let d = std::env::temp_dir().join(format!("kotora-iina-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }
        fn home(&self) -> Option<OsString> {
            Some(self.0.clone().into_os_string())
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn prefs(advanced: bool, dir: Option<&str>, options: &[(&str, &str)]) -> Prefs {
        Prefs { advanced, use_conf_dir: dir.is_some(), conf_dir: dir.map(str::to_string), options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    #[test]
    fn works_out_what_iina_does_with_the_socket() {
        let t = Tmp::new("setup");
        let h = t.home();
        // Advanced settings off: nothing applies, whatever is stored.
        assert_eq!(setup(&prefs(false, Some("~/.config/iina"), &[("input-ipc-server", "/tmp/iina.sock")]), h.clone()).unwrap(), Setup::Manual);
        // Advanced on, no config folder, no option.
        assert_eq!(setup(&prefs(true, None, &[("sub-auto", "fuzzy")]), h.clone()).unwrap(), Setup::Manual);
        // The option in the additional mpv options (last one wins; `~/` expands).
        let p = prefs(true, None, &[("input-ipc-server", "/tmp/old.sock"), ("--input-ipc-server", "~/iina.sock")]);
        assert_eq!(setup(&p, h.clone()).unwrap(), Setup::Set { socket: t.0.join("iina.sock"), file: None });
        // A config folder without mpv.conf: Kotora can add the line.
        let file = t.0.join(".config/iina/mpv.conf");
        assert_eq!(setup(&prefs(true, Some("~/.config/iina"), &[]), h.clone()).unwrap(), Setup::ConfDir { file: file.clone() });
        // A folder that isn't used ("Use config directory" off) is ignored.
        let mut p = prefs(true, Some("~/.config/iina"), &[]);
        p.use_conf_dir = false;
        assert_eq!(setup(&p, h.clone()).unwrap(), Setup::Manual);
        // Its mpv.conf sets a socket (`~~/` is the folder).
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "hwdec=auto\ninput-ipc-server=~~/ipc.sock\n").unwrap();
        assert_eq!(setup(&prefs(true, Some("~/.config/iina"), &[]), h).unwrap(), Setup::Set { socket: t.0.join(".config/iina/ipc.sock"), file: Some(file) });
    }

    #[test]
    fn enable_appends_to_the_config_folders_mpv_conf_only() {
        let t = Tmp::new("enable");
        let file = t.0.join("iina/mpv.conf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "hwdec=auto\n").unwrap();
        let sock = Path::new("/tmp/iina.sock");
        match enable_with(&Setup::ConfDir { file: file.clone() }, t.home(), sock).unwrap() {
            Enabled::Wrote { file: f, text } => {
                assert_eq!(f, file);
                assert!(text.ends_with("input-ipc-server=/tmp/iina.sock\n"), "{text}");
            }
            e => panic!("{e:?}"),
        }
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("hwdec=auto\n"));
        // Twice: nothing more is written.
        let before = std::fs::read_to_string(&file).unwrap();
        assert!(matches!(enable_with(&Setup::ConfDir { file: file.clone() }, t.home(), sock).unwrap(), Enabled::AlreadySet { .. }));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
        // Set in IINA's options: said so, nothing written.
        let set = Setup::Set { socket: sock.to_path_buf(), file: None };
        assert!(matches!(enable_with(&set, t.home(), sock).unwrap(), Enabled::AlreadySet { socket, .. } if socket == sock));
        // Manual: an error, no file.
        assert!(enable_with(&Setup::Manual, t.home(), sock).is_err());
    }

    /// The real `defaults` tool on this Mac: it must answer, whatever IINA's settings are.
    #[test]
    fn reads_this_macs_defaults_without_failing() {
        let _ = read_prefs().unwrap();
    }
}
