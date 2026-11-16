//! Search and add (Stage 5). The search page (the sidebar's Search, Ctrl K) shows the user's own
//! entries that match (every status), then MyAnimeList's results that are not on the list, each
//! with "+ Watching" and a menu of the other statuses. The list page's search box is a quick
//! filter: the list's matches, and one line that takes the query to the search page.
//!
//! The MyAnimeList search runs only when signed in and online, for 3 characters or more, after
//! the typing stops for [`DEBOUNCE`]: each keystroke bumps a number and a short thread sleeps, then
//! asks only if its number is still the newest. Answers are kept [`CACHE_SECS`] in memory (the
//! last [`CACHE_MAX`] queries), so going back and forth costs nothing. Every request goes through
//! the app's one MAL rate budget. Adding goes through the sync queue, so it works offline too.
//!
//! A MyAnimeList id in the box opens that anime's details page: a pasted
//! `myanimelist.net/anime/<id>[/slug]` link at once, a bare number (it may also be a title,
//! such as "86") with Enter or the "Open" line above the results ([`mal_id`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use kotora_core::Status;
use kotora_core::service::AnimeHit;
use rune::{El, Tx, col, el, icon, row, spacer, text};

use crate::icons;
use crate::model::{Auth, Page, State, kind_label};
use crate::theme::{self, PAL};
use crate::ui::{cell, cover, ghost, tag_tinted};

/// The pause in typing before MyAnimeList is asked.
pub const DEBOUNCE: Duration = Duration::from_millis(400);
/// How long an answer is reused.
pub const CACHE_SECS: i64 = 600;
/// Answers kept.
pub const CACHE_MAX: usize = 20;
/// Results asked for.
pub const LIMIT: u32 = 10;

/// The MyAnimeList part of the search.
#[derive(Default)]
pub struct Online {
    /// The query the results are for (trimmed, lower case).
    pub query: String,
    pub hits: Vec<AnimeHit>,
    /// Waiting for the pause or the answer.
    pub pending: bool,
    pub error: Option<String>,
    /// The result whose status menu is open.
    pub menu: Option<u32>,
    /// Recent answers: query, when, results.
    cache: Vec<(String, i64, Vec<AnimeHit>)>,
    /// Bumped by every change of the query; an older search's answer is dropped.
    seq: Arc<AtomicU64>,
}

impl Online {
    /// Scenes and tests: results as if MyAnimeList had answered.
    pub fn show(&mut self, query: &str, hits: Vec<AnimeHit>) {
        self.query = norm(query);
        self.hits = hits;
        self.pending = false;
        self.error = None;
    }
}

/// A MyAnimeList anime id in the search box: `Some((id, true))` for a pasted link
/// (`https://myanimelist.net/anime/41590/slug`, with or without the scheme or `www.`, or
/// `anime.php?id=41590`), `Some((id, false))` for a bare number.
pub fn mal_id(q: &str) -> Option<(u32, bool)> {
    let q = q.trim();
    let num = |t: &str| t.parse::<u32>().ok().filter(|&n| n > 0 && !t.starts_with('+'));
    if !q.is_empty() && q.len() <= 9 && q.bytes().all(|b| b.is_ascii_digit()) {
        return num(q).map(|id| (id, false));
    }
    let rest = q.strip_prefix("https://").or_else(|| q.strip_prefix("http://")).unwrap_or(q);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = rest.strip_prefix("myanimelist.net/")?;
    let digits = |t: &str| -> Option<u32> {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        num(&t[..end])
    };
    if let Some(t) = path.strip_prefix("anime/") {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        // `anime/41590`, `anime/41590/slug`, `anime/41590?…`; not `anime/season`.
        return (end > 0 && matches!(t[end..].chars().next(), None | Some('/' | '?' | '#'))).then(|| digits(t)).flatten().map(|id| (id, true));
    }
    let query = path.strip_prefix("anime.php?")?;
    query.split('&').find_map(|kv| kv.strip_prefix("id=")).and_then(digits).map(|id| (id, true))
}

