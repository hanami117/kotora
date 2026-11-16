//! App state. The real list comes from the store and MyAnimeList (`online.rs`); the fake list below
//! is for the headless scenes and the tests only.
//!
//! Every fake title here is made up (the same ones as `mockup/kotora.html`).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kotora_core::airing::{NotAired, Timetable};
use kotora_core::config::TitleLang;
use kotora_core::queue::{Op, OpState};
use kotora_core::{Change, ListItem, Media, Remote, Status, Values};
use rune::{AppHandle, Image, TextEdit};

use kotora_core::recognize::Found;
use kotora_detect::{Led, SourceStatus, Switch, Via};

use crate::precise::PlayerState;

use crate::art::Covers;
use crate::now::{Now, Prefs, Seen};
use crate::queue::{Cmd, Worker};

pub type Handle = AppHandle<State>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Shelves: continue watching, airing this week, plan to watch (`home.rs`).
    Home,
    /// The account: picture, name, MyAnimeList's numbers, the score spread (`profile.rs`).
    Profile,
    List(Status),
    NowPlaying,
    Queue,
    Settings,
    /// One anime's information (`details.rs`); `State::detail` says which.
    Details,
    /// The search page (`search.rs`): the list and MyAnimeList (Ctrl K).
    Search,
    /// The setup wizard (`setup.rs`): Client ID and sign-in, step by step.
    Setup,
    /// What airs this week and this season (`airing.rs`).
    Airing,
}

/// The chips above the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Airing,
    Finished,
    NewEpisode,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Airing, Filter::Finished, Filter::NewEpisode];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Airing => "Airing",
            Filter::Finished => "Finished airing",
            Filter::NewEpisode => "New episode",
        }
    }

    pub fn keeps(self, e: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Airing => e.air == Air::Airing,
            Filter::Finished => e.air == Air::Finished,
            Filter::NewEpisode => e.new_episode.is_some(),
        }
    }
}

/// Where a show is in its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Air {
    Airing,
    Finished,
    Upcoming,
    Unknown,
}

impl Air {
    /// MyAnimeList's `status` of the anime.
    pub fn mal_name(self) -> &'static str {
        match self {
            Air::Airing => "currently_airing",
            Air::Finished => "finished_airing",
            Air::Upcoming => "not_yet_aired",
            Air::Unknown => "",
        }
    }
}

/// How the list is ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Updated,
    Title,
    Score,
    Progress,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Updated, Sort::Title, Sort::Score, Sort::Progress];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Updated => "Last updated",
            Sort::Title => "Title",
            Sort::Score => "Score",
            Sort::Progress => "Progress",
        }
    }

    /// The menu's two directions: the order's own first (what it sorts by at first), then the
    /// reverse.
    pub fn directions(self) -> [&'static str; 2] {
        match self {
            Sort::Updated => ["Newest first", "Oldest first"],
            Sort::Title => ["A to Z", "Z to A"],
            Sort::Score => ["Highest first", "Lowest first"],
            Sort::Progress => ["Most watched first", "Least watched first"],
        }
    }

    /// The `config`-free key used by the menu's items.
    pub fn slug(self) -> &'static str {
        match self {
            Sort::Updated => "updated",
            Sort::Title => "title",
            Sort::Score => "score",
            Sort::Progress => "progress",
        }
    }
}

/// Where the sign-in stands. Decides what the list pages show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    /// No Client ID in `config.toml`: sign-in is off and the page says how to set it.
    NoClientId,
    SignedOut,
    /// The browser is open and we wait for the redirect.
    SigningIn,
    SignedIn,
    /// The token was refused: the saved list shows, with a Sign in button.
    Expired,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// The service's media id (fake here). Picks the cover tile's colours.
    pub id: u32,
    /// The title shown, in the language of Settings → Titles (derived once, see [`Entry::retitle`]).
    pub title: String,
    /// The service's main title (romaji on MyAnimeList), and the other two.
    pub main_title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    /// Every title, lower case, one per line (the search filter looks here).
    pub search: String,
    pub status: Status,
    pub progress: u32,
    /// `None`: unknown (still airing with no count).
    pub episodes: Option<u32>,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
    pub kind: &'static str,
    /// Unix seconds of the last change (the Updated column shows its age).
    pub updated_at: i64,
    pub air: Air,
    /// "Ep 7 on Sat" while airing (needs broadcast data; fake only for now).
    pub airing_note: Option<&'static str>,
    /// A released episode past the progress.
    pub new_episode: Option<u32>,
    /// Watching it again: the status stays Completed (MAL `is_rewatching`).
    pub rewatching: bool,
    /// Finished rewatches (MAL `num_times_rewatched`).
    pub times_rewatched: u32,
    /// A change for this entry waits in the queue.
    pub waiting: Option<&'static str>,
    /// The service's cover image (`None` in the fake list, and for shows without one).
    pub cover_url: Option<String>,
    /// When its episodes air, estimated (start date and weekly slot); `None` when unknown.
    pub timetable: Option<Timetable>,
}

/// MAL's `media_type` as the Type column writes it.
pub fn kind_label(format: &str) -> &'static str {
    match format {
        "tv" => "TV",
        "movie" => "Movie",
        "ova" => "OVA",
        "ona" => "ONA",
        "special" => "Special",
        "tv_special" => "TV special",
        "music" => "Music",
        "cm" => "CM",
        "pv" => "PV",
        _ => "",
    }
}

impl Entry {
    pub fn from_item(it: &ListItem, lang: TitleLang) -> Entry {
        let m = &it.media;
        let search = m.all_titles().map(str::to_lowercase).collect::<Vec<_>>().join("\n");
        Entry {
            id: m.id,
            title: m.display_title(lang).to_string(),
            main_title: m.title.clone(),
            title_en: m.title_en.clone(),
            title_ja: m.title_ja.clone(),
            search,
            status: it.status,
            progress: it.progress,
            episodes: m.episodes,
            score: it.score,
            kind: kind_label(&m.format),
            updated_at: it.updated_at,
            air: match m.air_status.as_str() {
                "currently_airing" => Air::Airing,
                "finished_airing" => Air::Finished,
                "not_yet_aired" => Air::Upcoming,
                _ => Air::Unknown,
            },
            airing_note: None,
            new_episode: None,
            rewatching: it.rewatching,
            times_rewatched: it.times_rewatched,
            waiting: None,
            cover_url: m.cover_url.clone(),
            timetable: Timetable::of_media(m),
        }
    }

    /// Shows the title in `lang` (romaji when that one is missing). Only on a change of the
    /// setting or of the data, never per frame.
    pub fn retitle(&mut self, lang: TitleLang) {
        let t = lang.pick(&self.main_title, self.title_en.as_deref(), self.title_ja.as_deref());
        if t != self.title {
            self.title = t.to_string();
        }
    }

    /// Whether the entry shows on the status page `st`: its own status, and the Watching page
    /// also shows rewatches (Completed on MAL), as Taiga's Currently Watching list does.
    pub fn on_page(&self, st: Status) -> bool {
        self.status == st || (st == Status::Watching && self.rewatching)
    }

    /// The grey line after the tags.
    pub fn note(&self) -> &'static str {
        // The tag already says "Waiting to sync"; the note says more only for a conflict or a failure.
        self.waiting.filter(|w| *w != waiting_label(OpState::Queued)).or(self.airing_note).unwrap_or(match self.air {
            Air::Finished => "Finished airing",
            Air::Upcoming => "Not yet aired",
            _ => "",
        })
    }

    pub fn values(&self) -> Values {
        Values { status: self.status, progress: self.progress, score: self.score, rewatching: self.rewatching, times_rewatched: self.times_rewatched }
    }

    /// The entry as `kot` prints it.
    pub fn ctl(&self) -> kotora_ctl::Entry {
        kotora_ctl::Entry {
            id: self.id,
            title: self.title.clone(),
            status: self.status.label().into(),
            progress: self.progress,
            episodes: self.episodes,
            score: self.score,
            waiting: self.waiting.map(Into::into),
            rewatching: self.rewatching,
            times_rewatched: self.times_rewatched,
        }
    }

    pub fn fraction(&self) -> f32 {
        match self.episodes {
            Some(n) if n > 0 => (self.progress as f32 / n as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// An edit held for "Mark anyway": its episode hasn't aired yet by the estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub id: u32,
    pub change: Change,
    /// "Episode 9 of Lanterns of Kirigaoka hasn't aired yet (expected Sat Nov 21, 15:30)."
    pub question: String,
    /// From the inline editor: it closes once the edit is made.
    pub editor: bool,
}

/// The inline editor's draft: nothing is queued until Save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Editor {
    pub id: u32,
    pub status: Status,
    pub progress: u32,
    pub score: Option<u8>,
}

/// What the queue's "waiting" tag says for an entry with an open change.
pub fn waiting_label(state: OpState) -> &'static str {
    match state {
        OpState::Conflict => "Conflict in queue",
        OpState::Failed => "Sync failed",
        _ => "Waiting to sync",
    }
}

