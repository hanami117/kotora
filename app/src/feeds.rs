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
use kotora_core::feeds::poll::{FeedState, Fetched, download_target, fetch};
use kotora_core::feeds::select::{Ctx, Filter, Marks, Selection, select};
use kotora_core::feeds::{Item, mask, nyaa_defaults, safe_http};
use kotora_core::recognize::{parse_release, relations::Relations};
use kotora_core::{Kind, ListItem, paths};
use rune::{El, ScrollSpec, TextEdit, TextStyle, Tx, col, el, row, spacer, text, text_input};

use crate::model::{Page, State};
use crate::theme::{self, PAL};
use crate::ui::{card, field_hint};

/// The empty feed field's hint (no source is built in; this only names an example).
pub const HINT: &str = "Paste an RSS feed URL, e.g. from nyaa.si";

pub const SWITCH_NOTE: &str = "Kotora checks only the feeds you add and opens releases in your torrent app. It never downloads anything itself.";

/// kv: the (series, episodes) already notified, so a restart doesn't repeat them.
const NOTIFIED_KEY: &str = "feeds:notified";
const NOTIFIED_KEEP: usize = 300;

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
    pub include: TextEdit,
    pub exclude: TextEdit,
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
    pub sel: Selection,
    /// Rows whose other releases are shown.
    pub open: HashSet<(u32, u32, u32)>,
    /// Already notified, loaded once from the store; `None` before the first check.
    pub notified: Option<Vec<(u32, u32, u32)>>,
    /// A Download running, and what it said last.
    pub busy: Option<(u32, u32, u32)>,
    pub said: Option<String>,
    worker: Option<Sender<Cmd>>,
}

enum Cmd {
    Set(Feeds),
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
}

fn words(s: &str) -> Vec<String> {
    s.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect()
}

