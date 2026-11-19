// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: the torrent feeds' behaviour (sources checked on an interval, the search
// address with %title% and its nyaa.si default from Taiga's settings_keys.cpp).

//! Torrent / RSS feeds (Settings → Feeds, the Feeds page, the `↓ Ep N` badge). Off by default,
//! no sources: until the user turns feeds on and adds a feed nothing runs and nothing is asked.
//!
//! One worker thread while feeds are on: it checks each feed every interval (30 min, at least
//! 10) with `If-None-Match`/`If-Modified-Since`, sleeps on its channel in between (no timer),
//! and hands the items, the recognition picks (shared with files) and the marks to the window.
//! The window matches them against the list (`kotora_core::feeds::select`): new episodes of
//! Watching, Plan to watch and On hold entries, one row per episode, the best release first.
//!
//! Download hands the magnet (or the .torrent file, fetched on click) to the system's handler,
//! the user's own torrent app, and marks the episode got. Kotora never downloads video.
//! Feed URLs can carry a key: they are only ever shown through `feeds::mask`.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use kotora_core::config::{self, FeedSource, Feeds};
use kotora_core::feeds::marks::Mark;
use kotora_core::feeds::poll::{FeedState, Fetched, fetch};
use kotora_core::feeds::select::Marks;
use kotora_core::feeds::taiga::{self, Archive, Entry, ItemState, World};
use kotora_core::feeds::{Item, NYAA_SEARCH, mask, nyaa_defaults, safe_http, search_url};
use kotora_core::recognize::{parse_release, relations::Relations};
use kotora_core::{Kind, ListItem, paths};
use rune::{El, TextEdit, TextStyle, Tx, col, el, row, spacer, text, text_input};

use crate::model::{Page, State};
use crate::theme::{self, PAL};
use crate::ui::field_hint;

/// The empty feed field's hint (no source is built in; this only names an example).
pub const HINT: &str = "Paste an RSS feed URL, e.g. from nyaa.si";

pub const SWITCH_NOTE: &str = "Kotora checks only the feeds you add and opens releases in your torrent app. It never downloads anything itself.";

/// kv: the (series, episodes) already notified, so a restart doesn't repeat them.
const NOTIFIED_KEY: &str = "feeds:notified";
const NOTIFIED_KEEP: usize = 300;
/// kv: the torrent history (Taiga's archive), one title a line, oldest first.
pub const ARCHIVE_KEY: &str = "feeds:archive";

/// One feed's line for Settings and the page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedLine {
    pub state: FeedState,
    pub checking: bool,
}

#[derive(Default)]
pub struct FeedsUi {
    pub cfg: Feeds,
    pub draft: TextEdit,
    pub error: Option<String>,
    /// A note under the field (the nyaa.si defaults, a feed added).
    pub note: Option<String>,
    /// The anime list, as the service gave it (titles, synonyms, counts).
    pub list: Vec<ListItem>,
    /// Every feed's items: (feed index, item).
    pub items: Vec<(i64, Item)>,
    /// Per feed, by URL.
    pub lines: HashMap<String, FeedLine>,
    pub picks: HashMap<String, (u32, String, String)>,
    pub marks: Marks,
    pub relations: Option<std::sync::Arc<Relations>>,
    /// The Torrents table: every item with its anime, category and state (Taiga's order).
    pub entries: Vec<Entry>,
    /// Taiga's filters (`feed_filters.xml`).
    pub filters: Vec<taiga::Filter>,
    /// The torrent history; `None` before it is read.
    pub archive: Option<Archive>,
    /// States the user set by hand (check, Discard), by item key, over the filters'.
    pub manual: HashMap<String, ItemState>,
    /// The item whose actions are shown.
    pub open_row: Option<String>,
    /// The filter box over the table, the header sort, and the rows they leave (indices).
    pub query: TextEdit,
    pub sort: (crate::torrents::SortCol, bool),
    pub visible: Vec<usize>,
    /// The table's lines (groups, items, the open row's actions) for the virtual list.
    pub slots: Vec<crate::torrents::Slot>,
    /// The page's tab: 0 Your shows, 1 All releases, 2 History.
    pub tab: u8,
    pub editor: crate::torrents::FilterEditor,
    /// The page's "Search for torrents" box (Taiga's main toolbar search on Torrents).
    pub search_box: TextEdit,
    /// The right-click menu: the item's key and where.
    pub ctx: Option<(String, f32, f32)>,
    /// "Marked N torrents." and the like (Taiga's status bar).
    pub status: Option<String>,
    /// Torrents → Your shows: the per-show searches.
    pub shows: crate::shows::Shows,
    pub app_edit: TextEdit,
    pub folder_edit: TextEdit,
    /// Already notified, loaded once from the store; `None` before the first check.
    pub notified: Option<Vec<(u32, u32, u32)>>,
    /// Downloads running (item keys), and what it said last.
    pub busy: HashSet<String>,
    pub said: Option<String>,
    /// Settings → Feeds → Search address.
    pub search_edit: TextEdit,
    /// The last "Search releases" (Taiga's Search for torrents).
    pub search: Option<SearchView>,
    worker: Option<Sender<Cmd>>,
}

