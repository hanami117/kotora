// Portions ported from Taiga (https://github.com/erengy/taiga), Copyright (C) Eren Okka and
// contributors, GPL-3.0: "Search for torrents" (src/ui/menu.cpp, src/track/feed_aggregator.cpp:
// the search address with %title% read for an anime on the list, its items examined and filtered
// as the feeds' are).

//! Torrents → Your shows: one row per anime you watch (then Plan to watch and On hold), with the
//! next episode, whether it is out, and the best release for it after your filters. The releases
//! come from your feeds and from a per-show search with the search address (Taiga's "Search for
//! torrents"), read in the background: two at a time, a pause between requests, `ETag` and
//! `If-Modified-Since`, again only after the feeds' interval, and only for shows that air or have
//! aired episodes you haven't watched.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kotora_core::Status;
use kotora_core::feeds::poll::{Fetched, fetch};
use kotora_core::feeds::taiga::{self, Entry as Release, ItemState};
use kotora_core::feeds::{Item, mask, search_url};
use rune::{El, Tx, col, el, row, spacer, text};

use crate::feeds::{ep_text, size_text, small};
use crate::model::{Air, Entry, State};
use crate::theme::{self, PAL};

/// Requests at once, and the pause each worker takes between two (nyaa is a shared site).
const AT_ONCE: usize = 2;
const PAUSE: Duration = Duration::from_millis(1500);
/// Shows searched at most (the first of Watching, then Plan to watch, then On hold).
const MAX_SHOWS: usize = 60;

/// One show's search.
#[derive(Clone, Debug, Default)]
pub struct Search {
    pub items: Vec<(i64, Item)>,
    pub entries: Vec<Release>,
    pub checked: Option<i64>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Shows {
    pub by_id: HashMap<u32, Search>,
    /// Rows whose other releases are shown.
    pub open: HashSet<u32>,
    /// Plan to watch and On hold start folded.
    pub unfolded: HashSet<u8>,
    worker: Option<Sender<Vec<Job>>>,
}

#[derive(Clone)]
struct Job {
    id: u32,
    url: String,
}

/// What a show's row says under its title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A release of this episode (or a range from it) is there.
    Out(u32),
    /// The next episode hasn't aired.
    NotOut(u32),
    UpToDate,
    Checking,
    NoReleases,
    /// No search address and no feed release.
    NoSearch,
}

impl Line {
    pub fn text(&self) -> String {
        match self {
            Line::Out(n) => format!("Ep {n} is out"),
            Line::NotOut(n) => format!("Ep {n} not out yet"),
            Line::UpToDate => "Up to date".into(),
            Line::Checking => "Checking\u{2026}".into(),
            Line::NoReleases => "No releases found".into(),
            Line::NoSearch => "Set a search address in Settings \u{2192} Feeds".into(),
        }
    }
}

pub struct Row {
    pub next: u32,
    pub line: Line,
    /// The best release first.
    pub releases: Vec<Release>,
}

/// Episodes aired: the next airing's number minus one, else the total once finished.
pub fn aired(e: &Entry) -> Option<u32> {
    match (&e.next_airing, e.air) {
        (Some(n), _) => Some(n.episode.saturating_sub(1)),
        (None, Air::Finished) => e.episodes,
        _ => None,
    }
}

/// Searched: airing, or with aired episodes not watched.
pub fn wanted(e: &Entry) -> bool {
    matches!(
        e.status,
        Status::Watching | Status::PlanToWatch | Status::OnHold
    ) && (e.air == Air::Airing
        || aired(e).is_some_and(|a| a > e.progress)
        || (e.air == Air::Unknown && e.episodes.is_none_or(|t| t > e.progress)))
}

fn height(r: &Release) -> u32 {
    r.release
        .parsed
        .resolution
        .as_deref()
        .and_then(taiga::resolution_height)
        .unwrap_or(0)
}

