//! The details page (Stage 5): one anime's MyAnimeList information on the left and the user's
//! entry on the right (the inline editor's fields, through the sync queue), or "Add to list" when
//! it isn't on the list. It opens in the main area from a list row's title, a search result or
//! Now Playing, and Back (or Escape) returns to the page it came from.
//!
//! The page shows what is already known at once (a [`Stub`]: title, cover, type), then the
//! information: from the store's `details` cache if it is there (fresh for [`FRESH_SECS`]), else
//! from MyAnimeList (`GET /anime/{id}`, signed in and online), which is then saved. The cache keeps
//! titles on the list, and others only a week, the newest 100 (no hoarding; `Store::save_details`).
//! A fetch for an anime on the list also refreshes its saved titles, count and cover.

use kotora_core::service::{AnimeHit, Details};
use kotora_core::store::Store;
use kotora_core::time::now_unix;
use kotora_core::{Kind, ListItem, Status, paths};
use rune::{Color, El, ScrollSpec, TextStyle, Tx, col, el, icon, row, spacer, text};

use crate::icons;
use crate::model::{Entry, Page, State, kind_label};
use crate::theme::{self, PAL};
use crate::ui::{action, button, card, cell, ghost, kv, plus_button, step_button, tag, tag_tinted};

/// Saved information younger than this is shown without asking MyAnimeList again.
pub const FRESH_SECS: i64 = 3 * 86_400;

/// What a page knows before the information arrives (from the row it was opened from).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stub {
    pub id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    pub format: String,
    pub episodes: Option<u32>,
    /// Anime or manga (a manga's page: `manga.rs` draws its "Your list" card).
    pub kind: Kind,
}

impl Stub {
    pub fn from_hit(h: &AnimeHit, lang: kotora_core::config::TitleLang) -> Stub {
        Stub { id: h.id, title: lang.pick(&h.title, h.title_en.as_deref(), h.title_ja.as_deref()).to_string(), cover_url: h.cover_url.clone(), format: h.format.clone(), episodes: h.episodes, kind: h.kind }
    }

    pub fn from_entry(e: &Entry) -> Stub {
        Stub { id: e.id, title: e.title.clone(), cover_url: e.cover_url.clone(), format: String::new(), episodes: e.episodes, kind: Kind::Anime }
    }

    pub fn bare(id: u32, title: &str) -> Stub {
        Stub { id, title: title.into(), ..Default::default() }
    }
}

/// The open details page.
pub struct Detail {
    pub stub: Stub,
    /// The page Back returns to.
    pub back: Page,
    pub info: Option<Details>,
    /// A fetch runs.
    pub loading: bool,
    pub error: Option<String>,
}

impl Detail {
    pub fn id(&self) -> u32 {
        self.stub.id
    }

    pub fn kind(&self) -> Kind {
        self.stub.kind
    }

    pub fn is_manga(&self) -> bool {
        self.stub.kind == Kind::Manga
    }

    /// The id the cover cache and the tiles use (manga ids overlap anime ones).
    pub fn cover_id(&self) -> u32 {
        if self.is_manga() { crate::manga::cover_id(self.id()) } else { self.id() }
    }
}

/// The information page's answer.
#[derive(Debug)]
pub struct Answer {
    pub id: u32,
    pub kind: Kind,
    pub result: Result<Details, String>,
    /// The saved entry, refreshed from the information (on the list only).
    pub item: Option<ListItem>,
    /// Nothing more comes for this page.
    pub done: bool,
}

impl State {
    /// Opens the page for an anime and fetches its information (never in headless runs).
    pub fn open_details(&mut self, stub: Stub) {
        let back = match (&self.page, &self.detail) {
            (Page::Details, Some(d)) => d.back,
            (p, _) => *p,
        };
        let (id, kind) = (stub.id, stub.kind);
        self.editor = None;
        self.manga.editor = None;
        self.online.menu = None;
        self.detail = Some(Detail { stub, back, info: None, loading: self.handle.is_some(), error: None });
        self.go(Page::Details);
        self.fetch_details(id, kind);
        if kind == Kind::Manga {
            self.publishing_for_details(id);
            self.want_resume(id);
        }
    }

    fn fetch_details(&mut self, id: u32, kind: Kind) {
        let Some(h) = self.handle.clone() else { return };
        let send = move |a: Answer| h.update_if(move |s, _| s.details_answer(a));
        std::thread::spawn(move || {
            let store = Store::open(&paths::db_file()).ok();
            let now = now_unix();
            let saved = store.as_ref().and_then(|st| st.details((crate::account::name(), kind), id).ok().flatten());
            let fresh = saved.as_ref().is_some_and(|(_, at)| now - at < FRESH_SECS);
            let had = saved.is_some();
            if let Some((d, _)) = saved {
                send(Answer { id, kind, result: Ok(d), item: None, done: fresh });
                if fresh {
                    return;
                }
            }
            let result = match crate::account::open() {
                Some(mut mal) => mal.details_of(kind, id).map_err(crate::online::error_text),
                None => Err(format!("Offline or signed out: the information comes from {}.", crate::account::active().title())),
            };
            let mut item = None;
            if let (Ok(d), Some(st)) = (&result, &store) {
                let _ = st.save_details(crate::account::name(), d, now);
                if st.refresh_media(crate::account::name(), &d.media(), now).unwrap_or(false) {
                    item = st.load_item((crate::account::name(), kind), id).ok().flatten();
                }
            }
            match result {
                // Saved information already shows: a failed refresh only ends the loading.
                Err(_) if had => send(Answer { id, kind, result: Err(String::new()), item: None, done: true }),
                result => send(Answer { id, kind, result, item, done: true }),
            }
        });
    }