/// One search: the anime, its results, and what is new in them.
#[derive(Default)]
pub struct SearchView {
    pub media_id: u32,
    pub title: String,
    pub loading: bool,
    pub error: Option<String>,
    pub items: Vec<(i64, Item)>,
    pub entries: Vec<Entry>,
}

enum Cmd {
    Set(Box<Feeds>),
    Check,
}

/// What a check pass gives the window.
pub struct Pass {
    items: Vec<(i64, Item)>,
    lines: HashMap<String, FeedLine>,
    picks: HashMap<String, (u32, String, String)>,
    marks: Marks,
    relations: Option<std::sync::Arc<Relations>>,
    notified: Option<Vec<(u32, u32, u32)>>,
    archive: Option<Vec<String>>,
}

/// One new episode's key: (anime, first, last episode).
pub fn span(e: &Entry) -> Option<(u32, u32, u32)> {
    let (a, b) = e.release.episodes?;
    Some((e.anime_id?, a, b))
}

/// What counts as new for the badge, the sidebar and notifications: an episode of an anime on
/// the list past its progress, not discarded, not an extra (OP, PV…).
pub fn is_new(e: &Entry) -> bool {
    !e.state.is_discarded()
        && e.new_episode
        && e.anime_id.is_some()
        && e.category != taiga::Category::Other
        && e.release.episodes.is_some()
}

/// A short name for a feed: the host.
fn host_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    host.trim_start_matches("www.").to_string()
}

impl State {
    /// At start (never headless): the settings, and the worker when feeds are on.
    pub fn feeds_load(&mut self, c: &config::Config) {
        self.feeds.cfg = c.feeds.clone();
        self.feeds.search_edit.text = c.feeds.search.clone();
        self.feeds.app_edit.text = c.feeds.app_path.clone();
        self.feeds.folder_edit.text = c.feeds.folder.clone();
        self.feeds.filters = crate::torrents::load_filters(&c.feeds);
        self.feeds_apply();
    }

    /// Starts, tells or leaves the worker after a change.
    pub(crate) fn feeds_tell_worker(&mut self) {
        self.feeds_apply();
    }

    fn feeds_apply(&mut self) {
        if let Some(w) = &self.feeds.worker {
            let _ = w.send(Cmd::Set(Box::new(self.feeds.cfg.clone())));
            return;
        }
        if !self.feeds.cfg.active() {
            return;
        }
        let Some(h) = self.handle.clone() else { return };
        let (tx, rx) = mpsc::channel();
        let cfg = self.feeds.cfg.clone();
        let service = crate::account::name();
        let anilist = crate::account::active() == kotora_core::service::ServiceId::AniList;
        if std::thread::Builder::new()
            .name("kotora-feeds".into())
            .spawn(move || run(h, rx, cfg, service, anilist))
            .is_ok()
        {
            self.feeds.worker = Some(tx);
        }
    }

    /// The list changed: the rows follow (an episode watched drops its row).
    pub fn feeds_list(&mut self, items: &[ListItem]) {
        self.feeds.list = items
            .iter()
            .filter(|it| it.media.kind == Kind::Anime)
            .cloned()
            .collect();
        if self.feeds.cfg.enabled {
            self.feeds_reselect();
            self.shows_refresh(false);
        }
    }

    /// Taiga's `ExamineData`: identify, filter, the archive, then the user's own states.
    pub fn feeds_reselect(&mut self) {
        let entries = self.feeds_examine(&self.feeds.items);
        let found = self
            .feeds
            .search
            .as_ref()
            .map(|sv| self.feeds_examine(&sv.items));
        self.feeds.entries = entries;
        if let (Some(sv), Some(e)) = (self.feeds.search.as_mut(), found) {
            sv.entries = e;
        }
        self.shows_reexamine();
        self.feeds_refilter();
    }

    pub(crate) fn feeds_examine(&self, items: &[(i64, Item)]) -> Vec<Entry> {
        let f = &self.feeds;
        let empty = Relations::default();
        let rel = f.relations.as_deref().unwrap_or(&empty);
        let (index, got) = (&self.library.index, &f.marks.got);
        let available = |id: u32, ep: u32| {
            index.by_media.get(&id).is_some_and(|m| m.contains_key(&ep))
                || got
                    .keys()
                    .any(|(m, a, b)| *m == id && (*a..=*b).contains(&ep))
        };
        let w = World {
            list: &f.list,
            available: &available,
        };
        let mut entries = taiga::examine(items, &f.list, &f.picks, rel);
        let none = Archive::default();
        taiga::run_filters(
            &mut entries,
            &f.filters,
            f.cfg.filters_enabled,
            f.archive.as_ref().unwrap_or(&none),
            &w,
        );
        for e in &mut entries {
            if let Some(st) = f.manual.get(&e.item.key()) {
                if e.state.is_discarded() && !st.is_discarded() {
                    // Shown with the filter's reason, so it never looks like a normal item.
                    e.why = format!(
                        "{} \u{b7} kept by you",
                        if e.why.is_empty() {
                            "Discarded"
                        } else {
                            e.why.as_str()
                        }
                    );
                } else if st.is_discarded() {
                    e.why = "Discarded by you".into();
                }
                e.state = *st;
            }
        }
        entries
    }