/// An AniList anime id in the search box: a pasted `anilist.co/anime/170001[/slug]` link, or a
/// bare number (as [`mal_id`]).
pub fn anilist_id(q: &str) -> Option<(u32, bool)> {
    let q = q.trim();
    if let Some(bare) = mal_id(q).filter(|(_, link)| !link) {
        return Some(bare);
    }
    let rest = q.strip_prefix("https://").or_else(|| q.strip_prefix("http://")).unwrap_or(q);
    let t = rest.strip_prefix("www.").unwrap_or(rest).strip_prefix("anilist.co/anime/")?;
    let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let ok = end > 0 && matches!(t[end..].chars().next(), None | Some('/' | '?' | '#'));
    t[..end].parse::<u32>().ok().filter(|n| ok && *n > 0).map(|id| (id, true))
}

/// The active service's id in the search box (a link of that service, or a number).
pub fn link_id(q: &str, svc: kotora_core::service::ServiceId) -> Option<(u32, bool)> {
    match svc {
        kotora_core::service::ServiceId::Mal => mal_id(q),
        kotora_core::service::ServiceId::AniList => anilist_id(q),
    }
}

fn norm(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Why the service isn't searched now, if it isn't.
pub fn blocked(s: &State) -> Option<String> {
    let q = norm(s.search_text());
    let svc = s.service.title();
    if s.auth == Auth::Expired {
        return Some(format!("Sign in again to search {svc}."));
    }
    if s.offline {
        return Some(format!("Offline: searching {svc} needs the network. Your list is searched above."));
    }
    if q.chars().count() < 3 {
        return Some(format!("Type 3 letters or more to search {svc} too."));
    }
    None
}

impl State {
    /// Every edit of the search box: the list part at once, MyAnimeList after a pause. A pasted
    /// MyAnimeList link opens its details page at once (and leaves the box empty).
    pub fn search_changed(&mut self) {
        if let Some((id, true)) = link_id(self.search_text(), self.service) {
            if self.page == Page::Search { self.find.replace_text("") } else { self.query.replace_text("") }
            self.refresh();
            self.open_mal_id(id);
            return;
        }
        self.refresh();
        // The list page's box only filters the list.
        if self.page != Page::Search {
            return;
        }
        let q = norm(self.search_text());
        let blocked = blocked(self).is_some() || self.auth != Auth::SignedIn;
        let now = self.clock();
        let o = &mut self.online;
        if q == o.query && (o.pending || o.error.is_some() || !o.hits.is_empty()) {
            return;
        }
        o.query = q.clone();
        o.menu = None;
        o.error = None;
        let seq = o.seq.fetch_add(1, Ordering::Relaxed) + 1;
        if blocked {
            o.hits.clear();
            o.pending = false;
            return;
        }
        o.cache.retain(|(_, at, _)| now - at < CACHE_SECS);
        if let Some((_, _, hits)) = o.cache.iter().find(|(cq, _, _)| *cq == q) {
            o.hits = hits.clone();
            o.pending = false;
            return;
        }
        o.hits.clear();
        o.pending = true;
        let Some(h) = self.handle.clone() else { return };
        let g = o.seq.clone();
        let _ = std::thread::Builder::new().name("kotora-search".into()).spawn(move || {
            std::thread::sleep(DEBOUNCE);
            if g.load(Ordering::Relaxed) != seq {
                return;
            }
            let r = match crate::account::open() {
                Some(mut svc) => svc.browse(&q, LIMIT).map_err(crate::online::error_text),
                None => Err(format!("Sign in to search {}.", crate::account::active().title())),
            };
            h.update_if(move |s, _| s.online_answer(seq, q, r));
        });
    }

    /// The search thread's answer; an old one is dropped.
    pub fn online_answer(&mut self, seq: u64, q: String, r: Result<Vec<AnimeHit>, String>) -> bool {
        let now = self.clock();
        let o = &mut self.online;
        if o.seq.load(Ordering::Relaxed) != seq {
            return false;
        }
        o.pending = false;
        match r {
            Ok(hits) => {
                o.cache.retain(|(cq, _, _)| *cq != q);
                o.cache.push((q, now, hits.clone()));
                if o.cache.len() > CACHE_MAX {
                    o.cache.remove(0);
                }
                o.hits = hits;
            }
            Err(e) => o.error = Some(e),
        }
        true
    }

    /// Retry after an error: forget it and ask again.
    pub fn search_again(&mut self) {
        self.online.error = None;
        self.online.query.clear();
        self.search_changed();
    }

    /// "+ Watching" or a status from the menu.
    pub fn add_hit(&mut self, id: u32, status: Status) {
        self.online.menu = None;
        let Some(hit) = self.online.hits.iter().find(|h| h.id == id) else { return };
        if let Err(e) = self.add_media(hit.media(), status) {
            self.error = Some(e);
        }
    }

    /// The details page of a MyAnimeList id (works for anime not on the list): the entry's or
    /// the result's title until the information arrives.
    pub fn open_mal_id(&mut self, id: u32) {
        self.online.menu = None;
        let stub = match (self.entries.iter().find(|e| e.id == id), self.online.hits.iter().find(|h| h.id == id)) {
            (Some(e), _) => crate::details::Stub::from_entry(e),
            (None, Some(h)) => crate::details::Stub::from_hit(h, self.prefs.titles),
            (None, None) => crate::details::Stub::bare(id, &format!("{} #{id}", self.service.title())),
        };
        self.open_details(stub);
    }

    /// Enter in the search box: a bare id opens its page. Returns whether it did.
    pub fn search_submit(&mut self) -> bool {
        let Some((id, _)) = link_id(self.search_text(), self.service) else { return false };
        self.open_mal_id(id);
        true
    }

    /// The search page, its field focused (the sidebar's Search, Ctrl K). Escape returns.
    pub fn open_search(&mut self) {
        if !self.has_list() {
            return;
        }
        if self.page != Page::Search {
            self.search_back = self.page;
            self.editor = None;
            self.go(Page::Search);
        }
        self.find.focused = true;
        self.search_changed();
    }

    /// The list's quick filter, taken to the search page (its "Search MyAnimeList" line).
    pub fn search_for_query(&mut self) {
        let q = self.query.text.trim().to_string();
        self.find.replace_text(&q);
        self.open_search();
    }

    /// Escape on the search page: the page it was opened from.
    pub fn close_search(&mut self) {
        self.online.menu = None;
        let back = if self.search_back == Page::Search { Page::List(Status::Watching) } else { self.search_back };
        self.go(back);
    }

    /// Results that are not on the list (those are shown above them).
    pub fn new_hits(&self) -> Vec<&AnimeHit> {
        self.online.hits.iter().filter(|h| !self.entries.iter().any(|e| e.id == h.id)).collect()
    }
}

/// One line of the search results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SRow {
    /// The box holds a bare number: open that MyAnimeList id.
    Id(u32),
    /// "On your list" and the count.
    ListHead,
    /// Nothing on the list matches.
    ListNone,
    /// A list entry: an index into `State::rows`.
    List(usize),
    /// "On MyAnimeList".
    Head,
    /// A result: an index into [`State::new_hits`].
    Hit(usize),
    /// Why there are no results (yet).
    Note,
    /// The list page's filter: "Search MyAnimeList for …" (opens the search page).
    Find,
}