/// What a worker tells the UI thread (`online.rs` and `queue.rs` send these through
/// `AppHandle::update_if`).
#[derive(Debug)]
pub enum Msg {
    /// The saved list and the queue from the database, at start.
    Cache { items: Vec<ListItem>, user: Option<String>, picture: Option<String>, profile: Option<Box<kotora_core::service::Profile>>, synced_at: Option<i64>, ops: Vec<Op> },
    /// The Profile page's facts, fresh after a sync.
    Profile(Box<kotora_core::service::Profile>),
    /// The queue changed: all of it, the entries it touched, and those that left the list.
    /// `offline`: a send failed for lack of network (true) or went through (false).
    Queue { ops: Vec<Op>, items: Vec<ListItem>, gone: Vec<u32>, offline: Option<bool> },
    /// The queue couldn't do something (database trouble, an action on a change that's gone).
    QueueError(String),
    /// A fresh list from the service, already saved.
    Synced { items: Vec<ListItem>, synced_at: i64 },
    /// The sync failed; the saved list stays.
    SyncFailed(Fail),
    /// Nothing to sync (signed out, no Client ID, `KOTORA_OFFLINE`): stop the loading state.
    Idle,
    SignedIn { user: String, picture: Option<String> },
    /// The profile picture was asked for again (at most daily, after a sync).
    Picture(Option<String>),
    SignInFailed(String),
    /// The sign-in page's address, and whether the browser was started with it.
    SignInUrl { url: String, opened: bool },
    SignedOut,
    /// The followed player and what recognition made of its file (`None`: nothing plays).
    Now(Option<Box<Seen>>),
    /// A file passed the threshold.
    Reached { key: String, file: String },
    /// The players' states.
    Players(Vec<SourceStatus>),
    /// The Client ID was written (its last four characters and the sign-in state now), or not.
    ClientIdSaved(Result<(String, Auth), String>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    Offline(String),
    SignInAgain,
    Other(String),
}

pub struct State {
    pub page: Page,
    pub filter: Filter,
    pub sort: Sort,
    /// The sort runs the other way ("Oldest first", "Z to A").
    pub sort_rev: bool,
    /// The Sort menu is open.
    pub sort_menu: bool,
    /// The list page's search box: a quick filter of the list.
    pub query: TextEdit,
    /// The search page's field: the list and MyAnimeList.
    pub find: TextEdit,
    /// The page the search page returns to (Escape).
    pub search_back: Page,
    pub auth: Auth,
    /// The signed-in user name and service; `None` when signed out.
    pub account: Option<(String, &'static str)>,
    /// The account's profile picture URL; the sidebar draws it round, else the initial.
    pub avatar: Option<String>,
    /// The Profile page's facts from the service (join date, anime statistics), when known.
    pub profile: Option<kotora_core::service::Profile>,
    /// While signing in: MyAnimeList's sign-in page, and whether the browser opened it.
    pub sign_in_link: Option<(String, bool)>,
    /// The setup wizard's step (`setup.rs`).
    pub setup: crate::setup::Step,
    /// The page the wizard returns to when closed (Settings for "Set up again").
    pub setup_back: Page,
    pub entries: Vec<Entry>,
    /// Indexes into `entries` for the list page: its status, the chip, the search, the sort.
    /// Rebuilt by `refresh()`; the view only reads it.
    pub rows: Vec<usize>,
    /// The first sync is running and nothing is saved yet: skeleton rows.
    pub loading: bool,
    /// A sync is running.
    pub syncing: bool,
    /// The last sync's error, in plain words (red alert on the list).
    pub error: Option<String>,
    /// Open changes in the queue (the badge): waiting, failed and conflicted.
    pub queue: u32,
    /// The sync queue: open changes, then those sent in the last day.
    pub ops: Vec<Op>,
    /// The row whose inline editor is open, and its draft.
    pub editor: Option<Editor>,
    /// "Mark anyway?" for an episode that hasn't aired yet (`+1`, the editor).
    pub ask: Option<Ask>,
    /// The queue's worker thread (`queue.rs`); runs only while there is work.
    pub worker: Arc<Worker>,
    pub offline: bool,
    /// Unix seconds of the last full sync.
    pub synced_at: Option<i64>,
    /// Scenes pin the clock; the real app reads it when it draws.
    pub fixed_now: Option<i64>,
    /// Now Playing (`now.rs`): the followed player, its file and the offer.
    pub now: Option<Now>,
    /// The players' states (Settings → Players, the Now Playing page).
    pub players: Vec<PlayerState>,
    /// The one-time "precise detection" offer for mpv (`precise.rs`).
    pub precise: crate::precise::Offer,
    /// IINA's precise detection hint (`iina.rs`, macOS).
    #[cfg(target_os = "macos")]
    pub iina: crate::iina::Iina,
    /// Settings → Updates and Players.
    pub prefs: Prefs,
    /// The detection threads (`detect.rs`); `None` headless.
    pub glue: Option<crate::detect::Glue>,
    /// The window is open (false after a `--hidden` start until `kot show`).
    pub window: bool,
    /// Set while a sign-in waits for the browser; setting it ends the wait.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The C2 icon at sidebar size, set in `setup`.
    pub logo: Option<Image>,
    /// The same icon at 56 pt, for the About card.
    pub logo_big: Option<Image>,
    /// Covers in the list, and Rune's image cache (`art.rs`).
    pub covers: Covers,
    /// The open details page.
    pub detail: Option<crate::details::Detail>,
    /// The MyAnimeList half of the search (`search.rs`).
    pub online: crate::search::Online,
    /// The Airing page (`airing.rs`).
    pub airing: crate::airing::Airing,
    /// The MyAnimeList Client ID field (Welcome and Settings).
    pub client_id: crate::clientid::ClientIdForm,
    pub handle: Option<Handle>,
    /// Desktop notifications (`notes/`); no backend headless and in tests.
    pub notes: crate::notes::Notifier,
    /// The tray or menu bar icon (`tray.rs`); `None` headless, in tests and where it failed.
    pub tray: Option<crate::tray::TrayCtl>,
    /// What the last position tick showed (`now::tick`); `None` while nothing ticks.
    pub tick: Option<(bool, u64)>,
    /// Library folders and "Play next episode" (`library.rs`).
    pub library: crate::library::Library,
    /// The list's grid views: the keyboard selection (`grid.rs`).
    pub grid: crate::grid::Grid,
    /// The window's size at the last build: a change is a resize, whose reflow lands at once
    /// (`ui::view`).
    pub main_size: std::cell::Cell<[f32; 2]>,
}

/// The clock of the fake state (scenes and tests): a fixed moment, so snaps never change.
pub const FAKE_NOW: i64 = 1_800_000_000;

impl State {
    /// The first state of the headless scenes: the fake list.
    pub fn fake() -> Self {
        let mut s = State {
            page: Page::List(Status::Watching),
            filter: Filter::All,
            sort: Sort::Updated,
            sort_rev: false,
            sort_menu: false,
            query: TextEdit::new(""),
            find: TextEdit::new(""),
            search_back: Page::List(Status::Watching),
            auth: Auth::SignedIn,
            account: Some(("demo_user".into(), "MyAnimeList")),
            avatar: None,
            profile: Some(crate::profile::fake()),
            sign_in_link: None,
            setup: crate::setup::Step::Welcome,
            setup_back: Page::List(Status::Watching),
            entries: fake_entries(),
            rows: Vec::new(),
            loading: false,
            syncing: false,
            error: None,
            queue: 0,
            ops: fake_ops(),
            editor: None,
            ask: None,
            worker: Arc::default(),
            offline: false,
            synced_at: Some(FAKE_NOW - 120),
            fixed_now: Some(FAKE_NOW),
            now: Some(fake_now()),
            players: fake_players(),
            precise: Default::default(),
            #[cfg(target_os = "macos")]
            iina: Default::default(),
            prefs: Prefs::default(),
            glue: None,
            window: true,
            cancel: None,
            logo: None,
            logo_big: None,
            covers: Covers::default(),
            detail: None,
            online: Default::default(),
            airing: Default::default(),
            client_id: crate::clientid::ClientIdForm { hint: Some("8f3a".into()), ..Default::default() },
            handle: None,
            notes: Default::default(),
            tray: None,
            tick: None,
            library: Default::default(),
            grid: Default::default(),
            main_size: Default::default(),
        };
        s.queue_changed();
        s.refresh();
        s
    }

    /// The first state of the real app: nothing loaded yet. `online::start` fills it in.
    pub fn start(auth: Auth) -> Self {
        let mut s = State::fake();
        s.entries.clear();
        s.auth = auth;
        s.account = None;
        s.profile = None;
        s.queue = 0;
        s.ops.clear();
        s.synced_at = None;
        s.fixed_now = None;
        s.now = None;
        s.players.clear();
        s.loading = matches!(auth, Auth::SignedIn);
        s.handle = None;
        s.client_id.hint = None;
        // First run: the setup wizard.
        if auth == Auth::NoClientId {
            s.page = Page::Setup;
        }
        s.refresh();
        s
    }

    pub fn clock(&self) -> i64 {
        self.fixed_now.unwrap_or_else(kotora_core::time::now_unix)
    }

