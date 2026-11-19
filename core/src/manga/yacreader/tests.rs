use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::*;

// The tables below are copied from YACReader 10.5.1 (tag `10.5.1`, DB version `10.5.0`),
// `YACReaderLibrary/db/data_base_management.cpp`: `createTables` (folder, comic, db_info) and
// `createComicInfoTable` (comic_info). Only the quoting is changed. The data in them is made up.
const SCHEMA_10_5: &str = "
CREATE TABLE comic_info (
    id INTEGER PRIMARY KEY,
    title TEXT,
    coverPage INTEGER DEFAULT 1,
    numPages INTEGER,
    number TEXT,
    isBis BOOLEAN,
    count INTEGER,
    volume TEXT,
    storyArc TEXT,
    arcNumber TEXT,
    arcCount INTEGER,
    genere TEXT,
    writer TEXT,
    penciller TEXT,
    inker TEXT,
    colorist TEXT,
    letterer TEXT,
    coverArtist TEXT,
    date TEXT,
    publisher TEXT,
    format TEXT,
    color BOOLEAN,
    ageRating TEXT,
    synopsis TEXT,
    characters TEXT,
    notes TEXT,
    hash TEXT UNIQUE NOT NULL,
    edited BOOLEAN DEFAULT 0,
    read BOOLEAN DEFAULT 0,
    hasBeenOpened BOOLEAN DEFAULT 0,
    rating REAL DEFAULT 0,
    currentPage INTEGER DEFAULT 1,
    bookmark1 INTEGER DEFAULT -1,
    bookmark2 INTEGER DEFAULT -1,
    bookmark3 INTEGER DEFAULT -1,
    brightness INTEGER DEFAULT -1,
    contrast INTEGER DEFAULT -1,
    gamma INTEGER DEFAULT -1,
    comicVineID TEXT,
    lastTimeOpened INTEGER,
    coverSizeRatio REAL,
    originalCoverSize STRING,
    manga BOOLEAN DEFAULT 0,
    added INTEGER,
    type INTEGER DEFAULT 0,
    editor TEXT,
    imprint TEXT,
    teams TEXT,
    locations TEXT,
    series TEXT,
    alternateSeries TEXT,
    alternateNumber TEXT,
    alternateCount INTEGER,
    languageISO TEXT,
    seriesGroup TEXT,
    mainCharacterOrTeam TEXT,
    review TEXT,
    tags TEXT,
    imageFiltersJson TEXT,
    lastTimeImageFiltersSet INTEGER DEFAULT 0,
    lastTimeCoverSet INTEGER DEFAULT 0,
    usesExternalCover BOOLEAN DEFAULT 0,
    lastTimeMetadataSet INTEGER DEFAULT 0
);
CREATE TABLE folder (
    id INTEGER PRIMARY KEY,
    parentId INTEGER NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    finished BOOLEAN DEFAULT 0,
    completed BOOLEAN DEFAULT 1,
    numChildren INTEGER,
    firstChildHash TEXT,
    customImage TEXT,
    manga BOOLEAN DEFAULT 0,
    type INTEGER DEFAULT 0,
    added INTEGER,
    updated INTEGER,
    FOREIGN KEY(parentId) REFERENCES folder(id) ON DELETE CASCADE
);
CREATE TABLE comic (id INTEGER PRIMARY KEY, parentId INTEGER NOT NULL, comicInfoId INTEGER NOT NULL,  fileName TEXT NOT NULL, path TEXT, FOREIGN KEY(parentId) REFERENCES folder(id) ON DELETE CASCADE, FOREIGN KEY(comicInfoId) REFERENCES comic_info(id));
CREATE TABLE db_info (version TEXT NOT NULL);
INSERT INTO db_info (version) VALUES ('10.5.0');
INSERT INTO folder (id, parentId, name, path) VALUES (1, 1, 'root', '/');
";

