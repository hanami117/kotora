//! Reading detection, stage 2: which comic files are open, from the process table (the same
//! `Procs` the players' process source uses: `/proc` on Linux, libproc on macOS).
//!
//! A comic is a file with a comic extension (`.cbz .cbr .cb7 .cbt .zip .pdf`) or a folder an open
//! image is in (`.jpg .png .webp …`: a chapter kept as a folder of pages). Taken from:
//!
//! - a known reader (Simple Comic, YACReader, MComix, Okular, Evince, Zathura, Komikku, Preview,
//!   …): anywhere;
//! - any other process of the user: only inside the comic folders the user named (with none
//!   named, other processes are not looked at, which keeps the scan to the readers' files).
//!
//! Indexers, backup and sync tools that open every file (Spotlight, Tracker, Baloo, Dropbox, …)
//! are never taken. Process names are read once per process; the open files of the processes
//! taken, at each scan. The app scans every 30 s, and only while the user turned it on.
//!
//! Windows: not yet (listing another process's open files needs a handle walk); `scan` there
//! returns nothing. TODO: window titles of known readers, as `winproc.rs` does for players.

use std::collections::{HashMap, HashSet};

use crate::procfs::Procs;

/// Comic file extensions (lower case).
pub const COMIC_EXTS: &[&str] = &["cbz", "cbr", "cb7", "cbt", "zip", "pdf"];
/// Page image extensions: an open one makes its folder the comic.
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "avif", "gif", "jxl"];

/// Known readers, by process name (lower case): the name shown.
const READERS: &[(&str, &str)] = &[
    ("simple comic", "Simple Comic"),
    ("yacreader", "YACReader"),
    ("mcomix", "MComix"),
    ("okular", "Okular"),
    ("evince", "Evince"),
    ("atril", "Atril"),
    ("zathura", "Zathura"),
    ("komikku", "Komikku"),
    ("preview", "Preview"),
    ("foliate", "Foliate"),
    ("qcomicbook", "QComicBook"),
    ("comix", "Comix"),
    ("komga", "Komga"),
];

/// Processes that open every file for their own reasons: never a reading.
const IGNORED: &[&str] = &[
    "mds", "mds_stores", "mdworker", "mdworker_shared", "mdsync", "spotlight", "quicklookd", "quicklookuiservice", "qlmanage", "backupd", "revisiond", "fileproviderd", "bird", "cloudd", "dropbox", "syncthing",
    "rsync", "cp", "tar", "unzip", "7z", "unrar", "tracker-miner-fs", "tracker-miner-fs-3", "tracker-extract", "tracker-extract-3", "localsearch-3", "baloo_file", "baloo_file_extractor", "nautilus", "dolphin",
    "thunar", "nemo", "finder", "kotora", "updatedb", "locate", "duplicati", "restic", "borg", "nextcloud", "owncloud", "megasync", "insync", "rclone", "onedrive",
];

/// The reader a process name stands for.
pub fn reader_of(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    READERS.iter().find(|(k, _)| n == *k).map(|(_, v)| *v)
}

fn ignored(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    IGNORED.contains(&n.as_str()) || n.starts_with("mdworker") || n.starts_with("tracker-") || n.starts_with("baloo")
}

fn ext_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// The comic a path open stands for: the file, or the folder of an open page image.
pub fn comic_of(path: &str) -> Option<String> {
    if !path.starts_with('/') || path.ends_with(" (deleted)") {
        return None;
    }
    let ext = ext_of(path)?;
    if COMIC_EXTS.contains(&ext.as_str()) {
        return Some(path.to_string());
    }
    if IMAGE_EXTS.contains(&ext.as_str()) {
        let (dir, _) = path.rsplit_once('/')?;
        return (!dir.is_empty()).then(|| dir.to_string());
    }
    None
}

/// Whether `path` is inside one of `folders` (or is one).
pub fn inside(path: &str, folders: &[String]) -> bool {
    folders.iter().any(|f| {
        let f = f.trim_end_matches('/');
        !f.is_empty() && (path == f || path.strip_prefix(f).is_some_and(|rest| rest.starts_with('/')))
    })
}

/// One open comic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenComic {
    pub pid: u32,
    /// The reader's name, or the process name.
    pub app: String,
    /// The comic file, or the folder of the image open.
    pub path: String,
}

#[derive(Clone, Debug)]
enum Seen {
    Reader(&'static str),
    Other(String),
    Skip,
}

/// Finds the open comics; remembers process names between scans.
pub struct ComicScanner<P: Procs> {
    procs: P,
    seen: HashMap<u32, Seen>,
}

impl<P: Procs> ComicScanner<P> {
    pub fn new(procs: P) -> ComicScanner<P> {
        ComicScanner { procs, seen: HashMap::new() }
    }

