//! "Continue reading": where the user stopped in a manga, from YACReader's library databases
//! (read only; Kotora keeps no pages of its own).
//!
//! For one manga of the list: the comic of its series opened last in any library (newest
//! `lastTimeOpened`), at its saved page. Read to the end: the next file of the series in that
//! library (a higher volume, or chapter), from page 1. Nothing when none is known, the file is
//! gone, or the last one was read to the end with no next one.

use std::path::PathBuf;

use super::reading;
use super::yacreader::{Library, Opened};
use crate::ListItem;
use crate::recognize::Found;

/// Where to continue.
#[derive(Clone, Debug, PartialEq)]
pub struct Resume {
    /// The library's folder and its id for the viewer (`--libraryId`), when YACReader's settings
    /// list it.
    pub library: PathBuf,
    pub library_id: Option<i64>,
    /// The viewer's `--comicId`.
    pub comic_id: i64,
    pub path: PathBuf,
    pub chapter: Option<u32>,
    pub volume: Option<u32>,
    /// The saved page (from 1) and the count; 0: unknown.
    pub page: i64,
    pub pages: i64,
    /// The comic after one read to the end (it starts at page 1).
    pub next: bool,
}

impl Resume {
    /// "Vol 11 · p. 87/190", "Ch 120 · p. 12/20", "Vol 12" (the next one).
    pub fn label(&self) -> String {
        let what = match (self.volume, self.chapter) {
            (Some(v), Some(c)) => format!("Vol {v} Ch {c}"),
            (Some(v), None) => format!("Vol {v}"),
            (None, Some(c)) => format!("Ch {c}"),
            (None, None) => self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        if self.next || self.pages <= 0 || self.page <= 1 {
            what
        } else {
            format!("{what} \u{b7} p. {}/{}", self.page.min(self.pages), self.pages)
        }
    }
}

fn whole(n: f64) -> Option<u32> {
    (n.is_finite() && n >= 1.0).then(|| n.floor() as u32)
}

/// (volume, chapter) for "the next one".
fn place(o: &Opened) -> (f64, f64) {
    (o.parsed.volume.map_or(0.0, |v| v.last), o.parsed.chapter.map_or(0.0, |c| c.last))
}

/// Whether the comic's series is this manga: the same title (sure, see [`reading::on_list`]),
/// or a series the user picked for it (`picked`: recognition keys pointing at it).
fn is_of(o: &Opened, item: &ListItem, picked: &[String]) -> bool {
    let series = o.series();
    if series.is_empty() {
        return false;
    }
    // Sure, or the series is one of the manga's own titles (VIZ's "Dragon Ball Z" is Dragon Ball).
    picked.contains(&reading::key(series))
        || matches!(reading::on_list(series, std::slice::from_ref(item), &None), Some(Found::List { sure: true, .. }))
        || reading::candidates(series, std::slice::from_ref(item)).first().is_some_and(|c| c.score >= reading::ALT_TITLE)
}

fn resume_of(lib: &Library, o: &Opened, next: bool) -> Resume {
    Resume {
        library: lib.path.clone(),
        library_id: lib.id,
        comic_id: o.id,
        path: o.path.clone(),
        chapter: o.parsed.last_chapter().and_then(whole),
        volume: o.parsed.volume.map(|v| v.last).and_then(whole),
        page: if next { 1 } else { o.current_page },
        pages: o.num_pages,
        next,
    }
}

/// Where to continue `item`, from the libraries (read only). See the module's notes.
pub fn resume(libs: &[Library], item: &ListItem, picked: &[String]) -> Option<Resume> {
    // The newest opened comic of the series, over every library.
    let mut last: Option<(&Library, Opened)> = None;
    for lib in libs {
        let Ok(db) = lib.open() else { continue };
        let Ok(opened) = db.opened_since(0) else { continue };
        if let Some(o) = opened.into_iter().find(|o| is_of(o, item, picked))
            && last.as_ref().is_none_or(|(_, l)| o.last_opened > l.last_opened)
        {
            last = Some((lib, o));
        }
    }
    let (lib, o) = last?;
    let r = if o.is_read() {
        // Read to the end: the next file of the series in that library.
        let key = reading::key(o.series());
        let after = place(&o);
        let all = lib.open().ok()?.comics().ok()?;
        let n = all.into_iter().filter(|c| reading::key(c.series()) == key && place(c) > after).min_by(|a, b| place(a).partial_cmp(&place(b)).unwrap_or(std::cmp::Ordering::Equal))?;
        resume_of(lib, &n, true)
    } else {
        resume_of(lib, &o, false)
    };
    r.path.is_file().then_some(r)
}

/// The libraries with YACReader's ids filled in from its settings, by folder (a library Kotora
/// was given by hand has none of its own).
pub fn with_ids(mut libs: Vec<Library>, known: &[Library]) -> Vec<Library> {
    for l in &mut libs {
        if l.id.is_none() {
            l.id = known.iter().find(|k| k.path == l.path).and_then(|k| k.id);
        }
    }
    libs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Media, Status};
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn scratch(name: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("kot-resume-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    const SCHEMA: &str = "
CREATE TABLE comic_info (id INTEGER PRIMARY KEY, numPages INTEGER, hash TEXT UNIQUE NOT NULL, read BOOLEAN DEFAULT 0,
  hasBeenOpened BOOLEAN DEFAULT 0, currentPage INTEGER DEFAULT 1, lastTimeOpened INTEGER, series TEXT);
CREATE TABLE comic (id INTEGER PRIMARY KEY, parentId INTEGER NOT NULL, comicInfoId INTEGER NOT NULL, fileName TEXT NOT NULL, path TEXT);
CREATE TABLE db_info (version TEXT NOT NULL);
INSERT INTO db_info (version) VALUES ('10.5.0');
";

    /// A library with these comics (path, page, pages, read, last opened); their files exist
    /// (empty) unless the path starts with `/gone`.
    fn library(root: &Path, rows: &[(&str, i64, i64, bool, Option<i64>)]) -> Library {
        let lib = root.join("Comics");
        fs::create_dir_all(lib.join(".yacreaderlibrary")).unwrap();
        let c = Connection::open(lib.join(".yacreaderlibrary/library.ydb")).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        for (i, (path, page, pages, read, last)) in rows.iter().enumerate() {
            let id = i as i64 + 1;
            c.execute("INSERT INTO comic_info (id, hash, numPages, currentPage, read, lastTimeOpened) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", rusqlite::params![id, format!("h{id}"), pages, page, read, last]).unwrap();
            let file = path.rsplit('/').next().unwrap();
            c.execute("INSERT INTO comic (id, parentId, comicInfoId, fileName, path) VALUES (?1, 1, ?1, ?2, ?3)", rusqlite::params![id, file, path]).unwrap();
            if !path.starts_with("/gone") {
                let f = lib.join(path.trim_start_matches('/'));
                fs::create_dir_all(f.parent().unwrap()).unwrap();
                fs::write(f, b"").unwrap();
            }
        }
        Library { name: "Comics".into(), path: lib, id: Some(1) }
    }

    fn manga(id: u32, title: &str) -> ListItem {
        let media = Media { id, kind: Kind::Manga, title: title.into(), ..Default::default() };
        ListItem { media, status: Status::Watching, progress: 0, score: None, rewatching: false, times_rewatched: 0, volumes: 0, updated_at: 0 }
    }

    #[test]
    fn where_the_user_stopped() {
        let s = scratch("stopped");
        let lib = library(&s.0, &[("/Dragon Ball Z v10.cbz", 190, 190, true, Some(100)), ("/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz", 87, 190, false, Some(200)), ("/Other v01.cbz", 3, 50, false, Some(300))]);
        let r = resume(std::slice::from_ref(&lib), &manga(1, "Dragon Ball Z"), &[]).unwrap();
        assert_eq!((r.comic_id, r.volume, r.page, r.pages, r.next, r.library_id), (2, Some(11), 87, 190, false, Some(1)));
        assert_eq!(r.label(), "Vol 11 \u{b7} p. 87/190");
        // Another series that starts alike is not this one.
        assert_eq!(resume(std::slice::from_ref(&lib), &manga(2, "Dragon Ball"), &[]), None);
        // Unless the user picked it.
        assert!(resume(std::slice::from_ref(&lib), &manga(2, "Dragon Ball"), &["dragon ball z".into()]).is_some());
    }

    #[test]
    fn read_to_the_end_gives_the_next_one_or_nothing() {
        let s = scratch("next");
        let lib = library(&s.0, &[("/Glass Lantern Club/c012.cbz", 20, 20, true, Some(500)), ("/Glass Lantern Club/c014.cbz", 1, 20, false, None), ("/Glass Lantern Club/c013.cbz", 1, 20, false, None)]);
        let r = resume(std::slice::from_ref(&lib), &manga(1, "Glass Lantern Club"), &[]).unwrap();
        assert_eq!((r.chapter, r.next, r.page, r.comic_id), (Some(13), true, 1, 3));
        assert_eq!(r.label(), "Ch 13");
        let s2 = scratch("last");
        let lib = library(&s2.0, &[("/Glass Lantern Club/c012.cbz", 20, 20, true, Some(500))]);
        assert_eq!(resume(std::slice::from_ref(&lib), &manga(1, "Glass Lantern Club"), &[]), None, "read to the end, no next one");
    }

    #[test]
    fn unknown_or_gone_is_nothing() {
        let s = scratch("gone");
        let lib = library(&s.0, &[("/gone/Tsuki no Niwa c003.cbz", 5, 20, false, Some(100)), ("/Never Opened/c001.cbz", 1, 20, false, None)]);
        assert_eq!(resume(std::slice::from_ref(&lib), &manga(1, "Tsuki no Niwa"), &[]), None, "the file is gone");
        assert_eq!(resume(std::slice::from_ref(&lib), &manga(2, "Never Opened"), &[]), None, "never opened");
        let none = Library { name: "x".into(), path: s.0.join("nowhere"), id: None };
        assert_eq!(resume(&[none], &manga(1, "Tsuki no Niwa"), &[]), None);
    }

    #[test]
    fn ids_come_from_yacreaders_settings() {
        let a = Library { name: "Movies".into(), path: PathBuf::from("/m"), id: None };
        let known = [Library { name: "Movies".into(), path: PathBuf::from("/m"), id: Some(3) }];
        assert_eq!(with_ids(vec![a], &known)[0].id, Some(3));
        assert_eq!(Resume { library: "/m".into(), library_id: None, comic_id: 1, path: "/m/x c120.cbz".into(), chapter: Some(120), volume: None, page: 12, pages: 20, next: false }.label(), "Ch 120 \u{b7} p. 12/20");
    }
}
