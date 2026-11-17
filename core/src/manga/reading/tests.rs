//! Made-up titles, files and rows only.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::*;
use crate::manga::name;
use crate::{Kind, Media};

fn item(id: u32, title: &str, en: Option<&str>, chapters: u32, total: Option<u32>) -> ListItem {
    let media = Media { id, kind: Kind::Manga, title: title.into(), title_en: en.map(String::from), episodes: total, ..Default::default() };
    ListItem { media, status: Status::Watching, progress: chapters, score: None, rewatching: false, times_rewatched: 0, volumes: 0, updated_at: 0 }
}

fn read_of(file: &str) -> Read {
    let parsed = name::parse_path(file);
    let o = Opened {
        id: 1,
        rel_path: format!("/{file}"),
        path: PathBuf::from("/lib").join(file),
        parsed,
        meta_series: None,
        current_page: 40,
        num_pages: 40,
        read_flag: false,
        last_opened: 100,
    };
    Read::from_opened(&o)
}

fn list() -> Vec<ListItem> {
    vec![
        item(1, "Glass Lantern Club", None, 361, None),
        item(2, "Tsuki no Niwa", Some("The Moon Garden"), 20, Some(120)),
        item(3, "Kirigaoka Detective Agency", None, 4, None),
        item(4, "Hoshi", None, 0, Some(1)),
    ]
}

#[test]
fn a_chapter_file_is_read() {
    let r = read_of("Glass Lantern Club v35 c362 (2024) (Digital).cbz");
    assert_eq!((r.series.as_str(), r.chapter, r.volume, r.volume_file), ("Glass Lantern Club", Some(362), Some(35), false));
    assert!(r.finished);
    assert_eq!(r.label(), "Ch 362 (Vol 35)");
    assert_eq!(r.file_name, "Glass Lantern Club v35 c362 (2024) (Digital).cbz");
}

#[test]
fn a_volume_file_and_a_folder_series() {
    let r = read_of("Tsuki no Niwa/Volume 03.cbz");
    assert_eq!((r.series.as_str(), r.chapter, r.volume, r.volume_file), ("Tsuki no Niwa", None, Some(3), true));
    let r = read_of("Tsuki no Niwa - Ch. 21.5.cbz");
    assert_eq!(r.chapter, Some(21), "a half chapter is the chapter before");
}

#[test]
fn the_same_title_is_sure() {
    let l = list();
    let f = on_list("Glass Lantern Club", &l, &None).unwrap();
    assert_eq!(f, Found::List { media_id: 1, sure: true, why: "same title in your list".into() });
    // The English title counts too; punctuation and case don't matter.
    assert!(matches!(on_list("the moon-garden", &l, &None), Some(Found::List { media_id: 2, sure: true, .. })));
}

#[test]
fn a_start_or_a_close_title_asks() {
    let l = list();
    assert!(matches!(on_list("Kirigaoka Detective", &l, &None), Some(Found::List { media_id: 3, sure: false, .. })));
    assert!(matches!(on_list("Glass Lanten Club", &l, &None), Some(Found::List { media_id: 1, sure: false, .. })));
    assert_eq!(on_list("Something Else Entirely", &l, &None), None);
    assert_eq!(on_list("", &l, &None), None);
}

#[test]
fn two_entries_with_the_title_ask() {
    let mut l = list();
    l.push(item(9, "Glass Lantern Club", None, 0, None));
    let Some(Found::List { sure, why, .. }) = on_list("Glass Lantern Club", &l, &None) else { panic!() };
    assert!(!sure);
    assert_eq!(why, "2 titles in your list match");
}

#[test]
fn picks_and_ignores_win() {
    let l = list();
    let pick = Some((3, "user".to_string(), String::new()));
    assert_eq!(on_list("Glass Lantern Club", &l, &pick), Some(Found::List { media_id: 3, sure: true, why: "you picked it".into() }));
    let ignore = Some((0, "ignore".to_string(), String::new()));
    assert_eq!(on_list("Glass Lantern Club", &l, &ignore), Some(Found::Ignored));
    // A pick that left the list is no answer.
    let gone = Some((77, "user".to_string(), String::new()));
    assert!(matches!(on_list("Glass Lantern Club", &l, &gone), Some(Found::List { media_id: 1, .. })));
}

#[test]
fn a_search_finds_a_manga_off_the_list() {
    let hit = |id: u32, title: &str| AnimeHit { kind: Kind::Manga, id, title: title.into(), ..Default::default() };
    let l = list();
    assert_eq!(from_search("Paper Swallows", &[hit(50, "Paper Swallows"), hit(51, "Other")], &l, "MyAnimeList"), Found::NotOnList { media_id: 50, title: "Paper Swallows".into() });
    assert!(matches!(from_search("Paper Swallows", &[hit(2, "Tsuki no Niwa")], &l, "MyAnimeList"), Found::List { media_id: 2, sure: false, .. }));
    assert_eq!(from_search("Paper Swallows", &[hit(51, "Completely Unrelated")], &l, "MyAnimeList"), Found::Nothing);
}