    /// The comics open now, one per path (the lowest pid's), sorted by path.
    pub fn scan(&mut self, folders: &[String]) -> Vec<OpenComic> {
        let pids = self.procs.pids();
        let live: HashSet<u32> = pids.iter().copied().collect();
        self.seen.retain(|p, _| live.contains(p));
        let mut out: Vec<OpenComic> = Vec::new();
        for pid in pids {
            let seen = match self.seen.get(&pid) {
                Some(s) => s.clone(),
                None => {
                    let s = match self.procs.name(pid) {
                        Some(n) if !self.procs.mine(pid) || ignored(&n) => Seen::Skip,
                        Some(n) => reader_of(&n).map_or(Seen::Other(n), Seen::Reader),
                        None => Seen::Skip,
                    };
                    self.seen.insert(pid, s.clone());
                    s
                }
            };
            let (app, anywhere) = match seen {
                Seen::Skip => continue,
                Seen::Reader(r) => (r.to_string(), true),
                // Other processes only matter inside the comic folders.
                Seen::Other(_) if folders.is_empty() => continue,
                Seen::Other(n) => (n, false),
            };
            for (_, f) in self.procs.files(pid) {
                let Some(c) = comic_of(&f) else { continue };
                if !inside(&c, folders) && !anywhere {
                    continue;
                }
                if !out.iter().any(|o| o.path == c) {
                    out.push(OpenComic { pid, app: app.clone(), path: c });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
}

/// The system's process table (macOS: libproc; Linux: `/proc`). `None` on Windows (not yet).
#[cfg(target_os = "macos")]
pub fn system() -> Option<ComicScanner<crate::libproc::LibProc>> {
    Some(ComicScanner::new(crate::libproc::LibProc::new()))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn system() -> Option<ComicScanner<crate::procfs::ProcFs>> {
    Some(ComicScanner::new(crate::procfs::ProcFs::system()))
}

/// Windows: TODO (see the module's notes).
#[cfg(windows)]
pub fn system() -> Option<ComicScanner<NoProcs>> {
    None
}

/// The empty process table (Windows, until it has a source).
pub struct NoProcs;

impl Procs for NoProcs {
    fn pids(&mut self) -> Vec<u32> {
        Vec::new()
    }
    fn name(&self, _pid: u32) -> Option<String> {
        None
    }
    fn mine(&self, _pid: u32) -> bool {
        false
    }
    fn files(&self, _pid: u32) -> Vec<(i32, String)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A made-up process table. `names_read` counts name lookups.
    struct Fake {
        procs: Vec<(u32, &'static str, bool, Vec<&'static str>)>,
        names_read: Cell<u32>,
    }

    impl Procs for Fake {
        fn pids(&mut self) -> Vec<u32> {
            self.procs.iter().map(|p| p.0).collect()
        }
        fn name(&self, pid: u32) -> Option<String> {
            self.names_read.set(self.names_read.get() + 1);
            self.procs.iter().find(|p| p.0 == pid).map(|p| p.1.to_string())
        }
        fn mine(&self, pid: u32) -> bool {
            self.procs.iter().any(|p| p.0 == pid && p.2)
        }
        fn files(&self, pid: u32) -> Vec<(i32, String)> {
            self.procs.iter().find(|p| p.0 == pid).map(|p| p.3.iter().enumerate().map(|(i, f)| (i as i32, f.to_string())).collect()).unwrap_or_default()
        }
    }

    fn table() -> Fake {
        Fake {
            procs: vec![
                (10, "Simple Comic", true, vec!["/Volumes/Disk/Glass Lantern Club c012.cbz", "/System/Library/Fonts/a.ttf"]),
                (11, "okular", true, vec!["/srv/manga/Tsuki no Niwa/Ch 3/005.jpg", "/srv/manga/Tsuki no Niwa/Ch 3/006.jpg"]),
                (12, "mdworker_shared", true, vec!["/srv/manga/Other/c001.cbz"]),
                (13, "someviewer", true, vec!["/srv/manga/Paper Swallows/v02.cbr", "/tmp/notes.pdf"]),
                (14, "Simple Comic", false, vec!["/srv/other/c001.cbz"]),
                (15, "mpv", true, vec!["/srv/anime/a.mkv"]),
            ],
            names_read: Cell::new(0),
        }
    }

    #[test]
    fn readers_anywhere_others_only_in_comic_folders() {
        let mut s = ComicScanner::new(table());
        let got = s.scan(&["/srv/manga/".into()]);
        let v: Vec<(&str, &str)> = got.iter().map(|o| (o.app.as_str(), o.path.as_str())).collect();
        assert_eq!(
            v,
            [
                ("Simple Comic", "/Volumes/Disk/Glass Lantern Club c012.cbz"),
                ("someviewer", "/srv/manga/Paper Swallows/v02.cbr"),
                ("Okular", "/srv/manga/Tsuki no Niwa/Ch 3"),
            ]
        );
    }

    #[test]
    fn without_folders_only_readers_are_looked_at() {
        let mut s = ComicScanner::new(table());
        let got = s.scan(&[]);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|o| o.app == "Simple Comic" || o.app == "Okular"));
    }

    #[test]
    fn names_are_read_once() {
        let mut s = ComicScanner::new(table());
        s.scan(&[]);
        let first = s.procs.names_read.get();
        s.scan(&[]);
        assert_eq!(s.procs.names_read.get(), first);
    }

    #[test]
    fn what_counts_as_a_comic() {
        assert_eq!(comic_of("/m/a/c01.CBZ").as_deref(), Some("/m/a/c01.CBZ"));
        assert_eq!(comic_of("/m/a/Ch 1/001.webp").as_deref(), Some("/m/a/Ch 1"));
        assert_eq!(comic_of("/m/a/movie.mkv"), None);
        assert_eq!(comic_of("relative.cbz"), None);
        assert_eq!(comic_of("/m/a/x.cbz (deleted)"), None);
        assert!(inside("/m/Manga/x.cbz", &["/m/Manga".into()]));
        assert!(!inside("/m/Manga2/x.cbz", &["/m/Manga".into()]));
        assert_eq!(reader_of("YACReader"), Some("YACReader"));
        assert!(ignored("mdworker_shared") && ignored("tracker-miner-fs-3") && !ignored("okular"));
    }
}
