//! Manga mode (MyAnimeList only): the sidebar's Anime | Manga switch, the manga list pages
//! (Reading, Plan to read, Completed, On hold, Dropped), the row editor and the "Your list" card
//! of a manga's details page.
//!
//! The manga list lives beside the anime one ([`MangaUi`]); Home, Now Playing, Airing and the
//! search stay anime only. The list is read by the sync worker only once Manga was turned on
//! (`manga:enabled`, off by default: the switch's first press turns it on). Edits go through the
//! same sync queue as anime ([`crate::queue::Cmd::Edit`] with the kind).
//!
//! Progress shows chapters, volumes or both, whichever was read ([`shows`]); nothing read yet
//! shows chapters. The editor always has both.

use kotora_core::queue::OpState;
use kotora_core::{Change, Kind, ListItem, Score, ScoreFormat, Status, Values};
use rune::{El, GrowthSpec, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};

use crate::icons;
use crate::model::{Msg, Page, State, waiting_label};
use crate::theme::{self, PAL};
use crate::ui::{alert, button, cell, columns, field, ghost, kbd, tag};

/// The cover cache's ids for manga: MAL anime and manga ids overlap, so a manga's cover and
/// tile use its id with the top bit set (no MAL id reaches it; the avatar is `u32::MAX`).
pub const COVER_BIT: u32 = 1 << 31;

pub fn cover_id(id: u32) -> u32 {
    id | COVER_BIT
}

/// Which counts a manga shows: chapters when read, volumes when read, both when both are;
/// nothing read yet shows chapters. (`kot list --manga` has the same rule.)
pub fn shows(chapters: u32, volumes: u32) -> (bool, bool) {
    (chapters > 0 || volumes == 0, volumes > 0)
}

/// Where a manga is in its run (MAL `status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Publishing,
    Finished,
    Hiatus,
    Discontinued,
    NotYet,
    Unknown,
}

impl Run {
    pub fn from_mal(s: &str) -> Run {
        match s {
            "currently_publishing" => Run::Publishing,
            "finished" => Run::Finished,
            "on_hiatus" => Run::Hiatus,
            "discontinued" => Run::Discontinued,
            "not_yet_published" => Run::NotYet,
            _ => Run::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Run::Publishing => "Publishing",
            Run::Finished => "Finished",
            Run::Hiatus => "On hiatus",
            Run::Discontinued => "Discontinued",
            Run::NotYet => "Not yet published",
            Run::Unknown => "",
        }
    }
}

/// One manga of the list, as the rows show it.
#[derive(Clone, Debug)]
pub struct MEntry {
    pub id: u32,
    pub title: String,
    pub main_title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    /// Every title, lower case, for the filter box.
    pub search: String,
    pub status: Status,
    pub chapters: u32,
    pub chapters_total: Option<u32>,
    pub volumes: u32,
    pub volumes_total: Option<u32>,
    pub score: Option<Score>,
    /// "Manga", "Light novel", "Manhwa", …
    pub format: String,
    pub run: Run,
    pub rereading: bool,
    pub times_reread: u32,
    pub updated_at: i64,
    pub waiting: Option<&'static str>,
    pub cover_url: Option<String>,
}

impl MEntry {
    pub fn from_item(it: &ListItem, lang: kotora_core::config::TitleLang) -> MEntry {
        let m = &it.media;
        MEntry {
            id: m.id,
            title: m.display_title(lang).to_string(),
            main_title: m.title.clone(),
            title_en: m.title_en.clone(),
            title_ja: m.title_ja.clone(),
            search: m.all_titles().map(str::to_lowercase).collect::<Vec<_>>().join("\n"),
            status: it.status,
            chapters: it.progress,
            chapters_total: m.episodes.filter(|n| *n > 0),
            volumes: it.volumes,
            volumes_total: m.volumes.filter(|n| *n > 0),
            score: it.score,
            format: if m.format.is_empty() { String::new() } else { kotora_core::mal::manga_type_label(&m.format) },
            run: Run::from_mal(&m.air_status),
            rereading: it.rewatching,
            times_reread: it.times_rewatched,
            updated_at: it.updated_at,
            waiting: None,
            cover_url: m.cover_url.clone(),
        }
    }

    pub fn retitle(&mut self, lang: kotora_core::config::TitleLang) {
        self.title = lang.pick(&self.main_title, self.title_en.as_deref(), self.title_ja.as_deref()).to_string();
    }

    pub fn values(&self) -> Values {
        Values { status: self.status, progress: self.chapters, score: self.score, rewatching: self.rereading, times_rewatched: self.times_reread, volumes: self.volumes }
    }

    /// (chapters, volumes) shown: [`shows`].
    pub fn shows(&self) -> (bool, bool) {
        shows(self.chapters, self.volumes)
    }

    /// The bar: chapters read of the count when chapters show, else volumes; empty while the
    /// count is unknown (still publishing).
    pub fn fraction(&self) -> f32 {
        let (n, total) = if self.shows().0 { (self.chapters, self.chapters_total) } else { (self.volumes, self.volumes_total) };
        match total {
            Some(t) if t > 0 => (n as f32 / t as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// The Reading page shows rereads too (Completed on MAL), like Watching does rewatches.
    pub fn on_page(&self, st: Status) -> bool {
        self.status == st || (st == Status::Watching && self.rereading)
    }

    /// `+1` steps chapters, or volumes when only volumes show.
    pub fn plus_unit(&self) -> Unit {
        if self.shows().0 { Unit::Chapters } else { Unit::Volumes }
    }

    fn at_end(&self, u: Unit) -> bool {
        match u {
            Unit::Chapters => self.chapters_total.is_some_and(|n| self.chapters >= n),
            Unit::Volumes => self.volumes_total.is_some_and(|n| self.volumes >= n),
        }
    }

    /// The grey note after the tags (the run, when no tag says it).
    fn note(&self) -> &'static str {
        match self.run {
            Run::Finished | Run::Discontinued | Run::NotYet => self.run.label(),
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Chapters,
    Volumes,
}

/// The chips above the manga list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MFilter {
    #[default]
    All,
    Publishing,
    Finished,
    Hiatus,
}

impl MFilter {
    pub const ALL: [MFilter; 4] = [MFilter::All, MFilter::Publishing, MFilter::Finished, MFilter::Hiatus];

    pub fn label(self) -> &'static str {
        match self {
            MFilter::All => "All",
            MFilter::Publishing => "Publishing",
            MFilter::Finished => "Finished",
            MFilter::Hiatus => "On hiatus",
        }
    }

    pub fn keeps(self, e: &MEntry) -> bool {
        match self {
            MFilter::All => true,
            MFilter::Publishing => e.run == Run::Publishing,
            MFilter::Finished => e.run == Run::Finished,
            MFilter::Hiatus => e.run == Run::Hiatus,
        }
    }
}

/// The manga editor's draft: nothing is queued until Save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MEditor {
    pub id: u32,
    pub status: Status,
    pub chapters: u32,
    pub volumes: u32,
    pub score: Option<Score>,
}

/// Manga mode's state.
pub struct MangaUi {
    /// The switch is on Manga: the sidebar's "My list" and the list pages are the manga ones.
    pub mode: bool,
    /// Syncs read the manga list (`manga:enabled`).
    pub enabled: bool,
    pub entries: Vec<MEntry>,
    /// Indices into `entries`, as the page shows them.
    pub rows: Vec<usize>,
    pub filter: MFilter,
    /// The manga page's filter box.
    pub query: TextEdit,
    /// The first read of the manga list runs (Manga just turned on).
    pub loading: bool,
    pub synced_at: Option<i64>,
    /// The last manga sync failed: why. Shown on the manga pages and by `kot list --manga`.
    pub error: Option<String>,
    pub editor: Option<MEditor>,
    /// "Remove … from your manga list?" asks about this manga (the bar on top of the page).
    pub confirm_remove: Option<u32>,
}

impl Default for MangaUi {
    fn default() -> Self {
        MangaUi {
            mode: false,
            enabled: false,
            entries: Vec::new(),
            rows: Vec::new(),
            filter: MFilter::All,
            query: TextEdit::new(""),
            loading: false,
            synced_at: None,
            error: None,
            editor: None,
            confirm_remove: None,
        }
    }
}

/// What workers tell the UI about the manga list.
#[derive(Debug)]
pub enum MangaMsg {
    /// The saved manga list at start (MyAnimeList signed in).
    Cache { enabled: bool, items: Vec<ListItem>, synced_at: Option<i64> },
    /// A fresh manga list, saved.
    Synced { items: Vec<ListItem>, synced_at: i64 },
    /// The manga half of a sync failed: why, in the page's words.
    Failed(String),
    /// The queue touched these manga (and these left the list).
    Items { items: Vec<ListItem>, gone: Vec<u32> },
    /// `manga:enabled` was written (or not).
    Enabled(Result<(), String>),
}

/// A failed manga sync in the words the page (and `kot list --manga`) shows.
pub fn fail_text(r: &kotora_core::sync::MangaSync, svc: &str) -> Option<String> {
    use kotora_core::service::ServiceError as E;
    use kotora_core::sync::MangaSync as M;
    Some(match r {
        M::Off | M::Synced { .. } => return None,
        M::Failed(E::SignInAgain) => format!("Your manga list wasn't read: {svc} needs you to sign in again."),
        M::Failed(E::Offline(_)) => format!("Your manga list wasn't read: can't reach {svc}."),
        M::Failed(E::RateLimited) => format!("Your manga list wasn't read: {svc} asked Kotora to slow down. Try again in a minute."),
        M::Failed(E::Other(m)) => format!("Your manga list wasn't read: {m}"),
        M::NotSaved(m) => format!("Your manga list wasn't saved: {m}"),
    })
}

impl State {
    /// The list page of status `st` in the switch's mode.
    pub fn list_page(&self, st: Status) -> Page {
        if self.manga.mode { Page::Manga(st) } else { Page::List(st) }
    }