/// The filter of Settings → Feeds.
pub fn filter_of(c: &Feeds) -> Filter {
    Filter { include: c.include.clone(), exclude: c.exclude.clone(), prefer_resolution: (c.resolution > 0).then_some(c.resolution), ..Filter::default() }
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
        self.feeds.include.text = c.feeds.include.join(", ");
        self.feeds.exclude.text = c.feeds.exclude.join(", ");
        self.feeds_apply();
    }

    /// Starts, tells or leaves the worker after a change.
    fn feeds_apply(&mut self) {
        if let Some(w) = &self.feeds.worker {
            let _ = w.send(Cmd::Set(self.feeds.cfg.clone()));
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
        if std::thread::Builder::new().name("kotora-feeds".into()).spawn(move || run(h, rx, cfg, service, anilist)).is_ok() {
            self.feeds.worker = Some(tx);
        }
    }

    /// The list changed: the rows follow (an episode watched drops its row).
    pub fn feeds_list(&mut self, items: &[ListItem]) {
        self.feeds.list = items.iter().filter(|it| it.media.kind == Kind::Anime).cloned().collect();
        if self.feeds.cfg.enabled {
            self.feeds_reselect();
        }
    }

    pub fn feeds_reselect(&mut self) {
        let f = &self.feeds;
        let empty = Relations::default();
        let rel = f.relations.as_deref().unwrap_or(&empty);
        let filter = filter_of(&f.cfg);
        let (aired, series) = (HashMap::new(), HashMap::new());
        let ctx = Ctx { list: &f.list, picks: &f.picks, relations: rel, aired: &aired, filter: &filter, series: &series, marks: &f.marks };
        self.feeds.sel = select(&f.items, &ctx);
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
        self.feeds_reselect();
        self.feeds_notify();
        true
    }

    /// One notification for the rows not notified before (never unsure ones).
    fn feeds_notify(&mut self) {
        let Some(done) = self.feeds.notified.as_mut() else { return };
        let fresh: Vec<(u32, u32, u32)> = self.feeds.sel.rows.iter().filter(|r| !r.version_out).map(|r| (r.media_id, r.episodes.0, r.episodes.1)).filter(|k| !done.contains(k)).collect();
        if fresh.is_empty() {
            return;
        }
        done.extend(fresh.iter().copied());
        let over = done.len().saturating_sub(NOTIFIED_KEEP);
        done.drain(..over);
        let keep: String = done.iter().map(|(m, a, b)| format!("{m}:{a}-{b}")).collect::<Vec<_>>().join(",");
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
        let parts: Vec<String> = fresh.iter().take(4).map(|(m, a, b)| format!("{} {}", self.feeds_title(*m), ep_text(*a, *b))).collect();
        let summary = if fresh.len() == 1 { "New release".to_string() } else { format!("{} new releases", fresh.len()) };
        let mut body = parts.join(", ");
        if fresh.len() > 4 {
            body.push_str(&format!(" and {} more", fresh.len() - 4));
        }
        self.notes.show(crate::notes::Note { id: "kotora-feeds".into(), kind: crate::notes::Kind::Aired, summary, body, tail: None });
    }

    pub fn feeds_title(&self, id: u32) -> String {
        self.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).unwrap_or_else(|| format!("Anime {id}"))
    }

    /// The badge's episode for a list entry: the first new one with a release.
    pub fn feeds_badge(&self, id: u32) -> Option<(u32, u32)> {
        if !self.feeds.cfg.enabled {
            return None;
        }
        self.feeds.sel.rows.iter().filter(|r| r.media_id == id && !r.version_out).map(|r| r.episodes).min()
    }

    /// New episodes for the sidebar badge.
    pub fn feeds_count(&self) -> usize {
        if self.feeds.cfg.enabled { self.feeds.sel.new_count() } else { 0 }
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
        let _ = std::thread::Builder::new().name("kotora-settings".into()).spawn(move || {
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

    pub fn set_feeds_resolution(&mut self, r: u32) {
        self.feeds.cfg.resolution = r;
        self.feeds_save("resolution", r.to_string());
        self.feeds_reselect();
    }

    pub fn set_feeds_minutes(&mut self, m: u32) {
        self.feeds.cfg.interval_minutes = m.max(Feeds::MIN_MINUTES);
        self.feeds_save("interval_minutes", self.feeds.cfg.interval_minutes.to_string());
        self.feeds_apply();
    }

    pub fn feeds_save_words(&mut self) {
        let (inc, exc) = (words(&self.feeds.include.text), words(&self.feeds.exclude.text));
        if inc == self.feeds.cfg.include && exc == self.feeds.cfg.exclude {
            return;
        }
        self.feeds_save("include", config::quote_list(&inc));
        self.feeds_save("exclude", config::quote_list(&exc));
        self.feeds.cfg.include = inc;
        self.feeds.cfg.exclude = exc;
        self.feeds_reselect();
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
                    self.feeds.error = Some("A feed address starts with https:// (or http://).".into());
                    return;
                }
            },
        };
        if self.feeds.cfg.sources.iter().any(|s| s.url == url) {
            self.feeds.error = Some("That feed is already added.".into());
            return;
        }
        let masked = mask(&url) != url;
        self.feeds.cfg.sources.push(FeedSource { name: name.clone(), url });
        self.feeds_save("sources", Feeds::sources_toml(&self.feeds.cfg.sources));
        self.feeds.draft = TextEdit::default();
        self.feeds.note = Some(if masked { format!("Added \u{201c}{name}\u{201d}. Its key is hidden here and never logged.") } else { format!("Added \u{201c}{name}\u{201d}.") });
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

    /// Download: the release's magnet or .torrent to the system's handler, then mark the
    /// episodes got (their other releases go).
    pub fn feeds_download(&mut self, item: usize, media: u32, eps: (u32, u32)) {
        let Some((_, it)) = self.feeds.items.get(item).cloned() else { return };
        let version = parse_release(&it.title).version;
        self.feeds.marks.got.insert((media, eps.0, eps.1), (it.info_hash.clone(), version));
        self.feeds_reselect();
        self.feeds.said = Some(format!("Opening {} {} in your torrent app\u{2026}", self.feeds_title(media), ep_text(eps.0, eps.1)));
        let Some(h) = self.handle.clone() else { return };
        self.feeds.busy = Some((media, eps.0, eps.1));
        let service = crate::account::name();
        let _ = std::thread::Builder::new().name("kotora-feed-open".into()).spawn(move || {
            let r = download_target(&it, &paths::cache_dir().join("torrents")).and_then(|t| crate::online::open_system(&t));
            if r.is_ok()
                && let Ok(st) = kotora_core::store::Store::open(&paths::db_file())
            {
                let at = kotora_core::time::now_unix();
                let _ = st.feed_mark(service, media, eps, &Mark::Got { info_hash: it.info_hash.clone(), version, title: it.title.clone() }, at);
            }
            h.update_if(move |s, _| {
                s.feeds.busy = None;
                if let Err(e) = r {
                    // Not opened: the row comes back.
                    s.feeds.marks.got.remove(&(media, eps.0, eps.1));
                    s.feeds_reselect();
                    s.feeds.said = Some(e);
                } else {
                    s.feeds.said = Some("Opened in your torrent app.".into());
                }
                true
            });
        });
    }
}