    /// A pass ended.
    pub fn feeds_got(&mut self, p: Pass) -> bool {
        self.feeds.items = p.items;
        self.feeds.lines = p.lines;
        self.feeds.picks = p.picks;
        self.feeds.marks = p.marks;
        if p.relations.is_some() {
            self.feeds.relations = p.relations;
        }
        if self.feeds.notified.is_none() {
            self.feeds.notified = p.notified;
        }
        if self.feeds.archive.is_none() {
            self.feeds.archive = p.archive.map(|titles| Archive { titles });
        }
        // A new check decides again: states set by hand last until then (as in Taiga).
        self.feeds.manual.clear();
        self.feeds_reselect();
        self.feeds_notify();
        self.shows_refresh(false);
        true
    }

    /// One notification for the rows not notified before (never unsure ones).
    fn feeds_notify(&mut self) {
        let Some(done) = self.feeds.notified.clone() else {
            return;
        };
        let mut fresh: Vec<(u32, u32, u32)> = Vec::new();
        for k in self
            .feeds
            .entries
            .iter()
            .filter(|e| is_new(e))
            .filter_map(span)
        {
            if !done.contains(&k) && !fresh.contains(&k) {
                fresh.push(k);
            }
        }
        if fresh.is_empty() {
            return;
        }
        // Taiga's "download new episodes": the items the filters selected, at once.
        if self.feeds.cfg.new_action == "download" {
            let keys: Vec<String> = self
                .feeds
                .entries
                .iter()
                .filter(|e| {
                    e.state == ItemState::Selected && span(e).is_some_and(|k| fresh.contains(&k))
                })
                .map(|e| e.item.key())
                .collect();
            if !keys.is_empty() {
                self.feeds_download(keys);
            }
        }
        let Some(done) = self.feeds.notified.as_mut() else {
            return;
        };
        done.extend(fresh.iter().copied());
        let over = done.len().saturating_sub(NOTIFIED_KEEP);
        done.drain(..over);
        let keep: String = done
            .iter()
            .map(|(m, a, b)| format!("{m}:{a}-{b}"))
            .collect::<Vec<_>>()
            .join(",");
        if self.handle.is_some() {
            std::thread::spawn(move || {
                if let Ok(st) = kotora_core::store::Store::open(&paths::db_file()) {
                    let _ = st.kv_set(NOTIFIED_KEY, &keep);
                }
            });
        }
        if !self.prefs.notify.new_release {
            return;
        }
        let parts: Vec<String> = fresh
            .iter()
            .take(4)
            .map(|(m, a, b)| format!("{} {}", self.feeds_title(*m), ep_text(*a, *b)))
            .collect();
        let summary = if fresh.len() == 1 {
            "New release".to_string()
        } else {
            format!("{} new releases", fresh.len())
        };
        let mut body = parts.join(", ");
        if fresh.len() > 4 {
            body.push_str(&format!(" and {} more", fresh.len() - 4));
        }
        self.notes.show(crate::notes::Note {
            id: "kotora-feeds".into(),
            kind: crate::notes::Kind::Aired,
            summary,
            body,
            tail: None,
        });
    }