/// A `comic_info` without `series` and the newer columns (shaped like YACReader 9.5 to 9.12).
const SCHEMA_OLD: &str = "
CREATE TABLE comic_info (
    id INTEGER PRIMARY KEY,
    numPages INTEGER,
    hash TEXT UNIQUE NOT NULL,
    read BOOLEAN DEFAULT 0,
    hasBeenOpened BOOLEAN DEFAULT 0,
    currentPage INTEGER DEFAULT 1,
    lastTimeOpened INTEGER
);
CREATE TABLE comic (id INTEGER PRIMARY KEY, parentId INTEGER NOT NULL, comicInfoId INTEGER NOT NULL,  fileName TEXT NOT NULL, path TEXT);
CREATE TABLE db_info (version TEXT NOT NULL);
INSERT INTO db_info (version) VALUES ('9.8.0');
";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("kot-yac-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Row {
    path: &'static str,
    page: i64,
    pages: i64,
    read: bool,
    last: Option<i64>,
    series: Option<&'static str>,
}

fn row(path: &'static str, page: i64, pages: i64, read: bool, last: Option<i64>) -> Row {
    Row {
        path,
        page,
        pages,
        read,
        last,
        series: None,
    }
}

/// Builds `<library>/.yacreaderlibrary/library.ydb` and returns the library's folder.
fn make_library(root: &Path, schema: &str, rows: &[Row], with_series_column: bool) -> PathBuf {
    let lib = root.join("library");
    fs::create_dir_all(lib.join(".yacreaderlibrary")).unwrap();
    let conn = Connection::open(lib.join(".yacreaderlibrary/library.ydb")).unwrap();
    conn.execute_batch(schema).unwrap();
    for (i, r) in rows.iter().enumerate() {
        let id = i as i64 + 1;
        conn.execute(
            "INSERT INTO comic_info (id, hash, numPages, currentPage, read, hasBeenOpened, lastTimeOpened) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, format!("hash{id}"), r.pages, r.page, r.read, r.last.is_some(), r.last],
        )
        .unwrap();
        if with_series_column && let Some(s) = r.series {
            conn.execute(
                "UPDATE comic_info SET series = ?1 WHERE id = ?2",
                rusqlite::params![s, id],
            )
            .unwrap();
        }
        let file = r.path.rsplit('/').next().unwrap();
        conn.execute("INSERT INTO comic (id, parentId, comicInfoId, fileName, path) VALUES (?1, 1, ?1, ?2, ?3)", rusqlite::params![id, file, r.path]).unwrap();
    }
    lib
}

#[test]
fn lists_what_was_opened_since() {
    let s = Scratch::new("since");
    let rows = [
        Row {
            series: Some("Lantern Road"),
            ..row(
                "/Lantern Road/Volume 03/Chapter 021.cbz",
                20,
                20,
                true,
                Some(1_700_000_300),
            )
        },
        row(
            "/Lantern Road/Volume 03/Chapter 022.cbz",
            5,
            20,
            false,
            Some(1_700_000_200),
        ),
        row(
            "/Night Owls (2019)/Night Owls v02 c011 (Digital).cbz",
            19,
            20,
            false,
            Some(1_700_000_100),
        ),
        row("/Old Series/c001.cbz", 10, 10, true, Some(1_600_000_000)), // before `since`
        row("/Never Opened/c001.cbz", 1, 30, false, None),
    ];
    let lib = make_library(&s.0, SCHEMA_10_5, &rows, true);
    let db = Db::open(&lib.join(".yacreaderlibrary/library.ydb")).unwrap();
    assert_eq!(db.version.as_deref(), Some("10.5.0"));

    let got = db.opened_since(1_700_000_000).unwrap();
    let names: Vec<_> = got.iter().map(|o| o.rel_path.as_str()).collect();
    assert_eq!(
        names,
        [
            "/Lantern Road/Volume 03/Chapter 021.cbz",
            "/Lantern Road/Volume 03/Chapter 022.cbz",
            "/Night Owls (2019)/Night Owls v02 c011 (Digital).cbz"
        ]
    );

    let a = &got[0];
    assert_eq!(a.series(), "Lantern Road");
    assert_eq!(a.meta_series.as_deref(), Some("Lantern Road"));
    assert_eq!(a.parsed.volume.map(|v| v.first), Some(3.0));
    assert_eq!(a.parsed.chapter.map(|v| v.first), Some(21.0));
    assert_eq!(
        (a.current_page, a.num_pages, a.read_flag, a.last_opened),
        (20, 20, true, 1_700_000_300)
    );
    assert!(a.is_read());
    assert_eq!(a.path, lib.join("Lantern Road/Volume 03/Chapter 021.cbz"));

    let b = &got[1];
    assert_eq!(b.meta_series, None);
    assert_eq!(b.series(), "Lantern Road");
    assert!(!b.is_read());

    // On the one-before-last page without the flag: still counts (`chapter_read`).
    let c = &got[2];
    assert_eq!(c.series(), "Night Owls");
    assert_eq!(c.parsed.year, Some(2019));
    assert_eq!(c.parsed.chapter.map(|v| v.first), Some(11.0));
    assert!(!c.read_flag);
    assert!(c.is_read());

    // Everything, and nothing.
    assert_eq!(db.opened_since(0).unwrap().len(), 4);
    assert!(db.opened_since(1_800_000_000).unwrap().is_empty());
}