/// The show's row: its releases past the progress (feeds and search, once each, not discarded),
/// the first new episode with any; the best first (selected by your filters, then the higher
/// resolution, then more seeders).
pub fn row_of(s: &State, e: &Entry) -> Row {
    let next = e.progress + 1;
    let search = s.feeds.shows.by_id.get(&e.id);
    let mut seen = HashSet::new();
    let mut all: Vec<Release> = Vec::new();
    for r in s
        .feeds
        .entries
        .iter()
        .chain(search.into_iter().flat_map(|x| x.entries.iter()))
    {
        let Some((a, b)) = r.release.episodes else {
            continue;
        };
        if r.anime_id != Some(e.id)
            || r.state.is_discarded()
            || b < next
            || r.category == taiga::Category::Other
            || !seen.insert(r.item.key())
        {
            continue;
        }
        let _ = a;
        all.push(r.clone());
    }
    let first = all
        .iter()
        .map(|r| r.release.episodes.map_or(u32::MAX, |(a, _)| a.max(next)))
        .min();
    let mut releases: Vec<Release> = match first {
        Some(ep) => all
            .into_iter()
            .filter(|r| {
                r.release
                    .episodes
                    .is_some_and(|(a, b)| a.max(next) == ep || (a <= ep && ep <= b))
            })
            .collect(),
        None => Vec::new(),
    };
    releases.sort_by(|x, y| {
        (y.state == ItemState::Selected)
            .cmp(&(x.state == ItemState::Selected))
            .then(height(y).cmp(&height(x)))
            .then(
                y.item
                    .seeders
                    .unwrap_or(0)
                    .cmp(&x.item.seeders.unwrap_or(0)),
            )
    });
    let line = if let Some(ep) = first {
        Line::Out(ep)
    } else if e.episodes.is_some_and(|t| e.progress >= t) {
        Line::UpToDate
    } else if aired(e).is_some_and(|a| a < next) {
        Line::NotOut(next)
    } else if search.is_some_and(|x| x.loading || x.checked.is_none() && x.error.is_none()) {
        Line::Checking
    } else if search.is_some_and(|x| x.checked.is_some()) {
        Line::NoReleases
    } else if s.feeds.cfg.search.trim().is_empty() {
        Line::NoSearch
    } else if wanted(e) {
        Line::Checking
    } else {
        Line::NoReleases
    };
    Row {
        next,
        line,
        releases,
    }
}

impl State {
    /// The search results examined again (the filters or the list changed).
    pub fn shows_reexamine(&mut self) {
        let ids: Vec<u32> = self.feeds.shows.by_id.keys().copied().collect();
        for id in ids {
            let items = self.feeds.shows.by_id[&id].items.clone();
            let entries = self.feeds_examine(&items);
            if let Some(x) = self.feeds.shows.by_id.get_mut(&id) {
                x.entries = entries;
            }
        }
    }

    /// Asks the worker to search the shows that want it (each at most once per interval unless
    /// `force`). Nothing without a search address, feeds off, or headless.
    pub fn shows_refresh(&mut self, force: bool) {
        let cfg = &self.feeds.cfg;
        if !cfg.enabled
            || cfg.search.trim().is_empty()
            || self.handle.is_none()
            || kotora_core::paths::offline_forced()
        {
            return;
        }
        let now = kotora_core::time::now_unix();
        let every = i64::from(cfg.minutes()) * 60;
        let mut order: Vec<&Entry> = self.entries.iter().filter(|e| wanted(e)).collect();
        order.sort_by_key(|e| match e.status {
            Status::Watching => 0,
            Status::PlanToWatch => 1,
            _ => 2,
        });
        let mut jobs = Vec::new();
        for e in order.into_iter().take(MAX_SHOWS) {
            let x = self.feeds.shows.by_id.entry(e.id).or_default();
            if x.loading || (!force && x.checked.is_some_and(|t| now - t < every)) {
                continue;
            }
            if let Some(url) = search_url(&cfg.search, &e.main_title) {
                x.loading = true;
                jobs.push(Job { id: e.id, url });
            }
        }
        if jobs.is_empty() {
            return;
        }
        if self.feeds.shows.worker.is_none() {
            let Some(h) = self.handle.clone() else { return };
            let (tx, rx) = mpsc::channel();
            if std::thread::Builder::new()
                .name("kotora-shows".into())
                .spawn(move || run(h, rx))
                .is_err()
            {
                return;
            }
            self.feeds.shows.worker = Some(tx);
        }
        if let Some(w) = &self.feeds.shows.worker {
            let _ = w.send(jobs);
        }
    }

