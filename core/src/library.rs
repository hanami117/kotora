//! Library folders: the folders where the user's anime files are, which episodes
//! they hold, and the file for the next episode ("Play next episode").
//!
//! - [`skip_dir`] and [`skip_file`]: the name rules (samples, extras, creditless openings and
//!   endings, trailers, hidden files).
//! - [`scan`]: walks one folder and updates the `library_file` rows incrementally: only new or
//!   changed files (size or time) are parsed again. A missing or empty folder is *offline* (an
//!   unplugged drive, an empty mount point): its rows stay, nothing is forgotten.
//! - [`Index::build`]: the rows matched to the list, per title and season once, with the same
//!   rules as the playing file (the user's "Wrong anime" picks, anime-relations). Only sure
//!   matches count: a guess never plays the wrong show.
//! - [`Index::next`]: the file for progress + 1.
//!
//! No network here: a file whose title isn't on the list is skipped (no MyAnimeList search per file).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::ListItem;
use crate::recognize::relations::Relations;
use crate::recognize::{self, Cached, Found, Parsed};
use crate::store::{Result, Store};

/// Video file extensions (lower case).
const VIDEO: &[&str] = &["mkv", "mp4", "m4v", "avi", "webm", "mov", "wmv", "ts", "m2ts", "ogm", "flv", "mpg", "mpeg", "rmvb"];

/// Folder names that hold no episodes (lower case, compared whole).
const SKIP_DIRS: &[&str] = &[
    "sample", "samples", "extra", "extras", "bonus", "bonuses", "featurette", "featurettes", "trailer", "trailers", "nc", "ncop", "nced", "ncoped",
    "creditless", "menu", "menus", "scans", "covers", "fonts", "subs", "subtitles", "attachments", "@eadir", "$recycle.bin", "lost+found",
];

/// Words in a file name that make it no episode (compared whole, after splitting on anything that
/// isn't a letter or digit; `nc op`, `NCOP1`, `OP2` and `ED` with a number all count).
fn skip_word(w: &str) -> bool {
    let num = |prefix: &str| w.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()));
    let num_only = |prefix: &str| w.strip_prefix(prefix).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    matches!(w, "sample" | "creditless" | "trailer" | "teaser" | "preview" | "previews" | "featurette")
        || num("ncop")
        || num("nced")
        || num("pv")
        || num("cm")
        || num("menu")
        || num_only("op")
        || num_only("ed")
}

/// Whether a folder (its name alone) is skipped with all it holds.
pub fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name.to_lowercase().as_str())
}

/// Whether a file name is a video file that may be an episode.
pub fn is_episode_file(name: &str) -> bool {
    if name.starts_with('.') {
        return false;
    }
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !VIDEO.contains(&ext.to_ascii_lowercase().as_str()) {
        return false;
    }
    !skip_file(stem)
}

/// The name rules on a file name without its extension.
pub fn skip_file(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    // "NC OP", "NC ED": the two words apart.
    words.windows(2).any(|p| p[0] == "nc" && (p[1].starts_with("op") || p[1].starts_with("ed"))) || words.iter().any(|w| skip_word(w))
}

/// One file found in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    /// The library folder it was found in (as the user added it).
    pub folder: String,
    pub size: u64,
    /// Modification time, unix seconds.
    pub mtime: i64,
    pub title: String,
    pub episode: Option<u32>,
    pub season: Option<u32>,
    pub group: Option<String>,
}

impl FileRow {
    fn parsed(&self) -> Parsed {
        Parsed {
            file_name: Path::new(&self.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            title: self.title.clone(),
            episode: self.episode,
            season: self.season,
            group: self.group.clone(),
            resolution: None,
        }
    }
}

/// What a scan of one folder found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// The folder is there and holds something. Offline: nothing was changed.
    pub online: bool,
    /// Episode files in the folder now (the saved ones when offline).
    pub files: usize,
    pub added: usize,
    pub removed: usize,
}