    /// Entries with this status on MAL (the sidebar's count).
    pub fn count(&self, s: Status) -> usize {
        self.entries.iter().filter(|e| e.status == s).count()
    }

    /// Completed entries being rewatched (the Watching page shows them too).
    pub fn rewatch_count(&self) -> usize {
        self.entries.iter().filter(|e| e.rewatching && e.status != Status::Watching).count()
    }

    /// Whether the list pages show a list (rather than the sign-in screen).
    pub fn has_list(&self) -> bool {
        matches!(self.auth, Auth::SignedIn | Auth::Expired)
    }

    /// What the page's search box holds: the search page's field, else the list's filter.
    pub fn search_text(&self) -> &str {
        if self.page == Page::Search { &self.find.text } else { &self.query.text }
    }

    /// Whether a search box holds a query: the list page shows the matches of the whole list
    /// instead of one status, the search page its results.
    pub fn searching(&self) -> bool {
        matches!(self.page, Page::List(_) | Page::Search) && !self.search_text().trim().is_empty()
    }

    /// Rebuilds `rows`: the page's status, then the chip, the search and the sort. Call it after
    /// any change to the page, filter, query, sort or entries. A search looks at every status
    /// (the page's own first) and ignores the chip.
    pub fn refresh(&mut self) {
        // The search page has no status of its own: the sidebar's order.
        let st = match self.page {
            Page::List(st) => st,
            Page::Search if self.searching() => Status::ALL[0],
            _ => {
                self.rows.clear();
                return;
            }
        };
        let q = self.search_text().trim().to_lowercase();
        let mut rows: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                if q.is_empty() { e.on_page(st) && self.filter.keeps(e) } else { e.search.contains(&q) }
            })
            .collect();
        let e = &self.entries;
        match self.sort {
            // Newest first; the sort is stable, so equal times keep the list's order.
            Sort::Updated => rows.sort_by_key(|&i| std::cmp::Reverse(e[i].updated_at)),
            Sort::Title => rows.sort_by_cached_key(|&i| e[i].title.to_lowercase()),
            Sort::Score => rows.sort_by_key(|&i| std::cmp::Reverse(e[i].score.unwrap_or(0))),
            Sort::Progress => rows.sort_by_key(|&i| std::cmp::Reverse(e[i].progress)),
        }
        if self.sort_rev {
            rows.reverse();
        }
        if !q.is_empty() {
            // The page's status first, then the sidebar's order (stable: the sort stays within).
            rows.sort_by_key(|&i| if e[i].status == st { 0 } else { 1 + Status::ALL.iter().position(|x| *x == e[i].status).unwrap_or(0) });
        }
        self.rows = rows;
    }

    pub fn go(&mut self, page: Page) {
        self.sort_menu = false;
        if page == Page::Settings && self.page != page {
            self.refresh_login();
        }
        if self.page != page {
            self.page = page;
            self.filter = Filter::All;
            self.refresh();
        }
    }

    pub fn set_filter(&mut self, f: Filter) {
        self.filter = f;
        self.refresh();
    }

    /// A new order starts in its own direction.
    pub fn set_sort(&mut self, sort: Sort) {
        if self.sort != sort {
            self.sort_rev = false;
        }
        self.sort = sort;
        self.refresh();
    }

    pub fn set_sort_rev(&mut self, rev: bool) {
        self.sort_rev = rev;
        self.refresh();
    }

