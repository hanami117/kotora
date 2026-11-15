//! `config.toml`: the settings the user edits, by hand or in Settings.
//!
//! ```toml
//! [mal]
//! client_id = "…"
//!
//! [updates]
//! auto = false                 # record confident matches without asking (off by default)
//! threshold = 80               # % of the episode after which it counts
//! keep_higher_progress = true  # a progress-only conflict where ours is higher sends ours
//!
//! [players]
//! mpv = true                   # also iina, vlc, mpc_qt, other_mpris
//! mpv_sockets = ["/run/user/1000/mpv.sock"]
//! process_scan = true          # see players' open files, no setup (no position)
//! process_minutes = 15         # open this long counts as watched (process scan only)
//!
//! [vlc]                        # macOS: VLC's web interface
//! port = 8080
//! password = "…"
//!
//! [notices]                    # one-time offers the user has answered
//! mpv_precise = true           # "enable precise detection" for mpv was shown
//!
//! [recognition]
//! relations_download = true    # refresh anime-relations from GitHub, at most once a week
//!
//! [display]
//! titles = "romaji"            # or "english", "japanese"; romaji when a title is missing
//! ```
//!
//! The MAL Client ID is public by design but still never goes into the repo, tests or logs, and
//! the VLC password is a secret: `Debug` hides both. Settings writes single lines with
//! [`set_line`], so the user's comments and order stay.

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub mal_client_id: Option<String>,
    pub updates: Updates,
    pub players: Players,
    pub vlc: Vlc,
    pub notices: Notices,
    pub recognition: Recognition,
    pub display: Display,
}

/// Which title the app (and `kot`, through the app) shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitleLang {
    /// The service's main title (romaji on MyAnimeList).
    #[default]
    Romaji,
    English,
    Japanese,
}

impl TitleLang {
    pub const ALL: [TitleLang; 3] = [TitleLang::Romaji, TitleLang::English, TitleLang::Japanese];

    /// The `config.toml` value.
    pub fn key(self) -> &'static str {
        match self {
            TitleLang::Romaji => "romaji",
            TitleLang::English => "english",
            TitleLang::Japanese => "japanese",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TitleLang::Romaji => "Romaji",
            TitleLang::English => "English",
            TitleLang::Japanese => "Japanese",
        }
    }

    /// The title to show: the English or Japanese one when asked for and known, else the main
    /// (romaji) title. Matching and recognition always use every title; this is display only.
    pub fn pick<'a>(self, main: &'a str, en: Option<&'a str>, ja: Option<&'a str>) -> &'a str {
        let alt = match self {
            TitleLang::Romaji => None,
            TitleLang::English => en,
            TitleLang::Japanese => ja,
        };
        alt.map(str::trim).filter(|t| !t.is_empty()).unwrap_or(main)
    }
}

/// Display settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Display {
    pub titles: TitleLang,
}

/// Recognition settings.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Recognition {
    /// Download a newer anime-relations file (episode redirection) at most once a week. Off: the
    /// copy built into the app is used.
    pub relations_download: bool,
}

impl Default for Recognition {
    fn default() -> Self {
        Recognition { relations_download: true }
    }
}

