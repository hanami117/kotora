//! Reading detection: a comic the user read in a reader → a manga on their list, and the chapter
//! (or volume) to offer. The manga side of `recognize.rs`.
//!
//! 1. A source says what was opened ([`yacreader`](super::yacreader): its library database).
//!    [`Read::from_opened`] keeps what matters: the series, the chapter or the volume, the page.
//! 2. The cache, per normalised series ([`key`]): the user's pick ("Wrong manga…") or "not a
//!    manga" wins.
//! 3. The manga list ([`candidates`], [`on_list`]): every title, normalised. The same title is a
//!    sure match when only one entry has it; a list title that starts with the file's series
//!    ("Kaguya-sama" for "Kaguya-sama wa Kokurasetai") or a close one asks.
//! 4. Nothing on the list: the caller may search once ([`from_search`]); a hit off the list is
//!    "not on your list: add?".
//!
//! [`offer`] then says what to mark: never at or below the progress, never past the count. A
//! whole-volume file (`Blue Lock v35.cbz`) marks its volume; a chapter file that names its volume
//! (`v35 c362`) marks the volume before as completed (34): being inside volume 35 means 34 is done.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::recognize::{Found, normalize, similarity, UNSURE_AT};
use crate::service::AnimeHit;
use crate::{ListItem, Status};

use super::yacreader::{self, Library, Opened};

/// A comic the user opened in a reader, as Kotora keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// Where it is (shown, and the identity of "the same file").
    pub path: PathBuf,
    /// The file's name, shown as is.
    pub file_name: String,
    /// The series to look up (the file name's, a folder's, or the reader's metadata).
    pub series: String,
    /// The chapter read up to (the highest of a range; a `.5` counts as the chapter before:
    /// the services keep whole chapters). `None`: the file names none.
    pub chapter: Option<u32>,
    /// The volume the file names.
    pub volume: Option<u32>,
    /// The file is a whole volume (a volume and no chapter): finishing it marks the volume.
    pub volume_file: bool,
    /// The page the user is on (from 1) and the count; 0: unknown.
    pub page: i64,
    pub pages: i64,
    /// Read to the end ([`super::chapter_read`]).
    pub finished: bool,
    /// When it was last opened or its page saved: Unix seconds.
    pub at: i64,
    /// The reader: "YACReader", or the process that has the file open ("Simple Comic").
    pub source: String,
    /// A weak signal (an open file, no page): it always asks, even with "Update automatically".
    pub weak: bool,
    /// Why a weak signal counts it as read: "Ch 13 opened next", "open for 10 minutes".
    pub why: Option<String>,
}

fn whole(n: f64) -> Option<u32> {
    (n.is_finite() && n >= 1.0).then(|| n.floor().min(f64::from(u32::MAX)) as u32)
}

impl Read {
    pub fn from_opened(o: &Opened) -> Read {
        let p = &o.parsed;
        let chapter = p.last_chapter().and_then(whole);
        let volume = p.volume.map(|v| v.last).and_then(whole);
        Read {
            file_name: o.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| o.rel_path.clone()),
            path: o.path.clone(),
            series: o.series().to_string(),
            chapter,
            volume,
            volume_file: p.chapter.is_none() && volume.is_some(),
            page: o.current_page,
            pages: o.num_pages,
            finished: o.is_read(),
            at: o.last_opened,
            source: "YACReader".into(),
            weak: false,
            why: None,
        }
    }

    /// The recognition cache's key: the normalised series.
    pub fn key(&self) -> String {
        key(&self.series)
    }

    /// "Ch 362", "Vol 35", "Ch 362 (Vol 35)", or the file's name when it names neither.
    pub fn label(&self) -> String {
        match (self.chapter, self.volume) {
            (Some(c), Some(v)) => format!("Ch {c} (Vol {v})"),
            (Some(c), None) => format!("Ch {c}"),
            (None, Some(v)) => format!("Vol {v}"),
            (None, None) => self.file_name.clone(),
        }
    }
}

/// The cache key of a series.
pub fn key(series: &str) -> String {
    normalize(series)
}

/// A list title that starts with the series this long or longer counts as the series.
const PREFIX_MIN: usize = 4;

/// One manga on the list and how well it matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub media_id: u32,
    pub title: String,
    pub score: f32,
}