    /// The fetch's answer; one for another page is dropped.
    pub fn details_answer(&mut self, a: Answer) -> bool {
        // A changed airing status was saved: Started airing may be due.
        if a.item.is_some() && a.kind == Kind::Anime {
            self.kick_air_notes();
        }
        match &a.item {
            Some(it) if a.kind == Kind::Manga => {
                if let Some(e) = self.manga.entries.iter_mut().find(|e| e.id == it.media.id) {
                    let waiting = e.waiting;
                    *e = crate::manga::MEntry { waiting, ..crate::manga::MEntry::from_item(it, self.prefs.titles) };
                }
            }
            Some(it) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == it.media.id) {
                    let waiting = e.waiting;
                    *e = Entry { waiting, ..Entry::from_item(it, self.prefs.titles) };
                }
            }
            None => {}
        }
        let Some(d) = self.detail.as_mut().filter(|d| d.id() == a.id && d.kind() == a.kind) else { return a.item.is_some() };
        d.loading = !a.done;
        match a.result {
            Ok(info) => {
                d.info = Some(info);
                d.error = None;
            }
            Err(e) if e.is_empty() => {}
            Err(e) => d.error = Some(e),
        }
        true
    }

    /// Back (the button, Escape): the page the details were opened from.
    pub fn details_back(&mut self) {
        let back = self.detail.as_ref().map_or(Page::List(Status::Watching), |d| d.back);
        self.editor = None;
        self.manga.editor = None;
        self.detail = None;
        self.go(back);
    }

    /// Try again after an error.
    pub fn details_retry(&mut self) {
        let Some(d) = &mut self.detail else { return };
        d.error = None;
        d.loading = self.handle.is_some();
        let (id, kind) = (d.id(), d.kind());
        self.fetch_details(id, kind);
    }

    /// Now Playing, "Not on your list": add it as Watching at the playing episode (add and
    /// record in one change; no episode in the name: at 0), and from then on the file counts as
    /// this anime (as a pick in "Wrong anime…"). The banner says it was recorded, with Undo.
    pub fn add_playing(&mut self) {
        let Some(n) = self.now.as_ref() else { return };
        let kotora_core::recognize::Found::NotOnList { media_id, title } = n.found.clone() else { return };
        let episode = n.episode().filter(|ep| *ep > 0);
        // What recognition knows; the queue worker fills in the rest (episodes, type) from the
        // service right after the add.
        let media = kotora_core::Media { id: media_id, title, title_en: n.alt_titles.en.clone(), title_ja: n.alt_titles.ja.clone(), cover_url: n.cover_url.clone(), ..Default::default() };
        match self.add_media_at(media, Status::Watching, episode) {
            Ok(_) => {
                self.pick_anime(media_id);
                if let (Some(ep), Some(n)) = (episode, &mut self.now) {
                    n.recorded = Some(crate::now::Recorded {
                        id: media_id,
                        episode: ep,
                        old_progress: 0,
                        old_status: Status::Watching,
                        completed: false,
                        auto: false,
                        old_rewatching: false,
                        old_times: 0,
                        rewatch: false,
                    });
                    n.acted = true;
                    n.dismissed = false;
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// "Add to list" on the page (an anime, or a manga to the manga list).
    pub fn add_shown(&mut self, status: Status) {
        let Some(d) = &self.detail else { return };
        if d.is_manga() {
            let media = match &d.info {
                Some(info) => info.media(),
                None => kotora_core::Media { kind: Kind::Manga, id: d.stub.id, title: d.stub.title.clone(), episodes: d.stub.episodes, format: d.stub.format.clone(), cover_url: d.stub.cover_url.clone(), ..Default::default() },
            };
            if let Err(e) = self.manga_add(media, status) {
                self.error = Some(e);
            }
            return;
        }
        let media = match &d.info {
            Some(info) => info.media(),
            None => kotora_core::Media {
                id: d.stub.id,
                title: d.stub.title.clone(),
                title_en: None,
                title_ja: None,
                synonyms: vec![],
                episodes: d.stub.episodes,
                format: d.stub.format.clone(),
                air_status: String::new(),
                cover_url: d.stub.cover_url.clone(),
                start_date: None,
                broadcast: None,
                authors: Vec::new(),
                kind: Default::default(),
                volumes: None,
                next_airing: None,
                id_mal: None,
            },
        };
        if let Err(e) = self.add_media(media, status) {
            self.error = Some(e);
        }
    }
}

// ------------------------------------------------------------------------------------- texts

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `2025-10-03` → "Oct 3, 2025"; `2025-10` → "Oct 2025"; `2025` stays.
pub fn date_text(d: &str) -> String {
    let p: Vec<&str> = d.split('-').collect();
    let month = |m: &str| m.parse::<usize>().ok().filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    match p.as_slice() {
        [y, m, day] => match (month(m), day.parse::<u32>()) {
            (Some(m), Ok(day)) => format!("{m} {day}, {y}"),
            _ => d.to_string(),
        },
        [y, m] => month(m).map_or_else(|| d.to_string(), |m| format!("{m} {y}")),
        _ => d.to_string(),
    }
}

/// 98765 → "98,765".
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn air_label(air: &str) -> &'static str {
    match air {
        "currently_airing" => "Airing",
        "finished_airing" => "Finished airing",
        "not_yet_aired" => "Not yet aired",
        _ => "",
    }
}

/// `light_novel` → "Light novel".
fn source_label(s: &str) -> String {
    let t = s.replace('_', " ");
    let mut c = t.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

// -------------------------------------------------------------------------------------- view

/// `glass`: on the banner, see-through over the cover's tint.
fn link_button(key: &str, label: &str, url: String, glass: bool) -> El {
    let r = row()
        .key(key.to_string())
        .role("link")
        .label(label.to_string())
        .focusable(true)
        .h(30.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .hover(PAL.hover())
        .press(PAL.press())
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut State| crate::online::open_url(s, url.clone()))
        .child(text(label.to_string()).font(theme::LABEL_BTN).color(PAL.text))
        .child(icon(&icons::EXTERNAL, 14.0).color(PAL.dim));
    if glass { r.radius(15.0).fill(PAL.bg.alpha(0.6)).border(1.0, crate::look::glass_border()) } else { r.radius(theme::R_SM).fill(PAL.raised).border(1.0, PAL.border_strong) }
}

fn back_button(glass: bool) -> El {
    let r = row()
        .key("det/back")
        .role("button")
        .label("Back")
        .focusable(true)
        .h(30.0)
        .pl(6.0)
        .pr(12.0)
        .gap(4.0)
        .center_y()
        .hover(PAL.hover())
        .press(PAL.press())
        .on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.instant();
            s.details_back();
        })
        .child(icon(&BACK_ICON, 16.0).color(PAL.dim))
        .child(text("Back").font(theme::LABEL_BTN).color(PAL.text));
    if glass { r.radius(15.0).fill(PAL.bg.alpha(0.6)).border(1.0, crate::look::glass_border()) } else { r.radius(theme::R_SM).border(1.0, PAL.border_strong) }
}

pub static BACK_ICON: rune::Icon = rune::Icon {
    name: "kot-back",
    view: 24.0,
    body: r#"<path d="M15 5l-7 7 7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// A number chip on the banner ("#212 ranked"); the score one has a star.
fn stat(key: &str, big: String, small: &str, star: bool) -> El {
    let mut c = row()
        .key(key.to_string())
        .h(30.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(15.0)
        .fill(PAL.bg.alpha(0.55))
        .border(1.0, crate::look::glass_border());
    if star {
        c = c.fill(PAL.st_onhold.alpha(0.16)).border(1.0, PAL.st_onhold.alpha(0.4)).child(icon(&icons::STAR, 14.0).color(PAL.st_onhold));
    }
    c.child(text(big).font(TextStyle::new(14.0, 600.0).features("'tnum' 1")).color(PAL.text)).child(text(small.to_string()).font(theme::LABEL).color(PAL.dim))
}

/// The cover's URL: the information's, else the row's it was opened from.
fn cover_url(d: &Detail) -> Option<&str> {
    d.info.as_ref().and_then(|i| i.cover_url.as_deref()).or(d.stub.cover_url.as_deref())
}

/// The title shown (Settings → Titles).
fn shown_title<'a>(s: &State, d: &'a Detail) -> &'a str {
    let lang = s.prefs.titles;
    d.info.as_ref().map_or(d.stub.title.as_str(), |i| lang.pick(&i.title, i.title_en.as_deref(), i.title_ja.as_deref()))
}

fn big_cover(s: &State, d: &Detail, w: f32, h: f32) -> El {
    crate::look::poster(s, d.cover_id(), &d.stub.title, cover_url(d), w, h, theme::R_LG)
        .key("det-cover")
        .border(1.0, crate::look::glass_border())
        .shadow(12.0, 32.0, Color::BLACK.alpha(if PAL.light { 0.22 } else { 0.55 }))
}

/// The other titles under the one shown (main, English, Japanese) and the synonyms.
fn other_titles(s: &State, d: &Detail) -> El {
    let title = shown_title(s, d);
    let mut c = col().gap(3.0);
    if let Some(i) = &d.info {
        let mut line = row().flex_wrap().gap(10.0);
        for t in [Some(&i.title), i.title_en.as_ref(), i.title_ja.as_ref()].into_iter().flatten().filter(|t| t.as_str() != title) {
            line = line.child(text(t.clone()).font(TextStyle::new(14.0, 500.0)).color(PAL.dim).wrap().selectable(true));
        }
        c = c.child(line);
        if !i.synonyms.is_empty() {
            c = c.child(text(format!("Also: {}", i.synonyms.join(", "))).font(theme::LABEL).color(PAL.dim).wrap().selectable(true));
        }
    }
    c
}

/// A manga: type, chapters and volumes (when known), where it is in its run.
fn manga_tags(d: &Detail) -> El {
    let info = d.info.as_ref();
    let chip = PAL.bg.alpha(0.6);
    let mut tags = row().key("det-tags").flex_wrap().gap(6.0);
    let format = info.map_or(d.stub.format.as_str(), |i| i.format.as_str());
    if !format.is_empty() {
        tags = tags.child(tag_tinted(&kotora_core::mal::manga_type_label(format), PAL.text, chip));
    }
    let plural = |n: u32, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    if let Some(n) = info.and_then(|i| i.episodes).or(d.stub.episodes).filter(|n| *n > 0) {
        tags = tags.child(tag_tinted(&plural(n, "chapter"), PAL.text, chip));
    }
    if let Some(n) = info.and_then(|i| i.volumes).filter(|n| *n > 0) {
        tags = tags.child(tag_tinted(&plural(n, "volume"), PAL.text, chip));
    }
    if let Some(i) = info {
        match crate::manga::Run::from_mal(&i.air_status) {
            crate::manga::Run::Publishing => tags = tags.child(tag("Publishing", PAL.airing, true).fill(chip)),
            crate::manga::Run::Unknown => {}
            r => tags = tags.child(tag_tinted(r.label(), PAL.text, chip)),
        }
    }
    tags
}

/// Type, episodes, airing, season.
fn tags(d: &Detail) -> El {
    if d.is_manga() {
        return manga_tags(d);
    }
    let info = d.info.as_ref();
    let format = info.map_or(d.stub.format.as_str(), |i| i.format.as_str());
    let episodes = info.and_then(|i| i.episodes).or(d.stub.episodes);
    let chip = PAL.bg.alpha(0.6);
    let mut tags = row().key("det-tags").flex_wrap().gap(6.0);
    if !kind_label(format).is_empty() {
        tags = tags.child(tag_tinted(kind_label(format), PAL.text, chip));
    }
    tags = tags.child(tag_tinted(&episodes.map_or("? episodes".into(), |n| if n == 1 { "1 episode".into() } else { format!("{n} episodes") }), PAL.text, chip));
    if let Some(i) = info {
        match i.air_status.as_str() {
            "currently_airing" => tags = tags.child(tag("Airing", PAL.airing, true).fill(chip)),
            a if !air_label(a).is_empty() => tags = tags.child(tag_tinted(air_label(a), PAL.text, chip)),
            _ => {}
        }
        if let Some(se) = i.season_text() {
            tags = tags.child(tag_tinted(&se, PAL.text, chip));
        }
    }
    tags
}

/// Score, rank, popularity, members (once the information is there).
fn stats(d: &Detail) -> El {
    let Some(i) = &d.info else { return el() };
    let mut stats = row().key("det-stats").flex_wrap().gap(8.0);
    stats = stats.child(stat("det/score", i.mean.map_or("\u{2013}".into(), |m| format!("{m:.2}")), "MAL", true));
    if let Some(r) = i.rank {
        stats = stats.child(stat("det/rank", format!("#{}", thousands(r)), "ranked", false));
    }
    if let Some(p) = i.popularity {
        stats = stats.child(stat("det/popularity", format!("#{}", thousands(p)), "popular", false));
    }
    if let Some(m) = i.members {
        stats = stats.child(stat("det/members", thousands(m), "members", false));
    }
    stats
}

/// Below the header: Synopsis and Information, or the error, or the skeleton while loading.
fn body_cards(d: &Detail, svc: &str) -> El {
    let mut c = col().gap(16.0);
    match (&d.info, &d.error) {
        (Some(i), _) => {
            let synopsis = if i.synopsis.is_empty() { "No synopsis yet.".to_string() } else { i.synopsis.clone() };
            c = c.child(card("det-synopsis", "Synopsis").child(text(synopsis).key("det-synopsis-text").font(theme::BODY).color(PAL.dim).wrap().selectable(true).pt(4.0)));
            let mut facts = col().key("det-facts").pt(4.0);
            let aired = match (i.start_date.as_deref(), i.end_date.as_deref()) {
                (Some(a), Some(b)) => format!("{} to {}", date_text(a), date_text(b)),
                (Some(a), None) if i.air_status == "currently_airing" => format!("Since {}", date_text(a)),
                (Some(a), None) => date_text(a),
                _ => "Not yet known".into(),
            };
            let fact = |label: &str, v: String| kv(label, text(v).font(theme::BODY).color(PAL.text).wrap());
            if d.is_manga() {
                let published = match (i.start_date.as_deref(), i.end_date.as_deref()) {
                    (Some(a), Some(b)) => format!("{} to {}", date_text(a), date_text(b)),
                    (Some(a), None) if i.air_status == "currently_publishing" || i.air_status == "on_hiatus" => format!("Since {}", date_text(a)),
                    (Some(a), None) => date_text(a),
                    _ => "Not yet known".into(),
                };
                facts = facts.child(fact("Published", published));
                if !i.authors.is_empty() {
                    let names: Vec<String> = i.authors.iter().map(|a| if a.role.is_empty() { a.name.clone() } else { format!("{} ({})", a.name, a.role) }).collect();
                    facts = facts.child(fact("Authors", names.join(", ")));
                }
                if !i.serialization.is_empty() {
                    facts = facts.child(fact("Serialization", i.serialization.join(", ")));
                }
                if !i.genres.is_empty() {
                    let mut g = row().flex_wrap().gap(6.0);
                    for name in &i.genres {
                        g = g.child(tag_tinted(name, PAL.dim, PAL.raised));
                    }
                    facts = facts.child(kv("Genres", g));
                }
                let running = matches!(i.air_status.as_str(), "currently_publishing" | "on_hiatus");
                let count = |n: Option<u32>| match n.filter(|n| *n > 0) {
                    Some(n) => n.to_string(),
                    None if running => "Unknown while publishing".into(),
                    None => "Unknown".into(),
                };
                facts = facts.child(fact("Chapters", count(i.episodes))).child(fact("Volumes", count(i.volumes)));
                return c.child(card("det-facts-card", "Information").child(facts));
            }
            facts = facts.child(fact("Aired", aired));
            if !i.studios.is_empty() {
                facts = facts.child(fact("Studios", i.studios.join(", ")));
            }
            if !i.genres.is_empty() {
                let mut g = row().flex_wrap().gap(6.0);
                for name in &i.genres {
                    g = g.child(tag_tinted(name, PAL.dim, PAL.raised));
                }
                facts = facts.child(kv("Genres", g));
            }
            if !i.source.is_empty() {
                facts = facts.child(fact("Source", source_label(&i.source)));
            }
            if let Some(m) = i.minutes {
                facts = facts.child(fact("Length", format!("{m} min per episode")));
            }
            c = c.child(card("det-facts-card", "Information").child(facts));
        }
        (None, Some(e)) => {
            c = c.child(
                row()
                    .key("det-error")
                    .gap(12.0)
                    .center_y()
                    .px(14.0)
                    .py(10.0)
                    .radius(theme::R_MD)
                    .fill(PAL.status(Status::Dropped).alpha(0.08))
                    .border(1.0, PAL.status(Status::Dropped).alpha(0.3))
                    .child(text(e.clone()).font(theme::BODY).color(PAL.text).wrap().grow(1.0).min_w(0.0))
                    .child(button("det/retry", "Try again", false, Some(State::details_retry))),
            );
        }
        (None, None) => {
            let mut sk = col().key("det-loading").gap(8.0).child(text(format!("Loading from {svc}\u{2026}")).font(theme::LABEL).color(PAL.faint));
            for i in 0..3 {
                sk = sk.child(el().key(format!("det-skel/{i}")).w_full().h(if i == 2 { 90.0 } else { 36.0 }).radius(theme::R_MD).fill(PAL.raised));
            }
            c = c.child(sk);
        }
    }
    c
}

/// The editor's fields in the narrow right column (the list page's editor is one wide line).
fn compact_editor(e: &Entry, ed: &crate::model::Editor, fmt: kotora_core::ScoreFormat) -> El {
    let mut statuses = row().flex_wrap().gap(6.0);
    for st in Status::ALL {
        let on = ed.status == st;
        statuses = statuses.child(
            cell(format!("ed/status/{}", st.slug()), st.label().into(), on, move |s| s.editor_status(st))
                .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                .child(text(st.label()).font(theme::LABEL).color(if on { PAL.text } else { PAL.dim })),
        );
    }
    let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
    let stepper = row()
        .key("ed/progress")
        .h(32.0)
        .center_y()
        .radius(theme::R_SM)
        .fill(PAL.field)
        .border(1.0, PAL.border_strong)
        .child(step_button("ed/minus", "\u{2212}", -1))
        .child(row().min_w(76.0).center().child(text(ed.progress.to_string()).font(theme::NUM_MED).color(PAL.text)).child(text(format!(" / {total}")).font(theme::NUM).color(PAL.dim)))
        .child(step_button("ed/plus", "+", 1));
    let scores = crate::scores::picker(fmt, ed.score);
    let label = |l: &str| text(l.to_uppercase()).font(theme::CAPTION).color(PAL.faint);
    col()
        .key("editor")
        .gap(10.0)
        .pt(12.0)
        .child(label("Status"))
        .child(statuses)
        .child(label("Progress"))
        .child(row().child(stepper))
        .child(label("Score"))
        .child(scores)
        .child(
            row()
                .gap(8.0)
                .pt(4.0)
                .justify_end()
                .child(button("ed/cancel", "Cancel", false, Some(|s: &mut State| s.editor = None)))
                .child(button(
                    "ed/save",
                    "Save",
                    true,
                    Some(|s: &mut State| {
                        if let Err(m) = s.save_editor() {
                            s.error = Some(m);
                        }
                    }),
                )),
        )
}

/// The user's entry (or Add to list), then the files on disk. `glass`: the panel on the narrow
/// banner card, see-through over the cover's tint.
fn mine_column(s: &State, d: &Detail, glass: bool) -> El {
    let id = d.id();
    let mut c = card("det-mine", "Your list");
    if glass {
        c = c.fill(PAL.bg.alpha(0.55)).border(1.0, crate::look::glass_border());
    }
    if d.is_manga() {
        let mine = crate::manga::mine_card(s, d, c);
        // A Reading manga: its releases (MangaUpdates) under the list card.
        return match crate::publishing::details_card(s, id) {
            Some(p) => col().key("det-mine-col").gap(16.0).child(mine.w_full()).child(p.w_full()),
            None => mine,
        };
    }
    match s.entries.iter().find(|e| e.id == id) {
        Some(e) => {
            let total = e.episodes.map_or("?".to_string(), |n| n.to_string());
            let mut head = row().gap(8.0).center_y().pt(6.0).child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(e.status))).child(text(e.status.label()).font(theme::BODY_MED).color(PAL.text));
            if e.rewatching {
                head = head.child(tag("Rewatching", PAL.st_completed, false));
            }
            if let Some(w) = e.waiting {
                head = head.child(tag(w, PAL.warn(), false));
            }
            c = c.child(head);
            let plus = e.episodes.is_none_or(|n| e.progress < n);
            c = c.child(
                row()
                    .key("det/progress")
                    .gap(10.0)
                    .pt(10.0)
                    .center_y()
                    .child(
                        row()
                            .grow(1.0)
                            .h(4.0)
                            .radius(2.0)
                            .clip_rounded(2.0)
                            .fill(PAL.text.alpha(0.14))
                            .child(el().grow(e.fraction()).h_full().radius(2.0).fill(PAL.status(e.status)))
                            .child(el().grow(1.0 - e.fraction())),
                    )
                    .child(row().child(text(e.progress.to_string()).font(theme::NUM_MED).color(PAL.text)).child(text(format!(" / {total}")).font(theme::NUM).color(PAL.dim)))
                    .child(if plus { plus_button(id) } else { el() }),
            );
            let mine = d.info.as_ref().and_then(|i| i.mine.as_ref());
            let date = |v: Option<&String>| text(v.map_or("\u{2013}".into(), |d| date_text(d))).font(theme::LABEL_NUM).color(PAL.text);
            let score = match e.score {
                Some(n) => crate::scores::mark(s.score_format(), n, theme::LABEL_NUM, PAL.text),
                None => text("No score").font(theme::LABEL_NUM).color(PAL.text),
            };
            let mut facts = col().pt(8.0).child(kv("Score", score.key("det/my-score")));
            facts = facts.child(kv("Started", date(mine.and_then(|m| m.start_date.as_ref())))).child(kv("Finished", date(mine.and_then(|m| m.finish_date.as_ref()))));
            if e.status == Status::Completed || e.times_rewatched > 0 {
                let times = match e.times_rewatched {
                    0 => "Not yet".to_string(),
                    n => kotora_core::queue::times(n),
                };
                facts = facts.child(kv("Rewatched", text(times).key("det/rewatched").font(theme::LABEL_NUM).color(PAL.text)));
            }
            c = c.child(facts);
            match s.editor.filter(|ed| ed.id == id) {
                Some(ed) => c = c.child(compact_editor(e, &ed, s.score_format())),
                None => {
                    let mut actions = row().pt(10.0).gap(8.0).child(button("det/edit", "Edit", false, Some(|s: &mut State| {
                        if let Some(d) = &s.detail {
                            let id = d.id();
                            s.toggle_editor(id);
                        }
                    })));
                    if e.status == Status::Completed && !e.rewatching {
                        actions = actions.child(action("det/rewatch".into(), "Rewatch", false, move |s, _| {
                            if let Err(m) = s.start_rewatch(id) {
                                s.error = Some(m);
                            }
                        }).h(32.0).px(12.0));
                    } else if e.rewatching {
                        actions = actions.child(ghost("det/stop-rewatch", "Stop rewatching", move |s| {
                            if let Err(m) = s.stop_rewatch(id) {
                                s.error = Some(m);
                            }
                        }));
                    }
                    actions = actions.child(spacer()).child(ghost("det/remove", "Remove from list\u{2026}", move |s| s.ask_remove(id)));
                    c = c.child(actions)
                }
            }
        }
        None => {
            c = c.child(text("Not on your list.").font(theme::BODY).color(PAL.dim).pt(6.0)).child(
                row().key("det/add").pt(10.0).gap(8.0).child(button("det/add-watching", "Add as Watching", true, Some(|s: &mut State| s.add_shown(Status::Watching)))),
            );
            let mut more = row().flex_wrap().gap(6.0).pt(8.0);
            for st in [Status::PlanToWatch, Status::Completed, Status::OnHold, Status::Dropped] {
                more = more.child(
                    cell(format!("det/add/{}", st.slug()), format!("Add as {}", st.label()), false, move |s| s.add_shown(st))
                        .child(el().size(8.0, 8.0).radius(4.0).fill(PAL.status(st)))
                        .child(text(st.label()).font(theme::LABEL).color(PAL.dim)),
                );
            }
            c = c.child(more).child(text(format!("It goes to the sync queue first, then to {}.", s.service.title())).font(theme::LABEL).color(PAL.faint).wrap().pt(10.0));
        }
    }
    if glass {
        return c;
    }
    col().gap(16.0).child(c).children(crate::library::details_block(s, id))
}