    /// The switch: Anime or Manga. The first move to Manga turns Manga on (syncs read the
    /// manga list from then on) and reads it now. The choice is kept (`[display] list_kind`).
    pub fn set_kind(&mut self, kind: Kind) {
        let manga = kind == Kind::Manga;
        let st = match self.lit_page() {
            Page::List(st) | Page::Manga(st) => st,
            _ => Status::Watching,
        };
        let changed = self.manga.mode != manga;
        self.manga.mode = manga;
        self.editor = None;
        self.manga.editor = None;
        if manga && !self.manga.enabled {
            self.enable_manga();
        }
        if self.page == Page::Details {
            self.detail = None;
        }
        self.go(self.list_page(st));
        if changed {
            crate::detect::save_setting(self, "display", "list_kind", kotora_core::config::quote(kind.name()));
        }
    }

    /// Turns Manga on in the store, then reads the manga list (never headless).
    fn enable_manga(&mut self) {
        self.manga.enabled = true;
        let Some(h) = self.handle.clone() else { return };
        self.manga.loading = true;
        self.manga.error = None;
        std::thread::spawn(move || {
            let r = kotora_core::store::Store::open(&kotora_core::paths::db_file())
                .and_then(|st| st.set_manga_enabled(true))
                .map_err(|e| format!("Can't turn Manga on: {e}"));
            h.update_if(move |s, _| s.apply(Msg::Manga(MangaMsg::Enabled(r))));
        });
    }

    /// Reads the manga list alone (Manga just turned on, Retry on a manga page), on a worker.
    pub fn sync_manga(&mut self) {
        let Some(h) = self.handle.clone() else { return };
        if !self.manga.enabled || !self.has_list() || kotora_core::paths::offline_forced() {
            self.manga.loading = false;
            return;
        }
        self.manga.loading = self.manga.synced_at.is_none();
        self.manga.error = None;
        std::thread::spawn(move || {
            let svc = crate::account::active();
            let msg = match (kotora_core::store::Store::open(&kotora_core::paths::db_file()), crate::account::open()) {
                (Ok(mut store), Some(mut service)) => {
                    let r = kotora_core::sync::sync_manga(service.as_mut(), &mut store, kotora_core::time::now_unix());
                    if matches!(r, kotora_core::sync::MangaSync::Synced { .. }) {
                        crate::mirror::kick(&h, false);
                    }
                    manga_msg(r, svc.title())
                }
                (Err(e), _) => Some(MangaMsg::Failed(format!("Can't open the saved list: {e}"))),
                (_, None) => Some(MangaMsg::Failed(format!("Your manga list wasn't read: {} needs you to sign in again.", svc.title()))),
            };
            h.update_if(move |s, _| match msg {
                Some(m) if crate::account::active() == svc => s.apply(Msg::Manga(m)),
                _ => {
                    s.manga.loading = false;
                    true
                }
            });
        });
    }

    pub fn manga_count(&self, st: Status) -> usize {
        self.manga.entries.iter().filter(|e| e.status == st).count()
    }

    pub fn manga_reread_count(&self) -> usize {
        self.manga.entries.iter().filter(|e| e.rereading && e.status != Status::Watching).count()
    }

    /// Rebuilds the manga page's rows (status, chip, filter box, sort).
    pub fn refresh_manga(&mut self) {
        // The search page in Manga mode: every manga matching its field, Reading's first.
        let (st, q) = match self.page {
            Page::Manga(st) => (st, self.manga.query.text.trim().to_lowercase()),
            Page::Search if self.manga.mode && self.searching() => (Status::Watching, self.find.text.trim().to_lowercase()),
            _ => {
                self.manga.rows.clear();
                return;
            }
        };
        let m = &self.manga;
        let mut rows: Vec<usize> = (0..m.entries.len())
            .filter(|&i| {
                let e = &m.entries[i];
                if q.is_empty() { e.on_page(st) && m.filter.keeps(e) } else { e.search.contains(&q) }
            })
            .collect();
        let e = &m.entries;
        match self.sort {
            crate::model::Sort::Updated => rows.sort_by_key(|&i| std::cmp::Reverse(e[i].updated_at)),
            crate::model::Sort::Title => rows.sort_by_cached_key(|&i| e[i].title.to_lowercase()),
            crate::model::Sort::Score => rows.sort_by_key(|&i| std::cmp::Reverse(e[i].score)),
            crate::model::Sort::Progress => rows.sort_by_key(|&i| std::cmp::Reverse((e[i].chapters, e[i].volumes))),
        }
        if self.sort_rev {
            rows.reverse();
        }
        if !q.is_empty() {
            rows.sort_by_key(|&i| if e[i].status == st { 0 } else { 1 + Status::ALL.iter().position(|x| *x == e[i].status).unwrap_or(0) });
        }
        self.manga.rows = rows;
    }

    pub fn set_manga_filter(&mut self, f: MFilter) {
        self.manga.filter = f;
        self.refresh_manga();
    }

    /// An edit of a manga (the editor, `+1`, reread): the row shows it at once, the queue sends
    /// it. Returns the entry's index.
    pub fn manga_edit(&mut self, id: u32, change: Change) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let now = self.clock();
        let i = self.manga.entries.iter().position(|e| e.id == id).ok_or("That manga is not on your list.")?;
        let e = &mut self.manga.entries[i];
        if let (Some(p), Some(n)) = (change.progress, e.chapters_total)
            && p > n
        {
            return Err(format!("{} has {n} chapter{}.", e.title, if n == 1 { "" } else { "s" }));
        }
        if let (Some(v), Some(n)) = (change.volumes, e.volumes_total)
            && v > n
        {
            return Err(format!("{} has {n} volume{}.", e.title, if n == 1 { "" } else { "s" }));
        }
        // As on MAL's website: the last chapter completes the entry (or ends a reread).
        let mut change = change.completes(&e.values(), e.chapters_total);
        // Completed fills the counts that are known and not picked.
        if change.status == Some(Status::Completed) && !e.rereading {
            change.progress = change.progress.or(e.chapters_total);
            change.volumes = change.volumes.or(e.volumes_total);
        }
        let change = change.without_same(&e.values());
        let after = change.apply(e.values());
        if after == e.values() {
            return Ok(i);
        }
        e.status = after.status;
        e.chapters = after.progress;
        e.volumes = after.volumes;
        e.score = after.score;
        e.rereading = after.rewatching;
        e.times_reread = after.times_rewatched;
        e.updated_at = now;
        let open: Vec<&kotora_core::queue::Op> = self.ops.iter().filter(|o| o.kind == Kind::Manga && o.media_id == id && o.state.is_open()).collect();
        let e = &mut self.manga.entries[i];
        if let [op] = open.as_slice()
            && op.state != OpState::Sending
            && op.old == after
        {
            e.waiting = None;
        } else if e.waiting.is_none() {
            e.waiting = Some(waiting_label(OpState::Queued));
        }
        crate::queue::send(self, crate::queue::Cmd::Edit { kind: Kind::Manga, media_id: id, change });
        self.refresh_manga();
        Ok(i)
    }

    /// The row's `+1`: one more chapter (or volume, when only volumes show).
    pub fn manga_plus_one(&mut self, id: u32) -> Result<usize, String> {
        let e = self.manga.entries.iter().find(|e| e.id == id).ok_or("That manga is not on your list.")?;
        let u = e.plus_unit();
        if e.at_end(u) {
            return Err(format!("{} is at its last {}.", e.title, if u == Unit::Chapters { "chapter" } else { "volume" }));
        }
        let change = match u {
            Unit::Chapters => Change { progress: Some(e.chapters + 1), ..Default::default() },
            Unit::Volumes => Change { volumes: Some(e.volumes + 1), ..Default::default() },
        };
        self.manga_edit(id, change)
    }

    /// "Reread": a completed manga is read again from chapter 1 (it stays Completed on MAL).
    pub fn manga_start_reread(&mut self, id: u32) -> Result<usize, String> {
        let e = self.manga.entries.iter().find(|e| e.id == id).ok_or("That manga is not on your list.")?;
        if e.rereading {
            return Err(format!("{} is being reread already.", e.title));
        }
        let change = Change::start_rewatch(&e.values()).ok_or_else(|| format!("{} is not completed: only a completed manga can be reread.", e.title))?;
        self.manga.editor = None;
        self.manga_edit(id, change)
    }

    /// "Stop rereading": the reread ends without counting; the chapters go back to the last.
    pub fn manga_stop_reread(&mut self, id: u32) -> Result<usize, String> {
        let e = self.manga.entries.iter().find(|e| e.id == id).ok_or("That manga is not on your list.")?;
        if !e.rereading {
            return Err(format!("{} is not being reread.", e.title));
        }
        let chapters = e.chapters_total.unwrap_or(e.chapters);
        self.manga.editor = None;
        self.manga_edit(id, Change { rewatching: Some(false), progress: Some(chapters), ..Default::default() })
    }

    /// A row press: opens its editor, or closes it.
    pub fn manga_toggle_editor(&mut self, id: u32) {
        self.editor = None;
        if self.manga.editor.is_some_and(|ed| ed.id == id) {
            self.manga.editor = None;
        } else if let Some(e) = self.manga.entries.iter().find(|e| e.id == id) {
            self.manga.editor = Some(MEditor { id, status: e.status, chapters: e.chapters, volumes: e.volumes, score: e.score });
        }
    }

    fn manga_editor_entry(&self) -> Option<&MEntry> {
        let id = self.manga.editor?.id;
        self.manga.entries.iter().find(|e| e.id == id)
    }

    /// The editor's − / +: one chapter or volume, within the known count. The last chapter
    /// completes the entry, as on MAL's website; stepping back off it undoes that.
    pub fn manga_editor_step(&mut self, u: Unit, delta: i32) {
        let Some(e) = self.manga_editor_entry() else { return };
        let (ch_max, vol_max, was) = (e.chapters_total, e.volumes_total, e.status);
        let Some(ed) = &mut self.manga.editor else { return };
        match u {
            Unit::Chapters => {
                let p = ed.chapters.saturating_add_signed(delta);
                ed.chapters = ch_max.map_or(p, |n| p.min(n));
                if delta > 0 && ch_max.is_some_and(|n| ed.chapters == n) {
                    ed.status = Status::Completed;
                    ed.volumes = vol_max.unwrap_or(ed.volumes);
                } else if delta < 0 && ed.status == Status::Completed && was != Status::Completed && ch_max.is_some_and(|n| ed.chapters < n) {
                    ed.status = was;
                }
            }
            Unit::Volumes => {
                let v = ed.volumes.saturating_add_signed(delta);
                ed.volumes = vol_max.map_or(v, |n| v.min(n));
            }
        }
    }