    pub fn feeds_title(&self, id: u32) -> String {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.title.clone())
            .unwrap_or_else(|| format!("Anime {id}"))
    }

    /// The badge's episode for a list entry: the first new one with a release.
    pub fn feeds_badge(&self, id: u32) -> Option<(u32, u32)> {
        if !self.feeds.cfg.enabled {
            return None;
        }
        self.feeds
            .entries
            .iter()
            .filter(|e| e.anime_id == Some(id) && is_new(e))
            .filter_map(|e| e.release.episodes)
            .min()
    }

    /// New episodes for the sidebar badge.
    pub fn feeds_count(&self) -> usize {
        if !self.feeds.cfg.enabled {
            return 0;
        }
        let set: HashSet<(u32, u32, u32)> = self
            .feeds
            .entries
            .iter()
            .filter(|e| is_new(e))
            .filter_map(span)
            .collect();
        set.len()
    }

    pub fn open_feeds(&mut self) {
        self.go(Page::Feeds);
    }

    pub fn feeds_check_now(&mut self) {
        if let Some(w) = &self.feeds.worker {
            let _ = w.send(Cmd::Check);
            for l in self.feeds.lines.values_mut() {
                l.checking = true;
            }
        }
    }

    // -------------------------------------------------------------------------- settings

    fn feeds_save(&mut self, key: &'static str, value: String) {
        let Some(h) = self.handle.clone() else { return };
        let _ = std::thread::Builder::new()
            .name("kotora-settings".into())
            .spawn(move || {
                if let Err(e) = config::write_setting(&paths::config_file(), "feeds", key, &value) {
                    h.update_if(move |s, _| {
                        s.feeds.error = Some(format!("Can't save the setting: {e}"));
                        true
                    });
                }
            });
    }

    pub fn set_feeds(&mut self, on: bool) {
        self.feeds.cfg.enabled = on;
        self.feeds_save("enabled", on.to_string());
        if on {
            // The notification switch asks for the system's permission now (macOS), not later.
            if self.prefs.notify.new_release {
                self.notes.prepare();
            }
            self.feeds_reselect();
        } else if self.page == Page::Feeds {
            self.go(Page::Settings);
        }
        self.feeds_apply();
    }

    pub fn set_feeds_notify(&mut self, on: bool) {
        self.prefs.notify.new_release = on;
        if on {
            self.notes.prepare();
        }
        crate::detect::save_setting(self, "notifications", "new_release", on.to_string());
    }

    pub fn set_feeds_minutes(&mut self, m: u32) {
        self.feeds.cfg.interval_minutes = m.max(Feeds::MIN_MINUTES);
        self.feeds_save(
            "interval_minutes",
            self.feeds.cfg.interval_minutes.to_string(),
        );
        self.feeds_apply();
    }

    /// The field changed: a nyaa.si address says what it will become.
    pub fn feeds_draft_changed(&mut self) {
        self.feeds.error = None;
        self.feeds.note = nyaa_defaults(&self.feeds.draft.text).map(|d| format!("nyaa.si: added as \u{201c}{}\u{201d}, Anime \u{b7} English-translated unless the address names a category.", d.name));
    }

    /// Add: the typed address (a nyaa.si one in its RSS form, with a name).
    pub fn feeds_add(&mut self) {
        let raw = self.feeds.draft.text.trim().to_string();
        if raw.is_empty() {
            return;
        }
        let (url, name) = match nyaa_defaults(&raw) {
            Some(d) => (d.url, d.name),
            None => match safe_http(&raw) {
                Some(u) => {
                    let n = host_name(&u);
                    (u, n)
                }
                None => {
                    self.feeds.error =
                        Some("A feed address starts with https:// (or http://).".into());
                    return;
                }
            },
        };
        if self.feeds.cfg.sources.iter().any(|s| s.url == url) {
            self.feeds.error = Some("That feed is already added.".into());
            return;
        }
        let masked = mask(&url) != url;
        if nyaa_defaults(&url).is_some() && self.feeds.cfg.search.trim().is_empty() {
            self.set_feeds_search(NYAA_SEARCH.to_string());
        }
        self.feeds.cfg.sources.push(FeedSource {
            name: name.clone(),
            url,
        });
        self.feeds_save("sources", Feeds::sources_toml(&self.feeds.cfg.sources));
        self.feeds.draft = TextEdit::default();
        self.feeds.note = Some(if masked {
            format!("Added \u{201c}{name}\u{201d}. Its key is hidden here and never logged.")
        } else {
            format!("Added \u{201c}{name}\u{201d}.")
        });
        self.feeds_apply();
    }

    pub fn feeds_remove(&mut self, url: &str) {
        self.feeds.cfg.sources.retain(|s| s.url != url);
        self.feeds.lines.remove(url);
        self.feeds_save("sources", Feeds::sources_toml(&self.feeds.cfg.sources));
        self.feeds.note = None;
        self.feeds_apply();
    }

    // -------------------------------------------------------------------------- download

    /// Taiga's Download: each item (in the order given) to the torrent app or the download
    /// folder; it goes into the history, and its episode is marked got.
    pub fn feeds_download(&mut self, keys: Vec<String>) {
        let all = self
            .feeds
            .entries
            .iter()
            .chain(self.feeds.search.iter().flat_map(|sv| sv.entries.iter()))
            .chain(
                self.feeds
                    .shows
                    .by_id
                    .values()
                    .flat_map(|x| x.entries.iter()),
            );
        let mut jobs = Vec::new();
        for k in &keys {
            if self.feeds.busy.contains(k) {
                continue;
            }
            let Some(e) = all.clone().find(|e| &e.item.key() == k) else {
                continue;
            };
            let anime = e
                .anime_id
                .map(|id| self.feeds_title(id))
                .unwrap_or_else(|| e.anime_title.clone());
            jobs.push((k.clone(), e.item.clone(), anime, span(e), e.release.version));
        }
        if jobs.is_empty() {
            return;
        }
        let archive = self.feeds.archive.get_or_insert_with(Archive::default);
        for (k, it, _, sp, version) in &jobs {
            archive.add(&it.title);
            self.feeds.busy.insert(k.clone());
            self.feeds.manual.remove(k);
            if let Some(sp) = sp {
                self.feeds
                    .marks
                    .got
                    .insert(*sp, (it.info_hash.clone(), *version));
            }
        }
        self.feeds_reselect();
        self.feeds.said = Some(if jobs.len() == 1 {
            format!("Downloading {}\u{2026}", jobs[0].1.title)
        } else {
            format!("Downloading {} torrents\u{2026}", jobs.len())
        });
        let Some(h) = self.handle.clone() else {
            for (k, ..) in &jobs {
                self.feeds.busy.remove(k);
            }
            return;
        };
        let opts = self.feeds.cfg.clone();
        let service = crate::account::name();
        let _ = std::thread::Builder::new()
            .name("kotora-feed-open".into())
            .spawn(move || {
                let mut results = Vec::new();
                let st = kotora_core::store::Store::open(&paths::db_file()).ok();
                for (k, it, anime, sp, version) in jobs {
                    let r = crate::torrents::open_one(&it, Some(&anime), &opts);
                    if r.is_ok()
                        && let (Some(st), Some((m, a, b))) = (&st, sp)
                    {
                        let at = kotora_core::time::now_unix();
                        let _ = st.feed_mark(
                            service,
                            m,
                            (a, b),
                            &Mark::Got {
                                info_hash: it.info_hash.clone(),
                                version,
                                title: it.title.clone(),
                            },
                            at,
                        );
                    }
                    results.push((k, it.title, sp, r));
                }
                h.update_if(move |s, _| {
                    let mut said = Vec::new();
                    for (k, title, sp, r) in results {
                        s.feeds.busy.remove(&k);
                        match r {
                            Ok(m) => said.push(m),
                            Err(e) => {
                                // Not opened: out of the history again, the row comes back.
                                if let Some(a) = s.feeds.archive.as_mut() {
                                    a.titles.retain(|t| t != &title);
                                }
                                if let Some(sp) = sp {
                                    s.feeds.marks.got.remove(&sp);
                                }
                                said.push(mask(&e));
                            }
                        }
                    }
                    s.feeds_save_archive();
                    s.feeds_reselect();
                    s.feeds.said = said.last().cloned();
                    true
                });
            });
    }

    /// The history to the store (Taiga keeps `archive_max` titles).
    pub fn feeds_save_archive(&mut self) {
        let Some(a) = &self.feeds.archive else { return };
        let keep = a.trimmed(self.feeds.cfg.archive_max as usize).join("\n");
        if self.handle.is_none() {
            return;
        }
        std::thread::spawn(move || {
            if let Ok(st) = kotora_core::store::Store::open(&paths::db_file()) {
                let _ = st.kv_set(ARCHIVE_KEY, &keep);
            }
        });
    }
}