/// The lines of the search results, top to bottom.
pub fn layout(s: &State) -> Vec<SRow> {
    let mut v = Vec::new();
    if let Some((id, false)) = link_id(s.search_text(), s.service) {
        v.push(SRow::Id(id));
    }
    // The list page's filter: the way to MyAnimeList first, then the list's matches.
    if s.page != Page::Search {
        v.push(SRow::Find);
    }
    v.push(SRow::ListHead);
    if s.rows.is_empty() {
        v.push(SRow::ListNone);
    }
    v.extend((0..s.rows.len()).map(SRow::List));
    if s.page != Page::Search {
        return v;
    }
    v.push(SRow::Head);
    let n = if blocked(s).is_some() || s.online.pending || s.online.error.is_some() { 0 } else { s.new_hits().len() };
    if n == 0 {
        v.push(SRow::Note);
    }
    v.extend((0..n).map(SRow::Hit));
    v
}

/// A line's key (stable, unique). List rows use the media id, as on the list page.
pub fn key(s: &State, r: SRow) -> String {
    match r {
        SRow::Id(id) => format!("sr/id/{id}"),
        SRow::ListHead => "sr/list-head".into(),
        SRow::ListNone => "sr/list-none".into(),
        SRow::List(i) => s.rows.get(i).map_or_else(|| format!("gone-{i}"), |&k| s.entries[k].id.to_string()),
        SRow::Head => "sr/online-head".into(),
        SRow::Hit(i) => s.new_hits().get(i).map_or_else(|| format!("hit-gone-{i}"), |h| format!("hit/{}", h.id)),
        SRow::Note => "sr/note".into(),
        SRow::Find => "sr/find".into(),
    }
}

