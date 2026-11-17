//! `config.toml`: the settings the user edits, by hand or in Settings.
//!
//! ```toml
//! [account]
//! service = "mal"              # or "anilist": the one service Kotora keeps the list on
//!
//! [mal]
//! client_id = "…"
//!
//! [anilist]
//! client_id = "12345"          # your own AniList API client's id (a number); optional: Kotora has its own
//!
//! [updates]
//! auto = false                 # record confident matches without asking (off by default)
//! threshold = 80               # % of the episode after which it counts
//! keep_higher_progress = true  # a progress-only conflict where ours is higher sends ours
//!
//! [players]
//! mpv = true                   # also iina, vlc, mpc_qt, other_mpris, mpc, potplayer
//! mpv_sockets = ["/run/user/1000/mpv.sock"]
//! process_scan = true          # see players' open files, no setup (no position)
//! process_minutes = 15         # open this long counts as watched (process scan only)
//!
//! [vlc]                        # macOS and Windows: VLC's web interface
//! port = 8080
//! password = "…"
//!
//! [mpc]                        # Windows: MPC-HC's and MPC-BE's web interface
//! port = 13579
//!
//! [notices]                    # one-time offers the user has answered
//! mpv_precise = true           # "enable precise detection" for mpv was shown
//! iina_precise = true          # the same for IINA (macOS)
//!
//! [notifications]              # desktop notifications, one switch each
//! detected = false             # a recognised episode starts playing
//! offer = true                 # "Update to N?", with Update and Not now
//! recorded = true              # an episode was recorded, with Undo
//! started_airing = true        # a Plan to watch show started airing
//! new_episode = true           # the next episode of a Watching show is out (estimated)
//! new_chapter = false          # a new chapter of a Reading manga (MangaUpdates; Manga on)
//! read_offer = true            # reading detection: Mark Ch N as read? (Manga on)
//! read_recorded = true         # reading detection: marked, with Undo
//!
//! [release_data]               # Publishing's sources (Manga on); at least one stays on
//! mangaupdates = true
//! mangadex = true
//!
//! [reading]                    # reading detection (Manga on)
//! yacreader = true             # watch YACReader's libraries (found from its settings)
//! libraries = ["/Volumes/Disk/Comics"]   # more library folders
//! find_libraries = true        # also the libraries in YACReader's own settings
//! open_files = false           # any reader: comic files open in a process (macOS, Linux; opt-in)
//! comic_folders = []           # only files in these folders (empty: anywhere)
//! open_minutes = 10            # open this long counts as read (else: when the next file opens)
//!
//! [recognition]
//! relations_download = true    # refresh anime-relations from GitHub, at most once a week
//!
//! [display]
//! titles = "romaji"            # or "english", "japanese"; romaji when a title is missing
//! theme = "system"             # or "light", "dark"; system follows the desktop (dark when unknown)
//! intro = true                 # the startup animation when Kotora opens with its window
//! settings_section = "general" # the Settings section opened last (written by the app)
//! list_kind = "anime"          # the sidebar's Anime | Manga switch as it was left (written by the app)
//!
//! [keys]
//! pages = "ctrl"               # with 1 to 5, opens the list pages: "ctrl" ("cmd" on macOS) or "alt"
//!
//! [startup]
//! background = false           # the login start shows only the tray icon (no window)
//! open_on = "home"             # or "last": the page shown when Kotora was last closed or hidden
//! mode = "last"                # or "anime", "manga" (Anime when Manga is off): the Anime | Manga switch
//! last_page = "home"           # the page left open (written by the app; used by open_on = "last")
//!
//! [discord]                    # Settings → General → Discord: off by default; nothing runs while off
//! enabled = false              # show what you watch or read on Discord (needs the Discord app on this computer)
//! client_id = ""               # your own Discord Application ID (empty: the one built into Kotora)
//! cover = true                 # the cover as the large image (Discord's servers fetch it; see the README)
//! buttons = true               # "View on AniList" / "View on MyAnimeList"
//! hide_paused = false          # clear the status while paused or idle
//! hide_title = false           # say "Watching anime" / "Reading manga", never the title
//!
//! [app]
//! check_updates = true         # once a day, ask kotora.moe whether a newer Kotora is out
//! [views]                      # each list page's view: "list", "grid" (posters) or "large"
//! watching = "grid"            # also plan_to_watch, completed, on_hold, dropped
//!
//! [manga_views]                # the same for the manga list pages (watching = Reading)
//! watching = "large"
//! ```
//!
//! The MAL Client ID is public by design but still never goes into the repo, tests or logs, and
//! the VLC password is a secret: `Debug` hides both. Settings writes single lines with
//! [`set_line`], so the user's comments and order stay.

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// `[account] service`: the list service the user chose on the first screen (`"mal"`), or
    /// `None` before they chose. Lower-cased; an empty value counts as none.
    pub service: Option<String>,
    pub mal_client_id: Option<String>,
    /// `[anilist] client_id`: the id of the user's AniList API client (a number).
    pub anilist_client_id: Option<String>,
    pub updates: Updates,
    pub players: Players,
    pub vlc: Vlc,
    pub mpc: Mpc,
    pub notices: Notices,
    pub recognition: Recognition,
    pub display: Display,
    pub startup: Startup,
    pub discord: Discord,
    pub views: Views,
    /// `[manga_views]`: the manga list pages' views (each kind keeps its own).
    pub manga_views: Views,
    pub notifications: Notifications,
    pub library: Library,
    pub app: AppSection,
    pub keys: Keys,
    pub release_data: ReleaseData,
    pub reading: Reading,
}