#[test]
fn the_metadata_series_fills_in_when_the_path_has_none() {
    let s = Scratch::new("meta");
    let rows = [Row {
        series: Some("Lantern Road"),
        ..row("/c021.cbz", 3, 20, false, Some(10))
    }];
    let lib = make_library(&s.0, SCHEMA_10_5, &rows, true);
    let got = Db::open(&lib.join(".yacreaderlibrary/library.ydb"))
        .unwrap()
        .opened_since(0)
        .unwrap();
    assert_eq!(got[0].parsed.series, "");
    assert_eq!(got[0].series(), "Lantern Road");
}

#[test]
fn an_older_schema_without_the_series_column_works() {
    let s = Scratch::new("old");
    let rows = [row("/Lantern Road/c021.cbz", 4, 20, false, Some(100))];
    let lib = make_library(&s.0, SCHEMA_OLD, &rows, false);
    let db = lib_db(&lib);
    assert_eq!(db.version.as_deref(), Some("9.8.0"));
    let got = db.opened_since(0).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].series(), "Lantern Road");
    assert_eq!(got[0].meta_series, None);
}

fn lib_db(lib: &Path) -> Db {
    Db::open(&lib.join(".yacreaderlibrary/library.ydb")).unwrap()
}

#[test]
fn a_schema_without_the_needed_columns_is_refused() {
    let s = Scratch::new("bad");
    let dir = s.0.join("library/.yacreaderlibrary");
    fs::create_dir_all(&dir).unwrap();
    let conn = Connection::open(dir.join("library.ydb")).unwrap();
    conn.execute_batch("CREATE TABLE comic_info (id INTEGER PRIMARY KEY, numPages INTEGER); CREATE TABLE comic (id INTEGER);").unwrap();
    drop(conn);
    let err = Db::open(&dir.join("library.ydb")).err().expect("must fail");
    assert!(err.0.contains("currentPage"), "{err}");
}

#[test]
fn a_missing_or_foreign_file_is_an_error_not_a_panic() {
    let s = Scratch::new("missing");
    assert!(Db::open(&s.0.join("nope/.yacreaderlibrary/library.ydb")).is_err());
    let junk = s.0.join("library.ydb");
    fs::write(&junk, b"this is not a database at all, just text").unwrap();
    assert!(Db::open(&junk).is_err());
}

/// The reader must leave no trace: same bytes, same modification time, no journal.
#[test]
fn reading_changes_nothing() {
    let s = Scratch::new("ro");
    let rows = [row("/Lantern Road/c021.cbz", 4, 20, false, Some(100))];
    let lib = make_library(&s.0, SCHEMA_10_5, &rows, true);
    let dir = lib.join(".yacreaderlibrary");
    let file = dir.join("library.ydb");
    let before = (
        fs::read(&file).unwrap(),
        fs::metadata(&file).unwrap().modified().unwrap(),
    );
    let listing = |d: &Path| {
        let mut v: Vec<_> = fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let names_before = listing(&dir);
    {
        let db = Db::open(&file).unwrap();
        db.opened_since(0).unwrap();
        db.opened_since(0).unwrap();
    }
    assert_eq!(fs::read(&file).unwrap(), before.0);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before.1);
    assert_eq!(listing(&dir), names_before);
}