    /// Completed fills both counts when they are known.
    pub fn manga_editor_status(&mut self, st: Status) {
        let Some(e) = self.manga_editor_entry() else { return };
        let (ch, vol) = (e.chapters_total, e.volumes_total);
        if let Some(ed) = &mut self.manga.editor {
            ed.status = st;
            if st == Status::Completed {
                ed.chapters = ch.unwrap_or(ed.chapters);
                ed.volumes = vol.unwrap_or(ed.volumes);
            }
        }
    }

    pub fn save_manga_editor(&mut self) -> Result<(), String> {
        let Some(ed) = self.manga.editor else { return Ok(()) };
        let Some(e) = self.manga.entries.iter().find(|e| e.id == ed.id) else {
            self.manga.editor = None;
            return Ok(());
        };
        let change = Change { status: Some(ed.status), progress: Some(ed.chapters), volumes: Some(ed.volumes), score: Some(ed.score), ..Default::default() }.without_same(&e.values());
        self.manga_edit(ed.id, change)?;
        self.manga.editor = None;
        Ok(())
    }

    /// After `ops` or the manga entries change: each manga row's waiting tag.
    pub fn manga_queue_changed(&mut self) {
        for e in &mut self.manga.entries {
            e.waiting = None;
        }
        for op in self.ops.iter().filter(|o| o.kind == Kind::Manga && o.state.is_open()) {
            if let Some(e) = self.manga.entries.iter_mut().find(|e| e.id == op.media_id)
                && (e.waiting.is_none() || op.state != OpState::Queued)
            {
                e.waiting = Some(waiting_label(op.state));
            }
        }
    }

    fn set_manga_items(&mut self, items: &[ListItem]) {
        let lang = self.prefs.titles;
        self.manga.entries = items.iter().map(|it| MEntry::from_item(it, lang)).collect();
        if self.manga.editor.is_some_and(|ed| !self.manga.entries.iter().any(|e| e.id == ed.id)) {
            self.manga.editor = None;
        }
        self.manga_queue_changed();
        self.refresh_manga();
    }

    pub fn apply_manga(&mut self, msg: MangaMsg) -> bool {
        match msg {
            MangaMsg::Cache { enabled, items, synced_at } => {
                self.manga.enabled = enabled;
                // A sync that already answered is newer.
                if self.manga.synced_at.is_none() {
                    self.set_manga_items(&items);
                    self.manga.synced_at = synced_at;
                }
                // The switch was left on Manga, but Manga was turned off since (`kot manga off`).
                if !enabled && self.manga.mode {
                    self.manga.mode = false;
                    if let Page::Manga(st) = self.page {
                        self.go(Page::List(st));
                    }
                }
            }
            MangaMsg::Synced { items, synced_at } => {
                self.set_manga_items(&items);
                self.manga.synced_at = Some(synced_at);
                self.manga.loading = false;
                self.manga.error = None;
            }
            MangaMsg::Failed(m) => {
                self.manga.loading = false;
                self.manga.error = Some(m);
            }
            MangaMsg::Items { items, gone } => {
                let lang = self.prefs.titles;
                for it in &items {
                    let e = MEntry::from_item(it, lang);
                    match self.manga.entries.iter_mut().find(|x| x.id == e.id) {
                        Some(x) => *x = e,
                        None => self.manga.entries.push(e),
                    }
                }
                self.manga.entries.retain(|e| !gone.contains(&e.id));
                if self.manga.editor.is_some_and(|ed| gone.contains(&ed.id)) {
                    self.manga.editor = None;
                }
                self.manga_queue_changed();
                self.refresh_manga();
            }
            MangaMsg::Enabled(Ok(())) => self.sync_manga(),
            MangaMsg::Enabled(Err(m)) => {
                self.manga.enabled = false;
                self.manga.loading = false;
                self.manga.error = Some(m);
            }
        }
        true
    }

    /// The title of a queued manga change: the entry's, in the chosen language.
    pub fn manga_title(&self, id: u32) -> Option<&str> {
        self.manga.entries.iter().find(|e| e.id == id).map(|e| e.title.as_str())
    }
}

/// How long the removal's toast with Undo stays (as the anime one).
const UNDO_SECS: f64 = 8.0;

impl State {
    /// The view of manga list page `st` (`[manga_views]`).
    pub fn manga_view_of(&self, st: Status) -> kotora_core::config::ListView {
        self.prefs.manga_views.get(st)
    }

    /// The toolbar's switch on a manga page: its view changes and `config.toml` keeps it.
    pub fn set_manga_view(&mut self, st: Status, v: kotora_core::config::ListView) {
        if self.manga_view_of(st) == v {
            return;
        }
        self.prefs.manga_views.set(st, v);
        crate::detect::save_setting(self, "manga_views", kotora_core::config::Views::key(st), kotora_core::config::quote(v.key()));
    }

    /// Search, the details page, `kot add --manga`: puts a manga on the list with `status` (it
    /// shows at once, waiting to sync) and queues the add. One already on the list gets the
    /// status instead. Completed starts at the last chapter and volume when they are known.
    pub fn manga_add(&mut self, mut media: kotora_core::Media, status: Status) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        media.kind = Kind::Manga;
        if let Some(i) = self.manga.entries.iter().position(|e| e.id == media.id) {
            self.manga_edit(media.id, Change { status: Some(status), ..Default::default() })?;
            return Ok(i);
        }
        let done = status == Status::Completed;
        let item = ListItem {
            progress: if done { media.episodes.unwrap_or(0) } else { 0 },
            volumes: if done { media.volumes.unwrap_or(0) } else { 0 },
            media: media.clone(),
            status,
            score: None,
            rewatching: false,
            times_rewatched: 0,
            updated_at: self.clock(),
        };
        let mut e = MEntry::from_item(&item, self.prefs.titles);
        e.waiting = Some(waiting_label(OpState::Queued));
        self.manga.entries.push(e);
        crate::queue::send(self, crate::queue::Cmd::Add { media: Box::new(media), status, progress: None });
        self.refresh_manga();
        Ok(self.manga.entries.len() - 1)
    }

    /// Asks before removing a manga (the bar on top of the page).
    pub fn ask_remove_manga(&mut self, id: u32) {
        self.ctx = None;
        if self.manga.entries.iter().any(|e| e.id == id) {
            self.confirm_remove = None;
            self.manga.confirm_remove = Some(id);
        }
    }

    /// The question the bar and `kot remove --manga` ask (the mirror is anime only).
    pub fn manga_remove_question(&self, id: u32) -> Option<String> {
        let e = self.manga.entries.iter().find(|e| e.id == id)?;
        Some(format!("Remove {} from your manga list? It goes from your {} list too, with its chapters, volumes and score.", e.title, self.service.title()))
    }

    /// Removes a manga now: off the list at once, its removal queued (a conflict if it changed
    /// on the service since), a toast with Undo.
    pub fn remove_manga(&mut self, id: u32) -> Result<(), String> {
        self.manga.confirm_remove = None;
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let i = self.manga.entries.iter().position(|e| e.id == id).ok_or("That manga is not on your list.")?;
        let e = self.manga.entries.remove(i);
        if self.manga.editor.is_some_and(|ed| ed.id == id) {
            self.manga.editor = None;
        }
        crate::queue::send(self, crate::queue::Cmd::Remove { kind: Kind::Manga, media_id: id });
        self.refresh_manga();
        let text = format!("Removed {} from your manga list", e.title);
        self.toast = Some(crate::library::Toast {
            text,
            warn: false,
            until: std::time::Instant::now() + std::time::Duration::from_secs_f64(UNDO_SECS),
            undo: Some(crate::library::Undo::MangaRemove(Box::new(e))),
        });
        Ok(())
    }

    /// Undo of a manga's removal: back at once; the worker discards the waiting removal (if it
    /// already left, the queue says so and the entry goes again).
    pub fn undo_remove_manga(&mut self, e: MEntry) {
        let id = e.id;
        if self.manga.entries.iter().any(|x| x.id == id) {
            return;
        }
        self.manga.entries.push(e);
        crate::queue::send(self, crate::queue::Cmd::UndoRemove { kind: Kind::Manga, media_id: id });
        self.manga_queue_changed();
        self.refresh_manga();
    }

    /// A change from the manga's right-click menu: made, then a toast with Undo.
    pub fn manga_ctx_edit(&mut self, id: u32, change: Change, done: String) {
        self.ctx = None;
        let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else { return };
        let old = e.values();
        if change.without_same(&old).is_empty() {
            return;
        }
        if let Err(m) = self.manga_edit(id, change) {
            self.error = Some(m);
            return;
        }
        let back = Change { status: Some(old.status), progress: Some(old.progress), score: Some(old.score), rewatching: Some(old.rewatching), times_rewatched: Some(old.times_rewatched), volumes: Some(old.volumes) };
        self.toast = Some(crate::library::Toast { text: done, warn: false, until: std::time::Instant::now() + std::time::Duration::from_secs_f64(6.0), undo: Some(crate::library::Undo::MangaEdit(id, back)) });
    }

    /// The menu's Move to: Completed fills the known counts (`manga_edit`).
    pub fn manga_ctx_move(&mut self, id: u32, st: Status) {
        let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else { return };
        let done = format!("Moved {} to {}", e.title, st.label_in(Kind::Manga));
        self.manga_ctx_edit(id, Change { status: Some(st), ..Default::default() }, done);
    }

    pub fn manga_ctx_score(&mut self, id: u32, score: Option<Score>) {
        let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else { return };
        let done = match score {
            Some(n) => format!("Scored {} {}", e.title, self.score_format().words(n)),
            None => format!("Removed the score of {}", e.title),
        };
        self.manga_ctx_edit(id, Change { score: Some(score), ..Default::default() }, done);
    }

    /// The menu's +1 (a chapter, or a volume when only volumes show).
    pub fn manga_ctx_plus(&mut self, id: u32) {
        let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else { return };
        let (u, title) = (e.plus_unit(), e.title.clone());
        let change = match u {
            Unit::Chapters => Change { progress: Some(e.chapters + 1), ..Default::default() },
            Unit::Volumes => Change { volumes: Some(e.volumes + 1), ..Default::default() },
        };
        let done = match u {
            Unit::Chapters => format!("Chapter {} of {title} recorded", e.chapters + 1),
            Unit::Volumes => format!("Volume {} of {title} recorded", e.volumes + 1),
        };
        self.manga_ctx_edit(id, change, done);
    }

    /// Edit… from the menu: the table's own editor, else the details page with its editor.
    pub fn manga_ctx_edit_entry(&mut self, id: u32) {
        self.ctx = None;
        let table = matches!(self.page, Page::Manga(st) if self.manga_view_of(st) == kotora_core::config::ListView::List);
        if table && self.manga.rows.iter().any(|&k| self.manga.entries[k].id == id) {
            if self.manga.editor.is_none_or(|ed| ed.id != id) {
                self.manga_toggle_editor(id);
            }
            return;
        }
        let Some(e) = self.manga.entries.iter().find(|e| e.id == id) else { return };
        let stub = stub(e);
        self.open_details(stub);
        self.manga_toggle_editor(id);
    }

    /// The entry as `kot` prints it (chapters, volumes).
    pub fn manga_ctl(&self, i: usize) -> kotora_ctl::Entry {
        let e = &self.manga.entries[i];
        kotora_ctl::Entry {
            id: e.id,
            title: e.title.clone(),
            status: e.status.label_in(Kind::Manga).into(),
            progress: e.chapters,
            episodes: e.chapters_total,
            score: e.score.map(Score::to_ten),
            score_text: e.score.map(|n| self.score_format().show(n)),
            waiting: e.waiting.map(Into::into),
            rewatching: e.rereading,
            times_rewatched: e.times_reread,
            volumes: Some(e.volumes),
            volumes_total: e.volumes_total,
        }
    }

    /// The manga `q` names: its id, a whole title, or a part of one that matches one manga.
    pub fn find_manga(&self, q: &str) -> Result<usize, String> {
        if !self.has_list() {
            return Err("Not signed in.".into());
        }
        let q = q.trim();
        if let Some(i) = q.parse::<u32>().ok().and_then(|id| self.manga.entries.iter().position(|e| e.id == id)) {
            return Ok(i);
        }
        let lower = q.to_lowercase();
        let es = &self.manga.entries;
        if let Some(i) = es.iter().position(|e| e.search.split('\n').any(|t| t == lower)) {
            return Ok(i);
        }
        let hits: Vec<usize> = (0..es.len()).filter(|&i| es[i].search.contains(&lower)).collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!("No manga on your list matches \u{201c}{q}\u{201d}.")),
            many => {
                let names: Vec<String> = many.iter().take(6).map(|&i| format!("  {:>6}  {}", es[i].id, es[i].title)).collect();
                Err(format!("Several manga match \u{201c}{q}\u{201d}; give more of the title or the id:\n{}", names.join("\n")))
            }
        }
    }
}