    /// An edit (the editor, `+1`, `kot`): the list shows it at once and the queue sends it.
    /// Returns the entry's index.
    pub fn edit(&mut self, id: u32, mut change: Change) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let now = self.clock();
        let i = self.entries.iter().position(|e| e.id == id).ok_or("That anime is not on your list.")?;
        let e = &mut self.entries[i];
        if let (Some(p), Some(n)) = (change.progress, e.episodes)
            && p > n
        {
            return Err(format!("{} has {n} episode{}.", e.title, if n == 1 { "" } else { "s" }));
        }
        if let Some(Some(sc)) = change.score
            && !(1..=10).contains(&sc)
        {
            return Err("A score goes from 1 to 10.".into());
        }
        // Like the MAL website: the last episode completes the entry, or ends a rewatch (one more
        // in the count), when the total is known and the change doesn't pick it itself.
        change = change.completes(&e.values(), e.episodes);
        let after = change.apply(e.values());
        if after == e.values() {
            return Ok(i);
        }
        e.status = after.status;
        e.progress = after.progress;
        e.score = after.score;
        e.rewatching = after.rewatching;
        e.times_rewatched = after.times_rewatched;
        e.updated_at = now;
        if e.new_episode.is_some_and(|n| e.progress >= n) {
            e.new_episode = None;
        }
        // Back to where the entry's only waiting change started: the queue drops it.
        let open: Vec<&Op> = self.ops.iter().filter(|o| o.media_id == id && o.state.is_open()).collect();
        let e = &mut self.entries[i];
        if let [op] = open.as_slice()
            && op.state != OpState::Sending
            && op.old == after
        {
            e.waiting = None;
        } else if e.waiting.is_none() {
            e.waiting = Some(waiting_label(OpState::Queued));
        }
        crate::queue::send(self, Cmd::Edit { media_id: id, change });
        self.recheck_now();
        self.refresh();
        Ok(i)
    }

    /// "Rewatch" (the row editor, the details page, `kot rewatch`): a completed anime is watched
    /// again from episode 0; it stays Completed on MAL, and the last episode ends the rewatch.
    pub fn start_rewatch(&mut self, id: u32) -> Result<usize, String> {
        let e = self.entries.iter().find(|e| e.id == id).ok_or("That anime is not on your list.")?;
        if e.rewatching {
            return Err(format!("{} is being rewatched already.", e.title));
        }
        let change = Change::start_rewatch(&e.values()).ok_or_else(|| format!("{} is not completed: only a completed anime can be rewatched.", e.title))?;
        self.editor = None;
        self.edit(id, change)
    }

    /// "Stop rewatching": the rewatch ends without counting; the progress goes back to the last
    /// episode (the entry is still Completed).
    pub fn stop_rewatch(&mut self, id: u32) -> Result<usize, String> {
        let e = self.entries.iter().find(|e| e.id == id).ok_or("That anime is not on your list.")?;
        if !e.rewatching {
            return Err(format!("{} is not being rewatched.", e.title));
        }
        let progress = e.episodes.unwrap_or(e.progress);
        self.editor = None;
        self.edit(id, Change { rewatching: Some(false), progress: Some(progress), ..Default::default() })
    }

    /// Search and add, the details page, Now Playing: puts an anime on the list with `status`
    /// (the list shows it at once, waiting to sync) and queues the add. One already on the list
    /// gets the status instead. Returns the entry's index.
    pub fn add_media(&mut self, media: Media, status: Status) -> Result<usize, String> {
        self.add_media_at(media, status, None)
    }

    /// [`State::add_media`] at an episode: added and recorded in one change ("Add as Watching at
    /// episode N").
    pub fn add_media_at(&mut self, media: Media, status: Status, episode: Option<u32>) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        if self.entries.iter().any(|e| e.id == media.id) {
            return self.edit(media.id, Change { status: Some(status), progress: episode, ..Default::default() });
        }
        let progress = match (status, media.episodes, episode) {
            (Status::Completed, Some(n), _) => n,
            (_, Some(n), Some(p)) => p.min(n),
            (_, None, Some(p)) => p,
            _ => 0,
        };
        let item = ListItem { media: media.clone(), status, progress, score: None, rewatching: false, times_rewatched: 0, updated_at: self.clock() };
        let mut e = Entry::from_item(&item, self.prefs.titles);
        e.waiting = Some(waiting_label(OpState::Queued));
        self.entries.push(e);
        crate::queue::send(self, Cmd::Add { media: Box::new(media), status, progress: episode });
        self.refresh();
        Ok(self.entries.len() - 1)
    }

    /// The page whose sidebar item is lit (the details page lights the page it came from).
    pub fn lit_page(&self) -> Page {
        match (&self.page, &self.detail) {
            (Page::Details, Some(d)) => d.back,
            (p, _) => *p,
        }
    }

    /// `+1`: optimistic, at once (design-spec §6).
    pub fn plus_one(&mut self, id: u32) -> Result<usize, String> {
        let change = self.plus_one_change(id)?;
        self.edit(id, change)
    }

    fn plus_one_change(&self, id: u32) -> Result<Change, String> {
        let e = self.entries.iter().find(|e| e.id == id).ok_or("That anime is not on your list.")?;
        if e.episodes.is_some_and(|n| e.progress >= n) {
            return Err(format!("{} is at its last episode.", e.title));
        }
        Ok(Change { progress: Some(e.progress + 1), ..Default::default() })
    }

    /// The row's `+1`: an episode that hasn't aired yet by the estimate asks first ("Mark
    /// anyway?"); anything else is [`State::plus_one`].
    pub fn plus_one_or_ask(&mut self, id: u32) -> Result<(), String> {
        let change = self.plus_one_change(id)?;
        self.edit_or_ask(id, change, false)
    }

    /// The episode a change marks, when it hasn't aired yet by the estimate (and is not past the
    /// count: that is refused by [`State::edit`]).
    pub fn not_aired(&self, id: u32, change: &Change) -> Option<NotAired> {
        let e = self.entries.iter().find(|e| e.id == id)?;
        let p = change.progress?;
        if e.episodes.is_some_and(|n| p > n) {
            return None;
        }
        kotora_core::airing::not_aired(e.air.mal_name(), e.timetable, e.progress, p, self.clock())
    }

    /// "Episode 9 of Lanterns of Kirigaoka hasn't aired yet (expected Sat Nov 21, 15:30)." for a
    /// change that marks an episode not aired yet; `None` for any other change.
    pub fn not_aired_text(&self, id: u32, change: &Change) -> Option<String> {
        let na = self.not_aired(id, change)?;
        let title = &self.entries.iter().find(|e| e.id == id)?.title;
        Some(format!("Episode {} of {title} hasn't aired yet{}.", na.episode, self.expected_text(na.expected)))
    }

    /// " (expected Sat Nov 21, 15:30)" in the user's zone, or nothing.
    pub fn expected_text(&self, at: Option<i64>) -> String {
        match at {
            Some(t) => {
                const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                let c = self.airing.clock(t);
                let day = kotora_core::airing::DAY_SHORT[usize::from(c.weekday % 7)];
                format!(" (expected {day} {} {}, {})", MONTHS[(c.month as usize).clamp(1, 12) - 1], c.date, c.hhmm())
            }
            None => String::new(),
        }
    }

    /// An edit by hand that may mark an episode not aired yet: held in [`State::ask`] until
    /// "Mark anyway" ([`State::mark_anyway`]) or Cancel. Other edits go through at once.
    pub fn edit_or_ask(&mut self, id: u32, change: Change, editor: bool) -> Result<(), String> {
        match self.not_aired_text(id, &change) {
            Some(question) => {
                self.ask = Some(Ask { id, change, question, editor });
                Ok(())
            }
            None => {
                self.ask = None;
                self.edit(id, change)?;
                if editor {
                    self.editor = None;
                }
                Ok(())
            }
        }
    }

    /// "Mark anyway": the held edit goes through.
    pub fn mark_anyway(&mut self) -> Result<(), String> {
        let Some(a) = self.ask.take() else { return Ok(()) };
        self.edit(a.id, a.change)?;
        if a.editor {
            self.editor = None;
        }
        Ok(())
    }

    // ------------------------------------------------------------------------- inline editor

    /// A row press: opens its editor, or closes it when it is open.
    pub fn toggle_editor(&mut self, id: u32) {
        if self.editor.is_some_and(|ed| ed.id == id) {
            self.editor = None;
        } else if let Some(e) = self.entries.iter().find(|e| e.id == id) {
            self.editor = Some(Editor { id, status: e.status, progress: e.progress, score: e.score });
        }
    }

    fn editor_episodes(&self) -> Option<u32> {
        let id = self.editor?.id;
        self.entries.iter().find(|e| e.id == id)?.episodes
    }

    pub fn editor_step(&mut self, delta: i32) {
        let max = self.editor_episodes();
        let was = self.editor.and_then(|ed| self.entries.iter().find(|e| e.id == ed.id)).map(|e| e.status);
        if let Some(ed) = &mut self.editor {
            let p = ed.progress.saturating_add_signed(delta);
            ed.progress = max.map_or(p, |n| p.min(n));
            // The last episode completes the entry, as on the MAL website.
            if delta > 0 && max.is_some_and(|n| n > 0 && ed.progress == n) {
                ed.status = Status::Completed;
            } else if delta < 0 && ed.status == Status::Completed && max.is_some_and(|n| ed.progress < n) && was.is_some_and(|w| w != Status::Completed) {
                // Stepping back off the last episode undoes that.
                ed.status = was.unwrap_or(ed.status);
            }
        }
    }

    /// Completed fills the progress when the count is known.
    pub fn editor_status(&mut self, st: Status) {
        let max = self.editor_episodes();
        if let Some(ed) = &mut self.editor {
            ed.status = st;
            if st == Status::Completed
                && let Some(n) = max
            {
                ed.progress = n;
            }
        }
    }

    pub fn editor_score(&mut self, score: Option<u8>) {
        if let Some(ed) = &mut self.editor {
            ed.score = score;
        }
    }

    /// Save: the changed fields become one edit.
    pub fn save_editor(&mut self) -> Result<(), String> {
        let Some(ed) = self.editor else { return Ok(()) };
        let Some(e) = self.entries.iter().find(|e| e.id == ed.id) else {
            self.editor = None;
            return Ok(());
        };
        let change = Change { status: Some(ed.status), progress: Some(ed.progress), score: Some(ed.score), ..Default::default() }.without_same(&e.values());
        self.edit_or_ask(ed.id, change, true)
    }

    // --------------------------------------------------------------------------------- queue

    /// After `ops` or `entries` change: the badge and each row's waiting tag.
    fn queue_changed(&mut self) {
        self.queue = self.ops.iter().filter(|o| o.state.is_open()).count() as u32;
        for e in &mut self.entries {
            e.waiting = None;
        }
        for op in self.ops.iter().filter(|o| o.state.is_open()) {
            if let Some(e) = self.entries.iter_mut().find(|e| e.id == op.media_id) {
                // A conflict or a failure says more than "waiting".
                if e.waiting.is_none() || op.state != OpState::Queued {
                    e.waiting = Some(waiting_label(op.state));
                }
            }
        }
    }

    pub fn queue_cmd(&mut self, cmd: Cmd) {
        crate::queue::send(self, cmd);
    }

    /// "Sync now", "Try now", `kot sync`: send what waits, then read the list again.
    pub fn sync_now(&mut self) {
        self.error = None;
        if self.queue > 0 {
            self.queue_cmd(Cmd::Kick { refresh: true });
        } else {
            crate::online::refresh(self);
        }
    }

    /// Settings → Titles: every title shown follows at once (`kot` too, as it asks the app), and
    /// `config.toml` keeps it. Matching still uses every title.
    pub fn set_titles(&mut self, lang: TitleLang) {
        if self.prefs.titles == lang {
            return;
        }
        self.prefs.titles = lang;
        for e in &mut self.entries {
            e.retitle(lang);
        }
        self.refresh();
        crate::detect::save_setting(self, "display", "titles", kotora_core::config::quote(lang.key()));
    }

    /// Settings → Appearance: the next frame paints in the new palette (`ui::view`), and
    /// `config.toml` keeps it.
    pub fn set_theme(&mut self, theme: kotora_core::config::Theme) {
        if self.prefs.theme == theme {
            return;
        }
        self.prefs.theme = theme;
        crate::detect::save_setting(self, "display", "theme", kotora_core::config::quote(theme.key()));
    }

    /// The title a queued change shows: the entry's, in the chosen language, else the saved one.
    pub fn op_title<'a>(&'a self, op: &'a Op) -> &'a str {
        self.entries.iter().find(|e| e.id == op.media_id).map_or(op.title.as_str(), |e| e.title.as_str())
    }

    /// The queue as `kot queue` prints it.
    pub fn ctl_queue(&self) -> Vec<kotora_ctl::QueueItem> {
        let service = "MyAnimeList";
        self.ops
            .iter()
            .map(|o| kotora_ctl::QueueItem {
                title: self.op_title(o).to_string(),
                change: kotora_core::queue::describe_op(o),
                state: o.state.label().into(),
                note: match o.state {
                    OpState::Conflict => Some(match &o.remote {
                        Some(r) => format!("{service} has {}", remote_text(r, o.episodes)),
                        None => format!("removed from your list on {service}"),
                    }),
                    OpState::Done => None,
                    _ => o.error.clone(),
                },
            })
            .collect()
    }

    /// The entry `q` names: a whole title first, then a part of one that matches one entry.
    pub fn find(&self, q: &str) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Err("Which anime? Give a part of its title.".into());
        }
        let whole: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].search.lines().any(|t| t == q)).collect();
        if whole.len() == 1 {
            return Ok(whole[0]);
        }
        let part: Vec<usize> = if whole.is_empty() { (0..self.entries.len()).filter(|&i| self.entries[i].search.contains(&q)).collect() } else { whole };
        match part.as_slice() {
            [] => Err(format!("No anime on your list matches \u{201c}{q}\u{201d}.")),
            [i] => Ok(*i),
            many => {
                let names: Vec<&str> = many.iter().take(5).map(|&i| self.entries[i].title.as_str()).collect();
                let more = if many.len() > 5 { format!(" and {} more", many.len() - 5) } else { String::new() };
                Err(format!("{} anime match \u{201c}{q}\u{201d}: {}{more}. Give more of the title.", many.len(), names.join(", ")))
            }
        }
    }

    fn set_items(&mut self, items: &[ListItem]) {
        let lang = self.prefs.titles;
        self.entries = items.iter().map(|it| Entry::from_item(it, lang)).collect();
        self.queue_changed();
        self.recheck_now();
        self.refresh();
    }

    /// Applies a worker's message. Returns whether anything visible changed.
    /// A worker's message. The tray follows what plays after each one.
    pub fn apply(&mut self, msg: Msg) -> bool {
        match &msg {
            Msg::Synced { .. } => crate::library::list_changed(self),
            Msg::Now(Some(seen)) => crate::library::saw_player(self, &seen.track.playing.player),
            _ => {}
        }
        let changed = self.apply_msg(msg);
        self.sync_tray();
        changed
    }

    fn apply_msg(&mut self, msg: Msg) -> bool {
        let service = "MyAnimeList";
        match msg {
            Msg::Cache { items, user, picture, profile, synced_at, ops } => {
                self.ops = ops;
                // A sync's answer is newer than the saved one.
                if self.profile.is_none() {
                    self.profile = profile.map(|p| *p);
                }
                // A sync that already finished is newer than the cache read.
                if self.synced_at.is_none() || self.entries.is_empty() {
                    self.set_items(&items);
                    self.synced_at = self.synced_at.or(synced_at);
                } else {
                    self.queue_changed();
                }
                // Changes left from the last run go out now.
                if self.auth == Auth::SignedIn && self.ops.iter().any(|o| o.state == OpState::Queued) {
                    self.queue_cmd(Cmd::Kick { refresh: false });
                }
                if let Some(u) = user {
                    self.account = Some((u, service));
                    self.avatar = self.avatar.take().or(picture);
                }
                self.loading = self.loading && items.is_empty() && self.syncing;
                true
            }
            Msg::Synced { items, synced_at } => {
                self.set_items(&items);
                self.synced_at = Some(synced_at);
                self.syncing = false;
                self.loading = false;
                self.offline = false;
                self.error = None;
                if self.auth == Auth::Expired {
                    self.auth = Auth::SignedIn;
                }
                true
            }
            Msg::SyncFailed(f) => {
                self.syncing = false;
                self.loading = false;
                match f {
                    Fail::Offline(m) => {
                        self.offline = true;
                        self.error = Some(m);
                    }
                    Fail::SignInAgain => {
                        self.auth = Auth::Expired;
                        self.worker.hold.store(true, Ordering::Relaxed);
                    }
                    Fail::Other(m) => self.error = Some(m),
                }
                true
            }
            Msg::Queue { ops, items, gone, offline } => {
                for it in &items {
                    let e = Entry::from_item(it, self.prefs.titles);
                    match self.entries.iter_mut().find(|x| x.id == e.id) {
                        Some(x) => *x = e,
                        None => self.entries.push(e),
                    }
                }
                self.entries.retain(|e| !gone.contains(&e.id));
                if self.editor.is_some_and(|ed| gone.contains(&ed.id)) {
                    self.editor = None;
                }
                self.ops = ops;
                self.recheck_now();
                if let Some(off) = offline {
                    self.offline = off;
                    if !off && self.error.as_deref().is_some_and(|e| e.starts_with("Can't reach")) {
                        self.error = None;
                    }
                }
                self.queue_changed();
                self.refresh();
                true
            }
            Msg::QueueError(m) => {
                self.error = Some(m);
                true
            }
            Msg::Idle => {
                self.syncing = false;
                self.loading = false;
                true
            }
            Msg::SignedIn { user, picture } => {
                self.auth = Auth::SignedIn;
                if self.setup == crate::setup::Step::SignIn {
                    self.setup = crate::setup::Step::Done;
                }
                self.account = Some((user, service));
                self.avatar = picture;
                self.cancel = None;
                self.sign_in_link = None;
                self.error = None;
                // The queue waited for this.
                self.worker.hold.store(false, Ordering::Relaxed);
                if self.queue > 0 {
                    self.queue_cmd(Cmd::Kick { refresh: false });
                }
                true
            }
            Msg::SignInUrl { url, opened } => {
                if self.auth != Auth::SigningIn {
                    return false;
                }
                self.sign_in_link = Some((url, opened));
                true
            }
            Msg::SignInFailed(m) => {
                // Keep the saved list when the user was only signing in again.
                self.auth = if self.entries.is_empty() { Auth::SignedOut } else { Auth::Expired };
                self.cancel = None;
                self.sign_in_link = None;
                self.error = Some(m);
                true
            }
            Msg::Now(seen) => self.now_seen(seen.map(|b| *b)),
            Msg::Reached { key, file } => self.now_reached(&key, &file),
            Msg::Players(v) => self.players_seen(v),
            Msg::ClientIdSaved(r) => self.client_id_saved(r),
            Msg::Profile(p) => {
                // A late answer after sign out is dropped.
                let changed = self.account.is_some() && self.profile.as_ref() != Some(&*p);
                if changed {
                    self.profile = Some(*p);
                }
                changed
            }
            Msg::Picture(url) => {
                let changed = self.account.is_some() && self.avatar != url;
                if changed {
                    self.covers.forget(crate::ui::AVATAR_ID);
                    self.avatar = url;
                }
                changed
            }
            Msg::SignedOut => {
                self.covers.clear();
                // MyAnimeList's answers go with the account (search results, the open page,
                // the seasons).
                self.online = Default::default();
                self.airing = Default::default();
                if self.page == Page::Details {
                    self.page = self.lit_page();
                }
                self.detail = None;
                self.auth = Auth::SignedOut;
                self.account = None;
                self.avatar = None;
                self.profile = None;
                self.entries.clear();
                self.ops.clear();
                self.queue = 0;
                self.editor = None;
                self.synced_at = None;
                self.syncing = false;
                self.loading = false;
                self.offline = false;
                self.error = None;
                self.refresh();
                true
            }
        }
    }

    pub fn ctl_status(&self) -> kotora_ctl::Status {
        kotora_ctl::Status {
            version: kotora_ctl::version_line(),
            account: self.account.as_ref().map(|(n, _)| n.clone()),
            service: self.account.as_ref().map(|(_, s)| s.to_string()).unwrap_or_default(),
            entries: self.entries.len() as u32,
            queue: self.queue,
            offline: self.offline,
            window: self.window,
        }
    }
}

