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
//! Readers that load an archive into memory and close it (the YACReader viewer does) have no
//! file open while you read: their **command line** says it instead. The YACReader viewer runs
//! as `YACReader <library folder> --comicId=N --libraryId=M` ([`OpenComic::yac`]: the app looks
//! comic N up in that library's database); a reader started on a file has its path there.
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
    /// The comic file, or the folder of the image open. For [`OpenComic::yac`], the library.
    pub path: String,
    /// The YACReader viewer showing comic `id` of the library at `path`: the app reads its file
    /// from the library's database.
    pub yac: Option<i64>,
}

/// What a reader's command line names: a comic file (or a page image's folder), or YACReader's
/// `<library> --comicId=N`.
pub fn from_args(app: &str, args: &[String]) -> Vec<(String, Option<i64>)> {
    let rest = args.get(1..).unwrap_or(&[]);
    let mut out = Vec::new();
    if app == "YACReader" {
        let id = rest.iter().find_map(|a| a.strip_prefix("--comicId=")).and_then(|n| n.trim().parse::<i64>().ok());
        let lib = rest.iter().find(|a| a.starts_with('/') && !a.starts_with("--"));
        if let (Some(id), Some(lib)) = (id, lib)
            && comic_of(lib).is_none()
        {
            out.push((lib.trim_end_matches('/').to_string(), Some(id)));
            return out;
        }
    }
    for a in rest.iter().filter(|a| a.starts_with('/')) {
        if let Some(c) = comic_of(a)
            && !out.iter().any(|(p, _)| *p == c)
        {
            out.push((c, None));
        }
    }
    out
}

/// What [`ComicScanner::scan`] looks at.
#[derive(Clone, Debug, Default)]
pub struct ScanOpts {
    /// Other processes' files count inside these.
    pub folders: Vec<String>,
    /// Open files (stage 2, `[reading] open_files`).
    pub files: bool,
    /// The YACReader viewer's command line (`[reading] yacreader`).
    pub yac_args: bool,
    /// Other known readers' command lines (with `files`).
    pub reader_args: bool,
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
    /// YACReader viewers at the last scan whose command line names no comic (opened on a file
    /// by Finder: the path came as an Apple Event). The app asks the viewer's log then.
    pub bare_viewers: u32,
    /// Every YACReader viewer at the last scan (with or without a comic in its command line).
    pub viewers: u32,
}

impl<P: Procs> ComicScanner<P> {
    pub fn new(procs: P) -> ComicScanner<P> {
        ComicScanner { procs, seen: HashMap::new(), bare_viewers: 0, viewers: 0 }
    }

    /// The comics open now, one per path (the lowest pid's), sorted by path. Open files only,
    /// as stage 2 began; [`ComicScanner::scan_with`] adds command lines.
    pub fn scan(&mut self, folders: &[String]) -> Vec<OpenComic> {
        self.scan_with(&ScanOpts { folders: folders.to_vec(), files: true, yac_args: false, reader_args: false })
    }