    pub fn shows_got(&mut self, id: u32, r: Result<Option<Vec<Item>>, String>) -> bool {
        let entries = match &r {
            Ok(Some(items)) => Some(
                self.feeds_examine(&items.iter().cloned().map(|i| (-1, i)).collect::<Vec<_>>()),
            ),
            _ => None,
        };
        let x = self.feeds.shows.by_id.entry(id).or_default();
        x.loading = false;
        x.checked = Some(kotora_core::time::now_unix());
        match r {
            Ok(Some(items)) => {
                x.items = items.into_iter().map(|i| (-1, i)).collect();
                x.entries = entries.unwrap_or_default();
                x.error = None;
            }
            Ok(None) => x.error = None,
            Err(e) => x.error = Some(mask(&e)),
        }
        true
    }
}

/// Per address: its `ETag` and `Last-Modified`.
type Validators = HashMap<String, (Option<String>, Option<String>)>;

/// Two workers pull the queue; each pauses between requests; `ETag`s are kept per address.
fn run(h: crate::model::Handle, rx: Receiver<Vec<Job>>) {
    let queue: Arc<Mutex<VecDeque<Job>>> = Arc::default();
    let tags: Arc<Mutex<Validators>> = Arc::default();
    loop {
        match rx.recv_timeout(Duration::from_secs(600)) {
            Ok(jobs) => {
                let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                for j in jobs {
                    if !q.iter().any(|x| x.id == j.id) {
                        q.push_back(j);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // The interval: the window asks again for the shows that are due.
                h.update_if(|s, _| {
                    s.shows_refresh(false);
                    false
                });
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
        std::thread::scope(|sc| {
            for _ in 0..AT_ONCE {
                let (queue, tags, h) = (queue.clone(), tags.clone(), h.clone());
                sc.spawn(move || {
                    loop {
                        let Some(job) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
                        else {
                            return;
                        };
                        let (etag, modified) = tags
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(&job.url)
                            .cloned()
                            .unwrap_or_default();
                        let r = match fetch(&job.url, etag.as_deref(), modified.as_deref()) {
                            Ok(Fetched::Same) => Ok(None),
                            Ok(Fetched::New {
                                body,
                                etag,
                                modified,
                            }) => {
                                tags.lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .insert(job.url.clone(), (etag, modified));
                                kotora_core::feeds::parse(&body)
                                    .map(|f| Some(f.items))
                                    .map_err(|e| e.to_string())
                            }
                            Err(e) => Err(e),
                        };
                        let id = job.id;
                        h.update_if(move |s, _| s.shows_got(id, r));
                        std::thread::sleep(PAUSE);
                    }
                });
            }
        });
    }
}

// ------------------------------------------------------------------------------------- view

fn facts(r: &Release) -> String {
    let mut v: Vec<String> = Vec::new();
    if let Some(g) = &r.release.parsed.group {
        v.push(g.clone());
    }
    let video = r.video_text();
    if !video.is_empty() {
        v.push(video);
    }
    if let Some(n) = r.item.size {
        v.push(size_text(n));
    }
    if let Some(n) = r.item.seeders {
        v.push(format!("{n} seeders"));
    }
    v.join(" \u{b7} ")
}

fn release_line(s: &State, id: u32, r: &Release, best: bool) -> El {
    let key = r.item.key();
    let busy = s.feeds.busy.contains(&key);
    let k = key.clone();
    row()
        .key(format!("shows/release/{id}/{key}"))
        .gap(12.0)
        .py(4.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(
                    text(r.item.title.clone())
                        .font(theme::LABEL)
                        .color(PAL.text)
                        .truncate(),
                )
                .child(text(facts(r)).font(theme::LABEL).color(PAL.dim).truncate()),
        )
        .child(small(
            format!("shows/download/{key}"),
            if busy {
                "Downloading\u{2026}"
            } else {
                "Download"
            },
            best,
            move |s| s.feeds_download(vec![k.clone()]),
        ))
}

fn show_row(s: &State, e: &Entry, inner: f32) -> El {
    let r = row_of(s, e);
    let id = e.id;
    let progress = match e.episodes {
        Some(t) => format!("{} of {t} watched", e.progress),
        None => format!("{} watched", e.progress),
    };
    let (line_color, line) = match &r.line {
        Line::Out(_) => (PAL.accent_hi, r.line.text()),
        Line::NotOut(_) | Line::UpToDate => (PAL.dim, r.line.text()),
        Line::NoReleases | Line::NoSearch => (PAL.faint, r.line.text()),
        Line::Checking => (PAL.faint, r.line.text()),
    };
    let next = if e.episodes.is_some_and(|t| e.progress >= t) {
        String::new()
    } else {
        format!("Next: {}", ep_text(r.next, r.next))
    };
    let mut left = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(e.title.clone())
                .font(theme::BODY_MED)
                .color(PAL.text)
                .truncate(),
        )
        .child(
            text(
                [progress, next]
                    .into_iter()
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" \u{b7} "),
            )
            .font(theme::LABEL)
            .color(PAL.dim)
            .truncate(),
        )
        .child(
            text(line)
                .key(format!("shows/line/{id}"))
                .font(theme::LABEL)
                .color(line_color)
                .truncate(),
        );
    let wide = inner > 760.0;
    let mut right = col().gap(2.0).items_end();
    if let Some(best) = r.releases.first() {
        let key = best.item.key();
        let busy = s.feeds.busy.contains(&key);
        let k = key.clone();
        let facts_el = text(facts(best))
            .font(theme::LABEL)
            .color(PAL.text)
            .truncate();
        right = right.child(
            row()
                .gap(10.0)
                .center_y()
                .child(if wide {
                    facts_el.max_w(320.0)
                } else {
                    facts_el.max_w(160.0)
                })
                .child(small(
                    format!("shows/download/{key}"),
                    if busy {
                        "Downloading\u{2026}"
                    } else {
                        "Download"
                    },
                    true,
                    move |s| s.feeds_download(vec![k.clone()]),
                )),
        );
        if r.releases.len() > 1 {
            let open = s.feeds.shows.open.contains(&id);
            let label = if open {
                "Fewer releases".to_string()
            } else {
                format!("+{} other releases", r.releases.len() - 1)
            };
            right = right.child(
                text(label)
                    .key(format!("shows/more/{id}"))
                    .role("button")
                    .font(theme::LABEL)
                    .color(PAL.accent_hi)
                    .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                        tx.instant();
                        if !s.feeds.shows.open.remove(&id) {
                            s.feeds.shows.open.insert(id);
                        }
                    }),
            );
        }
    }
    if !wide && r.releases.is_empty() {
        left = left.min_w(0.0);
    }
    let head = row()
        .key(format!("shows/row/{id}"))
        .gap(12.0)
        .py(6.0)
        .center_y()
        .child(crate::look::poster(
            s,
            id,
            &e.title,
            e.cover_url.as_deref(),
            40.0,
            56.0,
            theme::R_XS,
        ))
        .child(left)
        .child(right);
    let mut c = col().w_full().child(head);
    if s.feeds.shows.open.contains(&id) {
        let mut more = col().key(format!("shows/others/{id}")).pl(52.0).pb(6.0);
        for rel in r.releases.iter().skip(1).take(12) {
            more = more.child(release_line(s, id, rel, false));
        }
        c = c.child(more);
    }
    c
}