/// The details page. Wide windows: a full-bleed banner of the cover's tint, the cover over its
/// lower edge, the title on it and the user's entry on the right. Narrow ones (below
/// `theme::NARROW_W`): an inset banner card holding all of it, the entry as a glass panel.
pub fn page(s: &State, wide: bool, width: f32) -> El {
    let Some(d) = &s.detail else {
        return col().key("page/details").w_full().h_full().child(row().key("top").px(theme::PAGE_PAD).pt(18.0).child(back_button(false)));
    };
    let mut page = col().key("page/details").w_full().h_full();
    if let Some(e) = &s.error {
        page = page.child(crate::ui::alert("det/alert", PAL.status(Status::Dropped), e.clone(), None));
    }
    let scroll = col().key(format!("details-scroll/{}", d.id())).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0);
    if wide { page.child(scroll.child(header_wide(s, d))) } else { page.child(top_bar(s, d)).child(scroll.child(header_narrow(s, d, width))) }
}

fn updating(d: &Detail) -> El {
    if d.loading && d.info.is_some() { text("Updating\u{2026}").font(theme::LABEL).color(PAL.dim) } else { el() }
}


/// A: the banner, then two columns (cover, title and the information; the entry on the right).
fn header_wide(s: &State, d: &Detail) -> El {
    const BANNER_H: f32 = 236.0;
    const LIFT: f32 = 150.0;
    let scrim = if PAL.light { crate::look::Scrim { top: 0.35, mid: 0.55, bottom: 1.0 } } else { crate::look::Scrim { top: 0.2, mid: 0.45, bottom: 1.0 } };
    let banner = el()
        .key("det-banner")
        .w_full()
        .h(BANNER_H)
        .child(crate::look::backdrop(s, d.cover_id(), cover_url(d), scrim))
        .child(
            row()
                .key("top")
                .pin(Some(theme::PAGE_PAD), Some(18.0), Some(theme::PAGE_PAD), None)
                .gap(12.0)
                .center_y()
                .child(back_button(true))
                .child(spacer())
                .child(updating(d))
                .children(links(s, d, true)),
        );
    let title = text(shown_title(s, d).to_string()).key("det-title").font(crate::look::HERO_TITLE).color(PAL.text).wrap().max_lines(3).selectable(true);
    let head = row()
        .key("det-main")
        .gap(24.0)
        .items_start()
        .mt(-LIFT)
        .child(big_cover(s, d, 176.0, 264.0))
        .child(col().grow(1.0).min_w(0.0).pt(48.0).gap(6.0).selection_group(true).child(title).child(other_titles(s, d)).child(stats(d).mt(18.0)).child(tags(d).mt(6.0)));
    let left = col().grow(1.0).min_w(0.0).gap(20.0).child(head).child(body_cards(d, s.service.title()));
    let right = mine_column(s, d, false).w(320.0).mt(-LIFT + 56.0);
    col().w_full().child(banner).child(
        row().px(theme::PAGE_PAD + 4.0).gap(24.0).pb(theme::PAGE_PAD).max_w(1240.0).items_start().child(left).child(right),
    )
}

