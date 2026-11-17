//! The YACReader viewer opened on a file directly (Finder, a double-click, "Open with"), not
//! from its library: no `--comicId`, and on macOS no path in its command line either (the file
//! arrives as an Apple Event), and it keeps no file open. What it does write (checked against
//! YACReader 9.x/10.x on macOS, 2026-11-17):
//!
//! - `yacreader.log` in its settings folder: `"Opened archive file : <path>"` the moment a comic
//!   opens. [`LogTail`] reads only what was added since the last look.
//! - `bookmarks.yacr` beside it, on close: a `QDataStream` of `QMap<QString, Bookmark>` keyed by
//!   the lower-case file name followed by the file's size, with `lastPage` (from 0), the
//!   bookmarks and the time ([`bookmarks`], [`bookmark_key`]). It has no page count:
//!   [`zip_pages`] counts a `.cbz`'s images.
//!
//! The window title would say it too, but on macOS reading other apps' window titles needs the
//! Screen Recording permission: not used.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The viewer's settings folders, most likely first (macOS, Linux, Flatpak).
pub fn dirs() -> Vec<PathBuf> {
    let home = crate::paths::home_dir().unwrap_or_else(std::env::temp_dir);
    let data = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".local/share"));
    let tail = "YACReader/YACReader";
    let mut v = Vec::new();
    if cfg!(target_os = "macos") {
        v.push(home.join("Library/Application Support").join(tail));
    }
    v.push(data.join(tail));
    v.push(home.join(".var/app/com.yacreader.YACReader/data").join(tail));
    v
}

/// The first folder that has the viewer's log.
pub fn find_dir() -> Option<PathBuf> {
    dirs().into_iter().find(|d| d.join("yacreader.log").is_file())
}

const OPENED: &str = "Opened archive file : ";

/// The comic paths a piece of the log says were opened, in order.
pub fn opened_in(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let rest = &l[l.find(OPENED)? + OPENED.len()..];
            let p = rest.trim().trim_end_matches('"').trim_end();
            p.starts_with('/').then(|| p.to_string()).or_else(|| (p.len() > 2 && p.as_bytes()[1] == b':').then(|| p.to_string()))
        })
        .collect()
}

/// Follows `yacreader.log`: each [`LogTail::poll`] reads only the bytes added since the last.
pub struct LogTail {
    path: PathBuf,
    offset: u64,
    /// The comic the log said was opened last.
    pub last: Option<String>,
}

/// How far back the first look goes.
const FIRST_LOOK: u64 = 64 * 1024;

impl LogTail {
    pub fn new(dir: &Path) -> LogTail {
        LogTail { path: dir.join("yacreader.log"), offset: 0, last: None }
    }

    /// Reads what was added; `true` when it named a newly opened comic.
    pub fn poll(&mut self) -> bool {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return false };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // A new log (YACReader started again): from its start.
            self.offset = 0;
        }
        if self.offset == 0 {
            self.offset = len.saturating_sub(FIRST_LOOK);
        }
        if len == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut buf = Vec::new();
        if f.take(len - self.offset).read_to_end(&mut buf).is_err() {
            return false;
        }
        self.offset = len;
        match opened_in(&String::from_utf8_lossy(&buf)).pop() {
            Some(p) if self.last.as_deref() != Some(p.as_str()) => {
                self.last = Some(p);
                true
            }
            Some(_) => true,
            None => false,
        }
    }
}

/// `bookmarks.yacr`'s key for a file: its name in lower case, then its size in bytes.
pub fn bookmark_key(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    Some(format!("{}{size}", path.file_name()?.to_string_lossy().to_lowercase()))
}

/// One comic's bookmark: the page it was left on (from 0) and when (ms since the epoch, UTC
/// when YACReader wrote it in UTC, else local).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub last_page: i64,
    pub added_ms: i64,
}

struct Stream<'a>(&'a [u8]);

impl Stream<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        (self.0.len() >= n).then(|| {
            let (a, b) = self.0.split_at(n);
            self.0 = b;
            a
        })
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Option<i64> {
        self.take(8).map(|b| i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u32()?;
        if n == u32::MAX {
            return Some(String::new());
        }
        let b = self.take(n as usize)?;
        let units: Vec<u16> = b.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16(&units).ok()
    }
}