/// Deepest folder level walked (a loop of links, or a whole disk added by mistake).
const MAX_DEPTH: usize = 12;
/// Most files kept per folder (a whole disk added by mistake).
const MAX_FILES: usize = 50_000;

/// Walks `root`: every episode file with its size and time. `None` when the folder is missing,
/// empty (an unmounted mount point) or a read fails half way (a drive pulled out): then nothing
/// may be removed.
pub fn walk(root: &Path) -> Option<Vec<(PathBuf, u64, i64)>> {
    let meta = std::fs::metadata(root).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let mut out = Vec::new();
    let mut seen_dirs = HashSet::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut root_empty = true;
    while let Some((dir, depth)) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            // The root unreadable: offline. A sub-folder we may not read: skipped.
            Err(_) if depth == 0 => return None,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(_) => return None,
        };
        for entry in rd {
            let entry = entry.ok()?;
            if depth == 0 {
                root_empty = false;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Links are followed (a folder of links to another drive), loops are not.
            let Ok(m) = std::fs::metadata(entry.path()) else { continue };
            if m.is_dir() {
                if depth < MAX_DEPTH && !skip_dir(&name) && seen_dirs.insert(dir_id(&entry.path(), &m)) {
                    stack.push((entry.path(), depth + 1));
                }
            } else if m.is_file() && is_episode_file(&name) {
                let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
                out.push((entry.path(), m.len(), mtime));
                if out.len() >= MAX_FILES {
                    return Some(out);
                }
            }
        }
    }
    if root_empty {
        return None;
    }
    Some(out)
}

#[cfg(unix)]
fn dir_id(_path: &Path, m: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}

/// Windows has no inode in stable `std`: the folder's real path (links resolved) stands in.
#[cfg(not(unix))]
fn dir_id(path: &Path, _m: &std::fs::Metadata) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    real.hash(&mut h);
    (0, h.finish())
}

/// The saved rows of one folder.
pub fn load_folder(st: &Store, folder: &str) -> Result<Vec<FileRow>> {
    let mut q = st.conn.prepare("SELECT path, folder, size, mtime, title, episode, season, grp FROM library_file WHERE folder = ?1")?;
    let rows = q.query_map(params![folder], row)?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// All saved rows.
pub fn load_all(st: &Store) -> Result<Vec<FileRow>> {
    let mut q = st.conn.prepare("SELECT path, folder, size, mtime, title, episode, season, grp FROM library_file")?;
    let rows = q.query_map([], row)?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        path: r.get(0)?,
        folder: r.get(1)?,
        size: r.get::<_, i64>(2)? as u64,
        mtime: r.get(3)?,
        title: r.get(4)?,
        episode: r.get(5)?,
        season: r.get(6)?,
        group: r.get(7)?,
    })
}

/// Forgets a folder's files (the user removed the folder from Settings).
pub fn forget_folder(st: &Store, folder: &str) -> Result<()> {
    st.conn.execute("DELETE FROM library_file WHERE folder = ?1", params![folder])?;
    Ok(())
}

/// Forgets the files of every folder not in `keep` (removed from config.toml by hand).
pub fn keep_only(st: &Store, keep: &[String]) -> Result<usize> {
    let saved: Vec<String> = {
        let mut q = st.conn.prepare("SELECT DISTINCT folder FROM library_file")?;
        q.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
    };
    let mut n = 0;
    for f in saved.iter().filter(|f| !keep.contains(f)) {
        n += st.conn.execute("DELETE FROM library_file WHERE folder = ?1", params![f])?;
    }
    Ok(n)
}

/// Scans one folder and brings its rows up to date. Only new and changed files are parsed.
pub fn scan(st: &mut Store, folder: &str) -> Result<Scan> {
    let found = walk(Path::new(folder));
    apply(st, folder, found)
}