/// The narrow page's top bar: Back, the page's name, the link to MyAnimeList.
fn top_bar(s: &State, d: &Detail) -> El {
    row()
        .key("top")
        .gap(12.0)
        .center_y()
        .px(theme::PAGE_PAD)
        .pt(18.0)
        .pb(14.0)
        .child(back_button(false))
        .child(text(if d.is_manga() { "Manga information" } else { "Anime information" }).font(theme::SECTION).color(PAL.dim))
        .child(spacer())
        .child(updating(d))
        .children(links(s, d, false))
}

/// "Open on …": the active service's link (`det/open-mal`), and with the mirror on the other
/// service's (`det/open-other`) when its id is known. The narrow bar says "Open on" with one.
fn links(s: &State, d: &Detail, glass: bool) -> Vec<El> {
    let all = s.service_links(d.kind(), d.id());
    let one = all.len() == 1;
    all.into_iter()
        .enumerate()
        .map(|(i, (svc, url))| {
            let label = if glass || !one { svc.title().to_string() } else { format!("Open on {}", svc.title()) };
            link_button(if i == 0 { "det/open-mal" } else { "det/open-other" }, &label, url, glass)
        })
        .collect()
}

/// The main area's width under which the narrow page stacks the entry panel under the title.
const STACK_W: f32 = 780.0;