/// The fake player: Lanterns of Kirigaoka episode 6 in mpv, 68 % in, a sure match.
pub fn fake_now() -> Now {
    let file = "[Hoshi] Lanterns of Kirigaoka - 06 (1080p).mkv";
    Now {
        key: "mpv:/tmp/mpv.sock".into(),
        player: "mpv".into(),
        switch: Switch::Mpv,
        via: Via::MpvIpc.label(),
        file: file.into(),
        position: Some(968.0),
        duration: Some(1420.0),
        at: None,
        rate: 1.0,
        playing: true,
        reached: false,
        parsed: kotora_core::recognize::parse(file),
        found: Found::List { media_id: 0, sure: true, why: "same title in your list".into() },
        recognizing: false,
        candidates: vec![(0, "Lanterns of Kirigaoka".into()), (3, "Slow Life on Hoshizora Island".into()), (9, "Kumo to Kaze no Ryokan".into())],
        cover_url: None,
        relation: None,
        dismissed: false,
        acted: false,
        recorded: None,
        picking: false,
        mark: None,
        counted: false,
        announced: false,
    }
}

/// The fake players of this system (Settings → Players).
pub fn fake_players() -> Vec<PlayerState> {
    let st = |switch, via, line: &str, led| PlayerState::from(SourceStatus { switch, via, line: line.into(), led });
    if cfg!(target_os = "macos") {
        vec![st(Switch::Mpv, Via::MpvIpc, "/tmp/mpv.sock: playing", Led::Playing), st(Switch::Iina, Via::MpvIpc, "/tmp/iina.sock: not running", Led::Off)]
    } else {
        vec![
            st(Switch::Mpv, Via::MpvIpc, "$XDG_RUNTIME_DIR/mpv.sock: playing", Led::Playing),
            st(Switch::Vlc, Via::Mpris, "not running", Led::Off),
            st(Switch::MpcQt, Via::Mpris, "running, nothing playing", Led::Idle),
            st(Switch::OtherMpris, Via::Mpris, "not running", Led::Off),
        ]
    }
}

/// The service's side of a conflict: "Progress 8 / 24, Watching, score 7".
pub fn remote_text(r: &Remote, episodes: Option<u32>) -> String {
    let total = episodes.map_or("?".to_string(), |n| n.to_string());
    let score = r.values.score.map_or("no score".to_string(), |n| format!("score {n}"));
    format!("{} / {total}, {}, {score}", r.values.progress, r.values.status.label())
}