fn head(key: &str, label: String, end: Option<String>) -> El {
    let mut r = row()
        .key(key.to_string())
        .h(40.0)
        .px(theme::PAGE_PAD)
        .pt(12.0)
        .center_y()
        .child(text(label.to_uppercase()).font(theme::CAPTION).color(PAL.faint))
        .child(spacer());
    if let Some(e) = end {
        r = r.child(text(e).font(theme::LABEL).color(PAL.faint));
    }
    r
}

/// A line that is not a list entry (`ui::list_row` draws those).
pub fn line(s: &State, r: SRow) -> El {
    match r {
        SRow::Id(id) => id_line(s, id),
        SRow::ListHead => head("sr/list-head-line", format!("On your list \u{b7} {}", s.rows.len()), None),
        SRow::ListNone => text(format!("Nothing on your list matches \u{201c}{}\u{201d}.", s.search_text().trim()))
            .key("sr/list-none-line")
            .font(theme::BODY)
            .color(PAL.dim)
            .px(theme::PAGE_PAD)
            .py(10.0),
        SRow::Head => {
            let end = if s.online.pending {
                Some("Searching\u{2026}".to_string())
            } else if blocked(s).is_none() && s.online.error.is_none() && !s.online.hits.is_empty() {
                Some(format!("{} found", s.online.hits.len()))
            } else {
                None
            };
            head("sr/online-head-line", format!("On {}", s.service.title()), end)
        }
        SRow::Note => note(s),
        SRow::Find => find_line(s),
        SRow::Hit(i) => match s.new_hits().get(i) {
            Some(h) => hit_row(s, h),
            None => el(),
        },
        SRow::List(_) => el(),
    }
}

/// Above the list's matches: take the filter to the search page (MyAnimeList too).
fn find_line(s: &State) -> El {
    let kbd = if cfg!(target_os = "macos") { "\u{2318} K" } else { "Ctrl K" };
    row()
        .key("sr/find-line")
        .h(44.0)
        .px(theme::PAGE_PAD)
        .gap(10.0)
        .center_y()
        .role("button")
        .label(format!("Search {} too", s.service.title()))
        .hover(PAL.hover())
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.search_for_query();
        })
        .child(icon(&icons::SEARCH, 16.0).color(PAL.accent_hi))
        .child(text(format!("Search {} for \u{201c}{}\u{201d}", s.service.title(), s.query.text.trim())).font(theme::BODY_MED).color(PAL.text).truncate().min_w(0.0))
        .child(spacer())
        .child(crate::ui::kbd(kbd))
}

/// "MyAnimeList #41590 · Open (Enter)": a bare number may be an id as well as a title.
fn id_line(s: &State, id: u32) -> El {
    let known = s.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).or_else(|| s.online.hits.iter().find(|h| h.id == id).map(|h| h.title.clone()));
    let what = match known {
        Some(t) => format!("{} #{id}: {t}", s.service.title()),
        None => format!("{} #{id}", s.service.title()),
    };
    row()
        .key("sr/id-line")
        .h(44.0)
        .px(theme::PAGE_PAD)
        .gap(10.0)
        .center_y()
        .role("button")
        .label(format!("Open {} anime {id}", s.service.title()))
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_mal_id(id);
        })
        .child(icon(&icons::ARROW, 16.0).color(PAL.accent_hi))
        .child(text(format!("Open {what}")).font(theme::BODY_MED).color(PAL.text).truncate().min_w(0.0))
        .child(spacer())
        .child(crate::ui::kbd("Enter"))
}