/// B: one banner card (cover, tags, titles, numbers and the glass entry panel), then the rest.
fn header_narrow(s: &State, d: &Detail, width: f32) -> El {
    // Side by side only while the title keeps room (about 260 pt): cover, title and the entry
    // panel need 132 + 250 + two gaps + two paddings + that. Narrower, the panel drops under them.
    let stacked = width < STACK_W;
    let scrim = if PAL.light { crate::look::Scrim::even(0.62) } else { crate::look::Scrim::even(0.5) };
    let title = text(shown_title(s, d).to_string()).key("det-title").font(TextStyle::new(24.0, 600.0).tracking(-0.2)).color(PAL.text).wrap().max_lines(3).selectable(true);
    let banner = el()
        .key("det-banner")
        .w_full()
        .radius(16.0)
        .clip_rounded(16.0)
        .border(1.0, PAL.border)
        .child(crate::look::backdrop(s, d.cover_id(), cover_url(d), scrim))
        .child(
            {
                let info = col().grow(1.0).min_w(0.0).gap(6.0).selection_group(true).child(tags(d).pb(4.0)).child(title).child(other_titles(s, d)).child(stats(d).mt(12.0));
                let hero = row().key("det-main").w_full().gap(20.0).items_start().child(big_cover(s, d, 132.0, 198.0)).child(info);
                if stacked {
                    col().key("det-hero").w_full().padding(20.0).gap(20.0).child(hero).child(mine_column(s, d, true).w_full())
                } else {
                    row().key("det-hero").w_full().padding(20.0).gap(20.0).items_start().child(hero.grow(1.0).min_w(0.0)).child(mine_column(s, d, true).w(250.0))
                }
            },
        );
    col()
        .px(theme::PAGE_PAD)
        .gap(16.0)
        .pb(theme::PAGE_PAD)
        .child(banner)
        .children(if d.is_manga() { None } else { crate::library::details_block(s, d.id()) })
        .child(body_cards(d, s.service.title()).max_w(760.0))
}