fn group(s: &State, slot: u8, status: Status, title: &str, inner: f32) -> Option<El> {
    let mut shows: Vec<&Entry> = s.entries.iter().filter(|e| e.status == status).collect();
    if shows.is_empty() {
        return None;
    }
    // Out first, then the rest by title.
    let out = |e: &Entry| matches!(row_of(s, e).line, Line::Out(_));
    shows.sort_by(|a, b| {
        out(b)
            .cmp(&out(a))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    let folded = slot > 0 && !s.feeds.shows.unfolded.contains(&slot);
    let n_out = shows.iter().filter(|e| out(e)).count();
    let head = row()
        .key(format!("shows/group/{slot}"))
        .role("button")
        .gap(8.0)
        .py(6.0)
        .center_y()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if !s.feeds.shows.unfolded.remove(&slot) {
                s.feeds.shows.unfolded.insert(slot);
            }
        })
        .child(
            text(if folded { "\u{25b8}" } else { "\u{25be}" })
                .font(theme::LABEL)
                .color(PAL.dim),
        )
        .child(text(title.to_string()).font(theme::SECTION).color(PAL.text))
        .child(
            text(format!(
                "{} shows{}",
                shows.len(),
                if n_out > 0 {
                    format!(" \u{b7} {n_out} with an episode out")
                } else {
                    String::new()
                }
            ))
            .font(theme::LABEL)
            .color(PAL.dim),
        )
        .child(spacer());
    let mut c = col().key(format!("shows/{slot}")).w_full().child(head);
    if !folded {
        for (i, e) in shows.iter().enumerate() {
            if i > 0 {
                c = c.child(el().h(1.0).w_full().fill(PAL.border));
            }
            c = c.child(show_row(s, e, inner));
        }
    }
    Some(c)
}