#[test]
fn offers_only_go_forward() {
    let r = read_of("Glass Lantern Club v35 c362.cbz");
    assert_eq!(offer(361, None, 34, None, &r), Some(Offer { chapter: Some(362), volume: None }), "volume 34 is done already");
    assert_eq!(offer(361, None, 30, None, &r), Some(Offer { chapter: Some(362), volume: Some(34) }), "inside volume 35: 34 is done");
    assert_eq!(offer(361, None, 30, Some(33), &r).and_then(|o| o.volume), None, "never past the volume count");
    assert_eq!(offer(362, None, 30, None, &r), Some(Offer { chapter: None, volume: Some(34) }), "the volume alone");
    assert_eq!(offer(361, None, 0, None, &read_of("Glass Lantern Club v01 c003.cbz")).and_then(|o| o.volume), None, "inside volume 1: none done");
    assert_eq!(offer(362, None, 34, None, &r), None, "already counted");
    assert_eq!(offer(400, None, 40, None, &r), None, "never lower");
    assert_eq!(offer(10, Some(300), 34, None, &r), None, "past the count");
    assert_eq!(offer(10, Some(0), 0, None, &r).and_then(|o| o.chapter), Some(362), "0 is an unknown count");
}

#[test]
fn a_volume_file_offers_the_volume() {
    let r = read_of("Tsuki no Niwa v04.cbz");
    assert_eq!(offer(20, Some(120), 3, Some(12), &r), Some(Offer { chapter: None, volume: Some(4) }));
    assert_eq!(offer(20, Some(120), 4, Some(12), &r), None);
    assert_eq!(offer(20, Some(120), 0, Some(3), &r), None, "past the volume count");
    assert_eq!(Offer { chapter: Some(3), volume: Some(1) }.label(), "Ch 3 and Vol 1");
}

#[test]
fn a_one_shot_is_chapter_one() {
    let r = read_of("Hoshi (Oneshot).cbz");
    assert_eq!(offer(0, Some(1), 0, None, &r), Some(Offer { chapter: Some(1), volume: None }));
    assert_eq!(offer(0, None, 0, None, &r), None, "no number and no known count: nothing to offer");
}

#[test]
fn skips_are_said() {
    assert_eq!(skipped(20, 21), None);
    assert_eq!(skipped(20, 23), Some((21, 22)));
}

#[test]
fn a_typed_path_is_a_library() {
    assert_eq!(library_at("/m/Comics").path, PathBuf::from("/m/Comics"));
    assert_eq!(library_at("/m/Comics/.yacreaderlibrary/library.ydb").path, PathBuf::from("/m/Comics"));
    assert_eq!(library_at("/m/Comics").name, "Comics");
    let v = libraries(&["/m/A".into(), "/m/B".into()], false);
    assert_eq!(v.len(), 2);
}

// ----------------------------------------------------------------------- the watcher, on fixtures

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("kot-reading-{name}-{}", std::process::id()));
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

const SCHEMA: &str = "
CREATE TABLE comic_info (id INTEGER PRIMARY KEY, numPages INTEGER, hash TEXT UNIQUE NOT NULL, read BOOLEAN DEFAULT 0,
  hasBeenOpened BOOLEAN DEFAULT 0, currentPage INTEGER DEFAULT 1, lastTimeOpened INTEGER, series TEXT);
CREATE TABLE comic (id INTEGER PRIMARY KEY, parentId INTEGER NOT NULL, comicInfoId INTEGER NOT NULL, fileName TEXT NOT NULL, path TEXT);
CREATE TABLE db_info (version TEXT NOT NULL);
INSERT INTO db_info (version) VALUES ('10.5.0');
";

fn db(lib: &Path) -> Connection {
    fs::create_dir_all(lib.join(".yacreaderlibrary")).unwrap();
    let c = Connection::open(lib.join(".yacreaderlibrary/library.ydb")).unwrap();
    c.execute_batch(SCHEMA).unwrap();
    c
}

fn put(c: &Connection, id: i64, path: &str, page: i64, pages: i64, last: Option<i64>) {
    c.execute(
        "INSERT OR REPLACE INTO comic_info (id, hash, numPages, currentPage, lastTimeOpened) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, format!("h{id}"), pages, page, last],
    )
    .unwrap();
    let file = path.rsplit('/').next().unwrap();
    c.execute("INSERT OR REPLACE INTO comic (id, parentId, comicInfoId, fileName, path) VALUES (?1, 1, ?1, ?2, ?3)", rusqlite::params![id, file, path]).unwrap();
}

