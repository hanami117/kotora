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
    assert_eq!(entry(&s, 1).volumes, 11, "inside volume 12: 11 was done already");
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

#[test]
fn an_open_file_always_asks() {
    let mut s = state();
    s.prefs.auto = true;
    fake_open(&mut s, "Simple Comic", "/srv/manga/Inkwell Cartographers/c120.cbz", Some("Ch 121 opened next"));
    let r = s.reading.current.as_ref().unwrap();
    assert!(r.read.weak && r.sure() && r.marked.is_none(), "a weak signal never marks by itself");
    assert!(s.reading_banner());
    assert_eq!(how(&r.read), "Ch 121 opened next in Simple Comic");
    assert_eq!(s.ctl_reading().unwrap().reader, "Simple Comic");
    s.confirm_reading().unwrap();
    assert_eq!(entry(&s, 1).chapters, 120);
}

#[test]
fn an_open_file_not_counted_yet_only_shows() {
    let mut s = state();
    fake_open(&mut s, "Okular", "/srv/manga/Inkwell Cartographers/c120.cbz", None);
    assert!(!s.reading_banner());
    assert!(s.reading_offer().is_some());
}

#[test]
fn a_chapter_inside_a_volume_marks_the_volume_before() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v14 c120.cbz", 40);
    assert_eq!(s.reading_offer().map(|(_, o)| o), Some(Offer { chapter: Some(120), volume: Some(13) }));
    s.confirm_reading().unwrap();
    assert_eq!((entry(&s, 1).chapters, entry(&s, 1).volumes), (120, 13));
}

#[test]
fn the_manga_notification_switches_are_their_own() {
    let mut s = state();
    s.set_notify(crate::notes::Kind::ReadOffer, false);
    assert!(!s.prefs.notify.read_offer && s.prefs.notify.offer);
    s.set_notify(crate::notes::Kind::Offer, false);
    s.set_notify(crate::notes::Kind::ReadRecorded, true);
    assert!(s.prefs.notify.read_recorded && !s.prefs.notify.offer);
}

#[test]
fn open_files_and_settings_start_nothing_headless() {
    let mut s = state();
    s.reading.on = false;
    s.set_open_files(true);
    assert!(!s.reading.running());
    s.step_open_minutes(true);
    assert_eq!(s.reading.minutes, 15);
    s.step_open_minutes(false);
    s.step_open_minutes(false);
    assert_eq!(s.reading.minutes, 9);
    s.set_reading_find(false);
    assert!(!s.reading.find && !s.reading.running());
}

#[test]
fn a_comic_folder_must_be_there() {
    let mut s = state();
    assert!(add_comic_folder(&mut s, "relative/path").is_err());
    let dir = std::env::temp_dir();
    let p = crate::library::tidy(&dir.to_string_lossy());
    add_comic_folder(&mut s, &p).unwrap();
    assert_eq!(add_comic_folder(&mut s, &p).unwrap_err(), "That folder is already in the list.");
    remove_comic_folder(&mut s, &p);
    assert!(s.reading.folders.is_empty());
}

/// Live test: closing the comic must end "Now Reading" (and Discord) at once.
#[test]
fn closing_the_comic_clears_now_reading() {
    let mut s = state();
    fake_open(&mut s, "YACReader", "/media/comics/Dragon Ball Z v11 (2003) (Digital) (LuCaZ).cbz", None);
    assert!(s.now_reading().is_some());
    // The viewer quit (its process and command line are gone).
    s.reading_open(Vec::new());
    assert!(s.now_reading().is_none() && s.reading.current.is_none(), "nothing to ask: gone");
    // YACReader's write on close of a half-read comic comes after: it doesn't bring it back.
    fake_closed(&mut s, "Inkwell Cartographers v12 c120.cbz", 14);
    assert!(s.reading.current.is_none());
}