/// "Ep 8", "Ep 8–12".
pub fn ep_text(a: u32, b: u32) -> String {
    if a == b { format!("Ep {a}") } else { format!("Ep {a}\u{2013}{b}") }
}

// ---------------------------------------------------------------------------------- worker

fn run(h: crate::model::Handle, rx: Receiver<Cmd>, mut cfg: Feeds, service: &'static str, anilist: bool) {
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
                if !(force || st.due(now)) {
                    continue;
                }
                changed = true;
                match fetch(&src.url, st.etag.as_deref(), st.modified.as_deref()) {
                    Ok(Fetched::Same) => st.ok(now, cfg.minutes(), now as u64 ^ i as u64),
                    Ok(Fetched::New { body, etag, modified }) => match st.read(&body) {
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
                let pass = pass(&cfg, &states, &items, service, anilist, &mut relations, first);
                first = false;
                h.update_if(move |s, _| s.feeds_got(pass));
            }
        }
        // Asleep on the channel until the next feed is due (no timer while off).
        let wait = if cfg.active() {
            let now = kotora_core::time::now_unix();
            let next = cfg.sources.iter().filter_map(|s| states.get(&s.url)).map(|s| s.next).min().unwrap_or(now);
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
                states.retain(|url, _| c.sources.iter().any(|s| &s.url == url));
                items.retain(|url, _| c.sources.iter().any(|s| &s.url == url));
                cfg = c;
                // A removed feed's items go at once.
                if cfg.active() {
                    let pass = pass(&cfg, &states, &items, service, anilist, &mut relations, false);
                    h.update_if(move |s, _| s.feeds_got(pass));
                }
            }
            Ok(Cmd::Check) => force = true,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn pass(cfg: &Feeds, states: &HashMap<String, FeedState>, items: &HashMap<String, Vec<Item>>, service: &str, anilist: bool, relations: &mut Option<std::sync::Arc<Relations>>, first: bool) -> Pass {
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
    let marks = store.as_ref().and_then(|st| st.feed_marks(service).ok()).unwrap_or_default();
    let notified = if first {
        let text = store.as_ref().and_then(|st| st.kv_get(NOTIFIED_KEY).ok().flatten()).unwrap_or_default();
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
    let lines = states.iter().map(|(u, s)| (u.clone(), FeedLine { state: s.clone(), checking: false })).collect();
    Pass { items: all, lines, picks, marks, relations: Some(rel), notified }
}

/// The scenes' feeds: made-up releases for the fake list's shows, no worker.
pub fn fake(s: &mut State) {
    let list: Vec<ListItem> = s
        .entries
        .iter()
        .map(|e| ListItem {
            media: kotora_core::Media { id: e.id, title: e.main_title.clone(), title_en: e.title_en.clone(), episodes: e.episodes, format: "tv".into(), ..Default::default() },
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
    for e in s.entries.iter().filter(|e| matches!(e.status, kotora_core::Status::Watching | kotora_core::Status::PlanToWatch | kotora_core::Status::OnHold)).take(5) {
        let ep = e.progress + 1;
        if e.episodes.is_some_and(|c| ep > c) {
            continue;
        }
        for (g, res, seed, size) in [("Hoshi", "1080p", 412, 1_400_000_000u64), ("Tsuki", "720p", 96, 700_000_000), ("Kumo-Subs", "1080p HEVC", 51, 900_000_000)] {
            n += 1;
            let title = format!("[{g}] {} - {ep:02} [{res}]", e.main_title);
            items.push((
                0,
                Item { guid: format!("fake-{n}"), title, magnet: Some(format!("magnet:?xt=urn:btih:{n:040x}")), size: Some(size), seeders: Some(seed), page: Some(format!("https://tracker.example/view/{n}")), ..Item::default() },
            ));
        }
    }
    s.feeds.cfg.enabled = true;
    s.feeds.cfg.sources = vec![
        FeedSource { name: "nyaa.si: Hoshi".into(), url: "https://nyaa.si/user/Hoshi?page=rss&c=1_2".into() },
        FeedSource { name: "tracker.example".into(), url: "https://tracker.example/rss?passkey=0123456789abcdef".into() },
    ];
    s.feeds.items = items;
    s.feeds.notified = Some(Vec::new());
    s.feeds_reselect();
    let now = s.clock();
    for src in &s.feeds.cfg.sources {
        let st = FeedState { checked: Some(now - 600), next: now + 1200, items: 3, ..FeedState::default() };
        s.feeds.lines.insert(src.url.clone(), FeedLine { state: st, checking: false });
    }
}

// ------------------------------------------------------------------------------------ view

fn small(key: String, label: &str, primary: bool, on: impl Fn(&mut State) + 'static) -> El {
    let (fg, fill) = if primary { (PAL.on_accent, PAL.accent) } else { (PAL.text, PAL.raised) };
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
        .hover(if primary { rune::Color::BLACK.alpha(0.08) } else { PAL.hover() })
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            on(s);
        });
    let b = if primary { b } else { b.border(1.0, PAL.border_strong) };
    b.child(text(label.to_string()).font(theme::LABEL_BTN).color(fg))
}

fn size_text(n: u64) -> String {
    let gib = n as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib >= 1.0 { format!("{gib:.1} GiB") } else { format!("{:.0} MiB", n as f64 / (1024.0 * 1024.0)) }
}

/// "Hoshi · 1080p · 1.4 GiB · 412 seeders".
fn facts(it: &Item) -> String {
    let r = parse_release(&it.title);
    let mut v: Vec<String> = Vec::new();
    if let Some(g) = r.parsed.group {
        v.push(g);
    }
    if let Some(res) = r.parsed.resolution {
        v.push(res);
    }
    if let Some(ver) = r.version {
        v.push(format!("v{ver}"));
    }
    if let Some(sz) = it.size {
        v.push(size_text(sz));
    }
    if let Some(n) = it.seeders {
        v.push(format!("{n} seeder{}", if n == 1 { "" } else { "s" }));
    }
    v.join(" \u{b7} ")
}

/// A feed's status: "12 items · checked 14:02", "Checking…", the error and the retry.
fn status_line(s: &State, url: &str) -> (rune::Color, String) {
    let Some(l) = s.feeds.lines.get(url) else { return (PAL.faint, "Not checked yet".into()) };
    if l.checking {
        return (PAL.faint, "Checking\u{2026}".into());
    }
    let clock = |t: i64| {
        let c = kotora_core::time::Clock::at(t, kotora_core::time::local_offset(t));
        format!("{:02}:{:02}", c.minute / 60, c.minute % 60)
    };
    match (&l.state.error, l.state.checked) {
        (Some(e), _) => (PAL.warn(), format!("{} \u{b7} retry {}", mask(e), clock(l.state.next))),
        (None, Some(t)) => (PAL.ok(), format!("{} item{} \u{b7} checked {}", l.state.items, if l.state.items == 1 { "" } else { "s" }, clock(t))),
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
                        .child(text(src.name.clone()).font(theme::BODY_MED).color(PAL.text).truncate())
                        .child(text(mask(&src.url)).font(TextStyle::new(11.0, 400.0).mono()).color(PAL.dim).truncate())
                        .child(text(line).font(theme::LABEL).color(PAL.dim)),
                )
                .child(small(format!("set/feeds/remove/{}", mask(&src.url)), "Remove", false, move |s| s.feeds_remove(&url))),
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
        .border(1.0, if f.error.is_some() { PAL.warn() } else { PAL.border_strong })
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
    c = c.child(row().key("set/feeds/add-row").gap(8.0).pt(8.0).center_y().child(input).child(small("set/feeds/add".into(), "Add", false, |s| s.feeds_add())));
    if let Some(e) = &f.error {
        c = c.child(text(e.clone()).key("set/feeds/error").font(theme::LABEL).color(PAL.warn()).wrap().pt(4.0));
    } else if let Some(n) = &f.note {
        c = c.child(text(n.clone()).key("set/feeds/note").font(theme::LABEL).color(PAL.dim).wrap().pt(4.0));
    }
    if !f.cfg.sources.is_empty() {
        c = c.child(row().pt(8.0).child(spacer()).child(small("set/feeds/check".into(), "Check now", false, |s| s.feeds_check_now())));
    }
    c
}

fn word_field(key: &'static str, edit: &TextEdit, hint: &str, get: fn(&mut State) -> &mut TextEdit) -> El {
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
                .on_submit(|s: &mut State, tx: &mut Tx| {
                    tx.blur();
                    s.feeds_save_words();
                }),
        )
}

/// Settings → Feeds → Words: include and exclude, plain words separated by commas.
pub fn words_block(s: &State) -> El {
    col()
        .key("set/feeds/words")
        .gap(8.0)
        .w_full()
        .child(row().gap(10.0).center_y().child(text("Must have").font(theme::LABEL).color(PAL.dim).w(80.0)).child(word_field("set/feeds/include", &s.feeds.include, "e.g. multi sub", |s| &mut s.feeds.include)))
        .child(row().gap(10.0).center_y().child(text("Never").font(theme::LABEL).color(PAL.dim).w(80.0)).child(word_field("set/feeds/exclude", &s.feeds.exclude, "e.g. dub, raw", |s| &mut s.feeds.exclude)))
        .child(row().child(spacer()).child(small("set/feeds/save-words".into(), "Save words", false, |s| s.feeds_save_words())))
}

pub const RESOLUTIONS: [u32; 5] = [0, 2160, 1080, 720, 480];
pub const MINUTES: [u32; 4] = [10, 15, 30, 60];

pub fn resolution_label(r: u32) -> &'static str {
    match r {
        2160 => "2160p",
        1080 => "1080p",
        720 => "720p",
        480 => "480p",
        _ => "Highest",
    }
}