/// The mockup's queue: a conflict, two waiting, one failed, two sent. Media ids are the fake
/// list's (`WATCHING` order); the open changes match what the fake rows show.
fn fake_ops() -> Vec<Op> {
    let op = |id: i64, media_id: u32, change: Change, old: Values, state: OpState, ago: i64| Op {
        id,
        media_id,
        title: WATCHING[media_id as usize].title.into(),
        episodes: Some(WATCHING[media_id as usize].episodes),
        change,
        old,
        base_updated_at: FAKE_NOW - 90_000,
        created_at: FAKE_NOW - ago,
        edited_at: FAKE_NOW - ago,
        edits: 1,
        state,
        attempts: 0,
        next_try: 0,
        error: None,
        sent_at: (state == OpState::Done).then_some(FAKE_NOW - ago),
        remote: None,
        added: false,
    };
    let w = |progress, score| Values::new(Status::Watching, progress, score);
    let progress = |p| Change { progress: Some(p), ..Default::default() };
    vec![
        Op {
            remote: Some(Remote { values: w(8, None), updated_at: FAKE_NOW - 3_000 }),
            ..op(1, 2, progress(7), w(6, None), OpState::Conflict, 4_500)
        },
        Op { edits: 2, ..op(2, 0, progress(5), w(3, Some(8)), OpState::Queued, 900) },
        op(3, 8, Change { progress: Some(1), score: Some(Some(8)), ..Default::default() }, w(0, None), OpState::Queued, 1_900),
        Op {
            error: Some("MyAnimeList answered HTTP 400 (invalid_content: num_watched_episodes)".into()),
            ..op(4, 7, progress(11), w(10, Some(7)), OpState::Failed, 6_000)
        },
        op(5, 1, progress(8), w(7, Some(9)), OpState::Done, 7_200),
        op(6, 3, Change { score: Some(Some(7)), ..Default::default() }, w(3, Some(6)), OpState::Done, 12_600),
    ]
}

struct Fake {
    title: &'static str,
    progress: u32,
    episodes: u32,
    score: Option<u8>,
    kind: &'static str,
    updated: &'static str,
    airing: Option<&'static str>,
    new_episode: Option<u32>,
    rewatching: bool,
}

/// "2 h", "3 days", "1 week", … as seconds (the fake list's ages).
fn fake_age(label: &str) -> i64 {
    let (n, unit) = label.split_once(' ').unwrap_or((label, ""));
    let n: i64 = n.parse().unwrap_or(0);
    n * match unit {
        "h" => 3600,
        "day" | "days" => 86_400,
        "week" | "weeks" => 604_800,
        "month" | "months" => 2_592_000,
        "year" | "years" => 31_536_000,
        _ => 0,
    }
}

const fn fk(title: &'static str, progress: u32, episodes: u32, score: Option<u8>, updated: &'static str, airing: Option<&'static str>) -> Fake {
    Fake { title, progress, episodes, score, kind: "TV", updated, airing, new_episode: None, rewatching: false }
}

/// The Watching page of the mockup, in "Last updated" order, plus two more off screen.
const WATCHING: [Fake; 12] = [
    Fake { new_episode: Some(6), ..fk("Lanterns of Kirigaoka", 5, 12, Some(8), "now", Some("Ep 7 on Sat")) },
    fk("Starfall Delivery Service", 8, 13, Some(9), "2 h", Some("Ep 9 on Wed")),
    // A conflict in the fake queue (`fake_ops`).
    fk("Mikan Street Detectives", 7, 24, None, "1 day", None),
    fk("Slow Life on Hoshizora Island", 3, 12, Some(7), "2 days", Some("Ep 4 on Sun")),
    fk("Neon Ronin 2099", 22, 26, Some(8), "3 days", None),
    Fake { rewatching: true, ..fk("The Ninth Shrine Maiden", 6, 12, Some(10), "5 days", None) },
    fk("Mecha Bento Club", 4, 10, None, "1 week", Some("Ep 11 on Fri")),
    fk("Ghost of the Library Stacks", 11, 12, Some(7), "2 weeks", None),
    fk("Paper Crane Racing: Second Lap", 1, 12, Some(8), "1 month", None),
    Fake { kind: "Movie", ..fk("Kumo to Kaze no Ryokan", 0, 1, None, "2 months", None) },
    Fake { new_episode: Some(9), ..fk("Tidepool Radio", 8, 12, Some(6), "3 months", Some("Ep 10 on Tue")) },
    Fake { new_episode: Some(3), ..fk("Clockwork Bakery Wars", 2, 12, None, "4 months", None) },
];

/// Made-up English and Japanese titles of the first fake entries (Settings → Titles); the rest
/// have none, so they show the main title.
const FAKE_ALT: [(Option<&str>, Option<&str>); 5] = [
    (Some("The Lanterns on Kirigaoka Hill"), Some("霧ヶ丘のランタン")),
    (Some("Starfall Express"), Some("星降る配達便")),
    (None, Some("みかん通り探偵団")),
    (Some("Island Under the Stars"), Some("星空島スローライフ")),
    (None, Some("ネオン浪人2099")),
];

const WORDS_A: [&str; 16] = [
    "Moonlit", "Paper", "Crimson", "Quiet", "Silver", "Hidden", "Northern", "Electric", "Gentle", "Lost", "Velvet", "Second",
    "Endless", "Hollow", "Bright", "Winter",
];
const WORDS_B: [&str; 16] = [
    "Harbor", "Lantern", "Garden", "Signal", "Orchard", "Station", "Archive", "Comet", "Festival", "Lighthouse", "Workshop",
    "Valley", "Market", "Bridge", "Theatre", "Academy",
];
const WORDS_C: [&str; 8] = ["", " Season 2", " Rebirth", ": Side Stories", " Final Act", " Zero", "", " Movie"];

/// Made-up entries for the other statuses: the counts of the mockup's sidebar (48, 236, 7, 0).
fn fake_entries() -> Vec<Entry> {
    let mut out: Vec<Entry> = WATCHING
        .iter()
        .enumerate()
        .map(|(i, f)| Entry {
            id: i as u32,
            title: f.title.into(),
            main_title: f.title.into(),
            title_en: FAKE_ALT.get(i).and_then(|a| a.0).map(Into::into),
            title_ja: FAKE_ALT.get(i).and_then(|a| a.1).map(Into::into),
            // A rewatch is Completed on MAL; the Watching page shows it too.
            status: if f.rewatching { Status::Completed } else { Status::Watching },
            progress: f.progress,
            episodes: Some(f.episodes),
            score: f.score,
            search: [Some(f.title), FAKE_ALT.get(i).and_then(|a| a.0), FAKE_ALT.get(i).and_then(|a| a.1)]
                .into_iter()
                .flatten()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join("\n"),
            kind: f.kind,
            updated_at: FAKE_NOW - fake_age(f.updated),
            air: if f.airing.is_some() { Air::Airing } else { Air::Finished },
            airing_note: f.airing,
            new_episode: f.new_episode,
            rewatching: f.rewatching,
            times_rewatched: u32::from(f.rewatching),
            waiting: None,
            cover_url: None,
            timetable: None,
        })
        .collect();
    const UPDATED: [&str; 6] = ["3 days", "1 week", "2 weeks", "1 month", "6 months", "1 year"];
    let mut id = out.len() as u32;
    // Completed: 235 here and the one rewatched above make the mockup's 236.
    for (status, n) in [(Status::PlanToWatch, 48), (Status::Completed, 235), (Status::OnHold, 7)] {
        for k in 0..n {
            let h = id.wrapping_mul(2_654_435_761);
            let c = WORDS_C[(h >> 8) as usize % WORDS_C.len()];
            let title = format!("{} {}{}", WORDS_A[(h >> 16) as usize % 16], WORDS_B[(h >> 24) as usize % 16], c);
            let movie = c == " Movie";
            let episodes = if movie { 1 } else { [12, 13, 24, 25, 10, 26][h as usize % 6] };
            let progress = match status {
                Status::PlanToWatch => 0,
                Status::Completed => episodes,
                _ => (h >> 4) % episodes,
            };
            let search = title.to_lowercase();
            out.push(Entry {
                id,
                main_title: title.clone(),
                title_en: None,
                title_ja: None,
                title,
                status,
                progress,
                episodes: Some(episodes),
                score: (status == Status::Completed && k % 5 != 4).then_some(5 + (h % 6) as u8),
                search,
                kind: if movie { "Movie" } else { "TV" },
                updated_at: FAKE_NOW - fake_age(UPDATED[(k as usize * 6 / n as usize).min(5)]),
                air: Air::Finished,
                airing_note: None,
                new_episode: None,
                rewatching: false,
                times_rewatched: 0,
                waiting: None,
                cover_url: None,
                timetable: None,
            });
            id += 1;
        }
    }
    out
}

/// Scenes and tests: entry `id` airs weekly, with `aired` episodes out at the fake clock (the
/// next one in an hour less than a week).
pub(crate) fn airing_entry(s: &mut State, id: u32, aired: u32) {
    use kotora_core::airing::WEEK;
    let now = s.clock();
    let e = s.entries.iter_mut().find(|e| e.id == id).expect("entry");
    // `aired` 0: not yet aired, the first in an hour.
    let first = if aired == 0 { now + 3600 } else { now - i64::from(aired - 1) * WEEK - 3600 };
    e.air = if aired == 0 { Air::Upcoming } else { Air::Airing };
    e.timetable = Some(Timetable { first, episodes: e.episodes, shift: 0, until: None });
}

#[cfg(test)]
mod tests {
    use kotora_core::Media;

    use super::*;

    fn item(id: u32, title: &str, status: Status, progress: u32, score: Option<u8>, updated_at: i64) -> ListItem {
        ListItem {
            media: Media {
                id,
                title: title.into(),
                title_en: Some(format!("{title} English")),
                title_ja: None,
                synonyms: vec!["Hidden Alias".into()],
                episodes: Some(12),
                format: "ona".into(),
                air_status: "currently_airing".into(),
                cover_url: None,
                start_date: None,
                broadcast: None,
            },
            status,
            progress,
            score,
            rewatching: false,
            times_rewatched: 0,
            updated_at,
        }
    }