/// `[reading]`: reading detection (Manga). It runs only with Manga on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Reading {
    /// Watch YACReader's libraries (on by default; nothing runs without a library).
    pub yacreader: bool,
    /// Library folders the user added, beside the ones YACReader's settings list.
    pub libraries: Vec<String>,
    /// Also watch the libraries listed in YACReader's own settings (`YACReaderLibrary.ini`).
    pub find_libraries: bool,
    /// Stage 2, opt-in: comic files any process has open (macOS, Linux).
    pub open_files: bool,
    /// Folders the open comic files must be in (empty: anywhere).
    pub comic_folders: Vec<String>,
    /// A comic open this long counts as read (when no next file opened first).
    pub open_minutes: u32,
}

impl Default for Reading {
    fn default() -> Self {
        Reading { yacreader: true, libraries: Vec::new(), find_libraries: true, open_files: false, comic_folders: Vec::new(), open_minutes: 10 }
    }
}

/// `[discord]`: Rich Presence ("Show on Discord"). Off by default. `client_id` is a Discord
/// Application ID (public, like a MAL client id); empty means the one built into Kotora.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Discord {
    pub enabled: bool,
    #[serde(deserialize_with = "lenient_string")]
    pub client_id: String,
    pub cover: bool,
    pub buttons: bool,
    pub hide_paused: bool,
    pub hide_title: bool,
}

impl Default for Discord {
    fn default() -> Self {
        Discord { enabled: false, client_id: String::new(), cover: true, buttons: true, hide_paused: false, hide_title: false }
    }
}

/// `[release_data]`: Publishing's release sources (both on by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ReleaseData {
    pub mangaupdates: bool,
    pub mangadex: bool,
}

impl Default for ReleaseData {
    fn default() -> Self {
        ReleaseData { mangaupdates: true, mangadex: true }
    }
}

/// Kotora itself (`[app]`). Not `[updates]`, which is about the list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AppSection {
    /// Settings → About Kotora → Check for updates: once a day, ask whether a newer version is
    /// out (`update.rs`). On by default; off, nothing is sent.
    pub check_updates: bool,
}

impl Default for AppSection {
    fn default() -> Self {
        AppSection { check_updates: true }
    }
}

/// Settings → Notifications: which events show a desktop notification.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Notifications {
    /// A recognised episode starts playing ("Title · Ep 6"). Off by default.
    pub detected: bool,
    /// "Update to N?", with Update and Not now where the system shows buttons.
    pub offer: bool,
    /// An episode was recorded, with Undo.
    pub recorded: bool,
    /// A Plan to watch show started airing (MyAnimeList's status changed).
    pub started_airing: bool,
    /// The next episode of a Watching show is out, by the estimated broadcast time.
    pub new_episode: bool,
    /// A new chapter of a Reading manga is on MangaUpdates (Manga on). Off by default: it is
    /// the one switch that makes Kotora ask a third service in the background.
    pub new_chapter: bool,
    /// Reading detection: "Mark Ch 21 of Title as read?" (as `offer` for anime).
    pub read_offer: bool,
    /// Reading detection: a chapter was marked, with Undo (as `recorded` for anime).
    pub read_recorded: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Notifications { detected: false, offer: true, recorded: true, started_airing: true, new_episode: true, new_chapter: false, read_offer: true, read_recorded: true }
    }
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

/// Settings → Appearance: light, dark, or whatever the desktop prefers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follows the desktop (macOS appearance, the XDG portal's `color-scheme` on Linux); dark
    /// when the desktop says nothing.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    /// The `config.toml` value.
    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }

    /// Whether the light palette is in force, given the desktop's preference (`system_dark`).
    pub fn is_light(self, system_dark: bool) -> bool {
        match self {
            Theme::System => !system_dark,
            Theme::Light => true,
            Theme::Dark => false,
        }
    }
}

/// `[keys] pages`: the modifier that, with 1 to 5, opens Watching, Plan to watch, Completed,
/// On hold and Dropped. The other shortcuts (Ctrl , Ctrl K, Ctrl F; ⌘ on macOS) don't change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum PageKeys {
    /// Ctrl on Linux, ⌘ on macOS. The file says "ctrl" or "cmd"; either reads as this.
    #[default]
    #[serde(rename = "ctrl", alias = "cmd")]
    Primary,
    /// Alt (Option on macOS).
    #[serde(rename = "alt", alias = "option")]
    Alt,
}

impl PageKeys {
    pub const ALL: [PageKeys; 2] = [PageKeys::Primary, PageKeys::Alt];