fn title_score(series: &str, title: &str) -> f32 {
    let t = normalize(title);
    if t == series {
        1.0
    } else if series.chars().count() >= PREFIX_MIN && t.starts_with(&format!("{series} ")) {
        0.9
    } else {
        similarity(&t, series)
    }
}

/// Every list entry scored against the series, best first (only those above 0.3). Equal
/// scores: the manga being read first.
pub fn candidates(series: &str, items: &[ListItem]) -> Vec<Candidate> {
    let s = normalize(series);
    if s.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(Candidate, bool)> = items
        .iter()
        .map(|it| {
            let score = it.media.all_titles().map(|t| title_score(&s, t)).fold(0.0f32, f32::max);
            let reading = it.rewatching || it.status == Status::Watching;
            (Candidate { media_id: it.media.id, title: it.media.title.clone(), score }, reading)
        })
        .filter(|(c, _)| c.score > 0.3)
        .collect();
    out.sort_by(|(a, ar), (b, br)| b.score.total_cmp(&a.score).then(br.cmp(ar)));
    out.into_iter().map(|(c, _)| c).collect()
}

/// Steps 2 and 3. `None`: nothing on the list; the caller may search. `cached`: the cache's
/// answer for [`key`] (media id, source, title).
pub fn on_list(series: &str, items: &[ListItem], cached: &crate::recognize::Cached) -> Option<Found> {
    if let Some((id, source, _)) = cached {
        match source.as_str() {
            "ignore" => return Some(Found::Ignored),
            "user" if items.iter().any(|it| it.media.id == *id) => return Some(Found::List { media_id: *id, sure: true, why: "you picked it".into() }),
            _ => {}
        }
    }
    let c = candidates(series, items);
    let best = c.first().filter(|b| b.score >= UNSURE_AT)?;
    let ties = c.iter().filter(|x| x.score >= best.score).count();
    let sure = best.score >= 1.0 && ties == 1;
    let why = match (sure, ties) {
        (true, _) => "same title in your list".to_string(),
        (false, n) if n > 1 && best.score >= 1.0 => format!("{n} titles in your list match"),
        (false, _) if best.score >= 0.9 => format!("a title in your list starts with \u{201c}{series}\u{201d}"),
        (false, _) => format!("close to a title in your list ({:.0}%)", best.score * 100.0),
    };
    Some(Found::List { media_id: best.media_id, sure, why })
}

/// Step 4: a manga search's hits against the list. A hit on the list is unsure; else the
/// best hit off the list (`NotOnList`), when its title is close enough.
pub fn from_search(series: &str, hits: &[AnimeHit], items: &[ListItem], service: &str) -> Found {
    let s = normalize(series);
    for h in hits {
        if items.iter().any(|it| it.media.id == h.id) {
            return Found::List { media_id: h.id, sure: false, why: format!("found by a {service} search") };
        }
    }
    let close = |h: &&AnimeHit| {
        std::iter::once(h.title.as_str()).chain(h.title_en.as_deref()).chain(h.title_ja.as_deref()).chain(h.synonyms.iter().map(String::as_str)).any(|t| title_score(&s, t) >= UNSURE_AT)
    };
    match hits.iter().find(close) {
        Some(h) => Found::NotOnList { media_id: h.id, title: h.title.clone() },
        None => Found::Nothing,
    }
}

/// What finishing the file marks: a chapter, a volume, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    pub chapter: Option<u32>,
    pub volume: Option<u32>,
}

impl Offer {
    /// "Ch 21", "Vol 3", "Ch 21 and Vol 3".
    pub fn label(&self) -> String {
        match (self.chapter, self.volume) {
            (Some(c), Some(v)) => format!("Ch {c} and Vol {v}"),
            (Some(c), None) => format!("Ch {c}"),
            (None, Some(v)) => format!("Vol {v}"),
            (None, None) => String::new(),
        }
    }
}

/// The offer for `read` on the entry with these counts, if any. Only forward: a chapter above
/// the chapters read (and not past the known count), a volume above the volumes read (and not
/// past its count). A whole-volume file gives its volume; a chapter in volume N gives N - 1 (the
/// volumes before it are done). A one-shot (a count of 1 and no number in the name) is chapter 1.
pub fn offer(chapters: u32, chapters_total: Option<u32>, volumes: u32, volumes_total: Option<u32>, read: &Read) -> Option<Offer> {
    let one_shot = chapters_total == Some(1) && read.chapter.is_none() && read.volume.is_none();
    let chapter = read.chapter.or(one_shot.then_some(1)).filter(|c| *c > chapters && chapters_total.is_none_or(|n| n == 0 || *c <= n));
    let done_volume = if read.volume_file { read.volume } else { read.volume.and_then(|v| v.checked_sub(1)) };
    let volume = done_volume.filter(|v| *v > 0 && *v > volumes && volumes_total.is_none_or(|n| n == 0 || *v <= n));
    (chapter.is_some() || volume.is_some()).then_some(Offer { chapter, volume })
}