    fn loaded() -> State {
        let mut s = State::fake();
        s.apply(Msg::Synced {
            items: vec![
                item(1, "Bravo", Status::Watching, 3, Some(7), 100),
                item(2, "alpha", Status::Watching, 9, None, 300),
                item(3, "Charlie", Status::Watching, 1, Some(9), 200),
                item(4, "Delta", Status::Completed, 12, Some(10), 50),
            ],
            synced_at: 999,
        });
        s
    }

    fn titles(s: &State) -> Vec<&str> {
        s.rows.iter().map(|&i| s.entries[i].title.as_str()).collect()
    }

    #[test]
    fn fake_list_keeps_the_mockup_order() {
        let s = State::fake();
        assert_eq!(s.rows.len(), 12);
        assert_eq!(s.entries[s.rows[0]].title, "Lanterns of Kirigaoka");
        assert_eq!(s.entries[s.rows[11]].title, "Clockwork Bakery Wars");
    }

    #[test]
    fn edits_show_at_once_and_check_their_values() {
        let mut s = loaded();
        let i = s.plus_one(1).unwrap();
        assert_eq!((s.entries[i].progress, s.entries[i].waiting), (4, Some("Waiting to sync")));
        assert_eq!(s.entries[i].updated_at, FAKE_NOW);
        assert_eq!(titles(&s)[0], "Bravo", "the edited row moves to the top of Last updated");
        assert!(s.edit(1, Change { progress: Some(13), ..Default::default() }).unwrap_err().contains("12 episodes"));
        assert!(s.edit(1, Change { score: Some(Some(11)), ..Default::default() }).is_err());
        assert!(s.edit(99, Change::default()).is_err());
        s.edit(4, Change { progress: Some(12), ..Default::default() }).unwrap();
        assert!(s.plus_one(4).unwrap_err().contains("last episode"));
        let mut out = State::start(Auth::SignedOut);
        assert_eq!(out.edit(1, Change::default()).unwrap_err(), "Not signed in.");
    }

    #[test]
    fn the_last_episode_completes_the_entry() {
        let mut s = loaded();
        // +1 to the last episode.
        s.edit(2, Change { progress: Some(11), ..Default::default() }).unwrap();
        let i = s.plus_one(2).unwrap();
        assert_eq!((s.entries[i].progress, s.entries[i].status), (12, Status::Completed));
        // An explicit status wins (`kot set --status watching`).
        s.edit(3, Change { progress: Some(12), status: Some(Status::Watching), ..Default::default() }).unwrap();
        assert_eq!(s.entries.iter().find(|e| e.id == 3).unwrap().status, Status::Watching);
        // Unknown total: nothing happens.
        let e = s.entries.iter_mut().find(|e| e.id == 1).unwrap();
        e.episodes = None;
        s.edit(1, Change { progress: Some(40), ..Default::default() }).unwrap();
        assert_eq!(s.entries.iter().find(|e| e.id == 1).unwrap().status, Status::Watching);
        // Not the last episode: no change of status.
        let mut s = loaded();
        s.edit(1, Change { progress: Some(11), ..Default::default() }).unwrap();
        assert_eq!(s.entries.iter().find(|e| e.id == 1).unwrap().status, Status::Watching);
    }

    #[test]
    fn the_editor_completes_on_the_last_episode_and_steps_back() {
        let mut s = loaded();
        s.toggle_editor(1);
        s.editor_step(100);
        assert_eq!(s.editor.unwrap().status, Status::Completed);
        s.editor_step(-1);
        assert_eq!(s.editor.unwrap().status, Status::Watching);
        s.editor_step(1);
        s.save_editor().unwrap();
        let e = s.entries.iter().find(|e| e.id == 1).unwrap();
        assert_eq!((e.progress, e.status), (12, Status::Completed));
    }

    #[test]
    fn the_editor_drafts_and_saves_one_change() {
        let mut s = loaded();
        s.toggle_editor(1);
        s.editor_step(1);
        s.editor_step(-5);
        assert_eq!(s.editor.unwrap().progress, 0);
        s.editor_step(100);
        assert_eq!(s.editor.unwrap().progress, 12, "capped at the count");
        s.editor_step(-9);
        s.editor_score(None);
        assert_eq!(s.entries.iter().find(|e| e.id == 1).unwrap().progress, 3, "nothing changes before Save");
        s.save_editor().unwrap();
        assert!(s.editor.is_none());
        let e = s.entries.iter().find(|e| e.id == 1).unwrap();
        assert_eq!((e.progress, e.score), (3, None));
        s.toggle_editor(2);
        s.editor_status(Status::Completed);
        assert_eq!(s.editor.unwrap().progress, 12, "Completed fills the progress");
        s.toggle_editor(2);
        assert!(s.editor.is_none(), "a second press closes it");
    }

    #[test]
    fn find_takes_a_whole_title_or_a_part_that_matches_one() {
        let s = loaded();
        let title = |r: Result<usize, String>| r.map(|i| s.entries[i].title.clone());
        assert_eq!(title(s.find("ALPHA")), Ok("alpha".into()));
        assert_eq!(title(s.find("charl")), Ok("Charlie".into()));
        assert_eq!(title(s.find("delta english")), Ok("Delta".into()));
        assert!(s.find("english").unwrap_err().starts_with("4 anime match"));
        assert!(s.find("zzz").unwrap_err().starts_with("No anime"));
        assert!(s.find(" ").is_err());
    }

