//! A read-only reader of YACReader's library databases.
//!
//! YACReader (the comic reader) keeps one SQLite database per library, in
//! `<library folder>/.yacreaderlibrary/library.ydb`, and the list of libraries in its settings
//! file. Kotora reads both and **never writes to either**; YACReader may be running at the same
//! time.
//!
//! # Where things are (checked against YACReader 10.5.1, DB version `10.5.0`)
//!
//! - The library list: `YACReaderLibrary.ini`, key `LIBRARIES` in `[General]`, in YACReader's
//!   settings folder (`QStandardPaths::AppLocalDataLocation`):
//!   - Linux: `~/.local/share/YACReader/YACReaderLibrary/` (Flatpak:
//!     `~/.var/app/com.yacreader.YACReader/data/YACReader/YACReaderLibrary/`)
//!   - macOS: `~/Library/Application Support/YACReader/YACReaderLibrary/`
//!
//!   The value is a `QByteArray` written by Qt's `QSettings` (`@ByteArray(...)` with C-style
//!   escapes) that holds a `QDataStream` of `QList<YACReaderLibrary>`: a `u32` count, then per
//!   library the name (`QString`), the path (`QString`), the legacy id (`i32`) and the id
//!   (`QUuid`). YACReader before 9.14 wrote a `QMap<QString, QPair<int, QString>>` (name → id,
//!   path); that is read too.
//! - The tables used (`YACReaderLibrary/db/data_base_management.cpp`, `createTables`):
//!   `comic` (`id, parentId, comicInfoId, fileName, path`) joined to `comic_info` on
//!   `comic.comicInfoId = comic_info.id`. `comic.path` is relative to the library's folder and
//!   starts with `/`. In `comic_info`: `currentPage` (counted from 1), `numPages`, `read`,
//!   `hasBeenOpened`, `lastTimeOpened` (Unix seconds, since 9.5) and `series` (since 9.13, from
//!   the file's `ComicInfo.xml` when it has one).
//!
//! # Not disturbing YACReader
//!
//! The database is opened `SQLITE_OPEN_READ_ONLY` and with the `immutable=1` URI parameter:
//! SQLite takes no locks and makes no journal or `-shm` file. The price is that a read can catch
//! YACReader in the middle of a write (the rollback journal is not looked at), so a read may fail
//! with a "malformed" error or show a page that is a moment old. Try again later; nothing is lost.
//! (YACReader's database uses the default rollback journal, not WAL.)

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::chapter_read;
use super::name::{self, Parsed};

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// One of the user's YACReader libraries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    /// The library's folder.
    pub path: PathBuf,
}

impl Library {
    /// `<folder>/.yacreaderlibrary/library.ydb`. It is the file to watch for changes.
    pub fn db_path(&self) -> PathBuf {
        self.path.join(".yacreaderlibrary").join("library.ydb")
    }

    /// Opens the library's database, read only. See [`Db::open`].
    pub fn open(&self) -> Result<Db> {
        Db::open(&self.db_path())
    }
}

/// Where YACReader's `YACReaderLibrary.ini` may be, most likely first.
pub fn ini_candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".local/share"));
    let tail = "YACReader/YACReaderLibrary/YACReaderLibrary.ini";
    let mut v = Vec::new();
    if cfg!(target_os = "macos") {
        v.push(home.join("Library/Application Support").join(tail));
    }
    v.push(data.join(tail));
    v.push(home.join(".var/app/com.yacreader.YACReader/data").join(tail));
    v
}

/// The first existing settings file.
pub fn find_ini() -> Option<PathBuf> {
    ini_candidates().into_iter().find(|p| p.is_file())
}

/// The user's libraries, from `YACReaderLibrary.ini`. A missing file or key is an empty list.
pub fn libraries(ini: &Path) -> Result<Vec<Library>> {
    let text = match std::fs::read(ini) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error(format!("can't read {}: {e}", ini.display()))),
    };
    Ok(parse_libraries(&text))
}

/// The `LIBRARIES` key of an INI text. A value that doesn't decode gives no libraries.
pub fn parse_libraries(ini: &str) -> Vec<Library> {
    let Some(value) = ini.lines().find_map(|l| l.strip_prefix("LIBRARIES=")) else {
        return Vec::new();
    };
    let Some(bytes) = decode_qsettings_bytes(value.trim()) else {
        return Vec::new();
    };
    decode_library_list(&bytes).or_else(|| decode_legacy_list(&bytes)).unwrap_or_default()
}