impl State {
    /// A title pressed while a list row's editor is open on another entry of the same list: the
    /// editor moves to this one, as a press on the row does (not the details page). Returns
    /// whether it moved.
    pub fn move_editor_to(&mut self, stub: &Stub) -> bool {
        match (stub.kind, self.page) {
            (Kind::Anime, Page::List(_)) if self.editor.is_some_and(|ed| ed.id != stub.id) && self.entries.iter().any(|e| e.id == stub.id) => {
                self.toggle_editor(stub.id);
                true
            }
            (Kind::Manga, Page::Manga(_)) if self.manga.editor.is_some_and(|ed| ed.id != stub.id) && self.manga.entries.iter().any(|e| e.id == stub.id) => {
                self.manga_toggle_editor(stub.id);
                true
            }
            _ => false,
        }
    }
}

/// The title of a list row or the Now Playing card as a link to the details page.
pub fn title_link(label: El, stub: Stub) -> El {
    // The link is as wide as the title (it shrinks and truncates in a narrow column); the rest
    // of the column is the row's, so a press there opens the editor.
    let link = row()
        .key("title-link")
        .role("link")
        .label(format!("About {}", stub.title))
        .min_w(0.0)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| {
            tx.instant();
            if s.move_editor_to(&stub) {
                return;
            }
            s.open_details(stub.clone());
        })
        .child(label);
    row().w_full().min_w(0.0).child(link.shrink())
}