    #[test]
    fn the_queue_snapshot_moves_rows_badge_and_tags() {
        let mut s = State::fake();
        assert_eq!(s.queue, 4, "conflict, two waiting, one failed");
        let mikan = s.entries.iter().find(|e| e.title == "Mikan Street Detectives").unwrap();
        assert_eq!((mikan.waiting, mikan.note()), (Some("Conflict in queue"), "Conflict in queue"));
        let lanterns = s.entries.iter().find(|e| e.id == 0).unwrap();
        assert_eq!(lanterns.note(), "Ep 7 on Sat", "a plain wait says so in the tag only");
        // The worker reports: everything sent, one entry updated, one removed.
        let mut it = item(0, "Lanterns of Kirigaoka", Status::Watching, 6, Some(8), 5);
        it.media.episodes = Some(12);
        let done: Vec<Op> = s.ops.iter().filter(|o| o.state == OpState::Done).cloned().collect();
        assert!(s.apply(Msg::Queue { ops: done, items: vec![it], gone: vec![2], offline: Some(false) }));
        assert_eq!(s.queue, 0);
        assert!(s.entries.iter().all(|e| e.waiting.is_none()));
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 6);
        assert!(!s.entries.iter().any(|e| e.id == 2));
        s.apply(Msg::Queue { ops: vec![], items: vec![], gone: vec![], offline: Some(true) });
        assert!(s.offline);
    }

    #[test]
    fn sign_in_again_holds_the_queue_and_signing_in_frees_it() {
        let mut s = State::fake();
        s.apply(Msg::SyncFailed(Fail::SignInAgain));
        assert!(s.worker.hold.load(Ordering::Relaxed));
        s.apply(Msg::SignedIn { user: "demo_user".into(), picture: Some("https://cdn.example.invalid/u.jpg".into()) });
        assert_eq!(s.avatar.as_deref(), Some("https://cdn.example.invalid/u.jpg"));
        assert!(!s.worker.hold.load(Ordering::Relaxed));
        s.open_details(crate::details::Stub::bare(9001, "x"));
        s.online.show("x", crate::search::fake_hits());
        s.apply(Msg::SignedOut);
        assert!(s.ops.is_empty() && s.queue == 0);
        assert!(s.avatar.is_none(), "the picture goes with the account");
        assert!(!s.apply(Msg::Picture(Some("https://cdn.example.invalid/late.jpg".into()))), "a late answer after sign out is dropped");
        assert!(s.avatar.is_none());
        assert!(s.detail.is_none() && s.page != Page::Details && s.online.hits.is_empty());
    }

    #[test]
    fn sorts() {
        let mut s = loaded();
        assert_eq!(titles(&s), ["alpha", "Charlie", "Bravo"]);
        s.set_sort(Sort::Title);
        assert_eq!(titles(&s), ["alpha", "Bravo", "Charlie"]);
        s.set_sort(Sort::Score);
        assert_eq!(titles(&s), ["Charlie", "Bravo", "alpha"]);
        s.set_sort(Sort::Progress);
        assert_eq!(titles(&s), ["alpha", "Bravo", "Charlie"]);
        // The other way round: reversed, and a new order starts its own way again.
        s.set_sort(Sort::Title);
        s.set_sort_rev(true);
        assert_eq!(titles(&s), ["Charlie", "Bravo", "alpha"]);
        s.set_sort(Sort::Score);
        assert!(!s.sort_rev);
        assert_eq!(titles(&s), ["Charlie", "Bravo", "alpha"]);
    }

    /// Rewatches: start on a completed entry only, stop without counting, and the Watching page
    /// shows them while the sidebar counts MAL's status.
    #[test]
    fn rewatch_start_stop_and_the_watching_page() {
        let mut s = State::fake();
        let watching = s.count(Status::Watching);
        assert!(s.start_rewatch(0).unwrap_err().contains("not completed"));
        let id = s.entries.iter().find(|e| e.status == Status::Completed && !e.rewatching).unwrap().id;
        s.start_rewatch(id).unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((e.status, e.progress, e.rewatching, e.waiting), (Status::Completed, 0, true, Some("Waiting to sync")));
        assert!(s.start_rewatch(id).is_err(), "once");
        assert_eq!(s.count(Status::Watching), watching, "the sidebar counts MAL's status");
        s.go(Page::List(Status::Watching));
        assert!(s.rows.iter().any(|&i| s.entries[i].id == id), "the Watching page shows it");
        s.go(Page::List(Status::Completed));
        assert!(s.rows.iter().any(|&i| s.entries[i].id == id), "and the Completed page still does");
        s.stop_rewatch(id).unwrap();
        let e = s.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!((Some(e.progress), e.rewatching, e.times_rewatched), (e.episodes, false, 0), "back at the last episode, not counted");
        assert!(s.stop_rewatch(id).is_err());
    }

    #[test]
    fn searches_every_title_case_blind() {
        let mut s = loaded();
        s.query.replace_text("  ALPHA ");
        s.refresh();
        assert_eq!(titles(&s), ["alpha"]);
        s.query.replace_text("charlie english");
        s.refresh();
        assert_eq!(titles(&s), ["Charlie"]);
        s.query.replace_text("hidden alias");
        s.refresh();
        assert_eq!(titles(&s), ["alpha", "Charlie", "Bravo", "Delta"], "every status, the page's own first");
        s.query.replace_text("zzz");
        s.refresh();
        assert!(s.rows.is_empty());
        // The search follows the page.
        s.query.replace_text("delta");
        s.go(Page::List(Status::Completed));
        assert_eq!(titles(&s), ["Delta"]);
    }

    #[test]
    fn an_added_anime_shows_at_once_and_waits() {
        let mut s = loaded();
        let m = Media { id: 77, title: "Echo".into(), title_en: None, title_ja: None, synonyms: vec![], episodes: Some(10), format: "tv".into(), air_status: String::new(), cover_url: None, start_date: None, broadcast: None };
        let i = s.add_media(m.clone(), Status::Completed).unwrap();
        let e = &s.entries[i];
        assert_eq!((e.status, e.progress, e.waiting), (Status::Completed, 10, Some("Waiting to sync")));
        assert_eq!(s.count(Status::Completed), 2);
        // Again: a status change, not a second entry.
        s.add_media(m, Status::OnHold).unwrap();
        assert_eq!(s.entries.iter().filter(|e| e.id == 77).count(), 1);
        assert_eq!(s.entries.iter().find(|e| e.id == 77).unwrap().status, Status::OnHold);
        let mut out = State::start(Auth::SignedOut);
        assert!(out.add_media(Media { id: 1, title: "x".into(), title_en: None, title_ja: None, synonyms: vec![], episodes: None, format: String::new(), air_status: String::new(), cover_url: None, start_date: None, broadcast: None }, Status::Watching).is_err());
    }

    #[test]
    fn filters_use_the_airing_status() {
        let mut s = loaded();
        s.set_filter(Filter::Airing);
        assert_eq!(s.rows.len(), 3);
        s.set_filter(Filter::Finished);
        assert!(s.rows.is_empty());
        assert_eq!(s.entries[0].kind, "ONA");
    }

    #[test]
    fn messages_move_the_state() {
        let mut s = State::fake();
        assert!(s.apply(Msg::SyncFailed(Fail::Offline("Can't reach MyAnimeList. Showing your saved list.".into()))));
        assert!(s.offline && s.error.is_some() && !s.syncing);
        assert_eq!(s.entries.len(), 302, "the saved list stays");
        assert_eq!(s.queue, 4);
        s.apply(Msg::SyncFailed(Fail::SignInAgain));
        assert_eq!(s.auth, Auth::Expired);
        assert!(s.has_list());
        s.apply(Msg::Synced { items: vec![], synced_at: 5 });
        assert_eq!((s.auth, s.offline, s.error.is_none(), s.synced_at), (Auth::SignedIn, false, true, Some(5)));
        s.apply(Msg::SignedOut);
        assert_eq!(s.auth, Auth::SignedOut);
        assert!(s.entries.is_empty() && s.account.is_none() && !s.has_list());
        s.apply(Msg::SignInFailed("busy".into()));
        assert_eq!((s.auth, s.error.as_deref()), (Auth::SignedOut, Some("busy")));
    }

    #[test]
    fn an_old_cache_never_replaces_a_newer_sync() {
        let mut s = State::start(Auth::SignedIn);
        s.apply(Msg::Synced { items: vec![item(1, "Fresh", Status::Watching, 1, None, 1)], synced_at: 10 });
        s.apply(Msg::Cache { items: vec![item(2, "Stale", Status::Watching, 1, None, 1)], user: Some("demo_user".into()), picture: None, profile: None, synced_at: Some(5), ops: vec![] });
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].title, "Fresh");
        assert_eq!(s.account.as_ref().map(|a| a.0.as_str()), Some("demo_user"));
        assert_eq!(s.synced_at, Some(10));
    }

    #[test]
    fn a_cache_ends_the_loading_state_and_an_empty_one_waits_for_the_sync() {
        let mut s = State::fake();
        s.entries.clear();
        s.loading = true;
        s.syncing = true;
        s.apply(Msg::Cache { items: vec![], user: None, picture: None, profile: None, synced_at: None, ops: vec![] });
        assert!(s.loading, "nothing saved yet: keep the skeleton");
        s.apply(Msg::Cache { items: vec![item(1, "A", Status::Watching, 1, None, 1)], user: None, picture: None, profile: None, synced_at: Some(1), ops: vec![] });
        assert!(!s.loading);
    }

    /// Not aired yet by the estimate: `+1` and the editor ask "Mark anyway?"; nothing changes
    /// until then. Past the count stays a hard no.
    #[test]
    fn an_episode_not_aired_yet_asks_first() {
        let mut s = State::fake();
        // Lanterns of Kirigaoka (id 0): 5 of 12, 5 aired.
        airing_entry(&mut s, 0, 5);
        let p = |s: &State| s.entries.iter().find(|e| e.id == 0).unwrap().progress;
        s.plus_one_or_ask(0).unwrap();
        let a = s.ask.clone().expect("asks");
        assert!(a.question.starts_with("Episode 6 of Lanterns of Kirigaoka hasn't aired yet (expected "), "{}", a.question);
        assert_eq!(p(&s), 5, "nothing changes yet");
        s.ask = None;
        assert_eq!(p(&s), 5, "Cancel: nothing");
        s.plus_one_or_ask(0).unwrap();
        s.mark_anyway().unwrap();
        assert_eq!((p(&s), s.ask.is_none()), (6, true), "Mark anyway");
        // The editor: Save asks, the editor stays open; Mark anyway saves and closes it.
        s.toggle_editor(0);
        s.editor_step(2);
        s.save_editor().unwrap();
        assert!(s.ask.as_ref().is_some_and(|a| a.editor && a.question.starts_with("Episode 8 ")));
        assert!(s.editor.is_some() && p(&s) == 6);
        s.mark_anyway().unwrap();
        assert!(s.editor.is_none() && p(&s) == 8);
        // A score or a status alone never asks.
        s.toggle_editor(0);
        s.editor_score(Some(7));
        s.save_editor().unwrap();
        assert!(s.ask.is_none() && s.editor.is_none());
        // Past the count: refused, not asked.
        assert!(s.edit_or_ask(0, Change { progress: Some(13), ..Default::default() }, false).unwrap_err().contains("12 episodes"));
        assert!(s.ask.is_none());
    }

    #[test]
    fn aired_episodes_and_finished_shows_never_ask() {
        let mut s = State::fake();
        airing_entry(&mut s, 0, 7);
        s.plus_one_or_ask(0).unwrap();
        assert!(s.ask.is_none(), "episode 6 aired");
        assert_eq!(s.entries.iter().find(|e| e.id == 0).unwrap().progress, 6);
        // No slot known (the fake list), or finished: no estimate, no question.
        let mut s = State::fake();
        s.plus_one_or_ask(1).unwrap();
        assert!(s.ask.is_none());
        airing_entry(&mut s, 0, 5);
        s.entries.iter_mut().find(|e| e.id == 0).unwrap().air = Air::Finished;
        s.plus_one_or_ask(0).unwrap();
        assert!(s.ask.is_none());
        // Not yet aired at all: episode 1 asks.
        let mut s = State::fake();
        s.edit(0, Change { progress: Some(0), ..Default::default() }).unwrap();
        airing_entry(&mut s, 0, 0);
        s.plus_one_or_ask(0).unwrap();
        assert!(s.ask.as_ref().is_some_and(|a| a.question.starts_with("Episode 1 ")));
    }
}