/// `bookmarks.yacr`: key → bookmark. A file it can't read gives what was read before the fault.
pub fn bookmarks(bytes: &[u8]) -> HashMap<String, Bookmark> {
    let mut out = HashMap::new();
    let mut s = Stream(bytes);
    let Some(n) = s.u32() else { return out };
    for _ in 0..n {
        let Some(key) = s.string() else { break };
        let Some(last_page) = s.u32().map(|v| i64::from(v as i32)) else { break };
        // QList<int>: the bookmarked pages.
        let Some(count) = s.u32() else { break };
        if s.take(count as usize * 4).is_none() {
            break;
        }
        // QDateTime (Qt 5.2+ stream): Julian day (qint64), msecs of the day (quint32), spec (quint8),
        // then an offset (OffsetFromUTC) or a zone id (TimeZone).
        let (Some(jd), Some(ms), Some(spec)) = (s.i64(), s.u32(), s.u8()) else { break };
        match spec {
            2 if s.u32().is_none() => break,
            3 => {
                let Some(len) = s.u32() else { break };
                if len != u32::MAX && s.take(len as usize).is_none() {
                    break;
                }
            }
            _ => {}
        }
        // Julian day 2440588 is 1970-01-01.
        let added_ms = (jd - 2_440_588) * 86_400_000 + i64::from(ms);
        out.insert(key, Bookmark { last_page, added_ms });
    }
    out
}

/// Reads `bookmarks.yacr` in `dir`.
pub fn read_bookmarks(dir: &Path) -> HashMap<String, Bookmark> {
    std::fs::read(dir.join("bookmarks.yacr")).map(|b| bookmarks(&b)).unwrap_or_default()
}

