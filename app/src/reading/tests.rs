//! Made-up manga and files only (the fake manga list, `manga::fake_items`).

use super::*;

fn state() -> State {
    let mut s = State::fake();
    crate::manga::fake(&mut s);
    s.now = None;
    s
}

fn entry(s: &State, id: u32) -> &crate::manga::MEntry {
    s.manga.entries.iter().find(|e| e.id == id).unwrap()
}

#[test]
fn a_chapter_is_offered_once_read_to_the_end() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 14);
    assert_eq!(s.reading_offer().map(|(_, o)| o), Some(Offer { chapter: Some(120), volume: None }));
    assert!(!s.reading_banner(), "page 14 of 40: not yet");
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 39);
    assert!(s.reading_banner(), "the page before the last counts");
    assert!(s.reading.current.as_ref().unwrap().acted, "asked once");
}

#[test]
fn mark_read_queues_the_chapter_and_undo_takes_it_back() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    let (title, o) = s.confirm_reading().unwrap();
    assert_eq!((title.as_str(), o.chapter), ("Inkwell Cartographers", Some(120)));
    assert_eq!(entry(&s, 1).chapters, 120);
    assert_eq!(entry(&s, 1).volumes, 11, "a chapter file never moves volumes");
    assert!(s.reading_banner(), "Undo stays");
    assert!(s.reading_offer().is_none());
    s.undo_reading();
    assert_eq!(entry(&s, 1).chapters, 119);
    assert!(!s.reading_banner());
}

#[test]
fn never_lower_and_never_the_same() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers c119.cbz", 40);
    assert!(s.reading_offer().is_none());
    fake(&mut s, "Inkwell Cartographers c050.cbz", 40);
    assert!(s.reading_offer().is_none());
    assert!(!s.reading_banner());
}

#[test]
fn auto_marks_a_sure_match_but_asks_for_a_skip() {
    let mut s = state();
    s.prefs.auto = true;
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    let m = s.reading.current.as_ref().unwrap().marked.unwrap();
    assert!(m.auto);
    assert_eq!(entry(&s, 1).chapters, 120);
    fake(&mut s, "Inkwell Cartographers - Ch. 123.cbz", 40);
    assert!(s.reading.current.as_ref().unwrap().marked.is_none());
    assert_eq!(s.reading_note().as_deref(), Some("Skips chapters 121\u{2013}122"));
    assert_eq!(entry(&s, 1).chapters, 120);
}

#[test]
fn an_unsure_match_never_marks_by_itself() {
    let mut s = state();
    s.prefs.auto = true;
    fake(&mut s, "Inkwell Cartographer c120.cbz", 40);
    let r = s.reading.current.as_ref().unwrap();
    assert!(!r.sure() && r.marked.is_none());
    assert!(s.reading_banner());
    s.pick_manga(1);
    assert!(s.reading.current.as_ref().unwrap().sure());
    assert_eq!(entry(&s, 1).chapters, 120, "the pick makes it sure: auto marks it");
}

#[test]
fn a_volume_file_marks_the_volume() {
    let mut s = state();
    fake(&mut s, "Night Shift Alchemist v04.cbz", 40);
    assert_eq!(s.reading_offer().map(|(_, o)| o), Some(Offer { chapter: None, volume: Some(4) }));
    s.confirm_reading().unwrap();
    assert_eq!((entry(&s, 6).volumes, entry(&s, 6).chapters), (4, 0));
}

#[test]
fn a_plus_one_by_hand_answers_the_offer() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    assert!(s.reading_banner());
    s.manga_plus_one(1).unwrap();
    assert!(s.reading_offer().is_none());
    assert!(!s.reading_banner());
}

#[test]
fn not_now_holds_until_another_file() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    assert!(s.dismiss_reading());
    assert!(!s.reading_banner());
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    assert!(!s.reading_banner(), "the same file, seen again");
    fake(&mut s, "Inkwell Cartographers v12 c121.cbz", 40);
    assert!(s.reading_banner());
}

#[test]
fn a_plan_to_read_manga_becomes_reading() {
    let mut s = state();
    fake(&mut s, "Harbor of Small Lights c001.cbz", 40);
    s.confirm_reading().unwrap();
    assert_eq!((entry(&s, 8).chapters, entry(&s, 8).status), (1, Status::Watching));
}

#[test]
fn not_a_manga_is_quiet() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    s.ignore_reading();
    assert!(!s.reading_banner());
    assert_eq!(s.reading.current.as_ref().unwrap().found, Found::Ignored);
}

#[test]
fn kot_now_says_it() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 14);
    let r = s.ctl_reading().unwrap();
    assert_eq!((r.title.as_str(), r.label.as_str(), r.page, r.pages, r.finished), ("Inkwell Cartographers", "Ch 120 (Vol 12)", Some(14), Some(40), false));
    assert_eq!(r.offer.as_deref(), Some("Ch 120"));
    assert!(r.matched.starts_with("In your list (Reading), sure"), "{}", r.matched);
}

#[test]
fn nothing_runs_headless_or_with_manga_off() {
    let mut s = state();
    sync(&mut s);
    assert!(!s.reading.running(), "headless: no thread");
    s.manga.enabled = false;
    sync(&mut s);
    assert!(!s.reading.running());
}

#[test]
fn a_library_needs_its_database() {
    let mut s = state();
    let dir = std::env::temp_dir().join(format!("kot-reading-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.to_string_lossy().into_owned();
    assert!(add_library(&mut s, &p).unwrap_err().starts_with("No YACReader library at"));
    assert!(add_library(&mut s, "").is_err());
    std::fs::create_dir_all(dir.join(".yacreaderlibrary")).unwrap();
    std::fs::write(dir.join(".yacreaderlibrary/library.ydb"), b"").unwrap();
    add_library(&mut s, &p).unwrap();
    assert_eq!(s.reading.added, vec![p.clone()]);
    assert_eq!(add_library(&mut s, &p).unwrap_err(), "That library is already watched.");
    remove_library(&mut s, &p);
    assert!(s.reading.added.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