/// The manga half of a sync as a message for the UI (`None`: Manga is off).
pub fn manga_msg(r: kotora_core::sync::MangaSync, svc: &str) -> Option<MangaMsg> {
    if let Some(m) = fail_text(&r, svc) {
        return Some(MangaMsg::Failed(m));
    }
    match r {
        kotora_core::sync::MangaSync::Synced { items, synced_at } => Some(MangaMsg::Synced { items, synced_at }),
        _ => None,
    }
}

// --------------------------------------------------------------------------------------- view

/// The sidebar's Anime | Manga switch, under the account chip (MyAnimeList and AniList).
pub fn switch(s: &State) -> El {
    let seg = |kind: Kind, on: bool| {
        let fg = if on { PAL.text } else { PAL.dim };
        let mut c = el()
            .key(format!("kind/{}", kind.name()))
            .role("button")
            .label(kind.label())
            .checked(on)
            .grow(1.0)
            .h(26.0)
            .radius(theme::R_SM)
            .center()
            .child(text(kind.label()).font(if on { theme::BODY_MED } else { theme::BODY }).color(fg));
        if on {
            c = c.fill(PAL.raised).shadow(1.0, 2.0, rune::Color::BLACK.alpha(if PAL.light { 0.10 } else { 0.35 }));
        }
        if !on {
            c = c.focusable(true).hover(PAL.hover()).press(PAL.press()).cursor(rune::Cursor::Pointer).on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.set_kind(kind);
            });
        }
        c
    };
    let manga = s.manga.mode;
    col().key("kind-switch").mb(12.0).gap(4.0).child(
        row()
            .key("kind-seg")
            .w_full()
            .padding(2.0)
            .gap(2.0)
            .radius(theme::R_MD)
            .fill(PAL.field)
            .border(1.0, PAL.border_strong)
            .child(seg(Kind::Anime, !manga))
            .child(seg(Kind::Manga, manga)),
    )
}

fn filter_box(s: &State) -> El {
    let k = if cfg!(target_os = "macos") { "\u{2318} F" } else { "Ctrl F" };
    row()
        .key("search")
        .w(260.0)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border)
        .child(icon(&icons::SEARCH, 16.0).color(PAL.faint))
        .child(
            text_input(&s.manga.query, theme::BODY)
                .key("manga-filter-input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(crate::ui::field_hint(&s.manga.query, "Filter your manga"), PAL.faint)
                .on_edit(|s: &mut State| &mut s.manga.query)
                .on_input_key(crate::ui::field_page_key)
                .on_input_change(|s: &mut State, tx: &mut Tx| {
                    tx.instant();
                    s.refresh_manga();
                })
                .on_submit(|_s: &mut State, tx: &mut Tx| tx.blur()),
        )
        .child(kbd(k))
}

/// The alert over a manga page: sign in again, the manga sync's failure, else the anime one.
fn page_alert(s: &State) -> Option<El> {
    match &s.manga.error {
        Some(m) if s.auth != crate::model::Auth::Expired => {
            let mut msg = m.clone();
            if let Some(at) = s.manga.synced_at {
                msg = format!("{msg} Showing your saved manga list (synced {}).", crate::ui::sync_age(s.clock() - at));
            }
            Some(alert("alert/manga-error", PAL.status(Status::Dropped), msg, Some(button("alert/manga-retry", "Retry", false, Some(State::sync_manga)))))
        }
        _ => crate::ui::status_alert(s),
    }
}

/// A manga list page: top bar, chips, then the rows (each opens its editor).
pub fn page(s: &State, st: Status, wide: bool, width: f32) -> El {
    let label = st.label_in(Kind::Manga);
    let searching = !s.manga.query.text.trim().is_empty();
    let count = s.manga_count(st);
    let rereads = if st == Status::Watching { s.manga_reread_count() } else { 0 };
    let first = s.manga.loading && s.manga.synced_at.is_none();
    let title = if searching { "Your manga" } else { label };
    let shown = if searching { s.manga.rows.len() } else { count };
    let extra = (rereads > 0 && !first && !searching).then(|| format!("+ {rereads} rereading"));
    let mut head = crate::ui::top_with(s, title, (!first).then_some(shown), extra, false).child(spacer()).child(filter_box(s)).child(crate::ui::sort_select(s));
    if s.auth == crate::model::Auth::SignedIn {
        head = head.child(crate::ui::refresh_button(s.syncing || s.manga.loading));
    }
    // The view switch ends the top bar; a narrow window has no room there, so it ends the chips.
    if !first && wide {
        head = head.child(crate::grid::switcher(s, st, true));
    }
    let mut page = col().key(format!("page/manga-{}", st.slug())).w_full().h_full().child(head);
    if let Some(a) = page_alert(s) {
        page = page.child(a);
    }
    if first {
        let mut c = col().key("loading").px(theme::PAGE_PAD).pt(4.0).gap(8.0).child(text(format!("Reading your manga list from {}\u{2026}", s.service.title())).font(theme::LABEL).color(PAL.faint).pb(2.0));
        for i in 0..4 {
            c = c.child(el().key(format!("skel/{i}")).w_full().h(44.0).radius(theme::R_MD).fill(PAL.raised));
        }
        return page.child(c);
    }
    if count + rereads == 0 && !searching {
        let help = match st {
            Status::Watching => "Manga you're reading",
            Status::PlanToWatch => "Manga you plan to read",
            Status::Completed => "Manga you finish",
            Status::OnHold => "Manga you put on hold",
            Status::Dropped => "Manga you drop",
        };
        let line = if s.manga.synced_at.is_none() && s.manga.error.is_none() { "Your manga list wasn't read yet.".to_string() } else { format!("{help} on {} show up here.", s.service.title()) };
        return page.child(
            col()
                .key("empty")
                .grow(1.0)
                .min_h(0.0)
                .center()
                .gap(4.0)
                .pb(60.0)
                .child(text(format!("No manga in {label}")).font(theme::SECTION).color(PAL.text))
                .child(text(line).font(theme::LABEL).color(PAL.dim)),
        );
    }
    if !searching {
        page = page.child(if wide { chips(s, st) } else { chips(s, st).child(spacer()).child(crate::grid::switcher(s, st, false).mt(-3.0)) });
    }
    if s.manga.rows.is_empty() {
        let q = s.manga.query.text.trim();
        let line = if q.is_empty() { "No manga match this filter.".to_string() } else { format!("Nothing matches \u{201c}{q}\u{201d}.") };
        return page.child(col().key("no-match").grow(1.0).min_h(0.0).center().pb(60.0).child(text(line).font(theme::SECTION).color(PAL.text)));
    }
    if !searching && s.manga_view_of(st) != kotora_core::config::ListView::List {
        return page.child(crate::mgrid::cards(s, st, width));
    }
    let n = s.manga.rows.len();
    page.child(header(wide, s.sort)).child(
        col().key(format!("manga-rows-scroll/{}", st.slug())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0).child(
            col().key("rows").growth(GrowthSpec::default()).virtual_rows(
                n,
                theme::ROW_H,
                move |s: &State, i| s.manga.rows.get(i).map_or_else(|| format!("gone-{i}").into(), |&k| format!("m{}", s.manga.entries[k].id).into()),
                move |s: &State, i| match s.manga.rows.get(i) {
                    Some(&k) => list_row(s, &s.manga.entries[k], wide),
                    None => el(),
                },
            ),
        ),
    )
}