/// A made-up information page for the scenes (the fake list's first title, invented facts).
pub fn fake_details(id: u32, title: &str, mine: bool) -> Details {
    Details {
        id,
        title: title.into(),
        title_en: Some("The Lanterns on Kirigaoka Hill".into()),
        title_ja: Some("霧ヶ丘のランタン".into()),
        synonyms: vec!["Kirigaoka Lanterns".into(), "KiriLan".into()],
        format: "tv".into(),
        episodes: Some(12),
        air_status: "currently_airing".into(),
        start_date: Some("2026-10-02".into()),
        end_date: None,
        season: Some((2026, "fall".into())),
        studios: vec!["Studio Example".into()],
        genres: vec!["Drama".into(), "Mystery".into(), "Slice of Life".into()],
        source: "light_novel".into(),
        minutes: Some(24),
        mean: Some(8.44),
        rank: Some(212),
        popularity: Some(1840),
        members: Some(312_455),
        synopsis: "Every autumn the hill town of Kirigaoka hangs a thousand paper lanterns for the festival of returning \
                   souls. This year one lantern will not go out, and the girl who lit it has no memory of doing so. A \
                   made-up story written for Kotora's screenshots; no real anime is described here."
            .into(),
        cover_url: None,
        cover_small: None,
        mine: mine.then(|| kotora_core::service::MyEntry {
            status: "watching".into(),
            progress: 5,
            score: kotora_core::score::ten(8),
            rewatching: false,
            times_rewatched: 0,
            start_date: Some("2026-10-04".into()),
            finish_date: None,
            volumes: 0,
        }),
        ..Default::default()
    }
}