    pub fn scan_with(&mut self, o: &ScanOpts) -> Vec<OpenComic> {
        let folders = &o.folders;
        self.bare_viewers = 0;
        self.viewers = 0;
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
                Seen::Reader(r) => {
                    let yac = r == "YACReader";
                    if (yac && o.yac_args) || (!yac && o.reader_args) {
                        let named = from_args(r, &self.procs.args(pid));
                        if yac {
                            self.viewers += 1;
                            if named.is_empty() {
                                self.bare_viewers += 1;
                            }
                        }
                        for (c, id) in named {
                            if !out.iter().any(|x| x.path == c && x.yac == id) {
                                out.push(OpenComic { pid, app: r.to_string(), path: c, yac: id });
                            }
                        }
                    }
                    if !o.files {
                        continue;
                    }
                    (r.to_string(), true)
                }
                Seen::Other(_) if !o.files => continue,
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
                    out.push(OpenComic { pid, app: app.clone(), path: c, yac: None });
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
        args: Vec<(u32, Vec<&'static str>)>,
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
        fn args(&self, pid: u32) -> Vec<String> {
            self.args.iter().find(|a| a.0 == pid).map(|a| a.1.iter().map(|s| s.to_string()).collect()).unwrap_or_default()
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
            args: Vec::new(),
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

    /// The YACReader viewer as macOS runs it: the library, the comic's id, no file open.
    const YAC: &[&str] = &["/Applications/YACReader.app/Contents/MacOS/YACReader", "/Volumes/Disk/Movies", "--comicId=1", "--libraryId=1"];

    #[test]
    fn the_yacreader_viewer_is_read_from_its_command_line() {
        let args: Vec<String> = YAC.iter().map(|s| s.to_string()).collect();
        assert_eq!(from_args("YACReader", &args), [("/Volumes/Disk/Movies".to_string(), Some(1))]);
        let file: Vec<String> = ["/Applications/YACReader.app/Contents/MacOS/YACReader", "/Volumes/Disk/Dragon Ball Z v11.cbz"].iter().map(|s| s.to_string()).collect();
        assert_eq!(from_args("YACReader", &file), [("/Volumes/Disk/Dragon Ball Z v11.cbz".to_string(), None)]);
        let sc: Vec<String> = ["/Applications/Simple Comic.app/Contents/MacOS/Simple Comic", "-psn_0_1", "/srv/manga/Glass Lantern Club c012.cbr", "/etc/hosts"].iter().map(|s| s.to_string()).collect();
        assert_eq!(from_args("Simple Comic", &sc), [("/srv/manga/Glass Lantern Club c012.cbr".to_string(), None)]);
        assert!(from_args("YACReader", &args[..1]).is_empty(), "the library app alone");
    }

    #[test]
    fn two_viewer_processes_give_one_comic_and_files_stay_optional() {
        let mut t = table();
        t.procs.push((20, "YACReader", true, vec![]));
        t.procs.push((21, "YACReader", true, vec![]));
        t.args = vec![(20, YAC.to_vec()), (21, YAC.to_vec())];
        let mut s = ComicScanner::new(t);
        let got = s.scan_with(&ScanOpts { yac_args: true, ..Default::default() });
        assert_eq!(got, [OpenComic { pid: 20, app: "YACReader".into(), path: "/Volumes/Disk/Movies".into(), yac: Some(1) }]);
        // Open files off: the other readers' files are not looked at.
        assert!(got.iter().all(|o| o.app == "YACReader"));
        let all = s.scan_with(&ScanOpts { files: true, yac_args: true, reader_args: true, folders: vec![] });
        assert!(all.iter().any(|o| o.app == "Simple Comic") && all.iter().any(|o| o.yac == Some(1)));
        assert_eq!((s.bare_viewers, s.viewers), (0, 2));
    }

    /// macOS, a comic opened from the Finder: the viewer runs with no comic in its arguments.
    #[test]
    fn a_viewer_with_no_comic_in_its_arguments_is_counted() {
        let mut t = table();
        t.procs.push((30, "YACReader", true, vec![]));
        t.args = vec![(30, vec!["/Applications/YACReader.app/Contents/MacOS/YACReader", "-psn_0_123"])];
        let mut s = ComicScanner::new(t);
        assert!(s.scan_with(&ScanOpts { yac_args: true, ..Default::default() }).is_empty());
        assert_eq!(s.bare_viewers, 1);
        assert!(s.scan_with(&ScanOpts::default()).is_empty());
        assert_eq!(s.bare_viewers, 0, "not looked at with YACReader detection off");
    }

    /// Linux: `/proc/PID/cmdline` is NUL-separated (and NUL-ended).
    #[test]
    fn linux_cmdline_splits() {
        let b = b"/usr/bin/YACReader\0/srv/comics\0--comicId=3\0--libraryId=1\0";
        let args = crate::procfs::split_nul(b);
        assert_eq!(from_args("YACReader", &args), [("/srv/comics".to_string(), Some(3))]);
        let b = b"mcomix\0/srv/manga/Glass Lantern Club/c012.cbz\0";
        assert_eq!(from_args("MComix", &crate::procfs::split_nul(b)), [("/srv/manga/Glass Lantern Club/c012.cbz".to_string(), None)]);
    }
}