fn chips(s: &State, st: Status) -> El {
    let mut r = row().key("chips").gap(6.0).px(theme::PAGE_PAD).pb(10.0);
    for f in MFilter::ALL {
        let n = s.manga.entries.iter().filter(|e| e.on_page(st) && f.keeps(e)).count();
        if n == 0 && f != MFilter::All {
            continue;
        }
        let on = s.manga.filter == f;
        let chip = row()
            .key(format!("chip/{}", f.label()))
            .role("button")
            .label(f.label())
            .focusable(true)
            .h(26.0)
            .px(10.0)
            .center_y()
            .radius(13.0)
            .hover(PAL.hover())
            .press(PAL.press())
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.set_manga_filter(f);
            })
            .child(text(format!("{} {n}", f.label())).font(theme::LABEL_NUM).color(if on { PAL.accent_hi } else { PAL.dim }));
        r = r.child(if on { chip.fill(PAL.accent.alpha(0.14)) } else { chip.border(1.0, PAL.border) });
    }
    r
}

pub fn header(wide: bool, sort: crate::model::Sort) -> El {
    use crate::model::Sort;
    let h = |t: &str, sorted: bool| text(t.to_uppercase()).font(theme::CAPTION).color(if sorted { PAL.dim } else { PAL.faint });
    col()
        .key("thead")
        .w_full()
        .child(
            columns(
                el().w(34.0),
                h("Title", sort == Sort::Title),
                h("Chapters \u{b7} Volumes", sort == Sort::Progress),
                h("Score", sort == Sort::Score),
                h("Type", false).key("thead/type"),
                row().gap(4.0).center_y().child(h("Updated", sort == Sort::Updated)).child(icon(&icons::DOWN, 10.0).color(PAL.dim)),
                wide,
            )
            .h(30.0)
            .px(theme::PAGE_PAD),
        )
        .child(el().w_full().h(1.0).fill(PAL.border))
}

/// "119 / 142" with a unit word before it ("Ch", "Vol").
fn count_text(word: &str, n: u32, total: Option<u32>, big: bool) -> El {
    let total = total.map_or("\u{2013}".to_string(), |t| t.to_string());
    let (num, dim) = if big { (theme::NUM_MED, theme::NUM) } else { (rune::TextStyle::new(12.0, 500.0).features("'tnum' 1"), theme::LABEL_NUM) };
    row()
        .justify_end()
        .center_y()
        .child(text(format!("{word} ")).font(theme::LABEL).color(PAL.faint))
        .child(text(n.to_string()).font(num).color(PAL.text))
        .child(text(format!(" / {total}")).font(dim).color(PAL.dim))
}

/// The progress cell: the bar, the counts shown ([`shows`]: one line, or chapters over
/// volumes), and `+1`.
pub fn progress(s: &State, e: &MEntry, id: u32) -> El {
    let (ch, vol) = e.shows();
    let c = PAL.status(if e.rereading { Status::Completed } else { e.status });
    let nums = match (ch, vol) {
        (true, true) => col().gap(1.0).items_end().child(count_text("Ch", e.chapters, e.chapters_total, false)).child(count_text("Vol", e.volumes, e.volumes_total, false)),
        (false, true) => col().items_end().child(count_text("Vol", e.volumes, e.volumes_total, true)),
        _ => col().items_end().child(count_text("Ch", e.chapters, e.chapters_total, true)),
    };
    let plus = s.has_list() && !e.at_end(e.plus_unit());
    row()
        .gap(10.0)
        .center_y()
        .child(row().grow(1.0).h(4.0).radius(2.0).clip_rounded(2.0).fill(PAL.border).child(el().grow(e.fraction()).h_full().radius(2.0).fill(c)).child(el().grow(1.0 - e.fraction())))
        .child(nums.w(96.0))
        .child(if plus { plus_button(id, e.plus_unit()) } else { el().size(38.0, 26.0) })
}

fn plus_button(id: u32, u: Unit) -> El {
    el().key("plus")
        .role("button")
        .label(if u == Unit::Chapters { "One more chapter" } else { "One more volume" })
        .focusable(true)
        .size(38.0, 26.0)
        .radius(theme::R_SM)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if let Err(m) = s.manga_plus_one(id) {
                s.error = Some(m);
            }
        })
        .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent))
}

/// A row's cover: the tile, with the real cover over it once it is in memory.
pub fn row_cover(s: &State, e: &MEntry) -> El {
    let (w, h) = (34.0, 48.0);
    let id = cover_id(e.id);
    let tile = crate::ui::cover(id, &e.title, w, h, 12.0);
    match s.covers.get(id, e.cover_url.as_deref(), w, h) {
        Some(img) => tile.child(el().key("cover-img").absolute(0.0, 0.0).size(w, h).radius(theme::R_XS).pixels(img.pixels())),
        None => tile,
    }
}

/// The details page's first look at a manga of the list.
pub fn stub(e: &MEntry) -> crate::details::Stub {
    crate::details::Stub { id: e.id, title: e.title.clone(), cover_url: e.cover_url.clone(), format: String::new(), episodes: e.chapters_total, kind: Kind::Manga }
}

pub fn list_row(s: &State, e: &MEntry, wide: bool) -> El {
    let id = e.id;
    let mut sub = row().w_full().min_w(0.0).gap(8.0).center_y().clip_fade(0.0, 0.0, 16.0, 0.0);
    if !s.manga.query.text.trim().is_empty() || s.page == Page::Search {
        sub = sub.child(tag(e.status.label_in(Kind::Manga), PAL.status(e.status), true));
    }
    match e.run {
        Run::Publishing => sub = sub.child(tag("Publishing", PAL.airing, true)),
        Run::Hiatus => sub = sub.child(tag("On hiatus", PAL.st_dropped, false)),
        _ => {}
    }
    if e.rereading {
        sub = sub.child(tag("Rereading", PAL.st_completed, false));
    }
    if let Some(w) = e.waiting {
        sub = sub.child(tag(w, PAL.warn(), false));
    }
    if !e.note().is_empty() {
        sub = sub.child(text(e.note()).font(theme::LABEL).color(PAL.faint));
    }
    let name = text(e.title.clone()).font(theme::ROW_TITLE).color(PAL.text).group_color(PAL.accent_hi).truncate();
    let title = col().gap(2.0).child(crate::details::title_link(name, stub(e))).child(sub);
    let score = match e.score {
        Some(n) => crate::scores::mark(s.score_format(), n, theme::NUM_MED, PAL.text),
        None => text("\u{2013}").font(theme::BODY).color(PAL.faint),
    };
    let open = s.manga.editor.filter(|ed| ed.id == id);
    let line = columns(
        row_cover(s, e),
        title,
        progress(s, e, id),
        score,
        text(e.format.clone()).font(theme::LABEL).color(PAL.dim),
        text(kotora_core::time::age(e.updated_at, s.clock())).font(theme::LABEL).color(PAL.dim),
        wide,
    )
    .key("line")
    .h(theme::ROW_H)
    .px(theme::PAGE_PAD)
    .role("button")
    .label(format!("Edit {}", e.title))
    .hover(PAL.hover())
    // Right-click (Ctrl-click on macOS): the manga's menu at the pointer.
    .on_secondary_press(move |s: &mut State, p: rune::PointerPress, tx: &mut Tx| {
        tx.instant();
        s.open_manga_ctx(id, crate::ctxmenu::At::Point(p.x, p.y));
    })
    .on_press_tx(move |s: &mut State, tx: &mut Tx| {
        tx.instant();
        s.manga_toggle_editor(id);
    });
    let mut c = col().w_full().child(if open.is_some() { line.fill(PAL.surface) } else { line });
    if let Some(ed) = open {
        c = c.child(editor(e, &ed, false, s.score_format()));
    }
    c.child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
}

fn step_button(key: String, u: Unit, delta: i32) -> El {
    let what = if u == Unit::Chapters { "chapter" } else { "volume" };
    el().key(key)
        .role("button")
        .label(if delta < 0 { format!("One {what} less") } else { format!("One {what} more") })
        .focusable(true)
        .size(30.0, 30.0)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.manga_editor_step(u, delta);
        })
        .child(text(if delta < 0 { "\u{2212}" } else { "+" }).font(rune::TextStyle::new(15.0, 500.0)).color(PAL.dim))
}