/// One-time offers already answered (shown once, then never again).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Notices {
    /// The "enable precise detection" offer for mpv was answered (enabled or dismissed).
    pub mpv_precise: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Updates {
    pub auto: bool,
    /// Percent, 50–100.
    pub threshold: u8,
    pub keep_higher_progress: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { auto: false, threshold: 80, keep_higher_progress: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Players {
    pub mpv: bool,
    pub iina: bool,
    pub vlc: bool,
    pub mpc_qt: bool,
    pub other_mpris: bool,
    /// Empty: the default paths (`kotora_detect::mpv::default_sockets`).
    pub mpv_sockets: Vec<String>,
    /// The process source (players' open files; `kotora_detect::procfs`).
    pub process_scan: bool,
    /// Process source: minutes open after which a file counts, when its duration is unknown.
    pub process_minutes: u32,
}

impl Default for Players {
    fn default() -> Self {
        Players { mpv: true, iina: true, vlc: true, mpc_qt: true, other_mpris: true, mpv_sockets: Vec::new(), process_scan: true, process_minutes: 15 }
    }
}

impl Players {
    /// A switch by its `config.toml` key.
    pub fn get(&self, key: &str) -> bool {
        match key {
            "mpv" => self.mpv,
            "iina" => self.iina,
            "vlc" => self.vlc,
            "mpc_qt" => self.mpc_qt,
            "other_mpris" => self.other_mpris,
            _ => false,
        }
    }

    pub fn set(&mut self, key: &str, on: bool) {
        match key {
            "mpv" => self.mpv = on,
            "iina" => self.iina = on,
            "vlc" => self.vlc = on,
            "mpc_qt" => self.mpc_qt = on,
            "other_mpris" => self.other_mpris = on,
            _ => {}
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Vlc {
    pub port: u16,
    pub password: String,
}

impl Default for Vlc {
    fn default() -> Self {
        Vlc { port: 8080, password: String::new() }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("mal_client_id", &self.mal_client_id.as_ref().map(|_| "<set>"))
            .field("updates", &self.updates)
            .field("players", &self.players)
            .field("vlc_port", &self.vlc.port)
            .field("vlc_password", &(!self.vlc.password.is_empty()).then_some("<set>"))
            .field("notices", &self.notices)
            .field("recognition", &self.recognition)
            .field("display", &self.display)
            .finish()
    }
}

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    mal: Mal,
    #[serde(default)]
    updates: Updates,
    #[serde(default)]
    players: Players,
    #[serde(default)]
    vlc: Vlc,
    #[serde(default)]
    notices: Notices,
    #[serde(default)]
    recognition: Recognition,
    #[serde(default)]
    display: Display,
}

#[derive(Deserialize, Default)]
struct Mal {
    client_id: Option<String>,
}

impl Config {
    /// Parses the text of a `config.toml`. A blank Client ID counts as none; the threshold is
    /// kept between 50 and 100.
    pub fn parse(text: &str) -> Result<Config, String> {
        let f: File = toml::from_str(text).map_err(|e| format!("config.toml: {}", e.message()))?;
        let mut updates = f.updates;
        updates.threshold = updates.threshold.clamp(50, 100);
        Ok(Config {
            mal_client_id: f.mal.client_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            updates,
            players: f.players,
            vlc: f.vlc,
            notices: f.notices,
            recognition: f.recognition,
            display: f.display,
        })
    }

    /// Reads the file. A missing file is an empty config; a broken one is an error message.
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => Config::parse(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("can't read config.toml: {e}")),
        }
    }
}

/// Sets `key = value` (a TOML literal) in `[section]` of `text`: replaces the key's line, or adds
/// it at the section's end, or adds the section at the end. Everything else stays as it was.
pub fn set_line(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let is_header = |l: &str| l.trim_start().starts_with('[');
    let new_line = format!("{key} = {value}");
    match lines.iter().position(|l| l.trim() == header) {
        Some(start) => {
            let end = lines[start + 1..].iter().position(|l| is_header(l)).map_or(lines.len(), |i| start + 1 + i);
            let found = lines[start + 1..end].iter().position(|l| {
                let t = l.trim_start();
                t.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
            });
            match found {
                Some(i) => lines[start + 1 + i] = new_line,
                None => {
                    // After the section's last setting (blank lines and comments before the next
                    // section belong to that one).
                    let mut at = end;
                    while at > start + 1 && (lines[at - 1].trim().is_empty() || lines[at - 1].trim_start().starts_with('#')) {
                        at -= 1;
                    }
                    lines.insert(at, new_line);
                }
            }
        }
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(new_line);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// A TOML string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A pasted MyAnimeList Client ID, checked: trimmed (quotes and spaces around it dropped), not
/// empty, only hex digits, and about the length MAL gives (32). The error says what to do.
pub fn check_client_id(raw: &str) -> Result<String, String> {
    let id = raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`').trim();
    if id.is_empty() {
        return Err("Paste the Client ID first.".into());
    }
    if id.chars().any(char::is_whitespace) {
        return Err("A Client ID is one word, with no spaces in it.".into());
    }
    if !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("That doesn't look like a Client ID: it has only digits and the letters a to f. Copy the Client ID, not the Client Secret or the app name.".into());
    }
    // MAL's Client IDs have 32 characters and its Client Secrets 64, both hex: a long one is
    // almost surely the secret, which must never be stored.
    match id.len() {
        0..=19 => Err(format!("That is too short for a Client ID ({} characters; MyAnimeList's have 32). Copy all of it.", id.len())),
        41..=64 => Err(format!("That looks like the Client Secret ({} characters). Kotora doesn't need it: copy the Client ID instead (32 characters).", id.len())),
        65.. => Err(format!("That is too long for a Client ID ({} characters; MyAnimeList's have 32).", id.len())),
        _ => Ok(id.to_ascii_lowercase()),
    }
}

/// Saves the MAL Client ID (`[mal] client_id`), after [`check_client_id`]. Returns the ID saved.
pub fn save_client_id(path: &Path, raw: &str) -> Result<String, String> {
    let id = check_client_id(raw)?;
    write_setting(path, "mal", "client_id", &quote(&id))?;
    Ok(id)
}

/// The last four characters of a Client ID, for a masked display.
pub fn client_id_hint(id: &str) -> String {
    let n = id.chars().count();
    id.chars().skip(n.saturating_sub(4)).collect()
}

/// Writes one setting to the file (0600, through a temp file and a rename).
pub fn write_setting(path: &Path, section: &str, key: &str, value: &str) -> Result<(), String> {
    let old = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("can't read config.toml: {e}")),
    };
    let new = set_line(&old, section, key, value);
    // Never write a file the reader would refuse.
    Config::parse(&new)?;
    if let Some(dir) = path.parent() {
        crate::paths::create_private_dir(dir).map_err(|e| format!("can't create the config folder: {e}"))?;
    }
    let tmp = path.with_extension("toml.tmp");
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp).map_err(|e| format!("can't write config.toml: {e}"))?;
        f.write_all(new.as_bytes()).map_err(|e| format!("can't write config.toml: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("can't write config.toml: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_client_id() {
        let c = Config::parse("[mal]\nclient_id = \"  fake-id  \"\n").unwrap();
        assert_eq!(c.mal_client_id.as_deref(), Some("fake-id"));
    }

    #[test]
    fn missing_or_blank_means_none() {
        assert_eq!(Config::parse("").unwrap().mal_client_id, None);
        assert_eq!(Config::parse("[mal]\nclient_id = \" \"").unwrap().mal_client_id, None);
        assert_eq!(Config::load(Path::new("/nonexistent/kotora/config.toml")).unwrap(), Config::default());
    }

    #[test]
    fn broken_file_is_an_error_and_debug_hides_secrets() {
        assert!(Config::parse("[mal\n").is_err());
        let mut c = Config { mal_client_id: Some("fake-secret-looking-id".into()), ..Config::default() };
        c.vlc.password = "fake-vlc-pass".into();
        let d = format!("{c:?}");
        assert!(!d.contains("fake-secret") && !d.contains("fake-vlc-pass"), "{d}");
    }

    #[test]
    fn defaults_ask_first() {
        let c = Config::parse("[mal]\nclient_id = \"x\"\n").unwrap();
        assert_eq!(c.updates, Updates { auto: false, threshold: 80, keep_higher_progress: true });
        assert!(c.players.mpv && c.players.vlc && c.players.other_mpris);
        assert_eq!((c.vlc.port, c.vlc.password.as_str()), (8080, ""));
        assert!(!c.notices.mpv_precise);
        assert!(Config::parse("[notices]\nmpv_precise = true\n").unwrap().notices.mpv_precise);
        assert!(c.recognition.relations_download);
        assert!(!Config::parse("[recognition]\nrelations_download = false\n").unwrap().recognition.relations_download);
        let c = Config::parse("[updates]\nauto = true\nthreshold = 10\n[players]\nvlc = false\nmpv_sockets = [\"/tmp/a.sock\"]\n").unwrap();
        assert!(c.updates.auto);
        assert_eq!(c.updates.threshold, 50, "kept at 50 or more");
        assert!(!c.players.get("vlc") && c.players.get("mpv"));
        assert_eq!(c.players.mpv_sockets, ["/tmp/a.sock"]);
    }

    #[test]
    fn title_language_falls_back_to_romaji() {
        assert_eq!(Config::parse("").unwrap().display.titles, TitleLang::Romaji);
        assert_eq!(Config::parse("[display]\ntitles = \"english\"\n").unwrap().display.titles, TitleLang::English);
        assert_eq!(Config::parse("[display]\ntitles = \"japanese\"\n").unwrap().display.titles, TitleLang::Japanese);
        assert!(Config::parse("[display]\ntitles = \"klingon\"\n").is_err(), "an unknown value is refused, never written");
        for l in TitleLang::ALL {
            let t = set_line("", "display", "titles", &quote(l.key()));
            assert_eq!(Config::parse(&t).unwrap().display.titles, l);
        }
        let (main, en, ja) = ("Kirigaoka no Lantern", Some("Lanterns of Kirigaoka"), Some("霧ヶ丘のランタン"));
        assert_eq!(TitleLang::Romaji.pick(main, en, ja), main);
        assert_eq!(TitleLang::English.pick(main, en, ja), "Lanterns of Kirigaoka");
        assert_eq!(TitleLang::Japanese.pick(main, en, ja), "霧ヶ丘のランタン");
        assert_eq!(TitleLang::English.pick(main, None, ja), main, "no English title: romaji");
        assert_eq!(TitleLang::Japanese.pick(main, en, Some("  ")), main, "a blank title counts as none");
    }

    #[test]
    fn set_line_keeps_the_rest() {
        let text = "# my settings\n[mal]\nclient_id = \"x\" # mine\n\n[updates]\nauto = false\n\n# players below\n[players]\nmpv = true\n";
        let t = set_line(text, "updates", "auto", "true");
        assert_eq!(t, "# my settings\n[mal]\nclient_id = \"x\" # mine\n\n[updates]\nauto = true\n\n# players below\n[players]\nmpv = true\n");
        let t = set_line(&t, "updates", "threshold", "90");
        assert!(t.contains("[updates]\nauto = true\nthreshold = 90\n\n# players below"), "{t}");
        let t = set_line(&t, "vlc", "password", &quote("a\"b"));
        assert!(t.ends_with("mpv = true\n\n[vlc]\npassword = \"a\\\"b\"\n"), "{t}");
        assert_eq!(Config::parse(&t).unwrap().vlc.password, "a\"b");
        // `automatic = 1` is another key, not `auto`.
        let t = set_line("[updates]\nautomatic = 1\n", "updates", "auto", "true");
        assert_eq!(t, "[updates]\nautomatic = 1\nauto = true\n");
        assert_eq!(set_line("", "updates", "auto", "true"), "[updates]\nauto = true\n");
    }

    #[test]
    fn client_ids_are_checked() {
        let good = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(check_client_id(&format!("  \"{good}\"\n")).unwrap(), good.to_ascii_lowercase());
        assert!(check_client_id("   ").unwrap_err().contains("Paste"));
        assert!(check_client_id("0123 4567").unwrap_err().contains("no spaces"));
        assert!(check_client_id("my-kotora-app-secret-xyz").unwrap_err().contains("not the Client Secret"));
        assert!(check_client_id("abc123").unwrap_err().contains("too short"));
        assert!(check_client_id(&"a".repeat(80)).unwrap_err().contains("too long"));
        assert!(check_client_id(&"0123456789abcdef".repeat(4)).unwrap_err().contains("Client Secret"), "64 hex: the secret's shape");
        assert!(check_client_id(&"9".repeat(40)).is_ok());
        assert_eq!(client_id_hint(good), "CDEF");
        assert_eq!(client_id_hint("ab"), "ab");
    }

    #[test]
    fn saving_the_client_id_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("kot-config-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "# mine\n[updates]\nauto = true\n").unwrap();
        assert!(save_client_id(&path, "nope").is_err());
        assert_eq!(Config::load(&path).unwrap().mal_client_id, None, "nothing written for a bad ID");
        let id = "fedcba9876543210fedcba9876543210";
        assert_eq!(save_client_id(&path, id).unwrap(), id);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\n[updates]\nauto = true\n"), "{text}");
        let c = Config::load(&path).unwrap();
        assert_eq!(c.mal_client_id.as_deref(), Some(id));
        assert!(c.updates.auto);
        save_client_id(&path, "00000000000000000000000000000000").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().matches("client_id").count(), 1, "replaced, not added twice");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_a_private_file() {
        let dir = std::env::temp_dir().join(format!("kot-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        write_setting(&path, "updates", "auto", "true").unwrap();
        write_setting(&path, "players", "vlc", "false").unwrap();
        let c = Config::load(&path).unwrap();
        assert!(c.updates.auto && !c.players.vlc);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(write_setting(&path, "updates", "auto", "\"not a bool\"").is_err(), "a value the reader refuses is not written");
        assert!(Config::load(&path).unwrap().updates.auto);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