/// The chapters an offer of `chapter` would skip (first and last), when it is two or more ahead:
/// "Update automatically" asks then.
pub fn skipped(chapters: u32, chapter: u32) -> Option<(u32, u32)> {
    crate::recognize::skipped(chapters, chapter)
}

// ------------------------------------------------------------------------------- the watcher

/// How far back the first look goes: a comic read just before Kotora started still counts.
pub const FIRST_LOOK_SECS: i64 = 30 * 60;

/// Watches YACReader libraries. [`Watch::poll`] costs one `stat` per library when nothing
/// changed; a changed database is opened read only (no locks) and asked for what was opened
/// since the watch began.
pub struct Watch {
    libs: Vec<(Library, Option<SystemTime>)>,
    since: i64,
}

/// One library's state, for Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibState {
    pub path: PathBuf,
    /// `None`: read fine (with the comic count opened lately); `Some`: why not.
    pub error: Option<String>,
}

impl Watch {
    /// `now`: Unix seconds; the first look goes [`FIRST_LOOK_SECS`] back.
    pub fn new(libs: Vec<Library>, now: i64) -> Watch {
        Watch { libs: libs.into_iter().map(|l| (l, None)).collect(), since: now - FIRST_LOOK_SECS }
    }

    pub fn libraries(&self) -> impl Iterator<Item = &Library> {
        self.libs.iter().map(|(l, _)| l)
    }

    /// The newest comic opened in any library, when a database changed since the last poll
    /// (`None`: nothing changed). Libraries whose database can't be read are reported in
    /// `errors` and tried again at the next change.
    pub fn poll(&mut self, errors: &mut Vec<LibState>) -> Option<Read> {
        let mut changed = false;
        let mut newest: Option<Read> = None;
        for (lib, seen) in &mut self.libs {
            let db = lib.db_path();
            let m = std::fs::metadata(&db).and_then(|m| m.modified()).ok();
            if m.is_none() || m == *seen {
                continue;
            }
            match lib.open().and_then(|d| d.opened_since(self.since)) {
                Ok(v) => {
                    *seen = m;
                    changed = true;
                    if let Some(r) = v.first().map(Read::from_opened)
                        && newest.as_ref().is_none_or(|n| r.at > n.at)
                    {
                        newest = Some(r);
                    }
                }
                // Caught mid-write (`immutable=1` doesn't wait): the next poll tries again.
                Err(e) => errors.push(LibState { path: lib.path.clone(), error: Some(e.0) }),
            }
        }
        if !changed {
            return None;
        }
        newest
    }
}

/// The libraries to watch: the ones the user added (a library folder, or its `library.ydb`),
/// then the ones YACReader's settings list (`auto`), without doubles.
pub fn libraries(added: &[String], auto: bool) -> Vec<Library> {
    let mut v: Vec<Library> = added.iter().map(|p| library_at(p)).collect();
    if auto && let Some(ini) = yacreader::find_ini() {
        for l in yacreader::libraries(&ini).unwrap_or_default() {
            if !v.iter().any(|x| x.path == l.path) {
                v.push(l);
            }
        }
    }
    v
}

/// The file of comic `id` in the YACReader library at `library` (the viewer's `<library>
/// --comicId=N`), from its database, read only. `None`: no such library or comic.
pub fn viewer_comic(library: &str, id: i64) -> Option<PathBuf> {
    Some(library_at(library).open().ok()?.comic(id).ok()??.path)
}

/// A library from a path the user gave: the folder, or the database file inside it.
pub fn library_at(p: &str) -> Library {
    let path = PathBuf::from(p);
    let folder = if path.file_name().is_some_and(|n| n == "library.ydb") { path.parent().and_then(|d| d.parent()).map(PathBuf::from).unwrap_or(path.clone()) } else { path };
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Library { name, path: folder }
}

#[cfg(test)]
mod tests;