fn stepper(u: Unit, n: u32, total: Option<u32>) -> El {
    let slug = if u == Unit::Chapters { "chapters" } else { "volumes" };
    let total = total.map_or("\u{2013}".to_string(), |t| t.to_string());
    row()
        .key(format!("med/{slug}"))
        .h(32.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(step_button(format!("med/{slug}/minus"), u, -1))
        .child(row().min_w(76.0).center().child(text(n.to_string()).font(theme::NUM_MED).color(PAL.text)).child(text(format!(" / {total}")).font(theme::NUM).color(PAL.dim)))
        .child(step_button(format!("med/{slug}/plus"), u, 1))
}

/// "Reread" for a completed manga, "Stop rereading" during one: at once, like `+1`.
fn reread_cell(e: &MEntry) -> El {
    let id = e.id;
    if e.rereading {
        return cell("med/stop-reread".into(), "Stop rereading".into(), false, move |s| {
            if let Err(m) = s.manga_stop_reread(id) {
                s.error = Some(m);
            }
        })
        .ml(10.0)
        .child(text("Stop rereading").font(theme::LABEL).color(PAL.dim));
    }
    if e.status != Status::Completed {
        return el();
    }
    cell("med/reread".into(), "Reread from chapter 1".into(), false, move |s| {
        if let Err(m) = s.manga_start_reread(id) {
            s.error = Some(m);
        }
    })
    .ml(10.0)
    .child(icon(&icons::SYNC, 13.0).color(PAL.st_completed))
    .child(text("Reread").font(theme::LABEL).color(PAL.text))
}

/// The manga editor: status, chapters, volumes and score, then Cancel / Save (one change).
/// `compact`: the details page's narrow column.
pub fn editor(e: &MEntry, ed: &MEditor, compact: bool, fmt: ScoreFormat) -> El {
    let mut statuses = row().flex_wrap().gap(6.0);
    for st in Status::ALL {
        let on = ed.status == st;
        let label = st.label_in(Kind::Manga);
        statuses = statuses.child(
            cell(format!("med/status/{}", st.slug()), label.into(), on, move |s| s.manga_editor_status(st))
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                .child(text(label).font(theme::LABEL).color(if on { PAL.text } else { PAL.dim })),
        );
    }
    if !compact {
        statuses = statuses.child(reread_cell(e));
    }
    let scores = crate::scores::picker(fmt, ed.score);
    let buttons = row()
        .grow(1.0)
        .gap(8.0)
        .justify_end()
        .child(button("med/cancel", "Cancel", false, Some(|s: &mut State| s.manga.editor = None)))
        .child(button(
            "med/save",
            "Save",
            true,
            Some(|s: &mut State| {
                if let Err(m) = s.save_manga_editor() {
                    s.error = Some(m);
                }
            }),
        ));
    let ch = field("Chapters", stepper(Unit::Chapters, ed.chapters, e.chapters_total));
    let vol = field("Volumes", stepper(Unit::Volumes, ed.volumes, e.volumes_total));
    if compact {
        return col().key("editor").gap(12.0).pt(12.0).child(field("Status", statuses)).child(row().flex_wrap().gap(12.0).child(ch).child(vol)).child(field("Score", scores)).child(buttons.pt(4.0));
    }
    col()
        .key("editor")
        .mx(theme::PAGE_PAD)
        .mb(10.0)
        .py(12.0)
        .pl(62.0)
        .pr(14.0)
        .gap(14.0)
        .radius(theme::R_MD)
        .fill(PAL.surface)
        .border(1.0, PAL.border_strong)
        .child(field("Status", statuses))
        .child(row().flex_wrap().gap(24.0).items_end().child(ch).child(vol).child(field("Score", scores)).child(buttons))
}

/// The details page's "Your list" card for a manga: status and tags, chapters and volumes
/// (bars, counts, `+1` each), score, dates, rereads; Edit opens the editor in the card.
pub fn mine_card(s: &State, d: &crate::details::Detail, mut c: El) -> El {
    let id = d.id();
    let Some(e) = s.manga.entries.iter().find(|e| e.id == id) else {
        // Not on the list: add it (Reading first, the other statuses under it).
        let mut more = row().flex_wrap().gap(6.0).pt(8.0);
        for st in [Status::PlanToWatch, Status::Completed, Status::OnHold, Status::Dropped] {
            let label = st.label_in(Kind::Manga);
            more = more.child(
                cell(format!("det/add/{}", st.slug()), format!("Add as {label}"), false, move |s| s.add_shown(st))
                    .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                    .child(text(label).font(theme::LABEL).color(PAL.dim)),
            );
        }
        return c
            .child(text("Not on your manga list.").font(theme::BODY).color(PAL.dim).pt(6.0))
            .child(row().key("det/add").pt(10.0).gap(8.0).child(button("det/add-watching", "Add as Reading", true, Some(|s: &mut State| s.add_shown(Status::Watching)))))
            .child(more)
            .child(text(format!("It goes to the sync queue first, then to {}.", s.service.title())).font(theme::LABEL).color(PAL.faint).wrap().pt(10.0));
    };
    let mut head = row().gap(8.0).center_y().pt(6.0).child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(e.status))).child(text(e.status.label_in(Kind::Manga)).font(theme::BODY_MED).color(PAL.text));
    if e.rereading {
        head = head.child(tag("Rereading", PAL.st_completed, false));
    }
    if let Some(w) = e.waiting {
        head = head.child(tag(w, PAL.warn(), false));
    }
    c = c.child(head);
    let line = |key: &str, word: &str, n: u32, total: Option<u32>, u: Unit| {
        let f = match total {
            Some(t) if t > 0 => (n as f32 / t as f32).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let total_t = total.map_or("\u{2013}".to_string(), |t| t.to_string());
        let end = total.is_some_and(|t| n >= t);
        row()
            .key(key.to_string())
            .gap(10.0)
            .pt(10.0)
            .center_y()
            .child(text(word.to_string()).font(theme::LABEL).color(PAL.faint).w(62.0))
            .child(row().grow(1.0).h(4.0).radius(2.0).clip_rounded(2.0).fill(PAL.text.alpha(0.14)).child(el().grow(f).h_full().radius(2.0).fill(PAL.status(e.status))).child(el().grow(1.0 - f)))
            .child(row().child(text(n.to_string()).font(theme::NUM_MED).color(PAL.text)).child(text(format!(" / {total_t}")).font(theme::NUM).color(PAL.dim)))
            .child(if end { el().size(38.0, 26.0) } else { plus_one_of(id, u) })
    };
    // The counts read ([`shows`]); Edit has both.
    let (ch, vol) = e.shows();
    if ch {
        c = c.child(line("det/chapters", "Chapters", e.chapters, e.chapters_total, Unit::Chapters));
    }
    if vol {
        c = c.child(line("det/volumes", "Volumes", e.volumes, e.volumes_total, Unit::Volumes));
    }
    let mine = d.info.as_ref().and_then(|i| i.mine.as_ref());
    let date = |v: Option<&String>| text(v.map_or("\u{2013}".into(), |d| crate::details::date_text(d))).font(theme::LABEL_NUM).color(PAL.text);
    let score = match e.score {
        Some(n) => crate::scores::mark(s.score_format(), n, theme::LABEL_NUM, PAL.text),
        None => text("No score").font(theme::LABEL_NUM).color(PAL.text),
    };
    let mut facts = col().pt(8.0).child(crate::ui::kv("Score", score.key("det/my-score")));
    facts = facts.child(crate::ui::kv("Started", date(mine.and_then(|m| m.start_date.as_ref())))).child(crate::ui::kv("Finished", date(mine.and_then(|m| m.finish_date.as_ref()))));
    if e.status == Status::Completed || e.times_reread > 0 {
        let times = if e.times_reread == 0 { "Not yet".to_string() } else { kotora_core::queue::times(e.times_reread) };
        facts = facts.child(crate::ui::kv("Reread", text(times).key("det/reread").font(theme::LABEL_NUM).color(PAL.text)));
    }
    c = c.child(facts);
    match s.manga.editor.filter(|ed| ed.id == id) {
        Some(ed) => c.child(editor(e, &ed, true, s.score_format())),
        None => {
            let mut actions = row().pt(10.0).gap(8.0).child(crate::ui::action("det/edit".into(), "Edit", false, move |s, _| s.manga_toggle_editor(id)).h(32.0).px(12.0));
            if e.status == Status::Completed && !e.rereading {
                actions = actions.child(crate::ui::action("det/reread".into(), "Reread", false, move |s, _| {
                    if let Err(m) = s.manga_start_reread(id) {
                        s.error = Some(m);
                    }
                }).h(32.0).px(12.0));
            } else if e.rereading {
                actions = actions.child(ghost("det/stop-reread", "Stop rereading", move |s| {
                    if let Err(m) = s.manga_stop_reread(id) {
                        s.error = Some(m);
                    }
                }));
            }
            actions = actions.child(spacer()).child(ghost("det/remove", "Remove from list\u{2026}", move |s| s.ask_remove_manga(id)));
            c.child(actions)
        }
    }
}

/// `+1` on the details card, for one unit.
fn plus_one_of(id: u32, u: Unit) -> El {
    el().key(if u == Unit::Chapters { "det/plus-chapter" } else { "det/plus-volume" })
        .role("button")
        .label(if u == Unit::Chapters { "One more chapter" } else { "One more volume" })
        .focusable(true)
        .size(38.0, 26.0)
        .radius(theme::R_SM)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            let Some(e) = s.manga.entries.iter().find(|e| e.id == id) else { return };
            let change = match u {
                Unit::Chapters => Change { progress: Some(e.chapters + 1), ..Default::default() },
                Unit::Volumes => Change { volumes: Some(e.volumes + 1), ..Default::default() },
            };
            if let Err(m) = s.manga_edit(id, change) {
                s.error = Some(m);
            }
        })
        .child(text("+1").font(theme::LABEL_BTN).color(PAL.dim).group_color(PAL.on_accent))
}

// ------------------------------------------------------------------------------------- fakes

/// The made-up manga of the scenes (the mockup's titles; no real manga).
pub fn fake_items() -> Vec<ListItem> {
    use kotora_core::Media;
    use kotora_core::score::ten;
    #[allow(clippy::type_complexity)]
    let rows: [(u32, &str, &str, &str, Status, u32, Option<u32>, u32, Option<u32>, Option<u8>, bool, i64); 12] = [
        (1, "Inkwell Cartographers", "manga", "currently_publishing", Status::Watching, 119, None, 11, None, Some(9), false, 0),
        (2, "Salt & Gears", "manhwa", "currently_publishing", Status::Watching, 64, Some(88), 0, None, Some(8), false, 3 * 3600),
        (3, "Lighthouse Tea Club", "manga", "finished", Status::Completed, 12, Some(40), 2, Some(5), Some(10), true, 86_400),
        (4, "Moth Orchestra", "manga", "on_hiatus", Status::Watching, 201, None, 22, None, None, false, 4 * 86_400),
        (5, "Paper Wolves of Osaka", "manga", "currently_publishing", Status::Watching, 12, None, 0, None, Some(7), false, 7 * 86_400),
        (6, "Night Shift Alchemist", "light_novel", "finished", Status::Watching, 0, None, 3, Some(9), None, false, 14 * 86_400),
        (7, "Kumo Atelier", "one_shot", "finished", Status::Watching, 0, Some(1), 0, Some(1), None, false, 30 * 86_400),
        (8, "Harbor of Small Lights", "manga", "finished", Status::PlanToWatch, 0, Some(52), 0, Some(6), None, false, 40 * 86_400),
        (9, "Cobalt Postman", "manga", "finished", Status::Completed, 96, Some(96), 10, Some(10), Some(8), false, 60 * 86_400),
        (10, "The Quiet Foundry", "manhua", "finished", Status::Completed, 140, Some(140), 0, None, None, false, 90 * 86_400),
        (11, "Ferris Wheel Diaries", "manga", "currently_publishing", Status::OnHold, 33, None, 4, None, Some(6), false, 120 * 86_400),
        (12, "Rust Belt Saints", "manga", "discontinued", Status::Dropped, 18, Some(18), 2, Some(2), Some(4), false, 200 * 86_400),
    ];
    rows.into_iter()
        .map(|(id, title, format, run, status, ch, chs, vol, vols, score, reread, ago)| ListItem {
            media: Media { kind: Kind::Manga, id, title: title.into(), episodes: chs, volumes: vols, format: format.into(), air_status: run.into(), authors: vec!["Example Author".into()], ..Default::default() },
            status,
            progress: ch,
            score: score.and_then(|n| ten(u32::from(n))),
            rewatching: reread,
            times_rewatched: u32::from(reread),
            volumes: vol,
            updated_at: crate::model::FAKE_NOW - ago,
        })
        .collect()
}