fn note(s: &State) -> El {
    let base = row().key("sr/note-line").gap(10.0).center_y().px(theme::PAGE_PAD).py(10.0);
    if let Some(why) = blocked(s) {
        return base.child(text(why).font(theme::BODY).color(PAL.faint).wrap().grow(1.0).min_w(0.0));
    }
    if s.online.pending {
        return base.child(text(format!("Searching {}\u{2026}", s.service.title())).font(theme::BODY).color(PAL.faint));
    }
    if let Some(e) = &s.online.error {
        return base
            .child(text(e.clone()).font(theme::BODY).color(PAL.status(Status::Dropped)).wrap().grow(1.0).min_w(0.0))
            .child(ghost("sr/retry", "Try again", State::search_again).h(26.0));
    }
    let line = if s.online.hits.is_empty() {
        format!("{} found nothing for \u{201c}{}\u{201d}.", s.service.title(), s.search_text().trim())
    } else {
        format!("Everything {} found is on your list already.", s.service.title())
    };
    base.child(text(line).font(theme::BODY).color(PAL.faint))
}

/// "TV · 12 eps · Fall 2025 · ★ 8.12".
pub fn hit_meta(h: &AnimeHit) -> String {
    let mut bits = Vec::new();
    if let Some(n) = h.episodes {
        bits.push(if n == 1 { "1 ep".to_string() } else { format!("{n} eps") });
    }
    match h.air_status.as_str() {
        "currently_airing" => bits.push("Airing".into()),
        "not_yet_aired" => bits.push("Not yet aired".into()),
        _ => {}
    }
    if let Some(se) = h.season_text() {
        bits.push(se);
    }
    if let Some(m) = h.mean {
        bits.push(format!("\u{2605} {m:.2}"));
    }
    bits.join(" \u{b7} ")
}

/// The add button: outlined in orange, filled on hover (one primary action per view stays the
/// page's own; these are many).
fn add_button(id: u32) -> El {
    row()
        .key("add")
        .role("button")
        .label("Add as Watching")
        .focusable(true)
        .h(26.0)
        .px(10.0)
        .gap(4.0)
        .center_y()
        .radius(theme::R_SM)
        .border(1.0, PAL.accent.alpha(0.6))
        .hover(PAL.accent)
        .press(PAL.accent_lo)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.add_hit(id, Status::Watching);
        })
        .child(icon(&PLUS_ICON, 13.0).color(PAL.accent_hi).group_color(PAL.on_accent))
        .child(text("Watching").font(theme::LABEL_BTN).color(PAL.accent_hi).group_color(PAL.on_accent))
}

pub static PLUS_ICON: rune::Icon = rune::Icon {
    name: "kot-plus",
    view: 24.0,
    body: r#"<path d="M12 5v14M5 12h14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>"#,
};

fn menu_button(id: u32, open: bool) -> El {
    let b = el()
        .key("menu")
        .role("button")
        .label("Add with another status")
        .focusable(true)
        .size(26.0, 26.0)
        .radius(theme::R_SM)
        .border(1.0, PAL.border_strong)
        .center()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.online.menu = if s.online.menu == Some(id) { None } else { Some(id) };
        })
        .child(icon(&icons::DOWN, 14.0).color(PAL.dim));
    if open { b.fill(PAL.raised) } else { b }
}

/// The other statuses, under an open result.
pub fn status_menu(id: u32) -> El {
    let mut r = row().key("add-menu").gap(6.0).center_y().child(text("Add as".to_string()).font(theme::LABEL).color(PAL.faint).pr(4.0));
    for st in [Status::PlanToWatch, Status::Completed, Status::OnHold, Status::Dropped] {
        r = r.child(
            cell(format!("add/{}", st.slug()), format!("Add as {}", st.label()), false, move |s| s.add_hit(id, st))
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                .child(text(st.label()).font(theme::LABEL).color(PAL.dim)),
        );
    }
    r.child(ghost("add/cancel", "Cancel", |s| s.online.menu = None).h(28.0))
}