/// A read-only file in a read-only folder (a mounted copy, a locked share) opens too.
#[test]
#[cfg(unix)]
fn a_read_only_folder_opens() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("perm");
    let rows = [row("/Lantern Road/c021.cbz", 4, 20, false, Some(100))];
    let lib = make_library(&s.0, SCHEMA_10_5, &rows, true);
    let dir = lib.join(".yacreaderlibrary");
    fs::set_permissions(dir.join("library.ydb"), fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let got = lib_db(&lib).opened_since(0).map(|v| v.len());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(got.unwrap(), 1);
}

#[test]
fn odd_characters_in_the_library_path() {
    let s = Scratch::new("odd");
    // Windows allows no `?` in a name.
    let root = s.0.join(if cfg!(windows) {
        "Manga & Comics #1 (100%) what"
    } else {
        "Manga & Comics #1 (100%) what?"
    });
    fs::create_dir_all(&root).unwrap();
    let rows = [row("/Lantern Road/c021.cbz", 4, 20, false, Some(100))];
    let lib = make_library(&root, SCHEMA_10_5, &rows, true);
    assert_eq!(lib_db(&lib).opened_since(0).unwrap().len(), 1);
    #[cfg(unix)]
    assert_eq!(
        immutable_uri(Path::new("/a b/c#d?e%f/ü.ydb")),
        "file:/a%20b/c%23d%3Fe%25f/%C3%BC.ydb?immutable=1"
    );
}

#[test]
fn a_library_knows_its_database() {
    let lib = Library {
        name: "Manga".into(),
        path: PathBuf::from("/shelf/manga"),
        id: None,
    };
    assert_eq!(
        lib.db_path(),
        Path::new("/shelf/manga/.yacreaderlibrary/library.ydb")
    );
}

// The library list. This literal is shaped like what Qt writes (`\xNN` for hex-digit characters
// after a `\0`, `\n` for 10); the name is "Manga", the path "/tmp/lib", and the id is all zeros.
const INI_NEW: &str = r"[General]
LIBRARIES=@ByteArray(\0\0\0\x1\0\0\0\n\0M\0\x61\0n\0g\0\x61\0\0\0\x10\0/\0t\0m\0p\0/\0l\0i\0\x62\0\0\0\x1\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0)

[docroot]
path=/usr/share/yacreader/server/docroot
";

#[test]
fn reads_the_library_list() {
    let libs = parse_libraries(INI_NEW);
    assert_eq!(
        libs,
        [Library {
            name: "Manga".into(),
            path: PathBuf::from("/tmp/lib"),
            id: Some(1)
        }]
    );
}

/// A Qt `QDataStream` string, for building test input.
fn qstring(s: &str) -> Vec<u8> {
    let units: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut v = (units.len() as u32).to_be_bytes().to_vec();
    v.extend(units);
    v
}

/// Writes bytes the way `QSettings` does (the `@ByteArray(...)` form).
fn qsettings_bytes(b: &[u8]) -> String {
    let mut s = String::from("LIBRARIES=@ByteArray(");
    let mut after_escape = false;
    for &c in b {
        let plain = c.is_ascii_graphic() || c == b' ';
        if plain && c != b'\\' && c != b'"' && !(after_escape && c.is_ascii_hexdigit()) {
            s.push(c as char);
            after_escape = false;
        } else {
            match c {
                b'\\' => s.push_str("\\\\"),
                b'"' => s.push_str("\\\""),
                b'\n' => s.push_str("\\n"),
                0 => s.push_str("\\0"),
                _ => s.push_str(&format!("\\x{c:x}")),
            }
            after_escape = true;
        }
    }
    s.push(')');
    s
}