/// Puts the fake manga list in `s` (scenes and tests): Manga on, the switch on Manga.
pub fn fake(s: &mut State) {
    s.manga.enabled = true;
    s.manga.mode = true;
    s.manga.synced_at = Some(crate::model::FAKE_NOW - 120);
    s.set_manga_items(&fake_items());
    s.go(Page::Manga(Status::Watching));
}

/// Made-up MyAnimeList manga results (scenes and tests): "Salt & Gears" is on the fake list.
pub fn fake_hits() -> Vec<kotora_core::service::AnimeHit> {
    let h = |id: u32, title: &str, en: Option<&str>, format: &str, ch: Option<u32>, vols: Option<u32>, run: &str, mean: Option<f32>| kotora_core::service::AnimeHit {
        kind: Kind::Manga,
        id,
        title: title.into(),
        title_en: en.map(Into::into),
        format: format.into(),
        episodes: ch,
        volumes: vols,
        air_status: run.into(),
        mean,
        ..Default::default()
    };
    vec![
        h(2, "Salt & Gears", None, "manhwa", Some(88), None, "currently_publishing", Some(8.02)),
        h(501, "Salt Road Couriers", Some("Couriers of the Salt Road"), "manga", Some(64), Some(8), "finished", Some(7.84)),
        h(502, "Salt and Lantern Anthology", None, "light_novel", None, Some(3), "currently_publishing", Some(7.21)),
        h(503, "Saltwater Cartography", None, "one_shot", Some(1), Some(1), "finished", None),
    ]
}