impl State {
    pub fn set_feeds_search(&mut self, url: String) {
        let url = url.trim().to_string();
        self.feeds.cfg.search = url.clone();
        self.feeds.search_edit.text = url.clone();
        self.feeds_save("search", config::quote(&url));
    }

    pub fn feeds_save_search(&mut self) {
        let t = self.feeds.search_edit.text.trim().to_string();
        if t == self.feeds.cfg.search {
            return;
        }
        if !t.is_empty() && search_url(&t, "x").is_none() {
            self.feeds.error = Some(
                "The search address needs %title% and must start with https:// (or http://)."
                    .into(),
            );
            return;
        }
        self.feeds.error = None;
        self.set_feeds_search(t);
    }

    /// Whether "Search releases" can run for an anime on the list.
    pub fn feeds_can_search(&self, id: u32) -> bool {
        self.feeds.cfg.enabled
            && !self.feeds.cfg.search.trim().is_empty()
            && self.entries.iter().any(|e| e.id == id)
    }

    /// Search releases (Taiga's "Search for torrents"): the search address with the anime's
    /// title, read once, shown on the Feeds page.
    pub fn feeds_search(&mut self, id: u32) {
        let Some(e) = self.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let title = e.main_title.clone();
        self.feeds_search_title(title, id);
    }

    /// Taiga's `TorrentDialog::Search`: the search address with `%title%`, its results replace
    /// All releases (the box on the page shows the title). `id` 0: typed, no anime.
    pub fn feeds_search_title(&mut self, title: String, id: u32) {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        self.feeds.search_box.text = title.clone();
        self.feeds.open_row = None;
        let Some(url) = search_url(&self.feeds.cfg.search, &title) else {
            self.feeds.search = Some(SearchView {
                media_id: id,
                title,
                error: Some("Set a search address in Settings \u{2192} Feeds.".into()),
                ..SearchView::default()
            });
            self.open_feeds();
            return;
        };
        self.feeds.search = Some(SearchView {
            media_id: id,
            title,
            loading: true,
            ..SearchView::default()
        });
        self.feeds.tab = 1;
        self.open_feeds();
        let Some(h) = self.handle.clone() else { return };
        let _ = std::thread::Builder::new()
            .name("kotora-feed-search".into())
            .spawn(move || {
                let r = fetch(&url, None, None).and_then(|f| match f {
                    Fetched::New { body, .. } => kotora_core::feeds::parse(&body)
                        .map(|f| f.items)
                        .map_err(|e| e.to_string()),
                    Fetched::Same => Ok(Vec::new()),
                });
                h.update_if(move |s, _| s.feeds_found(id, r));
            });
    }

    pub fn feeds_found(&mut self, id: u32, r: Result<Vec<Item>, String>) -> bool {
        let Some(sv) = self.feeds.search.as_mut().filter(|sv| sv.media_id == id) else {
            return false;
        };
        sv.loading = false;
        match r {
            Ok(items) => sv.items = items.into_iter().map(|i| (-1, i)).collect(),
            Err(e) => sv.error = Some(e),
        }
        self.feeds_reselect();
        true
    }
}

/// "Ep 8", "Ep 8–12".
pub fn ep_text(a: u32, b: u32) -> String {
    if a == b {
        format!("Ep {a}")
    } else {
        format!("Ep {a}\u{2013}{b}")
    }
}

// ---------------------------------------------------------------------------------- worker