#[test]
fn reads_several_libraries_with_odd_names() {
    let mut b = 3u32.to_be_bytes().to_vec();
    for (name, path) in [
        ("Manga", "/data/manga"),
        ("Comics \"US\"", "/data/Comics (US)"),
        ("本", "/data/日本語/本"),
    ] {
        b.extend(qstring(name));
        b.extend(qstring(path));
        b.extend(7i32.to_be_bytes());
        b.extend([
            0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0xab, 0xcd,
        ]);
    }
    let libs = parse_libraries(&qsettings_bytes(&b));
    let got: Vec<_> = libs
        .iter()
        .map(|l| (l.name.as_str(), l.path.to_str().unwrap()))
        .collect();
    assert_eq!(
        got,
        [
            ("Manga", "/data/manga"),
            ("Comics \"US\"", "/data/Comics (US)"),
            ("本", "/data/日本語/本")
        ]
    );
    assert!(
        libs.iter().all(|l| l.id == Some(7)),
        "the viewer's --libraryId"
    );
}

#[test]
fn reads_the_old_library_list() {
    // YACReader before 9.14: QMap<QString, QPair<int, QString>>.
    let mut b = 1u32.to_be_bytes().to_vec();
    b.extend(qstring("Manga"));
    b.extend(1i32.to_be_bytes());
    b.extend(qstring("/old/manga"));
    let libs = parse_libraries(&qsettings_bytes(&b));
    assert_eq!(
        libs,
        [Library {
            name: "Manga".into(),
            path: PathBuf::from("/old/manga"),
            id: Some(1)
        }]
    );
}

#[test]
fn a_bad_library_list_is_empty() {
    assert!(parse_libraries("").is_empty());
    assert!(parse_libraries("[General]\nOTHER=1\n").is_empty());
    assert!(parse_libraries("LIBRARIES=@ByteArray()").is_empty());
    assert!(parse_libraries("LIBRARIES=@ByteArray(\\0\\0\\0\\x5)").is_empty()); // five libraries, no data
    assert!(parse_libraries("LIBRARIES=@ByteArray(\\0\\0\\0\\x1\\0\\0\\0\\x4\\0M)").is_empty()); // cut short
    assert!(parse_libraries("LIBRARIES=@ByteArray(\\").is_empty()); // not even a full escape
    assert!(parse_libraries("LIBRARIES=@Variant(\\0\\0)").is_empty());
}

#[test]
fn a_missing_ini_is_an_empty_list() {
    let s = Scratch::new("ini");
    assert!(libraries(&s.0.join("none.ini")).unwrap().is_empty());
    let f = s.0.join("YACReaderLibrary.ini");
    fs::write(&f, INI_NEW).unwrap();
    assert_eq!(libraries(&f).unwrap().len(), 1);
}

#[test]
fn the_settings_candidates_follow_xdg() {
    // Not set here (tests share the process environment): only check the shape.
    let c = ini_candidates();
    assert!(!c.is_empty());
    assert!(
        c.iter()
            .all(|p| p.ends_with("YACReader/YACReaderLibrary/YACReaderLibrary.ini"))
    );
}

/// The viewer's `--comicId`: the row is there while the comic is open, with what was saved
/// before (here: never opened, page 1, no time), as in the live test.
#[test]
fn a_comic_by_its_id_even_before_progress_is_written() {
    let s = Scratch::new("byid");
    let rows = [
        row(
            "/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz",
            1,
            0,
            false,
            None,
        ),
        row("/Other/c001.cbz", 3, 20, false, Some(1_700_000_000)),
    ];
    let lib = make_library(&s.0, SCHEMA_10_5, &rows, true);
    let db = Db::open(&lib.join(".yacreaderlibrary/library.ydb")).unwrap();
    let o = db.comic(1).unwrap().unwrap();
    assert_eq!(
        o.path,
        lib.join("Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz")
    );
    assert_eq!(
        (
            o.series(),
            o.parsed.volume.map(|v| v.first),
            o.parsed.chapter
        ),
        ("Dragon Ball Z", Some(11.0), None)
    );
    assert_eq!((o.current_page, o.read_flag, o.last_opened), (1, false, 0));
    assert!(!o.is_read());
    assert_eq!(db.comic(99).unwrap(), None);
}