/// A made-up manga information page (scenes): the mockup's facts.
pub fn fake_details(id: u32, title: &str) -> kotora_core::service::Details {
    kotora_core::service::Details {
        kind: Kind::Manga,
        id,
        title: title.into(),
        title_en: Some("The Cartographers of Inkwell Bay".into()),
        title_ja: Some("インク湾の地図師".into()),
        synonyms: vec!["Inkwell".into(), "Ink Maps".into()],
        format: "manga".into(),
        episodes: None,
        volumes: None,
        air_status: "currently_publishing".into(),
        start_date: Some("2022-03".into()),
        genres: vec!["Adventure".into(), "Fantasy".into(), "Mystery".into()],
        mean: Some(8.61),
        rank: Some(98),
        popularity: Some(1204),
        members: Some(88_310),
        synopsis: "Two apprentices map a bay whose coastline changes every time someone reads the old charts. A made-up \
                   story written for Kotora's screenshots; no real manga is described here."
            .into(),
        authors: vec![kotora_core::service::Author { name: "Example Author".into(), role: "Story & Art".into() }],
        serialization: vec!["Example Monthly".into()],
        mine: Some(kotora_core::service::MyEntry {
            status: "reading".into(),
            progress: 119,
            score: kotora_core::score::ten(9),
            rewatching: false,
            times_rewatched: 0,
            start_date: Some("2025-01-08".into()),
            finish_date: None,
            volumes: 11,
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::State;

    fn loaded() -> State {
        let mut s = State::fake();
        fake(&mut s);
        s
    }

    fn entry(s: &State, id: u32) -> &MEntry {
        s.manga.entries.iter().find(|e| e.id == id).unwrap()
    }

    #[test]
    fn the_progress_shows_what_was_read() {
        assert_eq!(shows(0, 0), (true, false), "nothing read: chapters");
        assert_eq!(shows(12, 0), (true, false));
        assert_eq!(shows(0, 3), (false, true), "volumes only");
        assert_eq!(shows(12, 3), (true, true));
        let s = loaded();
        assert_eq!(entry(&s, 6).plus_unit(), Unit::Volumes, "a light novel read by volume steps volumes");
        assert_eq!(entry(&s, 1).plus_unit(), Unit::Chapters);
    }

    #[test]
    fn the_switch_swaps_the_list_pages() {
        let mut s = State::fake();
        assert!(!s.manga.mode && !s.manga.enabled, "off by default");
        s.go(Page::List(Status::Completed));
        s.set_kind(Kind::Manga);
        assert!(s.manga.mode && s.manga.enabled, "the first press turns Manga on");
        assert_eq!(s.page, Page::Manga(Status::Completed), "the same status, in manga");
        assert_eq!(s.list_page(Status::OnHold), Page::Manga(Status::OnHold));
        s.set_kind(Kind::Anime);
        assert_eq!(s.page, Page::List(Status::Completed));
        assert!(s.manga.enabled, "back to Anime keeps Manga on (kot manga off turns it off)");
        // AniList has manga too.
        let mut a = State::fake();
        a.service = kotora_core::service::ServiceId::AniList;
        a.set_kind(Kind::Manga);
        assert!(a.manga.mode && a.manga.enabled, "AniList: the switch works the same");
    }

    #[test]
    fn pages_chips_and_the_filter_box() {
        let mut s = loaded();
        let titles = |s: &State| s.manga.rows.iter().map(|&i| s.manga.entries[i].title.clone()).collect::<Vec<_>>();
        // Reading, newest first, with the reread (Completed) last by its date.
        assert_eq!(titles(&s), ["Inkwell Cartographers", "Salt & Gears", "Lighthouse Tea Club", "Moth Orchestra", "Paper Wolves of Osaka", "Night Shift Alchemist", "Kumo Atelier"]);
        assert_eq!(s.manga_count(Status::Watching), 6);
        assert_eq!(s.manga_reread_count(), 1);
        s.set_manga_filter(MFilter::Publishing);
        assert_eq!(titles(&s), ["Inkwell Cartographers", "Salt & Gears", "Paper Wolves of Osaka"]);
        s.go(Page::Manga(Status::Completed));
        assert_eq!(s.manga.filter, MFilter::All, "a new page starts on All");
        s.manga.query.replace_text("rust");
        s.refresh_manga();
        assert_eq!(titles(&s), ["Rust Belt Saints"], "the box looks at every status");
    }

    #[test]
    fn edits_go_through_the_queue_and_check_the_counts() {
        let mut s = loaded();
        let anime_2 = s.entries.iter().find(|e| e.id == 2).map(|e| e.waiting);
        s.manga_plus_one(2).unwrap();
        let e = entry(&s, 2);
        assert_eq!((e.chapters, e.waiting), (65, Some("Waiting to sync")));
        // Volumes only: +1 is a volume.
        s.manga_plus_one(6).unwrap();
        assert_eq!((entry(&s, 6).chapters, entry(&s, 6).volumes), (0, 4));
        assert_eq!(s.manga_edit(2, Change { progress: Some(89), ..Default::default() }).unwrap_err(), "Salt & Gears has 88 chapters.");
        assert_eq!(s.manga_edit(6, Change { volumes: Some(10), ..Default::default() }).unwrap_err(), "Night Shift Alchemist has 9 volumes.");
        // The last chapter completes, and Completed fills the known volumes.
        s.manga_edit(7, Change { progress: Some(1), ..Default::default() }).unwrap();
        let e = entry(&s, 7);
        assert_eq!((e.status, e.chapters, e.volumes), (Status::Completed, 1, 1), "completing fills the known volume count too");
        s.manga_edit(8, Change { status: Some(Status::Completed), ..Default::default() }).unwrap();
        let e = entry(&s, 8);
        assert_eq!((e.status, e.chapters, e.volumes), (Status::Completed, 52, 6));
        // An anime with the same id is untouched (ids overlap between kinds).
        assert_eq!(s.entries.iter().find(|e| e.id == 2).map(|e| e.waiting), anime_2);
    }

    #[test]
    fn the_editor_drafts_both_counts_and_saves_one_change() {
        let mut s = loaded();
        s.manga_toggle_editor(2);
        s.manga_editor_step(Unit::Volumes, 1);
        s.manga_editor_step(Unit::Volumes, 1);
        s.manga_editor_step(Unit::Chapters, -1);
        s.editor_score(kotora_core::score::ten(9));
        assert_eq!(entry(&s, 2).chapters, 64, "nothing changes before Save");
        s.save_manga_editor().unwrap();
        let e = entry(&s, 2);
        assert_eq!((e.chapters, e.volumes, e.score), (63, 2, kotora_core::score::ten(9)));
        assert!(s.manga.editor.is_none());
        // Stepping to the last chapter completes; stepping back undoes it.
        s.manga_toggle_editor(2);
        for _ in 0..30 {
            s.manga_editor_step(Unit::Chapters, 1);
        }
        assert_eq!(s.manga.editor.map(|ed| (ed.status, ed.chapters)), Some((Status::Completed, 88)));
        s.manga_editor_step(Unit::Chapters, -1);
        assert_eq!(s.manga.editor.map(|ed| ed.status), Some(Status::Watching));
    }

    #[test]
    fn reread_and_its_end() {
        let mut s = loaded();
        s.manga_start_reread(9).unwrap();
        let e = entry(&s, 9);
        assert_eq!((e.status, e.rereading, e.chapters), (Status::Completed, true, 0));
        s.manga_edit(9, Change { progress: Some(96), ..Default::default() }).unwrap();
        let e = entry(&s, 9);
        assert_eq!((e.rereading, e.times_reread), (false, 1), "the last chapter ends the reread, one more in the count");
        assert!(s.manga_start_reread(1).is_err(), "only a completed manga");
    }

    #[test]
    fn a_failed_sync_is_said_and_a_good_one_clears_it() {
        use kotora_core::service::ServiceError;
        use kotora_core::sync::MangaSync;
        let mut s = loaded();
        let m = manga_msg(MangaSync::Failed(ServiceError::Offline("x".into())), "MyAnimeList").unwrap();
        s.apply(Msg::Manga(m));
        assert_eq!(s.manga.error.as_deref(), Some("Your manga list wasn't read: can't reach MyAnimeList."));
        assert_eq!(s.manga.entries.len(), 12, "the saved list stays");
        assert!(manga_msg(MangaSync::Off, "MyAnimeList").is_none());
        s.apply(Msg::Manga(MangaMsg::Synced { items: fake_items(), synced_at: 5 }));
        assert!(s.manga.error.is_none());
        // `kot manga off` meanwhile: the switch goes back to Anime at the next start.
        let mut t = State::fake();
        t.manga.mode = true;
        t.go(Page::Manga(Status::Watching));
        t.apply(Msg::Manga(MangaMsg::Cache { enabled: false, items: vec![], synced_at: None }));
        assert_eq!((t.manga.mode, t.page), (false, Page::List(Status::Watching)));
    }

    #[test]
    fn queue_tags_follow_the_kind() {
        let mut s = loaded();
        let op = |kind: Kind, media_id: u32| kotora_core::queue::Op { kind, media_id, state: OpState::Queued, ..s.ops[0].clone() };
        s.ops = vec![op(Kind::Manga, 3), op(Kind::Anime, 1)];
        s.apply(Msg::Manga(MangaMsg::Items { items: vec![], gone: vec![] }));
        assert_eq!(entry(&s, 3).waiting, Some("Waiting to sync"));
        assert_eq!(entry(&s, 1).waiting, None, "an anime change never tags the manga with its id");
    }
}

/// Add, remove, search and the menu (stage 4b).
#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::library::Undo;
    use kotora_core::config::ListView;

    fn loaded() -> State {
        let mut s = State::fake();
        fake(&mut s);
        s
    }

    fn entry(s: &State, id: u32) -> Option<&MEntry> {
        s.manga.entries.iter().find(|e| e.id == id)
    }

    /// The search page in Manga mode: the manga list's matches, MyAnimeList's manga minus
    /// those, and adds that go to the manga list (never the anime one).
    #[test]
    fn manga_search_and_add() {
        let mut s = loaded();
        s.find.replace_text("salt");
        s.open_search();
        assert_eq!(s.search_kind(), Kind::Manga);
        s.online.show("salt", fake_hits());
        let titles: Vec<&str> = s.manga.rows.iter().map(|&k| s.manga.entries[k].title.as_str()).collect();
        assert_eq!(titles, ["Salt & Gears"], "the manga list's match");
        assert!(s.rows.is_empty(), "no anime rows on a manga search");
        assert_eq!(s.new_hits().iter().map(|h| h.id).collect::<Vec<_>>(), [501, 502, 503], "the one on the list is not offered");
        s.add_hit(501, Status::PlanToWatch);
        let e = entry(&s, 501).unwrap();
        assert_eq!((e.status, e.chapters, e.waiting), (Status::PlanToWatch, 0, Some("Waiting to sync")));
        assert!(!s.entries.iter().any(|e| e.id == 501), "not an anime");
        s.add_hit(503, Status::Completed);
        let e = entry(&s, 503).unwrap();
        assert_eq!((e.chapters, e.volumes), (1, 1), "Completed starts at the last chapter and volume");
        // Already on the list: the status changes instead.
        s.manga_add(kotora_core::Media { id: 2, kind: Kind::Manga, ..Default::default() }, Status::OnHold).unwrap();
        assert_eq!(entry(&s, 2).unwrap().status, Status::OnHold);
        assert_eq!(s.manga.entries.iter().filter(|e| e.id == 2).count(), 1);
        // A result opens the manga's page.
        s.open_mal_id(502);
        assert!(s.detail.as_ref().is_some_and(|d| d.is_manga() && d.id() == 502));
        // Back to Anime: the same words search anime again (results are kept per kind).
        let mut s = loaded();
        s.find.replace_text("salt");
        s.open_search();
        s.online.show("salt", fake_hits());
        s.manga.mode = false;
        s.search_changed();
        assert_eq!(s.online.kind, Kind::Anime);
        assert!(s.online.hits.is_empty(), "manga results never show as anime ones");
    }

    #[test]
    fn manga_links_open_manga_pages() {
        use crate::search::{mal_id, mal_id_in};
        assert_eq!(mal_id_in("https://myanimelist.net/manga/2001/Paper_Lanterns", Kind::Manga), Some((2001, true)));
        assert_eq!(mal_id_in("myanimelist.net/manga.php?id=2001", Kind::Manga), Some((2001, true)));
        assert_eq!(mal_id_in("https://myanimelist.net/anime/41590", Kind::Manga), None, "an anime link is not a manga");
        assert_eq!(mal_id("https://myanimelist.net/manga/2001"), None);
        assert_eq!(mal_id_in("2001", Kind::Manga), Some((2001, false)));
        // AniList manga links, on AniList.
        use crate::search::link_id_of;
        use kotora_core::service::ServiceId;
        assert_eq!(link_id_of("https://anilist.co/manga/30013/Paper-Lanterns", ServiceId::AniList, Kind::Manga), Some((30013, true)));
        assert_eq!(link_id_of("https://anilist.co/anime/30013", ServiceId::AniList, Kind::Manga), None);
        assert_eq!(link_id_of("https://anilist.co/manga/30013", ServiceId::Mal, Kind::Manga), None, "each service its own links");
    }

    /// The details page of a manga not on the list adds it to the manga list.
    #[test]
    fn details_add_a_manga() {
        let mut s = loaded();
        s.open_details(crate::details::Stub { kind: Kind::Manga, ..crate::details::Stub::bare(777, "Paper Kite Express") });
        let mut d = fake_details(777, "Paper Kite Express");
        d.mine = None;
        d.episodes = Some(30);
        s.details_answer(crate::details::Answer { id: 777, kind: Kind::Manga, result: Ok(d), item: None, done: true });
        s.add_shown(Status::Watching);
        assert_eq!(entry(&s, 777).map(|e| (e.status, e.chapters_total)), Some((Status::Watching, Some(30))));
        assert!(!s.entries.iter().any(|e| e.id == 777));
    }

    /// Remove asks first, leaves at once, Undo puts it back; an anime with the same id stays.
    #[test]
    fn remove_asks_then_undo_puts_it_back() {
        let mut s = loaded();
        let anime = s.entries.iter().any(|e| e.id == 2);
        s.open_manga_ctx(2, crate::ctxmenu::At::Point(10.0, 10.0));
        s.ask_remove_manga(2);
        assert_eq!((s.ctx.is_none(), s.manga.confirm_remove, s.confirm_remove), (true, Some(2), None));
        assert!(s.manga_remove_question(2).unwrap().starts_with("Remove Salt & Gears from your manga list?"));
        assert!(entry(&s, 2).is_some(), "nothing goes before the answer");
        s.remove_manga(2).unwrap();
        assert!(entry(&s, 2).is_none() && s.manga.confirm_remove.is_none());
        assert!(!s.manga.rows.iter().any(|&k| s.manga.entries[k].id == 2));
        assert_eq!(s.entries.iter().any(|e| e.id == 2), anime, "the anime 2 is untouched");
        assert!(matches!(s.toast.as_ref().and_then(|t| t.undo.clone()), Some(Undo::MangaRemove(_))));
        s.toast_undo();
        let e = entry(&s, 2).unwrap();
        assert_eq!((e.chapters, e.status), (64, Status::Watching), "back as it was");
        assert!(s.manga.rows.iter().any(|&k| s.manga.entries[k].id == 2));
        assert!(s.remove_manga(9_999).is_err());
        // Removing the manga being edited closes its editor.
        s.manga_toggle_editor(2);
        s.remove_manga(2).unwrap();
        assert!(s.manga.editor.is_none());
    }

    /// The menu's Move to, Score and +1 go through the queue, each with Undo.
    #[test]
    fn the_menu_moves_scores_and_steps_with_undo() {
        let mut s = loaded();
        let before = entry(&s, 8).unwrap().values();
        s.open_manga_ctx(8, crate::ctxmenu::At::Point(1.0, 1.0));
        s.manga_ctx_move(8, Status::Completed);
        assert!(s.ctx.is_none());
        let e = entry(&s, 8).unwrap();
        assert_eq!((e.status, e.chapters, e.volumes), (Status::Completed, 52, 6));
        assert!(s.toast.as_ref().is_some_and(|t| t.text == "Moved Harbor of Small Lights to Completed"));
        s.toast_undo();
        assert_eq!(entry(&s, 8).unwrap().values(), before, "Undo puts it back");
        s.manga_ctx_score(2, kotora_core::score::ten(10));
        assert_eq!(entry(&s, 2).unwrap().score, kotora_core::score::ten(10));
        s.manga_ctx_plus(6);
        assert_eq!(entry(&s, 6).unwrap().volumes, 4, "volumes only: a volume");
        assert!(s.toast.as_ref().is_some_and(|t| t.text == "Volume 4 of Night Shift Alchemist recorded"));
        // Edit… in the table: the row's editor; in the grid: the details page with it.
        s.manga_ctx_edit_entry(2);
        assert_eq!(s.manga.editor.map(|e| e.id), Some(2));
        s.prefs.manga_views.set(Status::Watching, ListView::Grid);
        s.manga_ctx_edit_entry(1);
        assert!(s.page == Page::Details && s.detail.as_ref().is_some_and(|d| d.is_manga()));
    }

    /// Each kind keeps its views.
    #[test]
    fn views_are_kept_per_kind() {
        let mut s = loaded();
        s.set_manga_view(Status::Watching, ListView::Large);
        assert_eq!((s.manga_view_of(Status::Watching), s.view_of(Status::Watching)), (ListView::Large, ListView::List));
        s.set_view(Status::Completed, ListView::Grid);
        assert_eq!(s.manga_view_of(Status::Completed), ListView::List);
    }

    /// A removal MyAnimeList changed meanwhile: the conflict says chapters, volumes and Reading.
    #[test]
    fn a_manga_conflict_reads_as_manga() {
        let r = kotora_core::Remote { values: kotora_core::Values { volumes: 9, ..kotora_core::Values::new(Status::Watching, 70, kotora_core::score::ten(8)) }, updated_at: 0 };
        assert_eq!(crate::model::remote_text_of(Kind::Manga, &r, Some(88), ScoreFormat::Point10), "Ch 70 / 88, vol 9, Reading, score 8");
        assert_eq!(crate::model::remote_text_of(Kind::Anime, &r, Some(88), ScoreFormat::Point10), "70 / 88, Watching, score 8");
    }
}