/// Moves the file's mtime on, as a later write by YACReader would (file systems with a coarse
/// clock would see no change within one second).
fn touch_later(lib: &Path, secs: u64) {
    let f = fs::File::options().write(true).open(lib.join(".yacreaderlibrary/library.ydb")).unwrap();
    f.set_modified(SystemTime::now() + std::time::Duration::from_secs(secs)).unwrap();
}

#[test]
fn the_watcher_reports_the_newest_and_only_on_change() {
    let s = Scratch::new("watch");
    let lib = s.0.join("Comics");
    let c = db(&lib);
    let now = 1_800_000_000;
    put(&c, 1, "/Glass Lantern Club/c361.cbz", 40, 40, Some(now - 100));
    put(&c, 2, "/Glass Lantern Club/c362.cbz", 3, 40, Some(now - 10));
    put(&c, 3, "/Old/c001.cbz", 40, 40, Some(now - 2 * FIRST_LOOK_SECS));
    put(&c, 4, "/Never Opened/c001.cbz", 1, 40, None);
    let mut w = Watch::new(vec![library_at(&lib.to_string_lossy())], now);
    let mut errors = Vec::new();
    let r = w.poll(&mut errors).expect("the first look reads the database");
    assert_eq!((r.series.as_str(), r.chapter, r.page, r.finished), ("Glass Lantern Club", Some(362), 3, false));
    assert!(errors.is_empty());
    assert_eq!(w.poll(&mut errors), None, "unchanged: only a stat");
    // YACReader saves the last page when the comic is closed.
    put(&c, 2, "/Glass Lantern Club/c362.cbz", 40, 40, Some(now - 5));
    touch_later(&lib, 5);
    let r = w.poll(&mut errors).unwrap();
    assert!(r.finished);
    assert_eq!(w.libraries().count(), 1);
}

#[test]
fn a_missing_library_is_quiet_and_a_broken_one_is_said() {
    let s = Scratch::new("broken");
    let mut errors = Vec::new();
    let mut w = Watch::new(vec![library_at(&s.0.join("Unplugged").to_string_lossy())], 0);
    assert_eq!(w.poll(&mut errors), None);
    assert!(errors.is_empty(), "an unplugged drive is no error");
    let bad = s.0.join("Bad");
    fs::create_dir_all(bad.join(".yacreaderlibrary")).unwrap();
    fs::write(bad.join(".yacreaderlibrary/library.ydb"), b"not a database").unwrap();
    let mut w = Watch::new(vec![library_at(&bad.to_string_lossy())], 0);
    assert_eq!(w.poll(&mut errors), None);
    assert_eq!(errors.len(), 1);
}

#[test]
fn polling_writes_nothing() {
    let s = Scratch::new("ro");
    let lib = s.0.join("Comics");
    let c = db(&lib);
    put(&c, 1, "/Glass Lantern Club/c1.cbz", 2, 40, Some(1_800_000_000));
    drop(c);
    let file = lib.join(".yacreaderlibrary/library.ydb");
    let before = (fs::read(&file).unwrap(), fs::metadata(&file).unwrap().modified().unwrap());
    let mut w = Watch::new(vec![library_at(&lib.to_string_lossy())], 1_800_000_000);
    assert!(w.poll(&mut Vec::new()).is_some());
    let after = (fs::read(&file).unwrap(), fs::metadata(&file).unwrap().modified().unwrap());
    assert_eq!(before, after);
    let names: Vec<_> = fs::read_dir(lib.join(".yacreaderlibrary")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "no journal or -shm file: {names:?}");
}

/// Lightness: an idle poll (nothing changed) is one `stat` per library. Run with
/// `cargo test -p kotora-core idle_poll_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn idle_poll_cost() {
    let s = Scratch::new("cost");
    let lib = s.0.join("Comics");
    let c = db(&lib);
    put(&c, 1, "/Glass Lantern Club/c1.cbz", 2, 40, Some(1_800_000_000));
    let mut w = Watch::new(vec![library_at(&lib.to_string_lossy())], 1_800_000_000);
    let mut e = Vec::new();
    let t = std::time::Instant::now();
    assert!(w.poll(&mut e).is_some());
    let first = t.elapsed();
    let t = std::time::Instant::now();
    for _ in 0..1000 {
        assert!(w.poll(&mut e).is_none());
    }
    println!("first read {first:?}; idle poll {:?} each", t.elapsed() / 1000);
}