pub fn resolution_slug(r: u32) -> &'static str {
    match r {
        2160 => "2160",
        1080 => "1080",
        720 => "720",
        480 => "480",
        _ => "any",
    }
}

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
            .child(text(label).font(theme::CAPTION.tracking(0.0)).color(PAL.accent_hi)),
    )
}

/// Details → under "Your list": the releases of the next new episode, with Download.
pub fn details_card(s: &State, id: u32) -> Option<El> {
    let r = s.feeds.sel.rows.iter().filter(|r| r.media_id == id && !r.version_out).min_by_key(|r| r.episodes)?;
    let mut c = card("det-feeds", "Available");
    c = c.child(release_line(s, r.media_id, r.episodes, r.releases[0], true, "det"));
    if r.releases.len() > 1 {
        c = c.child(text(format!("{} more in Feeds", r.releases.len() - 1)).font(theme::LABEL).color(PAL.dim).pt(6.0));
    }
    Some(c)
}

fn release_line(s: &State, media: u32, eps: (u32, u32), item: usize, best: bool, prefix: &str) -> El {
    let it = &s.feeds.items[item].1;
    let busy = s.feeds.busy == Some((media, eps.0, eps.1));
    let label = if busy { "Opening\u{2026}" } else { "Download" };
    row()
        .key(format!("{prefix}/release/{media}/{}-{}/{item}", eps.0, eps.1))
        .gap(12.0)
        .py(6.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(it.title.clone()).font(if best { theme::BODY_MED } else { theme::BODY }).color(PAL.text).truncate())
                .child(text(facts(it)).font(theme::LABEL).color(PAL.dim).truncate()),
        )
        .child(small(format!("{prefix}/download/{media}/{}-{}/{item}", eps.0, eps.1), label, best, move |s| s.feeds_download(item, media, eps)))
}