    /// The `config.toml` value.
    pub fn key(self) -> &'static str {
        match self {
            PageKeys::Primary if cfg!(target_os = "macos") => "cmd",
            PageKeys::Primary => "ctrl",
            PageKeys::Alt => "alt",
        }
    }

    /// The modifier as Settings and the hints name it.
    pub fn label(self) -> &'static str {
        match self {
            PageKeys::Primary if cfg!(target_os = "macos") => "\u{2318} Cmd",
            PageKeys::Primary => "Ctrl",
            PageKeys::Alt if cfg!(target_os = "macos") => "\u{2325} Option",
            PageKeys::Alt => "Alt",
        }
    }

    /// The chord with `digit`, in Rune's chord syntax ("mod+1", "alt+1").
    pub fn chord(self, digit: char) -> String {
        match self {
            PageKeys::Primary => format!("mod+{digit}"),
            PageKeys::Alt => format!("alt+{digit}"),
        }
    }

    /// A short hint for the chord with `digit`: "Ctrl 1", "Alt 1", "\u{2318} 1", "\u{2325} 1".
    pub fn hint(self, digit: char) -> String {
        let m = match self {
            PageKeys::Primary if cfg!(target_os = "macos") => "\u{2318}",
            PageKeys::Primary => "Ctrl",
            PageKeys::Alt if cfg!(target_os = "macos") => "\u{2325}",
            PageKeys::Alt => "Alt",
        };
        format!("{m} {digit}")
    }
}

/// `[keys]`: keyboard shortcuts that can change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Keys {
    pub pages: PageKeys,
}

/// Display settings.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Display {
    pub titles: TitleLang,
    pub theme: Theme,
    /// The startup animation on a cold start with a window (on by default).
    pub intro: bool,
    /// The Settings section opened last ("general", "detection", …; the app reads it).
    pub settings_section: String,
    /// The sidebar's Anime | Manga switch, as it was left (`"anime"` or `"manga"`).
    pub list_kind: crate::Kind,
}

impl Default for Display {
    fn default() -> Self {
        Display { titles: TitleLang::default(), theme: Theme::default(), intro: true, settings_section: String::new(), list_kind: crate::Kind::Anime }
    }
}

/// How a list page shows its entries: the table, a grid of posters, or large cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListView {
    #[default]
    List,
    Grid,
    Large,
}

impl ListView {
    pub const ALL: [ListView; 3] = [ListView::List, ListView::Grid, ListView::Large];

    /// The `config.toml` value.
    pub fn key(self) -> &'static str {
        match self {
            ListView::List => "list",
            ListView::Grid => "grid",
            ListView::Large => "large",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ListView::List => "List",
            ListView::Grid => "Grid",
            ListView::Large => "Large",
        }
    }
}

/// `[views]`: the view each list page keeps (the switch in the list's toolbar).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Views {
    pub watching: ListView,
    pub plan_to_watch: ListView,
    pub completed: ListView,
    pub on_hold: ListView,
    pub dropped: ListView,
}

impl Views {
    /// The `config.toml` key of a list.
    pub fn key(st: crate::Status) -> &'static str {
        match st {
            crate::Status::Watching => "watching",
            crate::Status::PlanToWatch => "plan_to_watch",
            crate::Status::Completed => "completed",
            crate::Status::OnHold => "on_hold",
            crate::Status::Dropped => "dropped",
        }
    }

    pub fn get(&self, st: crate::Status) -> ListView {
        match st {
            crate::Status::Watching => self.watching,
            crate::Status::PlanToWatch => self.plan_to_watch,
            crate::Status::Completed => self.completed,
            crate::Status::OnHold => self.on_hold,
            crate::Status::Dropped => self.dropped,
        }
    }

    pub fn set(&mut self, st: crate::Status, v: ListView) {
        let slot = match st {
            crate::Status::Watching => &mut self.watching,
            crate::Status::PlanToWatch => &mut self.plan_to_watch,
            crate::Status::Completed => &mut self.completed,
            crate::Status::OnHold => &mut self.on_hold,
            crate::Status::Dropped => &mut self.dropped,
        };
        *slot = v;
    }
}

/// Settings → Startup. Whether Kotora starts at login is the login entry itself
/// (`app/src/autostart.rs`), not a setting here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Startup {
    /// The login start passes `--hidden`: the tray or menu bar icon only, no window.
    pub background: bool,
    /// `open_on` as written. Read with [`Startup::open_on`]: a bad value is a warning, never an error.
    #[serde(deserialize_with = "lenient_string")]
    pub open_on: String,
    /// `mode` as written. Read with [`Startup::mode`].
    #[serde(deserialize_with = "lenient_string")]
    pub mode: String,
    /// The page Kotora was left on (`"home"`, `"list:watching"`, `"details:manga:30013"`, …),
    /// written by the app when the window closes or hides.
    #[serde(deserialize_with = "lenient_string")]
    pub last_page: String,
}

/// Any TOML value as text, so a wrong type is a value to warn about, not a parse error.
fn lenient_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = toml::Value::deserialize(d)?;
    Ok(match v {
        toml::Value::String(s) => s,
        other => format!("{other:?}"),
    })
}