/// Torrents → Your shows.
pub fn view(s: &State, inner: f32) -> El {
    let mut body = col().key("shows").gap(14.0).w_full();
    let groups = [
        (0u8, Status::Watching, "Watching"),
        (1, Status::PlanToWatch, "Plan to watch"),
        (2, Status::OnHold, "On hold"),
    ];
    let mut any = false;
    for (slot, st, title) in groups {
        if let Some(g) = group(s, slot, st, title, inner) {
            body = body.child(g);
            any = true;
        }
    }
    if !any {
        body = body.child(
            text("Nothing in Watching, Plan to watch or On hold.")
                .key("shows/none")
                .font(theme::BODY)
                .color(PAL.dim)
                .py(12.0),
        );
    }
    if s.feeds.cfg.search.trim().is_empty() {
        body = body.child(text("Releases come from your feeds only. Set a search address (e.g. nyaa.si\u{2019}s) in Settings \u{2192} Feeds to look up each show.").key("shows/no-search").font(theme::LABEL).color(PAL.dim).wrap());
    }
    let f = &s.feeds;
    if !f.cfg.sources.is_empty() {
        let mut feeds = crate::ui::card("feeds/sources", "Your feeds");
        for src in &f.cfg.sources {
            let (dot, line) = crate::feeds::status_line(s, &src.url);
            feeds = feeds.child(
                row()
                    .gap(10.0)
                    .py(4.0)
                    .center_y()
                    .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
                    .child(
                        text(src.name.clone())
                            .font(theme::BODY)
                            .color(PAL.text)
                            .truncate()
                            .min_w(0.0),
                    )
                    .child(spacer())
                    .child(text(line).font(theme::LABEL).color(PAL.dim)),
            );
        }
        body = body.child(feeds);
    }
    body
}