/// The Feeds page: new episodes for the list, then what needs a look, then the feeds.
pub fn page(s: &State, width: f32) -> El {
    let inner = (width.min(1000.0) - 2.0 * theme::PAGE_PAD).max(320.0);
    let f = &s.feeds;
    let mut body = col().gap(16.0).w_full();
    if f.cfg.sources.is_empty() {
        body = body.child(
            col()
                .key("feeds/empty")
                .gap(10.0)
                .py(30.0)
                .items_center()
                .w_full()
                .child(text("No feeds yet").font(theme::BODY_MED).color(PAL.text))
                .child(text("Add an RSS feed in Settings \u{2192} Feeds. Kotora checks only the feeds you add.").font(theme::BODY).color(PAL.dim).wrap())
                .child(small("feeds/open-settings".into(), "Open Settings", true, |s| crate::settings::open(s, crate::settings::Section::Feeds))),
        );
    } else {
        let mut list = card("feeds/new", "New for your list");
        if f.sel.rows.is_empty() {
            let waiting = f.lines.is_empty() || f.lines.values().any(|l| l.checking);
            list = list.child(text(if waiting { "Checking your feeds\u{2026}" } else { "Nothing new for Watching, Plan to watch or On hold." }).key("feeds/none").font(theme::BODY).color(PAL.dim).py(8.0));
        }
        for r in &f.sel.rows {
            let key = (r.media_id, r.episodes.0, r.episodes.1);
            let title = s.feeds_title(r.media_id);
            let mut head = row().gap(8.0).center_y().pt(10.0).child(text(title).font(theme::BODY_MED).color(PAL.text).truncate().min_w(0.0)).child(crate::ui::tag_tinted(&ep_text(r.episodes.0, r.episodes.1), PAL.accent_hi, PAL.tint(PAL.accent, 0.14)));
            if r.version_out {
                head = head.child(crate::ui::tag("newer version", PAL.warn(), false));
            }
            head = head.child(spacer());
            if r.releases.len() > 1 {
                let open = f.open.contains(&key);
                let label = if open { "Fewer".to_string() } else { format!("{} more", r.releases.len() - 1) };
                head = head.child(
                    text(label)
                        .key(format!("feeds/more/{}/{}-{}", r.media_id, r.episodes.0, r.episodes.1))
                        .role("button")
                        .font(theme::LABEL)
                        .color(PAL.accent_hi)
                        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                            tx.instant();
                            if !s.feeds.open.remove(&key) {
                                s.feeds.open.insert(key);
                            }
                        }),
                );
            }
            let mut group = col().key(format!("feeds/row/{}/{}-{}", r.media_id, r.episodes.0, r.episodes.1)).w_full().child(head);
            group = group.child(release_line(s, r.media_id, r.episodes, r.releases[0], true, "feeds"));
            if f.open.contains(&key) {
                for &i in &r.releases[1..] {
                    group = group.child(release_line(s, r.media_id, r.episodes, i, false, "feeds"));
                }
            }
            list = list.child(group);
        }
        body = body.child(list);
        if !f.sel.unsure.is_empty() {
            let mut look = card("feeds/unsure", "Needs a look");
            look = look.child(text("These might be shows on your list. They never count as new.").font(theme::LABEL).color(PAL.dim));
            for u in f.sel.unsure.iter().take(20) {
                look = look.child(
                    col()
                        .gap(2.0)
                        .py(6.0)
                        .child(text(f.items[u.item].1.title.clone()).font(theme::BODY).color(PAL.text).truncate())
                        .child(text(format!("{}? {}", s.feeds_title(u.media_id), u.why)).font(theme::LABEL).color(PAL.dim).truncate()),
                );
            }
            body = body.child(look);
        }
        let mut feeds = card("feeds/sources", "Your feeds");
        for src in &f.cfg.sources {
            let (dot, line) = status_line(s, &src.url);
            feeds = feeds.child(row().gap(10.0).py(4.0).center_y().child(el().size(7.0, 7.0).radius(3.5).fill(dot)).child(text(src.name.clone()).font(theme::BODY).color(PAL.text).truncate().min_w(0.0)).child(spacer()).child(text(line).font(theme::LABEL).color(PAL.dim)));
        }
        body = body.child(feeds);
    }
    if let Some(said) = &f.said {
        body = body.child(text(said.clone()).key("feeds/said").font(theme::LABEL).color(PAL.dim).wrap());
    }
    let check = (!f.cfg.sources.is_empty()).then(|| small("feeds/check".into(), "Check now", false, |s| s.feeds_check_now()));
    col()
        .key("page/feeds")
        .w_full()
        .h_full()
        .child(crate::ui::top(s, "Feeds", Some(f.sel.new_count()), false))
        .children(check.map(|b| row().px(theme::PAGE_PAD).pb(8.0).child(spacer()).child(b)))
        .child(
            col()
                .key("feeds-scroll")
                .scroll(ScrollSpec { stick_bottom: false })
                .grow(1.0)
                .min_h(0.0)
                .child(col().px(theme::PAGE_PAD).pt(4.0).pb(theme::PAGE_PAD).w(inner + 2.0 * theme::PAGE_PAD).child(body)),
        )
}