/// `@ByteArray(...)` with Qt's escapes: `\\ \" \n \t \r \a \b \f \v`, octal (`\0`, up to three
/// digits) and hex (`\x1`, any number of digits, the low byte counts). A plain value is taken as
/// its own bytes.
fn decode_qsettings_bytes(value: &str) -> Option<Vec<u8>> {
    let inner = match value.strip_prefix("@ByteArray(") {
        Some(rest) => rest.strip_suffix(')')?,
        None => value,
    };
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = *b.get(i)?;
        i += 1;
        match c {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(10),
            b'r' => out.push(13),
            b't' => out.push(9),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' | b'?' => out.push(c),
            b'0'..=b'7' => {
                let mut v = u32::from(c - b'0');
                let mut n = 1;
                while n < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                    v = v * 8 + u32::from(b[i] - b'0');
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            b'x' => {
                let mut v = 0u32;
                let mut n = 0;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    v = (v << 4) | (b[i] as char).to_digit(16)?;
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    return None;
                }
                out.push(v as u8);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// A reader over a `QDataStream` (big endian).
struct Stream<'a>(&'a [u8]);

impl Stream<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A `QString`: byte length, then UTF-16 big endian. `0xFFFFFFFF` is a null string.
    fn string(&mut self) -> Option<String> {
        let len = self.u32()?;
        if len == u32::MAX {
            return Some(String::new());
        }
        let bytes = self.take(len as usize)?;
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16(&units).ok()
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

/// YACReader 9.14 and later: `QList<YACReaderLibrary>`.
fn decode_library_list(bytes: &[u8]) -> Option<Vec<Library>> {
    let mut s = Stream(bytes);
    let count = s.u32()?;
    let mut v = Vec::new();
    for _ in 0..count {
        let name = s.string()?;
        let path = s.string()?;
        s.u32()?; // the legacy id
        s.take(16)?; // the QUuid
        v.push(Library { name, path: PathBuf::from(path) });
    }
    s.done().then_some(v)
}

/// Before 9.14: `QMap<QString, QPair<int, QString>>` (name → id, path).
fn decode_legacy_list(bytes: &[u8]) -> Option<Vec<Library>> {
    let mut s = Stream(bytes);
    let count = s.u32()?;
    let mut v = Vec::new();
    for _ in 0..count {
        let name = s.string()?;
        s.u32()?;
        let path = s.string()?;
        v.push(Library { name, path: PathBuf::from(path) });
    }
    s.done().then_some(v)
}

/// A comic YACReader has opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    /// The file's path inside the library, as YACReader keeps it (`/Series/c021.cbz`).
    pub rel_path: String,
    /// The file's path: the library's folder and `rel_path`.
    pub path: PathBuf,
    /// What the name and its folders say ([`name::parse_path`]). Its `series` is empty when
    /// neither the file nor a folder has one.
    pub parsed: Parsed,
    /// The series YACReader's own metadata (`ComicInfo.xml`) gives, when it has one.
    pub meta_series: Option<String>,
    /// The page the user is on, counted from 1.
    pub current_page: i64,
    pub num_pages: i64,
    /// YACReader's own "read" flag.
    pub read_flag: bool,
    /// When it was last opened or its page last saved: Unix seconds.
    pub last_opened: i64,
}

impl Opened {
    /// Whether Kotora counts the chapter as read: [`chapter_read`].
    pub fn is_read(&self) -> bool {
        chapter_read(self.read_flag, self.current_page, self.num_pages)
    }

    /// The series to look up: the file name's (or a folder's), else YACReader's metadata.
    pub fn series(&self) -> &str {
        if self.parsed.series.is_empty() { self.meta_series.as_deref().unwrap_or("") } else { &self.parsed.series }
    }
}

/// An open library database. Read only.
pub struct Db {
    conn: Connection,
    library_dir: PathBuf,
    /// `db_info.version`, for example `10.5.0`.
    pub version: Option<String>,
    has_series: bool,
}

/// `file:` URI for `path` with `immutable=1`. Everything but unreserved characters and `/` is
/// percent-encoded (`?`, `#` and `%` would end or change the path).
fn immutable_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file:");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s.push_str("?immutable=1");
    s
}

impl Db {
    /// Opens `library.ydb` read only and without locks (see the module's notes). It fails when
    /// the file isn't there (the library's drive may be unplugged) or isn't a YACReader database
    /// that has the columns this reader needs.
    pub fn open(db_file: &Path) -> Result<Db> {
        if !db_file.is_file() {
            return Err(Error(format!("no YACReader database at {}", db_file.display())));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(immutable_uri(db_file), flags)?;
        // `<library>/.yacreaderlibrary/library.ydb` → `<library>`.
        let library_dir = db_file.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
        let version = conn.query_row("SELECT version FROM db_info", [], |r| r.get::<_, String>(0)).ok();
        let columns = columns_of(&conn, "comic_info")?;
        for need in ["currentPage", "numPages", "read", "lastTimeOpened"] {
            if !columns.iter().any(|c| c == need) {
                return Err(Error(format!("YACReader database {} has no comic_info.{need}", version.as_deref().unwrap_or("of unknown version"))));
            }
        }
        let has_series = columns.iter().any(|c| c == "series");
        Ok(Db { conn, library_dir, version, has_series })
    }

    /// The comics opened at or after `since` (Unix seconds), newest first. Comics YACReader has
    /// only listed, never opened, are not in it.
    pub fn opened_since(&self, since: i64) -> Result<Vec<Opened>> {
        let series = if self.has_series { "ci.series" } else { "NULL" };
        let sql = format!(
            "SELECT c.path, c.fileName, {series}, \
                    COALESCE(ci.currentPage, 0), COALESCE(ci.numPages, 0), COALESCE(ci.read, 0), \
                    CAST(ci.lastTimeOpened AS INTEGER) \
             FROM comic c JOIN comic_info ci ON ci.id = c.comicInfoId \
             WHERE ci.lastTimeOpened IS NOT NULL AND CAST(ci.lastTimeOpened AS INTEGER) >= ?1 \
             ORDER BY CAST(ci.lastTimeOpened AS INTEGER) DESC, c.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([since], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path, file_name, meta, current_page, num_pages, read, last_opened) = row?;
            let rel_path = path.filter(|p| !p.is_empty()).unwrap_or_else(|| format!("/{file_name}"));
            out.push(Opened {
                path: self.library_dir.join(rel_path.trim_start_matches('/')),
                parsed: name::parse_path(&rel_path),
                meta_series: meta.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                rel_path,
                current_page,
                num_pages,
                read_flag: read != 0,
                last_opened,
            });
        }
        Ok(out)
    }
}

fn columns_of(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names)
}

#[cfg(test)]
mod tests;