fn run(
    h: crate::model::Handle,
    rx: Receiver<Cmd>,
    mut cfg: Feeds,
    service: &'static str,
    anilist: bool,
) {
    let mut states: HashMap<String, FeedState> = HashMap::new();
    let mut items: HashMap<String, Vec<Item>> = HashMap::new();
    let mut relations: Option<std::sync::Arc<Relations>> = None;
    let mut first = true;
    let mut force = false;
    loop {
        if cfg.active() && !paths::offline_forced() {
            let now = kotora_core::time::now_unix();
            let mut changed = false;
            for (i, src) in cfg.sources.iter().enumerate() {
                let st = states.entry(src.url.clone()).or_default();
                if !(force || (cfg.autocheck && st.due(now)) || st.checked.is_none()) {
                    continue;
                }
                changed = true;
                match fetch(&src.url, st.etag.as_deref(), st.modified.as_deref()) {
                    Ok(Fetched::Same) => st.ok(now, cfg.minutes(), now as u64 ^ i as u64),
                    Ok(Fetched::New {
                        body,
                        etag,
                        modified,
                    }) => match st.read(&body) {
                        Ok(f) => {
                            st.etag = etag;
                            st.modified = modified;
                            st.ok(now, cfg.minutes(), now as u64 ^ i as u64);
                            items.insert(src.url.clone(), f.items);
                        }
                        Err(e) => st.failed(now, cfg.minutes(), e.to_string()),
                    },
                    Err(e) => st.failed(now, cfg.minutes(), e),
                }
            }
            force = false;
            if changed || first {
                let pass = pass(
                    &cfg,
                    &states,
                    &items,
                    service,
                    anilist,
                    &mut relations,
                    first,
                );
                first = false;
                h.update_if(move |s, _| s.feeds_got(pass));
            }
        }
        // Asleep on the channel until the next feed is due (no timer while off).
        let wait = if cfg.active() && cfg.autocheck {
            let now = kotora_core::time::now_unix();
            let next = cfg
                .sources
                .iter()
                .filter_map(|s| states.get(&s.url))
                .map(|s| s.next)
                .min()
                .unwrap_or(now);
            Some(Duration::from_secs((next - now).clamp(1, 24 * 3600) as u64))
        } else {
            None
        };
        let got = match wait {
            Some(d) => rx.recv_timeout(d),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(Cmd::Set(c)) => {
                let c = *c;
                states.retain(|url, _| c.sources.iter().any(|s| &s.url == url));
                items.retain(|url, _| c.sources.iter().any(|s| &s.url == url));
                cfg = c;
                // A removed feed's items go at once.
                if cfg.active() {
                    let pass = pass(
                        &cfg,
                        &states,
                        &items,
                        service,
                        anilist,
                        &mut relations,
                        false,
                    );
                    h.update_if(move |s, _| s.feeds_got(pass));
                }
            }
            Ok(Cmd::Check) => force = true,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn pass(
    cfg: &Feeds,
    states: &HashMap<String, FeedState>,
    items: &HashMap<String, Vec<Item>>,
    service: &str,
    anilist: bool,
    relations: &mut Option<std::sync::Arc<Relations>>,
    first: bool,
) -> Pass {
    let mut all = Vec::new();
    for (i, src) in cfg.sources.iter().enumerate() {
        for it in items.get(&src.url).into_iter().flatten() {
            all.push((i as i64, it.clone()));
        }
    }
    let store = kotora_core::store::Store::open(&paths::db_file()).ok();
    let mut picks = HashMap::new();
    if let Some(st) = &store {
        for (_, it) in &all {
            let key = parse_release(&it.title).parsed.key();
            if picks.contains_key(&key) {
                continue;
            }
            if let Ok(Some(p)) = st.recognition_of(service, Kind::Anime, &key) {
                picks.insert(key, p);
            }
        }
    }
    let marks = store
        .as_ref()
        .and_then(|st| st.feed_marks(service).ok())
        .unwrap_or_default();
    let notified = if first {
        let text = store
            .as_ref()
            .and_then(|st| st.kv_get(NOTIFIED_KEY).ok().flatten())
            .unwrap_or_default();
        Some(
            text.split(',')
                .filter_map(|p| {
                    let (m, r) = p.split_once(':')?;
                    let (a, b) = r.split_once('-')?;
                    Some((m.parse().ok()?, a.parse().ok()?, b.parse().ok()?))
                })
                .collect(),
        )
    } else {
        None
    };
    let rel = relations
        .get_or_insert_with(|| {
            let mut r = kotora_core::recognize::relations::load(&paths::cache_dir());
            r.anilist = anilist;
            std::sync::Arc::new(r)
        })
        .clone();
    let lines = states
        .iter()
        .map(|(u, s)| {
            (
                u.clone(),
                FeedLine {
                    state: s.clone(),
                    checking: false,
                },
            )
        })
        .collect();
    let archive = first.then(|| {
        store
            .as_ref()
            .and_then(|st| st.kv_get(ARCHIVE_KEY).ok().flatten())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .filter(|t| !t.is_empty())
            .collect()
    });
    Pass {
        items: all,
        lines,
        picks,
        marks,
        relations: Some(rel),
        notified,
        archive,
    }
}

/// The scenes' feeds: made-up releases for the fake list's shows, no worker.
pub fn fake(s: &mut State) {
    let list: Vec<ListItem> = s
        .entries
        .iter()
        .map(|e| ListItem {
            media: kotora_core::Media {
                id: e.id,
                title: e.main_title.clone(),
                title_en: e.title_en.clone(),
                episodes: e.episodes,
                format: "tv".into(),
                ..Default::default()
            },
            status: e.status,
            progress: e.progress,
            score: None,
            rewatching: e.rewatching,
            times_rewatched: e.times_rewatched,
            volumes: 0,
            updated_at: 0,
        })
        .collect();
    s.feeds.list = list;
    let mut items = Vec::new();
    let mut n = 0u32;
    for e in s
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                kotora_core::Status::Watching
                    | kotora_core::Status::PlanToWatch
                    | kotora_core::Status::OnHold
            )
        })
        .take(5)
    {
        let ep = e.progress + 1;
        if e.episodes.is_some_and(|c| ep > c) {
            continue;
        }
        for (g, res, seed, size) in [
            ("Hoshi", "1080p", 412, 1_400_000_000u64),
            ("Tsuki", "720p", 96, 700_000_000),
            ("Kumo-Subs", "1080p HEVC", 51, 900_000_000),
        ] {
            n += 1;
            let title = format!("[{g}] {} - {ep:02} [{res}]", e.main_title);
            items.push((
                0,
                Item {
                    guid: format!("fake-{n}"),
                    title,
                    magnet: Some(format!("magnet:?xt=urn:btih:{n:040x}")),
                    size: Some(size),
                    seeders: Some(seed),
                    leechers: Some(seed / 4),
                    description: format!("#{n} | {g}"),
                    page: Some(format!("https://tracker.example/view/{n}")),
                    ..Item::default()
                },
            ));
        }
    }
    s.feeds.cfg.enabled = true;
    s.feeds.cfg.sources = vec![
        FeedSource {
            name: "nyaa.si: Hoshi".into(),
            url: "https://nyaa.si/user/Hoshi?page=rss&c=1_2".into(),
        },
        FeedSource {
            name: "tracker.example".into(),
            url: "https://tracker.example/rss?passkey=0123456789abcdef".into(),
        },
    ];
    s.feeds.cfg.search = NYAA_SEARCH.into();
    s.feeds.search_edit.text = NYAA_SEARCH.into();
    // Not-in-list noise beside the list's shows, and a batch.
    for (k, t) in [
        "[Ame] Some Other Show - 03 [1080p]",
        "[Ame] Some Other Show - 04 [720p]",
        "[Hoshi] Nobody Knows (01-12) [BD 1080p] [Batch]",
    ]
    .iter()
    .enumerate()
    {
        items.push((
            1,
            Item {
                guid: format!("noise-{k}"),
                title: t.to_string(),
                magnet: Some(format!("magnet:?xt=urn:btih:{:040x}", 500 + k)),
                size: Some(300_000_000),
                seeders: Some(7),
                leechers: Some(2),
                description: "#100 | 7 comments".into(),
                ..Item::default()
            },
        ));
    }
    s.feeds.items = items;
    s.feeds.notified = Some(Vec::new());
    s.feeds.filters = taiga::default_filters();
    s.feeds.archive = Some(Archive::default());
    s.feeds_reselect();
    let now = s.clock();
    for src in &s.feeds.cfg.sources {
        let st = FeedState {
            checked: Some(now - 600),
            next: now + 1200,
            items: 3,
            ..FeedState::default()
        };
        s.feeds.lines.insert(
            src.url.clone(),
            FeedLine {
                state: st,
                checking: false,
            },
        );
    }
}