/// Ghost "Details" for the Now Playing not-on-list card (re-exported for `ui`).
pub fn details_ghost(stub: Stub) -> El {
    ghost("np/details", "Details", move |s| s.open_details(stub.clone())).h(26.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_numbers_read_plainly() {
        assert_eq!(date_text("2025-10-03"), "Oct 3, 2025");
        assert_eq!(date_text("2025-10"), "Oct 2025");
        assert_eq!(date_text("2025"), "2025");
        assert_eq!(date_text("soon"), "soon");
        assert_eq!(thousands(98_765), "98,765");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
        assert_eq!(source_label("light_novel"), "Light novel");
    }

    #[test]
    fn back_returns_where_it_came_from_and_answers_for_other_pages_are_dropped() {
        let mut s = State::fake();
        s.go(Page::List(Status::Completed));
        s.open_details(Stub::bare(0, "Lanterns of Kirigaoka"));
        assert_eq!(s.page, Page::Details);
        // From one details page to another: Back still goes to the list.
        s.open_details(Stub::bare(3, "Slow Life on Hoshizora Island"));
        assert!(!s.details_answer(Answer { id: 0, kind: Kind::Anime, result: Ok(fake_details(0, "x", false)), item: None, done: true }));
        assert!(s.details_answer(Answer { id: 3, kind: Kind::Anime, result: Ok(fake_details(3, "Slow Life", true)), item: None, done: true }));
        assert!(s.detail.as_ref().unwrap().info.is_some());
        s.details_back();
        assert_eq!(s.page, Page::List(Status::Completed));
        assert!(s.detail.is_none());
    }

    #[test]
    fn an_answer_refreshes_the_saved_entry_and_add_puts_it_on_the_list() {
        let mut s = State::fake();
        s.open_details(Stub::bare(9001, "Velvet Orchard Chronicle"));
        s.details_answer(Answer { id: 9001, kind: Kind::Anime, result: Ok(fake_details(9001, "Velvet Orchard Chronicle", false)), item: None, done: true });
        s.add_shown(Status::PlanToWatch);
        let e = s.entries.iter().find(|e| e.id == 9001).unwrap();
        assert_eq!((e.status, e.episodes, e.kind), (Status::PlanToWatch, Some(12), "TV"));
        // A failed refresh over saved information keeps it and stops loading.
        s.detail.as_mut().unwrap().loading = true;
        s.details_answer(Answer { id: 9001, kind: Kind::Anime, result: Err(String::new()), item: None, done: true });
        let d = s.detail.as_ref().unwrap();
        assert!(d.info.is_some() && !d.loading && d.error.is_none());
    }
}