/// `[startup] open_on`: the page Kotora opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenOn {
    #[default]
    Home,
    /// The page shown when Kotora was last closed or hidden.
    Last,
}

impl OpenOn {
    pub const ALL: [OpenOn; 2] = [OpenOn::Home, OpenOn::Last];

    pub fn key(self) -> &'static str {
        match self {
            OpenOn::Home => "home",
            OpenOn::Last => "last",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OpenOn::Home => "Home",
            OpenOn::Last => "Last page",
        }
    }

    pub fn from_key(s: &str) -> Option<OpenOn> {
        OpenOn::ALL.into_iter().find(|o| o.key() == s.trim().to_lowercase())
    }
}

/// `[startup] mode`: the Anime | Manga switch at start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartMode {
    /// As it was left (`[display] list_kind`).
    #[default]
    Last,
    Anime,
    /// Manga when Manga is on, else Anime.
    Manga,
}

impl StartMode {
    pub const ALL: [StartMode; 3] = [StartMode::Last, StartMode::Anime, StartMode::Manga];

    pub fn key(self) -> &'static str {
        match self {
            StartMode::Last => "last",
            StartMode::Anime => "anime",
            StartMode::Manga => "manga",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StartMode::Last => "Last used",
            StartMode::Anime => "Anime",
            StartMode::Manga => "Manga",
        }
    }

    pub fn from_key(s: &str) -> Option<StartMode> {
        StartMode::ALL.into_iter().find(|m| m.key() == s.trim().to_lowercase())
    }
}

impl Startup {
    /// Where to open; Home when the key is absent or not a choice.
    pub fn open_on(&self) -> OpenOn {
        OpenOn::from_key(&self.open_on).unwrap_or_default()
    }

    /// The switch at start; Last used when the key is absent or not a choice.
    pub fn mode(&self) -> StartMode {
        StartMode::from_key(&self.mode).unwrap_or_default()
    }
}

impl Config {
    /// What in the file is ignored and why (a value that is not a choice): shown on stderr at
    /// start, never an error.
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        if !self.startup.open_on.is_empty() && OpenOn::from_key(&self.startup.open_on).is_none() {
            w.push(format!("config.toml: [startup] open_on = {:?} is not \"home\" or \"last\"; opening on Home", self.startup.open_on));
        }
        if !self.startup.mode.is_empty() && StartMode::from_key(&self.startup.mode).is_none() {
            w.push(format!("config.toml: [startup] mode = {:?} is not \"last\", \"anime\" or \"manga\"; using the last used", self.startup.mode));
        }
        w
    }
}

/// Settings → Library: the folders where the anime files are, and the player "Play next
/// episode" opens.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Library {
    /// Folders, as added. A folder that is missing (an unplugged drive) stays here: it shows as
    /// offline, and its files come back with it.
    pub folders: Vec<String>,
    /// `auto` (the player Kotora last saw playing, else the system's), `default` (the system's),
    /// `mpv`, `iina`, `vlc`.
    pub open_with: String,
    /// The player Kotora last saw playing (`mpv`, `iina`, `vlc`), for `auto`.
    pub last_player: String,
}

impl Default for Library {
    fn default() -> Self {
        Library { folders: Vec::new(), open_with: "auto".into(), last_player: String::new() }
    }
}

/// A TOML array of strings.
pub fn quote_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", "))
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
    /// The same offer for IINA (macOS), answered.
    pub iina_precise: bool,
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
    /// MPC-HC and MPC-BE (Windows).
    pub mpc: bool,
    /// PotPlayer (Windows).
    pub potplayer: bool,
    /// Empty: the default paths (`kotora_detect::mpv::default_sockets`).
    pub mpv_sockets: Vec<String>,
    /// The process source (players' open files; `kotora_detect::procfs`).
    pub process_scan: bool,
    /// Process source: minutes open after which a file counts, when its duration is unknown.
    pub process_minutes: u32,
}

impl Default for Players {
    fn default() -> Self {
        Players { mpv: true, iina: true, vlc: true, mpc_qt: true, other_mpris: true, mpc: true, potplayer: true, mpv_sockets: Vec::new(), process_scan: true, process_minutes: 15 }
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
            "mpc" => self.mpc,
            "potplayer" => self.potplayer,
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
            "mpc" => self.mpc = on,
            "potplayer" => self.potplayer = on,
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

/// MPC-HC's and MPC-BE's web interface (Windows).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Mpc {
    pub port: u16,
}

impl Default for Mpc {
    fn default() -> Self {
        Mpc { port: 13579 }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("service", &self.service)
            .field("mal_client_id", &self.mal_client_id.as_ref().map(|_| "<set>"))
            .field("anilist_client_id", &self.anilist_client_id.as_ref().map(|_| "<set>"))
            .field("updates", &self.updates)
            .field("players", &self.players)
            .field("vlc_port", &self.vlc.port)
            .field("vlc_password", &(!self.vlc.password.is_empty()).then_some("<set>"))
            .field("notices", &self.notices)
            .field("recognition", &self.recognition)
            .field("display", &self.display)
            .field("startup", &self.startup)
            .field("discord", &self.discord)
            .field("notifications", &self.notifications)
            .field("library_folders", &self.library.folders.len())
            .field("app", &self.app)
            .field("keys", &self.keys)
            .field("reading", &self.reading)
            .finish()
    }
}

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    account: Account,
    #[serde(default)]
    mal: Mal,
    #[serde(default)]
    anilist: Mal,
    #[serde(default)]
    updates: Updates,
    #[serde(default)]
    players: Players,
    #[serde(default)]
    vlc: Vlc,
    #[serde(default)]
    mpc: Mpc,
    #[serde(default)]
    notices: Notices,
    #[serde(default)]
    recognition: Recognition,
    #[serde(default)]
    display: Display,
    #[serde(default)]
    startup: Startup,
    #[serde(default)]
    discord: Discord,
    #[serde(default)]
    views: Views,
    #[serde(default)]
    manga_views: Views,
    #[serde(default)]
    notifications: Notifications,
    #[serde(default)]
    library: Library,
    #[serde(default)]
    app: AppSection,
    #[serde(default)]
    keys: Keys,
    #[serde(default)]
    release_data: ReleaseData,
    #[serde(default)]
    reading: Reading,
}