// ------------------------------------------------------------------------------------ view

pub(crate) fn small(
    key: String,
    label: &str,
    primary: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    let (fg, fill) = if primary {
        (PAL.on_accent, PAL.accent)
    } else {
        (PAL.text, PAL.raised)
    };
    let b = row()
        .key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .h(28.0)
        .px(12.0)
        .gap(6.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(fill)
        .hover(if primary {
            rune::Color::BLACK.alpha(0.08)
        } else {
            PAL.hover()
        })
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            on(s);
        });
    let b = if primary {
        b
    } else {
        b.border(1.0, PAL.border_strong)
    };
    b.child(text(label.to_string()).font(theme::LABEL_BTN).color(fg))
}

pub(crate) fn size_text(n: u64) -> String {
    let gib = n as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib >= 1.0 {
        format!("{gib:.1} GiB")
    } else {
        format!("{:.0} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

/// A feed's status: "12 items · checked 14:02", "Checking…", the error and the retry.
pub(crate) fn status_line(s: &State, url: &str) -> (rune::Color, String) {
    let Some(l) = s.feeds.lines.get(url) else {
        return (PAL.faint, "Not checked yet".into());
    };
    if l.checking {
        return (PAL.faint, "Checking\u{2026}".into());
    }
    let clock = |t: i64| {
        let c = kotora_core::time::Clock::at(t, kotora_core::time::local_offset(t));
        format!("{:02}:{:02}", c.minute / 60, c.minute % 60)
    };
    match (&l.state.error, l.state.checked) {
        (Some(e), _) => (
            PAL.warn(),
            format!("{} \u{b7} retry {}", mask(e), clock(l.state.next)),
        ),
        (None, Some(t)) => (
            PAL.ok(),
            format!(
                "{} item{} \u{b7} checked {}",
                l.state.items,
                if l.state.items == 1 { "" } else { "s" },
                clock(t)
            ),
        ),
        (None, None) => (PAL.faint, "Not checked yet".into()),
    }
}

/// Settings → Feeds → Feeds: the list, the field, Add.
pub fn sources_block(s: &State) -> El {
    let f = &s.feeds;
    let mut c = col().key("set/feeds/list").w_full();
    for src in &f.cfg.sources {
        let (dot, line) = status_line(s, &src.url);
        let url = src.url.clone();
        c = c.child(
            row()
                .key(format!("set/feeds/feed/{}", mask(&src.url)))
                .gap(12.0)
                .py(6.0)
                .center_y()
                .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(
                            text(src.name.clone())
                                .font(theme::BODY_MED)
                                .color(PAL.text)
                                .truncate(),
                        )
                        .child(
                            text(mask(&src.url))
                                .font(TextStyle::new(11.0, 400.0).mono())
                                .color(PAL.dim)
                                .truncate(),
                        )
                        .child(text(line).font(theme::LABEL).color(PAL.dim)),
                )
                .child(small(
                    format!("set/feeds/remove/{}", mask(&src.url)),
                    "Remove",
                    false,
                    move |s| s.feeds_remove(&url),
                )),
        );
    }
    let input = row()
        .key("set/feeds/box")
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(
            1.0,
            if f.error.is_some() {
                PAL.warn()
            } else {
                PAL.border_strong
            },
        )
        .child(
            text_input(&f.draft, TextStyle::new(12.0, 400.0).mono())
                .key("set/feeds/input")
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(&f.draft, HINT), PAL.faint)
                .on_edit(|s: &mut State| &mut s.feeds.draft)
                .on_input_key(crate::ui::field_page_key)
                .on_input_change(|s: &mut State, _tx: &mut Tx| s.feeds_draft_changed())
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    s.feeds_add();
                }),
        );
    c = c.child(
        row()
            .key("set/feeds/add-row")
            .gap(8.0)
            .pt(8.0)
            .center_y()
            .child(input)
            .child(small("set/feeds/add".into(), "Add", false, |s| {
                s.feeds_add()
            })),
    );
    if let Some(e) = &f.error {
        c = c.child(
            text(e.clone())
                .key("set/feeds/error")
                .font(theme::LABEL)
                .color(PAL.warn())
                .wrap()
                .pt(4.0),
        );
    } else if let Some(n) = &f.note {
        c = c.child(
            text(n.clone())
                .key("set/feeds/note")
                .font(theme::LABEL)
                .color(PAL.dim)
                .wrap()
                .pt(4.0),
        );
    }
    if !f.cfg.sources.is_empty() {
        c = c.child(row().pt(8.0).child(spacer()).child(small(
            "set/feeds/check".into(),
            "Check now",
            false,
            |s| s.feeds_check_now(),
        )));
    }
    c
}