/// [`scan`] with the walk done (tests give their own).
pub fn apply(st: &mut Store, folder: &str, found: Option<Vec<(PathBuf, u64, i64)>>) -> Result<Scan> {
    let old: HashMap<String, (u64, i64)> = load_folder(st, folder)?.into_iter().map(|r| (r.path, (r.size, r.mtime))).collect();
    let Some(found) = found else {
        return Ok(Scan { online: false, files: old.len(), added: 0, removed: 0 });
    };
    let tx = st.conn.transaction()?;
    let mut now = HashSet::new();
    let mut added = 0;
    for (path, size, mtime) in &found {
        let p = path.to_string_lossy().into_owned();
        if old.get(&p) != Some(&(*size, *mtime)) {
            let parsed = recognize::parse(&p);
            let (title, episode, season, group) = match parsed {
                Some(x) => (x.title, x.episode, x.season, x.group),
                None => (String::new(), None, None, None),
            };
            tx.execute(
                "INSERT OR REPLACE INTO library_file (path, folder, size, mtime, title, episode, season, grp) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![p, folder, *size as i64, mtime, title, episode, season, group],
            )?;
            if !old.contains_key(&p) {
                added += 1;
            }
        }
        now.insert(p);
    }
    let mut removed = 0;
    for p in old.keys().filter(|p| !now.contains(*p)) {
        removed += tx.execute("DELETE FROM library_file WHERE path = ?1", params![p])?;
    }
    tx.commit()?;
    Ok(Scan { online: true, files: now.len(), added, removed })
}

/// One local file of an episode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub path: String,
    pub folder: String,
    pub group: Option<String>,
    pub mtime: i64,
}

/// The files matched to the list: media id → episode → files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    pub by_media: HashMap<u32, BTreeMap<u32, Vec<Local>>>,
    /// Episode files whose title matched nothing on the list for sure.
    pub unmatched: usize,
}

/// What "Play next episode" would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The file to open.
    Ready { episode: u32, path: String },
    /// The episode is only in a folder that is offline now (its drive unplugged).
    Offline { episode: u32, folder: String },
    /// No file of this episode in the folders.
    Missing { episode: u32 },
    /// The last episode is watched.
    Done,
}

impl Next {
    /// The episode it is about (none when done).
    pub fn episode(&self) -> Option<u32> {
        match self {
            Next::Ready { episode, .. } | Next::Offline { episode, .. } | Next::Missing { episode } => Some(*episode),
            Next::Done => None,
        }
    }

    /// Why Play can't, in plain words; `None` when it can.
    pub fn reason(&self) -> Option<String> {
        match self {
            Next::Ready { .. } => None,
            Next::Offline { episode, folder } => Some(format!("Episode {episode} is on {}, which is offline", place_name(folder))),
            Next::Missing { episode } => Some(format!("Episode {episode} not in your folders")),
            Next::Done => Some("All episodes watched".into()),
        }
    }
}

/// A folder's last name ("Anime" for `/Volumes/Disk/Anime`), for short lines.
pub fn folder_name(folder: &str) -> String {
    Path::new(folder).file_name().map_or_else(|| folder.to_string(), |n| n.to_string_lossy().into_owned())
}