/// A MyAnimeList result: cover, title, what it is, and the add buttons. A press opens its page.
fn hit_row(s: &State, h: &AnimeHit) -> El {
    let id = h.id;
    let title = s.prefs.titles.pick(&h.title, h.title_en.as_deref(), h.title_ja.as_deref());
    let tile = cover(id, title, 34.0, 48.0, 12.0);
    let cov = match s.covers.get(id, h.cover_url.as_deref(), 34.0, 48.0) {
        Some(img) => tile.child(el().absolute(0.0, 0.0).size(34.0, 48.0).radius(theme::R_XS).pixels(img.pixels())),
        None => tile,
    };
    let kind = kind_label(&h.format);
    let mut sub = row().gap(8.0).center_y();
    if !kind.is_empty() {
        sub = sub.child(tag_tinted(kind, PAL.dim, PAL.raised));
    }
    sub = sub.child(text(hit_meta(h)).font(theme::LABEL_NUM).color(PAL.faint));
    // The second title: English under the main one, else the main one under the chosen one.
    let second = if title == h.title { h.title_en.as_deref() } else { Some(h.title.as_str()) };
    if let Some(t) = second.filter(|t| !t.eq_ignore_ascii_case(title)) {
        sub = sub.child(text(t.to_string()).font(theme::LABEL).color(PAL.dim).truncate().min_w(0.0));
    }
    let open = s.online.menu == Some(id);
    let stub = crate::details::Stub::from_hit(h, s.prefs.titles);
    let line = row()
        .key("line")
        .h(theme::ROW_H)
        .px(theme::PAGE_PAD)
        .gap(theme::COL_GAP)
        .center_y()
        .role("button")
        .label(format!("About {title}"))
        .hover(PAL.hover())
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.open_details(stub.clone());
        })
        .child(cov)
        .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(title.to_string()).font(theme::ROW_TITLE).color(PAL.text).truncate()).child(sub))
        .child(row().gap(6.0).center_y().child(add_button(id)).child(menu_button(id, open)));
    let mut c = col().w_full().child(if open { line.fill(PAL.surface) } else { line });
    if open {
        c = c.child(
            col()
                .key("menu-strip")
                .mx(theme::PAGE_PAD)
                .mb(10.0)
                .px(14.0)
                .py(10.0)
                .pl(62.0)
                .radius(theme::R_MD)
                .fill(PAL.surface)
                .border(1.0, PAL.border_strong)
                .child(status_menu(id)),
        );
    }
    c.child(el().w_full().h(1.0).fill(PAL.border.alpha(0.6)))
}

/// Whether the list page shows the search results.
pub fn shows(s: &State) -> bool {
    s.searching() && s.has_list() && matches!(s.page, Page::List(_))
}

