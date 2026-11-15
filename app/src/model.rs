//! App state. The real list comes from the store and MyAnimeList (`online.rs`); the fake list below
//! is for the headless scenes and the tests only.
//!
//! Every fake title here is made up (the same ones as `mockup/kotora.html`).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kotora_core::queue::{Op, OpState};
use kotora_core::{Change, ListItem, Remote, Status, Values};
use rune::{AppHandle, Image, TextEdit};

use kotora_core::recognize::Found;
use kotora_detect::{Led, SourceStatus, Switch, Via};

use crate::art::Covers;
use crate::now::{Now, Prefs, Seen};
use crate::queue::{Cmd, Worker};

pub type Handle = AppHandle<State>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    List(Status),
    NowPlaying,
    Queue,
    Settings,
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

    /// The next order (the Sort control cycles).
    pub fn next(self) -> Sort {
        Sort::ALL[(Sort::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1) % Sort::ALL.len()]
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
    pub title: String,
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
    pub rewatching: bool,
    /// A change for this entry waits in the queue.
    pub waiting: Option<&'static str>,
    /// The service's cover image (`None` in the fake list, and for shows without one).
    pub cover_url: Option<String>,
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
    pub fn from_item(it: &ListItem) -> Entry {
        let m = &it.media;
        let search = m.all_titles().map(str::to_lowercase).collect::<Vec<_>>().join("\n");
        Entry {
            id: m.id,
            title: m.title.clone(),
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
            waiting: None,
            cover_url: m.cover_url.clone(),
        }
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
        Values { status: self.status, progress: self.progress, score: self.score }
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
        }
    }

    pub fn fraction(&self) -> f32 {
        match self.episodes {
            Some(n) if n > 0 => (self.progress as f32 / n as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
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
    Cache { items: Vec<ListItem>, user: Option<String>, synced_at: Option<i64>, ops: Vec<Op> },
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
    SignedIn { user: String },
    SignInFailed(String),
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
    /// The search box.
    pub query: TextEdit,
    pub auth: Auth,
    /// The signed-in user name and service; `None` when signed out.
    pub account: Option<(String, &'static str)>,
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
    pub players: Vec<SourceStatus>,
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
    /// The MyAnimeList Client ID field (Welcome and Settings).
    pub client_id: crate::clientid::ClientIdForm,
    pub handle: Option<Handle>,
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
            query: TextEdit::new(""),
            auth: Auth::SignedIn,
            account: Some(("demo_user".into(), "MyAnimeList")),
            entries: fake_entries(),
            rows: Vec::new(),
            loading: false,
            syncing: false,
            error: None,
            queue: 0,
            ops: fake_ops(),
            editor: None,
            worker: Arc::default(),
            offline: false,
            synced_at: Some(FAKE_NOW - 120),
            fixed_now: Some(FAKE_NOW),
            now: Some(fake_now()),
            players: fake_players(),
            prefs: Prefs::default(),
            glue: None,
            window: true,
            cancel: None,
            logo: None,
            logo_big: None,
            covers: Covers::default(),
            client_id: crate::clientid::ClientIdForm { hint: Some("8f3a".into()), ..Default::default() },
            handle: None,
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
        s.queue = 0;
        s.ops.clear();
        s.synced_at = None;
        s.fixed_now = None;
        s.now = None;
        s.players.clear();
        s.loading = matches!(auth, Auth::SignedIn);
        s.handle = None;
        s.client_id.hint = None;
        s.refresh();
        s
    }

    pub fn clock(&self) -> i64 {
        self.fixed_now.unwrap_or_else(kotora_core::time::now_unix)
    }

    pub fn count(&self, s: Status) -> usize {
        self.entries.iter().filter(|e| e.status == s).count()
    }

    /// Whether the list pages show a list (rather than the sign-in screen).
    pub fn has_list(&self) -> bool {
        matches!(self.auth, Auth::SignedIn | Auth::Expired)
    }

    /// Rebuilds `rows`: the page's status, then the chip, the search and the sort. Call it after
    /// any change to the page, filter, query, sort or entries.
    pub fn refresh(&mut self) {
        let Page::List(st) = self.page else {
            self.rows.clear();
            return;
        };
        let q = self.query.text.trim().to_lowercase();
        let mut rows: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                e.status == st && self.filter.keeps(e) && (q.is_empty() || e.search.contains(&q))
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
        self.rows = rows;
    }

    pub fn go(&mut self, page: Page) {
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

    pub fn set_sort(&mut self, sort: Sort) {
        self.sort = sort;
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
        // Like the MAL website: the last episode completes the entry (when the total is known and
        // the change doesn't pick a status itself).
        if change.status.is_none()
            && let (Some(p), Some(n)) = (change.progress, e.episodes)
            && n > 0
            && p == n
            && e.progress < n
            && e.status != Status::Completed
        {
            change.status = Some(Status::Completed);
        }
        let after = change.apply(e.values());
        if after == e.values() {
            return Ok(i);
        }
        e.status = after.status;
        e.progress = after.progress;
        e.score = after.score;
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
        self.refresh();
        Ok(i)
    }

    /// `+1`: optimistic, at once (design-spec §6).
    pub fn plus_one(&mut self, id: u32) -> Result<usize, String> {
        let e = self.entries.iter().find(|e| e.id == id).ok_or("That anime is not on your list.")?;
        if e.episodes.is_some_and(|n| e.progress >= n) {
            return Err(format!("{} is at its last episode.", e.title));
        }
        let p = e.progress + 1;
        self.edit(id, Change { progress: Some(p), ..Default::default() })
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
        let change = Change { status: Some(ed.status), progress: Some(ed.progress), score: Some(ed.score) }.without_same(&e.values());
        self.edit(ed.id, change)?;
        self.editor = None;
        Ok(())
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

    /// The queue as `kot queue` prints it.
    pub fn ctl_queue(&self) -> Vec<kotora_ctl::QueueItem> {
        let service = "MyAnimeList";
        self.ops
            .iter()
            .map(|o| kotora_ctl::QueueItem {
                title: o.title.clone(),
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
        self.entries = items.iter().map(Entry::from_item).collect();
        self.queue_changed();
        self.refresh();
    }

    /// Applies a worker's message. Returns whether anything visible changed.
    pub fn apply(&mut self, msg: Msg) -> bool {
        let service = "MyAnimeList";
        match msg {
            Msg::Cache { items, user, synced_at, ops } => {
                self.ops = ops;
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
                    let e = Entry::from_item(it);
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
            Msg::SignedIn { user } => {
                self.auth = Auth::SignedIn;
                self.account = Some((user, service));
                self.cancel = None;
                self.error = None;
                // The queue waited for this.
                self.worker.hold.store(false, Ordering::Relaxed);
                if self.queue > 0 {
                    self.queue_cmd(Cmd::Kick { refresh: false });
                }
                true
            }
            Msg::SignInFailed(m) => {
                // Keep the saved list when the user was only signing in again.
                self.auth = if self.entries.is_empty() { Auth::SignedOut } else { Auth::Expired };
                self.cancel = None;
                self.error = Some(m);
                true
            }
            Msg::Now(seen) => self.now_seen(seen.map(|b| *b)),
            Msg::Reached { key, file } => self.now_reached(&key, &file),
            Msg::Players(v) => self.players_seen(v),
            Msg::ClientIdSaved(r) => self.client_id_saved(r),
            Msg::SignedOut => {
                self.covers.clear();
                self.auth = Auth::SignedOut;
                self.account = None;
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
        dismissed: false,
        acted: false,
        recorded: None,
        picking: false,
    }
}

/// The fake players of this system (Settings → Players).
pub fn fake_players() -> Vec<SourceStatus> {
    let st = |switch, via, line: &str, led| SourceStatus { switch, via, line: line.into(), led };
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
    let w = |progress, score| Values { status: Status::Watching, progress, score };
    let progress = |p| Change { progress: Some(p), ..Default::default() };
    vec![
        Op {
            remote: Some(Remote { values: w(8, None), rewatching: false, updated_at: FAKE_NOW - 3_000 }),
            ..op(1, 2, progress(7), w(6, None), OpState::Conflict, 4_500)
        },
        Op { edits: 2, ..op(2, 0, progress(5), w(3, Some(8)), OpState::Queued, 900) },
        op(3, 8, Change { progress: Some(1), score: Some(Some(8)), status: None }, w(0, None), OpState::Queued, 1_900),
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
            status: Status::Watching,
            progress: f.progress,
            episodes: Some(f.episodes),
            score: f.score,
            search: f.title.to_lowercase(),
            kind: f.kind,
            updated_at: FAKE_NOW - fake_age(f.updated),
            air: if f.airing.is_some() { Air::Airing } else { Air::Finished },
            airing_note: f.airing,
            new_episode: f.new_episode,
            rewatching: f.rewatching,
            waiting: None,
            cover_url: None,
        })
        .collect();
    const UPDATED: [&str; 6] = ["3 days", "1 week", "2 weeks", "1 month", "6 months", "1 year"];
    let mut id = out.len() as u32;
    for (status, n) in [(Status::PlanToWatch, 48), (Status::Completed, 236), (Status::OnHold, 7)] {
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
                waiting: None,
                cover_url: None,
            });
            id += 1;
        }
    }
    out
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
            },
            status,
            progress,
            score,
            rewatching: false,
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
        s.apply(Msg::SignedIn { user: "demo_user".into() });
        assert!(!s.worker.hold.load(Ordering::Relaxed));
        s.apply(Msg::SignedOut);
        assert!(s.ops.is_empty() && s.queue == 0);
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
        assert_eq!(Sort::Progress.next(), Sort::Updated);
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
        assert_eq!(s.rows.len(), 3);
        s.query.replace_text("zzz");
        s.refresh();
        assert!(s.rows.is_empty());
        // The search follows the page.
        s.query.replace_text("delta");
        s.go(Page::List(Status::Completed));
        assert_eq!(titles(&s), ["Delta"]);
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
        assert_eq!(s.entries.len(), 303, "the saved list stays");
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
        s.apply(Msg::Cache { items: vec![item(2, "Stale", Status::Watching, 1, None, 1)], user: Some("demo_user".into()), synced_at: Some(5), ops: vec![] });
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
        s.apply(Msg::Cache { items: vec![], user: None, synced_at: None, ops: vec![] });
        assert!(s.loading, "nothing saved yet: keep the skeleton");
        s.apply(Msg::Cache { items: vec![item(1, "A", Status::Watching, 1, None, 1)], user: None, synced_at: Some(1), ops: vec![] });
        assert!(!s.loading);
    }
}