impl Index {
    /// Matches the rows to the list. `cached`: the recognition cache's answer for a title key
    /// (the user's picks). Each title and season is matched once; anime-relations then moves each
    /// episode on its own.
    pub fn build(rows: &[FileRow], items: &[ListItem], cached: &dyn Fn(&str) -> Cached, rel: &Relations) -> Index {
        let mut idx = Index::default();
        let mut per_key: HashMap<String, Option<u32>> = HashMap::new();
        for r in rows {
            if r.title.is_empty() {
                idx.unmatched += 1;
                continue;
            }
            let mut p = r.parsed();
            let key = p.key();
            let id = *per_key.entry(key.clone()).or_insert_with(|| {
                // The title alone: an episode past the count is the relations' business below.
                let bare = Parsed { episode: None, ..p.clone() };
                match recognize::on_list(&bare, items, &cached(&key), rel) {
                    Some(Found::List { media_id, sure: true, .. }) => Some(media_id),
                    _ => match cached(&key) {
                        // A search found it on the list before (the playing file): good enough.
                        Some((id, src, _)) if src == "search" && items.iter().any(|it| it.media.id == id) => Some(id),
                        _ => None,
                    },
                }
            });
            let Some(id) = id else {
                idx.unmatched += 1;
                continue;
            };
            let Some(item) = items.iter().find(|it| it.media.id == id) else { continue };
            // A movie (one episode) with no number is episode 1.
            if p.episode.is_none() && item.media.episodes == Some(1) {
                p.episode = Some(1);
            }
            let found = recognize::redirect(Found::List { media_id: id, sure: true, why: String::new() }, &mut p, items, rel);
            let (Found::List { media_id, .. }, Some(ep)) = (found, p.episode) else {
                idx.unmatched += 1;
                continue;
            };
            if ep == 0 {
                continue;
            }
            idx.by_media.entry(media_id).or_default().entry(ep).or_default().push(Local { path: r.path.clone(), folder: r.folder.clone(), group: r.group.clone(), mtime: r.mtime });
        }
        idx
    }

    /// The episode numbers there are files for.
    pub fn episodes(&self, media_id: u32) -> Vec<u32> {
        self.by_media.get(&media_id).map(|m| m.keys().copied().collect()).unwrap_or_default()
    }

    /// The file for `progress + 1`. Among several files of that episode: one in a folder that is
    /// online, of the same release group as the episode before when there is one, else the newest.
    pub fn next(&self, media_id: u32, progress: u32, total: Option<u32>, online: &dyn Fn(&str) -> bool) -> Next {
        let episode = progress + 1;
        if total.is_some_and(|n| n > 0 && episode > n) {
            return Next::Done;
        }
        let eps = self.by_media.get(&media_id);
        let Some(files) = eps.and_then(|m| m.get(&episode)).filter(|v| !v.is_empty()) else { return Next::Missing { episode } };
        let here: Vec<&Local> = files.iter().filter(|f| online(&f.folder)).collect();
        if here.is_empty() {
            return Next::Offline { episode, folder: files[0].folder.clone() };
        }
        let group_before = eps.and_then(|m| m.get(&progress)).and_then(|v| v.iter().find(|f| online(&f.folder)).or(v.first())).and_then(|f| f.group.clone());
        let pick = here
            .iter()
            .filter(|f| group_before.is_some() && f.group == group_before)
            .max_by_key(|f| f.mtime)
            .or_else(|| here.iter().max_by_key(|f| f.mtime))
            .expect("not empty");
        Next::Ready { episode, path: pick.path.clone() }
    }
}

/// Episode numbers in short: "1–6, 8, 10–12".
pub fn ranges(eps: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < eps.len() {
        let start = eps[i];
        let mut end = start;
        while i + 1 < eps.len() && eps[i + 1] == end + 1 {
            i += 1;
            end = eps[i];
        }
        out.push(if end == start { start.to_string() } else if end == start + 1 { format!("{start}, {end}") } else { format!("{start}\u{2013}{end}") });
        i += 1;
    }
    out.join(", ")
}

/// Where a folder is, shortly: the drive's name for a folder on an external drive
/// (`/Volumes/Archive/…`, `/run/media/user/Archive/…`, `/media/user/Archive/…`, `/mnt/Archive/…`),
/// else the folder's last name.
pub fn place_name(folder: &str) -> String {
    let parts: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
    let drive = match parts.as_slice() {
        ["Volumes", d, ..] | ["mnt", d, ..] => Some(*d),
        ["run", "media", _, d, ..] | ["media", _, d, ..] => Some(*d),
        _ => None,
    };
    drive.map_or_else(|| folder_name(folder), str::to_string)
}

/// Whether two folders overlap (one is the other, or inside it): the same file would belong to
/// both.
pub fn overlaps(a: &str, b: &str) -> bool {
    let (a, b) = (Path::new(a), Path::new(b));
    a.starts_with(b) || b.starts_with(a)
}

#[cfg(test)]
mod tests;