#[test]
fn a_digital_volume_offers_the_volume() {
    let r = read_of("Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz");
    assert_eq!((r.series.as_str(), r.volume, r.volume_file, r.chapter), ("Dragon Ball Z", Some(11), true, None));
    assert_eq!(offer(0, None, 10, Some(26), &r), Some(Offer { chapter: None, volume: Some(11) }));
    assert_eq!(on_list("Dragon Ball Z", &list(), &None), None, "not on the list: the caller searches (Add as Reading)");
}

#[test]
fn the_viewers_comic_by_id() {
    let s = Scratch::new("viewer");
    let lib = s.0.join("Movies");
    let c = db(&lib);
    put(&c, 1, "/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz", 1, 0, None);
    let l = lib.to_string_lossy();
    assert_eq!(viewer_comic(&l, 1), Some(lib.join("Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz")));
    assert_eq!(viewer_comic(&l, 2), None);
    assert_eq!(viewer_comic(&s.0.join("nowhere").to_string_lossy(), 1), None);
    // Fed to the open-file rules: shown at once, as a whole volume of Dragon Ball Z.
    let path = viewer_comic(&l, 1).unwrap().to_string_lossy().into_owned();
    let mut t = crate::manga::openfile::Tracker::new(10);
    let r = t.update(&[crate::manga::openfile::Open { app: "YACReader".into(), path }], 1_800_000_000).unwrap();
    assert_eq!((r.series.as_str(), r.volume, r.volume_file, r.finished, r.source.as_str()), ("Dragon Ball Z", Some(11), true, false, "YACReader"));
}

/// Live test: "Dragon Ball Z v11" was a sure match for "Dragon Ball" (MyAnimeList lists
/// "Dragon Ball Z" among Dragon Ball's other titles). The Z is part of the title.
fn dragon_ball() -> ListItem {
    let mut it = item(42, "Dragon Ball", None, 0, Some(520));
    it.media.synonyms = vec!["Dragon Ball Z".into(), "DB".into()];
    it
}

#[test]
fn dragon_ball_is_not_dragon_ball_z_or_super() {
    let db = dragon_ball();
    let dbs = item(43, "Dragon Ball Super", None, 0, None);
    // The same series: sure.
    assert!(matches!(on_list("Dragon Ball", std::slice::from_ref(&db), &None), Some(Found::List { media_id: 42, sure: true, .. })));
    // Z, Super, GT, Part 2: another series. Not even an unsure match: the search decides.
    for s in ["Dragon Ball Z", "Dragon Ball Super", "Dragon Ball GT", "Dragon Ball Part 2"] {
        assert_eq!(on_list(s, std::slice::from_ref(&db), &None), None, "{s}");
    }
    let both = vec![db.clone(), dbs.clone()];
    assert!(matches!(on_list("Dragon Ball Super", &both, &None), Some(Found::List { media_id: 43, sure: true, .. })));
    assert_eq!(on_list("Dragon Ball Z", &both, &None), None, "Z is neither");
    // Still a low suggestion for "Wrong manga…".
    let c = candidates("Dragon Ball Z", &both);
    assert!(c.iter().any(|c| c.media_id == 42 && c.score <= OTHER_SERIES), "{c:?}");
    // The shorter file name for a longer title still asks (a series by its start).
    assert!(matches!(on_list("Dragon Ball", std::slice::from_ref(&dbs), &None), Some(Found::List { media_id: 43, sure: false, .. })));
    // A typo is not another series.
    assert!(matches!(on_list("Glass Lanten Club", &list(), &None), Some(Found::List { media_id: 1, .. })));
}

#[test]
fn the_search_prefers_the_very_series_over_a_shorter_one_on_the_list() {
    let hit = |id: u32, title: &str, syn: &[&str]| AnimeHit { kind: Kind::Manga, id, title: title.into(), synonyms: syn.iter().map(|s| s.to_string()).collect(), ..Default::default() };
    let l = vec![dragon_ball()];
    // MyAnimeList's order: Dragon Ball (on the list, "Dragon Ball Z" as a synonym) first.
    let hits = [hit(42, "Dragon Ball", &["Dragon Ball Z"]), hit(9001, "Dragon Ball Z", &[]), hit(9002, "Dragon Ball Super", &[])];
    assert_eq!(from_search("Dragon Ball Z", &hits, &l, "MyAnimeList"), Found::NotOnList { media_id: 9001, title: "Dragon Ball Z".into() });
    // Only the shorter one found: nothing (no wrong answer).
    assert_eq!(from_search("Dragon Ball Z", &hits[..1], &l, "MyAnimeList"), Found::Nothing);
    assert!(matches!(from_search("Dragon Ball", &hits, &l, "MyAnimeList"), Found::List { media_id: 42, sure: false, .. }));
}