#[derive(Deserialize, Default)]
struct Account {
    service: Option<String>,
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
            service: f.account.service.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()),
            mal_client_id: f.mal.client_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            anilist_client_id: f.anilist.client_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            updates,
            players: f.players,
            vlc: f.vlc,
            mpc: f.mpc,
            notices: f.notices,
            recognition: f.recognition,
            display: f.display,
            startup: f.startup,
            discord: Discord { client_id: f.discord.client_id.trim().to_string(), ..f.discord },
            views: f.views,
            manga_views: f.manga_views,
            notifications: f.notifications,
            library: f.library,
            app: f.app,
            keys: f.keys,
            // Both off is not a choice: MangaUpdates comes back on.
            release_data: if f.release_data.mangaupdates || f.release_data.mangadex { f.release_data } else { ReleaseData::default() },
            reading: f.reading,
        })
    }

    /// Whether the first screen ("Which service do you use?") is still due: the user has not
    /// chosen, has no Client ID in the file and has no saved token. Anyone who already uses
    /// Kotora with MyAnimeList skips it, and nothing is written for them.
    pub fn needs_service_choice(&self, has_token: bool) -> bool {
        self.service.is_none() && self.mal_client_id.is_none() && self.anilist_client_id.is_none() && !has_token
    }

    /// The service Kotora keeps the list on: `[account] service`, MyAnimeList when none is
    /// chosen (everybody from 0.1) or the value is unknown.
    pub fn service_id(&self) -> crate::service::ServiceId {
        self.service.as_deref().and_then(crate::service::ServiceId::from_name).unwrap_or_default()
    }

    /// The Client ID of `service`, when one is saved.
    pub fn client_id(&self, service: crate::service::ServiceId) -> Option<&str> {
        match service {
            crate::service::ServiceId::Mal => self.mal_client_id.as_deref(),
            crate::service::ServiceId::AniList => self.anilist_client_id.as_deref(),
        }
    }

    /// The Client ID Kotora signs in with: the environment variable (development), else the
    /// one in `config.toml` ("Use my own client ID"), else the one built into Kotora.
    pub fn effective_client_id(&self, service: crate::service::ServiceId) -> Option<String> {
        let env = std::env::var(service.client_id_env()).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        env.or_else(|| self.client_id(service).map(str::to_string)).or_else(|| service.built_in_client_id().map(str::to_string))
    }

    /// Whether Kotora has a Client ID to sign in with, saved or built in.
    pub fn has_client_id(&self, service: crate::service::ServiceId) -> bool {
        self.effective_client_id(service).is_some()
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

/// The services the first screen offers: the value written to `[account] service`.
pub const SERVICE_MAL: &str = "mal";
pub const SERVICE_ANILIST: &str = "anilist";

/// Checks a pasted AniList Client ID: AniList's are whole numbers (its client secrets are 40
/// letters and digits, never needed). Returns the cleaned ID.
pub fn check_anilist_client_id(raw: &str) -> Result<String, String> {
    let id = raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`').trim();
    if id.is_empty() {
        return Err("Paste the Client ID first.".into());
    }
    if id.chars().any(char::is_whitespace) {
        return Err("A Client ID is one word, with no spaces in it.".into());
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(if id.len() >= 30 {
            "That looks like the Client Secret. Kotora doesn't need it: copy the ID instead (a number).".into()
        } else {
            "That doesn't look like an AniList Client ID: it is a number, like 12345.".into()
        });
    }
    if id.len() > 10 || id.trim_start_matches('0').is_empty() {
        return Err("That doesn't look like an AniList Client ID: it is a number, like 12345.".into());
    }
    Ok(id.to_string())
}

/// Saves the AniList Client ID (`[anilist] client_id`), after [`check_anilist_client_id`].
pub fn save_anilist_client_id(path: &Path, raw: &str) -> Result<String, String> {
    let id = check_anilist_client_id(raw)?;
    write_setting(path, "anilist", "client_id", &quote(&id))?;
    Ok(id)
}

/// Saves the Client ID of `service`, checked the way that service's IDs look.
pub fn save_client_id_for(path: &Path, service: crate::service::ServiceId, raw: &str) -> Result<String, String> {
    match service {
        crate::service::ServiceId::Mal => save_client_id(path, raw),
        crate::service::ServiceId::AniList => save_anilist_client_id(path, raw),
    }
}

/// Goes back to Kotora's built-in Client ID for `service`: blanks `[mal] client_id` or
/// `[anilist] client_id` (a blank one counts as none).
pub fn clear_client_id(path: &Path, service: crate::service::ServiceId) -> Result<(), String> {
    write_setting(path, service.name(), "client_id", &quote(""))
}

/// Saves the chosen service (`[account] service`).
pub fn save_service(path: &Path, service: &str) -> Result<(), String> {
    write_setting(path, "account", "service", &quote(service))
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
    fn discord_is_off_by_default_and_read_leniently() {
        let d = Config::default().discord;
        assert!(!d.enabled && d.cover && d.buttons && !d.hide_paused && !d.hide_title && d.client_id.is_empty());
        let c = Config::parse("[discord]\nenabled = true\nclient_id = \" 123 \"\nhide_title = true\ncover = false\n").unwrap().discord;
        assert!(c.enabled && c.hide_title && !c.cover && c.buttons);
        assert_eq!(c.client_id, "123");
        assert!(Config::parse("[discord]\nclient_id = 42\n").is_ok(), "a number is no parse error");
    }

    #[test]
    fn page_keys_are_ctrl_by_default_and_alt_on_request() {
        assert_eq!(Config::parse("").unwrap().keys.pages, PageKeys::Primary);
        assert_eq!(Config::parse("[keys]\npages = \"alt\"\n").unwrap().keys.pages, PageKeys::Alt);
        assert_eq!(Config::parse("[keys]\npages = \"ctrl\"\n").unwrap().keys.pages, PageKeys::Primary);
        assert_eq!(Config::parse("[keys]\npages = \"cmd\"\n").unwrap().keys.pages, PageKeys::Primary, "the macOS name reads too");
        assert!(Config::parse("[keys]\npages = \"hyper\"\n").is_err(), "an unknown value is refused");
        for k in PageKeys::ALL {
            let t = set_line("[display]\ntheme = \"dark\"\n", "keys", "pages", &quote(k.key()));
            let c = Config::parse(&t).unwrap();
            assert_eq!(c.keys.pages, k);
            assert_eq!(c.display.theme, Theme::Dark, "the other sections stay");
        }
        assert_eq!(PageKeys::Alt.chord('3'), "alt+3");
        assert_eq!(PageKeys::Primary.chord('1'), "mod+1");
        assert_eq!(PageKeys::Alt.hint('2'), if cfg!(target_os = "macos") { "\u{2325} 2" } else { "Alt 2" });
        assert_eq!(PageKeys::Primary.hint('5'), if cfg!(target_os = "macos") { "\u{2318} 5" } else { "Ctrl 5" });
    }

    #[test]
    fn library_folders_round_trip() {
        let c = Config::default();
        assert!(c.library.folders.is_empty());
        assert_eq!(c.library.open_with, "auto");
        let folders = vec!["/Volumes/Disk \"A\"/Anime".to_string(), "/srv/anime\\x".to_string()];
        let text = set_line("", "library", "folders", &quote_list(&folders));
        let c = Config::parse(&text).unwrap();
        assert_eq!(c.library.folders, folders);
        assert_eq!(Config::parse(&set_line(&text, "library", "folders", &quote_list(&[]))).unwrap().library.folders, Vec::<String>::new());
    }

    #[test]
    fn the_service_choice_is_read_and_skipped_for_existing_users() {
        let none = Config::parse("").unwrap();
        assert_eq!(none.service, None);
        assert!(none.needs_service_choice(false), "a fresh install is asked");
        assert!(!none.needs_service_choice(true), "a saved token means MyAnimeList already");
        let id = Config::parse("[mal]\nclient_id = \"fake-id\"\n").unwrap();
        assert!(!id.needs_service_choice(false), "a Client ID in the file counts too");
        let text = set_line("", "account", "service", &quote(SERVICE_MAL));
        assert_eq!(text, "[account]\nservice = \"mal\"\n");
        let c = Config::parse(&text).unwrap();
        assert_eq!(c.service.as_deref(), Some("mal"));
        assert!(!c.needs_service_choice(false), "chosen once, never asked again");
        assert_eq!(Config::parse("[account]\nservice = \" MAL \"\n").unwrap().service.as_deref(), Some("mal"));
        assert_eq!(Config::parse("[account]\nservice = \"\"\n").unwrap().service, None);
    }

    #[test]
    fn save_service_keeps_the_rest_and_the_mode() {
        let dir = std::env::temp_dir().join(format!("kotora-svc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        write_setting(&path, "updates", "auto", "true").unwrap();
        save_service(&path, SERVICE_MAL).unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.service.as_deref(), Some("mal"));
        assert!(c.updates.auto, "the other settings stay");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        assert!(Config::parse("[notices]\niina_precise = true\n").unwrap().notices.iina_precise);
        assert!(c.recognition.relations_download);
        assert!(!Config::parse("[recognition]\nrelations_download = false\n").unwrap().recognition.relations_download);
        assert!(!c.startup.background, "the login start opens the window by default");
        assert!(Config::parse("[startup]\nbackground = true\n").unwrap().startup.background);
        let s = Config::parse("").unwrap();
        assert_eq!((s.startup.open_on(), s.startup.mode()), (OpenOn::Home, StartMode::Last), "today's behaviour");
        assert!(s.warnings().is_empty());
        let s = Config::parse("[startup]\nopen_on = \"last\"\nmode = \"Manga\"\n").unwrap();
        assert_eq!((s.startup.open_on(), s.startup.mode()), (OpenOn::Last, StartMode::Manga));
        let s = Config::parse("[startup]\nopen_on = \"anywhere\"\nmode = \"both\"\n").unwrap();
        assert_eq!((s.startup.open_on(), s.startup.mode()), (OpenOn::Home, StartMode::Last), "a bad value is not an error");
        assert_eq!(s.warnings().len(), 2);
        let s = Config::parse("[startup]\nopen_on = 3\nmode = true\n").unwrap();
        assert_eq!((s.startup.open_on(), s.startup.mode(), s.warnings().len()), (OpenOn::Home, StartMode::Last, 2), "a wrong type too");
        assert!(c.app.check_updates, "the update check is on by default (the site says so)");
        let off = set_line("[updates]\nauto = true\n", "app", "check_updates", "false");
        let off = Config::parse(&off).unwrap();
        assert!(!off.app.check_updates && off.updates.auto, "[app] is not the list's [updates]");
        let c = Config::parse("[updates]\nauto = true\nthreshold = 10\n[players]\nvlc = false\nmpv_sockets = [\"/tmp/a.sock\"]\n").unwrap();
        assert!(c.updates.auto);
        assert_eq!(c.updates.threshold, 50, "kept at 50 or more");
        assert!(!c.players.get("vlc") && c.players.get("mpv"));
        assert_eq!(c.players.mpv_sockets, ["/tmp/a.sock"]);
    }

    #[test]
    fn notifications_offer_and_recorded_on_detected_off() {
        let n = Config::parse("").unwrap().notifications;
        assert_eq!(n, Notifications { detected: false, offer: true, recorded: true, started_airing: true, new_episode: true, new_chapter: false, read_offer: true, read_recorded: true });
        // The manga reading switches are their own: turning the anime ones off leaves them on.
        assert!(Config::parse("[notifications]\noffer = false\nrecorded = false\n").unwrap().notifications.read_offer);
        let r = Config::parse("[reading]\nfind_libraries = false\nopen_files = true\nopen_minutes = 4\n").unwrap().reading;
        assert_eq!((r.yacreader, r.find_libraries, r.open_files, r.open_minutes), (true, false, true, 4));
        assert!(Config::parse("[notifications]\nnew_chapter = true\n").unwrap().notifications.new_chapter);
        let n = Config::parse("[notifications]\ndetected = true\noffer = false\n").unwrap().notifications;
        assert_eq!(n, Notifications { detected: true, offer: false, ..Notifications::default() });
        let n = Config::parse("[notifications]\nstarted_airing = false\nnew_episode = false\n").unwrap().notifications;
        assert_eq!((n.started_airing, n.new_episode, n.offer), (false, false, true));
    }

    #[test]
    fn list_views_read_and_write_per_list() {
        use crate::Status;
        let c = Config::parse("").unwrap();
        assert!(Status::ALL.iter().all(|st| c.views.get(*st) == ListView::List), "the table by default");
        let mut text = String::new();
        for (st, v) in [(Status::Watching, ListView::Grid), (Status::Completed, ListView::Large), (Status::OnHold, ListView::Grid)] {
            text = set_line(&text, "views", Views::key(st), &quote(v.key()));
        }
        let c = Config::parse(&text).unwrap();
        assert_eq!(c.views.get(Status::Watching), ListView::Grid);
        assert_eq!(c.views.get(Status::Completed), ListView::Large);
        assert_eq!(c.views.get(Status::OnHold), ListView::Grid);
        assert_eq!(c.views.get(Status::PlanToWatch), ListView::List);
        let mut v = c.views;
        v.set(Status::Dropped, ListView::Large);
        assert_eq!(v.get(Status::Dropped), ListView::Large);
        assert!(Config::parse("[views]\nwatching = \"mosaic\"\n").is_err(), "an unknown value is refused");
        // The manga pages keep their own views.
        let c = Config::parse("[views]\nwatching = \"grid\"\n[manga_views]\nwatching = \"large\"\n").unwrap();
        assert_eq!((c.views.get(Status::Watching), c.manga_views.get(Status::Watching)), (ListView::Grid, ListView::Large));
        assert_eq!(c.manga_views.get(Status::Completed), ListView::List);
    }

    #[test]
    fn theme_setting_reads_writes_and_resolves() {
        assert_eq!(Config::parse("").unwrap().display.theme, Theme::System, "System by default");
        for t in Theme::ALL {
            let toml = set_line("[display]\ntitles = \"english\"\n", "display", "theme", &quote(t.key()));
            let c = Config::parse(&toml).unwrap();
            assert_eq!(c.display.theme, t);
            assert_eq!(c.display.titles, TitleLang::English, "the other display line stays");
        }
        assert!(Config::parse("[display]\ntheme = \"sepia\"\n").is_err(), "an unknown value is refused");
        assert!(Config::parse("").unwrap().display.intro, "the startup animation is on by default");
        assert!(!Config::parse("[display]\nintro = false\n").unwrap().display.intro);
        assert_eq!(Config::parse("").unwrap().display.list_kind, crate::Kind::Anime, "Anime unless the switch was left on Manga");
        assert_eq!(Config::parse("[display]\nlist_kind = \"manga\"\n").unwrap().display.list_kind, crate::Kind::Manga);
        assert_eq!(Config::parse("").unwrap().display.settings_section, "");
        assert_eq!(Config::parse("[display]\nsettings_section = \"detection\"\n").unwrap().display.settings_section, "detection");
        assert!(Theme::System.is_light(false) && !Theme::System.is_light(true), "System follows the desktop");
        assert!(Theme::Light.is_light(true) && Theme::Light.is_light(false));
        assert!(!Theme::Dark.is_light(true) && !Theme::Dark.is_light(false));
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
    fn anilist_ids_and_the_chosen_service_are_read() {
        use crate::service::ServiceId;
        let c = Config::parse("[account]\nservice = \"anilist\"\n[anilist]\nclient_id = \" 12345 \"\n").unwrap();
        assert_eq!((c.service_id(), c.client_id(ServiceId::AniList), c.client_id(ServiceId::Mal)), (ServiceId::AniList, Some("12345"), None));
        assert!(!c.needs_service_choice(false));
        assert!(!Config { anilist_client_id: Some("1".into()), ..Config::default() }.needs_service_choice(false), "an AniList ID counts too");
        assert_eq!(Config::default().service_id(), ServiceId::Mal, "0.1 users are on MyAnimeList");
        assert_eq!(Config::parse("[account]\nservice = \"kitsu\"\n").unwrap().service_id(), ServiceId::Mal);
        assert!(!format!("{c:?}").contains("12345"));
    }

    #[test]
    fn the_client_id_falls_back_to_the_built_in_one() {
        use crate::service::{ANILIST_CLIENT_ID, MAL_CLIENT_ID, ServiceId};
        // No environment variable is set for this one in tests.
        let empty = Config::default();
        assert_eq!(empty.effective_client_id(ServiceId::AniList).as_deref(), Some(ANILIST_CLIENT_ID));
        assert!(empty.has_client_id(ServiceId::AniList));
        let mal = MAL_CLIENT_ID.unwrap();
        assert_eq!(mal.len(), 32, "a MyAnimeList id: 32 hex characters");
        assert!(check_client_id(mal).is_ok());
        assert_eq!(empty.effective_client_id(ServiceId::Mal).as_deref(), Some(mal));
        assert!(empty.has_client_id(ServiceId::Mal));
        let own = Config { anilist_client_id: Some("777".into()), mal_client_id: Some("m".into()), ..Config::default() };
        assert_eq!(own.effective_client_id(ServiceId::AniList).as_deref(), Some("777"), "the user's own wins");
        assert_eq!(own.effective_client_id(ServiceId::Mal).as_deref(), Some("m"));
        assert!(ANILIST_CLIENT_ID.parse::<u32>().is_ok_and(|n| n > 0));
        // The saved ID alone still says whether the user brought their own.
        assert_eq!(empty.client_id(ServiceId::AniList), None);
    }

    #[test]
    fn anilist_client_ids_are_checked() {
        assert_eq!(check_anilist_client_id(" \"12345\"\n").unwrap(), "12345");
        assert!(check_anilist_client_id("").unwrap_err().contains("Paste"));
        assert!(check_anilist_client_id("12 345").unwrap_err().contains("no spaces"));
        assert!(check_anilist_client_id("aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5bC7d").unwrap_err().contains("Client Secret"));
        assert!(check_anilist_client_id("abc").unwrap_err().contains("a number"));
        assert!(check_anilist_client_id("0").is_err());
        assert!(check_anilist_client_id("12345678901").is_err());
    }

    #[test]
    fn going_back_to_the_built_in_id_blanks_the_saved_one() {
        let dir = std::env::temp_dir().join(format!("kotora-clear-id-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[mal]\nclient_id = \"0123456789abcdef0123456789abcdef\" # mine\n\n[anilist]\nclient_id = \"12345\"\n\n[updates]\nauto = true\n").unwrap();
        clear_client_id(&path, crate::service::ServiceId::Mal).unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!((c.mal_client_id.as_deref(), c.anilist_client_id.as_deref()), (None, Some("12345")), "only that service's");
        clear_client_id(&path, crate::service::ServiceId::AniList).unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.anilist_client_id, None);
        assert!(c.updates.auto, "the rest stays");
        std::fs::remove_dir_all(&dir).unwrap();
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