pub(crate) fn word_field(
    key: &'static str,
    edit: &TextEdit,
    hint: &str,
    get: fn(&mut State) -> &mut TextEdit,
    submit: fn(&mut State),
) -> El {
    row()
        .key(format!("{key}/box"))
        .role("textbox")
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .px(10.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(
            text_input(edit, TextStyle::new(12.0, 400.0))
                .key(key)
                .grow(1.0)
                .min_w(0.0)
                .single_line()
                .color(PAL.text)
                .placeholder(field_hint(edit, hint), PAL.faint)
                .on_edit(get)
                .on_input_key(crate::ui::field_page_key)
                .on_submit(move |s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    submit(s);
                }),
        )
}

pub const MINUTES: [u32; 4] = [10, 15, 30, 60];

pub fn minutes_label(m: u32) -> &'static str {
    match m {
        10 => "10 min",
        15 => "15 min",
        60 => "1 hour",
        _ => "30 min",
    }
}

pub fn minutes_slug(m: u32) -> &'static str {
    match m {
        10 => "10",
        15 => "15",
        60 => "60",
        _ => "30",
    }
}

/// The `↓ Ep 8` badge (list rows and details); a press opens the Feeds page.
pub fn badge(s: &State, id: u32) -> Option<El> {
    let (a, b) = s.feeds_badge(id)?;
    let label = format!("\u{2193} {}", ep_text(a, b));
    Some(
        row()
            .key(format!("feeds/badge/{id}"))
            .role("button")
            .label(format!("{} available in your feeds", ep_text(a, b)))
            .h(18.0)
            .px(6.0)
            .center_y()
            .radius(theme::R_XS)
            .fill(PAL.tint(PAL.accent, 0.18))
            .hover(PAL.hover())
            .on_press_tx(|s: &mut State, tx: &mut Tx| {
                tx.instant();
                s.open_feeds();
            })
            .child(
                text(label)
                    .font(theme::CAPTION.tracking(0.0))
                    .color(PAL.accent_hi),
            ),
    )
}

/// Settings → Feeds → Search address.
pub fn search_block(s: &State) -> El {
    row()
        .key("set/feeds/search")
        .gap(8.0)
        .w_full()
        .center_y()
        .child(word_field(
            "set/feeds/search-input",
            &s.feeds.search_edit,
            NYAA_SEARCH,
            |s| &mut s.feeds.search_edit,
            State::feeds_save_search,
        ))
        .child(small("set/feeds/search-save".into(), "Save", false, |s| {
            s.feeds_save_search()
        }))
}