/// Made-up results for the scenes (titles invented, no real data).
pub fn fake_hits() -> Vec<AnimeHit> {
    let h = |id: u32, title: &str, en: Option<&str>, format: &str, eps: Option<u32>, air: &str, season: Option<(u16, &str)>, mean: Option<f32>| AnimeHit {
        id,
        title: title.into(),
        title_en: en.map(Into::into),
        title_ja: None,
        synonyms: vec![],
        format: format.into(),
        episodes: eps,
        air_status: air.into(),
        season: season.map(|(y, s)| (y, s.to_string())),
        mean,
        cover_url: None,
        ..Default::default()
    };
    vec![
        h(9101, "Lanterns of Kirigaoka: Festival Night", Some("Lanterns: Festival Night"), "movie", Some(1), "finished_airing", Some((2026, "summer")), Some(8.31)),
        h(9102, "Lanterns of Kirigaoka 2nd Season", None, "tv", Some(12), "currently_airing", Some((2026, "fall")), Some(8.05)),
        h(9103, "Kirigaoka Lantern Shop", Some("The Lantern Shop on the Hill"), "ona", Some(24), "finished_airing", Some((2023, "spring")), Some(7.12)),
        h(9104, "Lanterns of Kirigaoka: Side Stories", None, "special", Some(6), "not_yet_aired", None, None),
        // On the fake list already (id 0): never shown among the results.
        h(0, "Lanterns of Kirigaoka", None, "tv", Some(12), "currently_airing", Some((2026, "fall")), Some(8.44)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The search page, empty.
    fn on_page() -> State {
        let mut s = State::fake();
        s.go(Page::Search);
        s
    }

    fn searching(q: &str) -> State {
        let mut s = on_page();
        s.find.replace_text(q);
        s.search_changed();
        s
    }

    #[test]
    fn mal_ids_and_links() {
        assert_eq!(mal_id("41590"), Some((41590, false)));
        assert_eq!(mal_id(" 86 "), Some((86, false)));
        for link in [
            "https://myanimelist.net/anime/41590",
            "https://myanimelist.net/anime/41590/Some_Title_2nd_Season",
            "http://www.myanimelist.net/anime/41590/",
            "myanimelist.net/anime/41590?q=x",
            "https://myanimelist.net/anime.php?id=41590",
        ] {
            assert_eq!(mal_id(link), Some((41590, true)), "{link}");
        }
        for not in ["", "0", "86 Eighty-Six", "+12", "1234567890", "https://myanimelist.net/manga/2", "https://myanimelist.net/anime/season", "https://example.org/anime/5", "https://myanimelist.net/anime/12abc"] {
            assert_eq!(mal_id(not), None, "{not}");
        }
    }

    /// A pasted link opens the page at once and empties the box; a number keeps the search
    /// and offers the id, which Enter opens. Both work for anime not on the list.
    #[test]
    fn anilist_links_and_ids() {
        use kotora_core::service::ServiceId;
        assert_eq!(link_id("170001", ServiceId::AniList), Some((170001, false)));
        for link in ["https://anilist.co/anime/170001", "https://anilist.co/anime/170001/Kirigaoka-no-Lantern/", "anilist.co/anime/170001?x=1"] {
            assert_eq!(link_id(link, ServiceId::AniList), Some((170001, true)), "{link}");
        }
        for not in ["https://anilist.co/manga/5", "https://anilist.co/anime/abc", "https://myanimelist.net/anime/41590"] {
            assert_eq!(link_id(not, ServiceId::AniList), None, "{not}");
        }
        assert_eq!(link_id("https://anilist.co/anime/170001", ServiceId::Mal), None, "each service its own links");
    }

    #[test]
    fn an_id_opens_the_details_page() {
        let mut s = searching("https://myanimelist.net/anime/41590/Velvet_Orchard");
        assert_eq!(s.page, Page::Details);
        assert!(s.find.text.is_empty());
        let d = s.detail.as_ref().unwrap();
        assert_eq!((d.id(), d.stub.title.as_str()), (41590, "MyAnimeList #41590"));
        s.details_back();
        assert_eq!(s.page, Page::Search);
        // The list page's filter box takes a link too.
        let mut s = State::fake();
        s.query.replace_text("myanimelist.net/anime/41590");
        s.search_changed();
        assert_eq!((s.page, s.detail.as_ref().map(|d| d.id())), (Page::Details, Some(41590)));
        s.details_back();
        assert!(matches!(s.page, Page::List(_)) && s.query.text.is_empty());

        let mut s = searching("41590");
        assert_eq!(s.page, Page::Search);
        assert_eq!(layout(&s).first(), Some(&SRow::Id(41590)));
        assert!(s.search_submit());
        assert_eq!(s.detail.as_ref().map(|d| d.id()), Some(41590));

        // An id on the list opens with the entry's title.
        let mut s = searching("3");
        assert!(s.search_submit());
        assert_eq!(s.detail.as_ref().map(|d| d.stub.title.as_str()), Some("Slow Life on Hoshizora Island"));
        let mut s = searching("kirigaoka");
        assert!(!s.search_submit() && !layout(&s).iter().any(|r| matches!(r, SRow::Id(_))));
    }

    #[test]
    fn short_queries_and_offline_never_ask() {
        let s = searching("ab");
        assert!(!s.online.pending && blocked(&s).unwrap().contains("3 letters"));
        let mut s = on_page();
        s.offline = true;
        s.find.replace_text("lantern");
        s.search_changed();
        assert!(!s.online.pending && blocked(&s).unwrap().contains("Offline"));
    }

    #[test]
    fn a_search_waits_then_old_answers_are_dropped_and_new_ones_cached() {
        let mut s = searching("lantern");
        assert!(s.online.pending, "waits for the pause");
        let first = s.online.seq.load(Ordering::Relaxed);
        s.find.replace_text("lanterns");
        s.search_changed();
        assert!(!s.online_answer(first, "lantern".into(), Ok(fake_hits())), "an older search's answer is dropped");
        let now = s.online.seq.load(Ordering::Relaxed);
        assert!(s.online_answer(now, "lanterns".into(), Ok(fake_hits())));
        assert_eq!(s.new_hits().len(), 4, "the one on the list is left out");
        // Back to the same query later: from memory, nothing asked.
        s.find.replace_text("lantern");
        s.search_changed();
        s.find.replace_text("lanterns");
        s.search_changed();
        assert!(!s.online.pending && s.online.hits.len() == 5);
    }

    #[test]
    fn adding_a_result_moves_it_to_the_list() {
        let mut s = on_page();
        s.find.replace_text("lantern");
        s.refresh();
        s.online.show("lantern", fake_hits());
        s.online.menu = Some(9103);
        s.add_hit(9103, Status::PlanToWatch);
        let e = s.entries.iter().find(|e| e.id == 9103).unwrap();
        assert_eq!((e.status, e.waiting), (Status::PlanToWatch, Some("Waiting to sync")));
        assert!(s.online.menu.is_none());
        assert!(s.new_hits().iter().all(|h| h.id != 9103));
        assert!(s.rows.iter().any(|&i| s.entries[i].id == 9103), "it matches, so it shows among the list's matches");
    }

    /// The list page's box only filters: no MyAnimeList search, one line to the search page,
    /// which takes the query along; Escape comes back.
    #[test]
    fn the_list_box_filters_and_hands_over() {
        let mut s = State::fake();
        s.go(Page::List(Status::Completed));
        s.query.replace_text("lantern");
        s.search_changed();
        assert!(!s.online.pending, "a filter never asks MyAnimeList");
        let l = layout(&s);
        assert_eq!((l[0], l[1], l[2]), (SRow::Find, SRow::ListHead, SRow::List(0)));
        assert!(!l.contains(&SRow::Head));
        let matches = s.rows.len();
        s.search_for_query();
        assert_eq!((s.page, s.find.text.as_str()), (Page::Search, "lantern"));
        assert!(s.online.pending && s.find.focused);
        assert_eq!(s.rows.len(), matches, "the list's matches show on the search page too");
        s.close_search();
        assert_eq!((s.page, s.query.text.as_str()), (Page::List(Status::Completed), "lantern"));
        // Signed out: no search page.
        let mut s = State::fake();
        s.auth = crate::model::Auth::SignedOut;
        s.open_search();
        assert_ne!(s.page, Page::Search);
    }

    #[test]
    fn the_layout_puts_the_list_first() {
        let mut s = on_page();
        s.find.replace_text("lantern");
        s.refresh();
        s.online.show("lantern", fake_hits());
        let l = layout(&s);
        assert_eq!(l[0], SRow::ListHead);
        assert_eq!(l[1], SRow::List(0));
        let head = l.iter().position(|r| *r == SRow::Head).unwrap();
        assert_eq!(l[head + 1..], [SRow::Hit(0), SRow::Hit(1), SRow::Hit(2), SRow::Hit(3)]);
        s.find.replace_text("zzzz");
        s.refresh();
        s.online.show("zzzz", vec![]);
        assert_eq!(layout(&s), [SRow::ListHead, SRow::ListNone, SRow::Head, SRow::Note]);
    }
}