#[test]
fn a_closed_comic_that_asks_stays_in_the_banner_only() {
    let mut s = state();
    fake_open(&mut s, "YACReader", "/comics/Inkwell Cartographers v12 c120.cbz", None);
    s.reading_open(Vec::new());
    // Read to the end, saved on close: the question stays, Now Reading doesn't.
    fake_closed(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    assert!(s.reading_banner());
    assert!(s.now_reading().is_none());
    // Marked: Undo stays while closed.
    s.confirm_reading().unwrap();
    s.reading_open(Vec::new());
    assert!(s.reading_banner() && s.reading.current.is_some());
}

#[test]
fn opening_again_makes_it_now_reading_again() {
    let mut s = state();
    fake_open(&mut s, "Okular", "/srv/manga/Inkwell Cartographers/c120.cbz", None);
    let p = s.reading.current.as_ref().unwrap().read.path.clone();
    s.reading_open(vec![p.clone()]);
    assert!(s.now_reading().is_some());
    s.reading_open(vec![std::path::PathBuf::from("/srv/manga/Other/c001.cbz")]);
    assert!(s.now_reading().is_none(), "another file open: this one closed");
}

#[test]
fn continue_reading_shows_where_it_stopped_and_how_it_opens() {
    let mut s = state();
    assert!(s.resume_of(1).is_none());
    s.want_resume(1);
    assert!(s.reading.resume_asked.is_empty(), "no thread headless: nothing asked");
    fake_resume(&mut s, 1, 87, false);
    let r = s.resume_of(1).unwrap().clone();
    assert_eq!(r.label(), "Vol 11 \u{b7} p. 87/190");
    // As YACReaderLibrary opens a comic: the library, the comic id, the library id.
    assert_eq!(launch_args(&r), ["/media/comics", "--comicId=7", "--libraryId=1"]);
    let no_id = kotora_core::manga::resume::Resume { library_id: None, ..r };
    assert_eq!(launch_args(&no_id), ["/media/comics/Inkwell Cartographers v11.cbz"], "the file alone");
    // Unknown again: hidden.
    s.apply_reading(ReadingMsg::Resume { media_id: 1, at: None });
    assert!(s.resume_of(1).is_none() && resume_line(&s, 1).is_none());
}

#[test]
fn a_closed_comic_leaves_continue_reading_in_now_reading() {
    let mut s = state();
    fake(&mut s, "Inkwell Cartographers v12 c120.cbz", 14);
    fake_resume(&mut s, 1, 87, false);
    s.reading_open(Vec::new());
    assert!(s.reading.current.is_none());
    assert_eq!(s.reading.last_media, Some(1));
    assert!(resume_line(&s, 1).is_some());
}

/// Live test: VIZ's "Dragon Ball Z v11" is Dragon Ball volume 27. Kotora asks "Is this Dragon
/// Ball?" and never marks it by itself.
#[test]
fn viz_dragon_ball_z_asks_and_offers_the_right_volume() {
    let mut s = state();
    s.prefs.auto = true;
    fake_dbz(&mut s);
    let r = s.reading.current.as_ref().unwrap();
    assert!(matches!(r.found, Found::List { media_id: 42, sure: false, .. }));
    assert_eq!((r.read.volume, r.read.renumbered.as_deref()), (Some(27), Some("Dragon Ball Z v11 = Dragon Ball vol 27")));
    assert_eq!(s.reading_offer().map(|(_, o)| o), Some(Offer { chapter: None, volume: Some(27) }));
    // Read to the end, auto on: still asks.
    let mut done = r.read.clone();
    done.finished = true;
    let (found, candidates) = (r.found.clone(), r.candidates.clone());
    s.reading_seen(Seen { read: done, found, candidates, cover_url: None });
    assert!(s.reading.current.as_ref().unwrap().marked.is_none() && s.reading_banner());
    // "Yes, that one": remembered, sure.
    s.pick_manga(42);
    assert!(s.reading.current.as_ref().unwrap().sure());
}