/// The pages of a `.cbz`/`.zip`: its image entries, from the zip's central directory (no
/// decompression). `None`: not a zip, or unreadable.
pub fn zip_pages(path: &Path) -> Option<i64> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    // The end-of-central-directory record is in the last 64 KiB + 22 bytes.
    let tail = len.min(65_557);
    f.seek(SeekFrom::Start(len - tail)).ok()?;
    let mut buf = vec![0u8; tail as usize];
    f.read_exact(&mut buf).ok()?;
    let at = buf.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])?;
    let eocd = &buf[at..];
    let u16le = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as u64;
    let u32le = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as u64;
    if eocd.len() < 22 {
        return None;
    }
    let (entries, size, offset) = (u16le(eocd, 10), u32le(eocd, 12), u32le(eocd, 16));
    if offset + size > len || size > 16 * 1024 * 1024 {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut cd = vec![0u8; size as usize];
    f.read_exact(&mut cd).ok()?;
    let (mut i, mut pages) = (0usize, 0i64);
    for _ in 0..entries {
        if cd.len() < i + 46 || cd[i..i + 4] != [0x50, 0x4b, 0x01, 0x02] {
            break;
        }
        let (n, x, c) = (u16le(&cd, i + 28) as usize, u16le(&cd, i + 30) as usize, u16le(&cd, i + 32) as usize);
        let name = String::from_utf8_lossy(cd.get(i + 46..i + 46 + n)?).to_lowercase();
        let image = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif", ".jxl", ".bmp"].iter().any(|e| name.ends_with(e));
        if image && !name.contains("__macosx/") {
            pages += 1;
        }
        i += 46 + n + x + c;
    }
    (pages > 0).then_some(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_says_what_was_opened() {
        let log = "DEBUG 2026-11-17T22:30:01.012 \"Opened archive file : /Volumes/Disk/Dragon Ball Z v13 (2003) (Digital) (LuCaZ).cbz\" \n \nWARN  2026-11-17T22:30:39.950 QThread::wait: Thread tried to wait on itself \nDEBUG 2026-11-17T22:30:45.149 \"Opened archive file : /Volumes/Disk/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz\" \n";
        assert_eq!(opened_in(log), ["/Volumes/Disk/Dragon Ball Z v13 (2003) (Digital) (LuCaZ).cbz", "/Volumes/Disk/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz"]);
        assert!(opened_in("ERROR Sending Comic Info : unable to connect to the server").is_empty());
    }

    #[test]
    fn the_tail_reads_only_what_was_added() {
        let dir = std::env::temp_dir().join(format!("kot-viewer-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("yacreader.log");
        std::fs::write(&log, "DEBUG x \"Opened archive file : /c/a v01.cbz\" \n").unwrap();
        let mut t = LogTail::new(&dir);
        assert!(t.poll());
        assert_eq!(t.last.as_deref(), Some("/c/a v01.cbz"));
        assert!(!t.poll(), "nothing added");
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        std::io::Write::write_all(&mut f, b"WARN y\nDEBUG z \"Opened archive file : /c/a v02.cbz\" \n").unwrap();
        assert!(t.poll());
        assert_eq!(t.last.as_deref(), Some("/c/a v02.cbz"));
        // YACReader started again with a shorter log.
        std::fs::write(&log, "DEBUG \"Opened archive file : /c/b.cbz\"\n").unwrap();
        assert!(t.poll());
        assert_eq!(t.last.as_deref(), Some("/c/b.cbz"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn qstring(s: &str) -> Vec<u8> {
        let units: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut v = (units.len() as u32).to_be_bytes().to_vec();
        v.extend(units);
        v
    }

    #[test]
    fn bookmarks_as_yacreader_writes_them() {
        // Shaped like the live file: a null key first, then a comic with page 187 (from 0).
        let mut b = 2u32.to_be_bytes().to_vec();
        b.extend(u32::MAX.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend([0, 0, 0, 0, 0, 0x25, 0x8e, 0xb2, 0x04, 0xd7, 0x24, 0xc9, 0]);
        b.extend(qstring("dragon ball z v11 (2003) (digital) (lucaz).cbz160525781"));
        b.extend(187u32.to_be_bytes());
        b.extend(2u32.to_be_bytes());
        b.extend(10u32.to_be_bytes());
        b.extend(20u32.to_be_bytes());
        b.extend([0, 0, 0, 0, 0, 0x25, 0x8e, 0xb2, 0x04, 0xd7, 0x24, 0xc9, 0]);
        let m = bookmarks(&b);
        let bm = m["dragon ball z v11 (2003) (digital) (lucaz).cbz160525781"];
        assert_eq!(bm.last_page, 187);
        // 2026-11-17 22:33:28.521.
        assert_eq!(bm.added_ms, (2_461_362 - 2_440_588) * 86_400_000 + 81_208_521);
        assert_eq!(bookmarks(&b[..40]).len(), 1, "a cut file: what was read");
        assert!(bookmarks(&[]).is_empty());
    }

    #[test]
    fn a_cbz_counts_its_images() {
        // A stored (uncompressed) zip built by hand: 3 pages, a ComicInfo.xml, a macOS fork.
        let mut zip = Vec::new();
        let mut cd = Vec::new();
        let names = ["001.jpg", "002.PNG", "sub/003.webp", "ComicInfo.xml", "__MACOSX/._001.jpg"];
        for n in names {
            let off = zip.len() as u32;
            zip.extend([0x50, 0x4b, 0x03, 0x04]);
            zip.extend([0u8; 22]);
            zip.extend((n.len() as u16).to_le_bytes());
            zip.extend(0u16.to_le_bytes());
            zip.extend(n.as_bytes());
            cd.extend([0x50, 0x4b, 0x01, 0x02]);
            cd.extend([0u8; 24]);
            cd.extend((n.len() as u16).to_le_bytes());
            cd.extend([0u8; 12]);
            cd.extend(off.to_le_bytes());
            cd.extend(n.as_bytes());
        }
        let cd_off = zip.len() as u32;
        zip.extend(&cd);
        zip.extend([0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        zip.extend((names.len() as u16).to_le_bytes());
        zip.extend((names.len() as u16).to_le_bytes());
        zip.extend((cd.len() as u32).to_le_bytes());
        zip.extend(cd_off.to_le_bytes());
        zip.extend(0u16.to_le_bytes());
        let p = std::env::temp_dir().join(format!("kot-viewer-{}.cbz", std::process::id()));
        std::fs::write(&p, &zip).unwrap();
        assert_eq!(zip_pages(&p), Some(3));
        assert!(bookmark_key(&p).unwrap().ends_with(&format!(".cbz{}", zip.len())));
        std::fs::write(&p, b"not a zip").unwrap();
        assert_eq!(zip_pages(&p), None);
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(test)]
mod real_file {
    /// A copy of a real `bookmarks.yacr` (path in `KOT_BOOKMARKS`); prints counts only.
    #[test]
    #[ignore = "needs a copy of a real file"]
    fn parses_a_real_copy() {
        let Some(p) = std::env::var_os("KOT_BOOKMARKS") else { return };
        let m = super::bookmarks(&std::fs::read(p).unwrap());
        let mut pages: Vec<i64> = m.values().map(|b| b.last_page).collect();
        pages.sort();
        println!("entries {} pages {:?}", m.len(), pages);
    }
}
