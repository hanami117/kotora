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

/// Live test (Arch): closed, then "Not now": Now Reading and kot now kept the closed comic, and
/// a comic opened in Okular next never showed.
#[test]
fn a_closed_comic_answered_not_now_is_gone_and_the_next_one_shows() {
    let mut s = state();
    fake_open(&mut s, "YACReader", "/comics/Inkwell Cartographers v12 c120.cbz", None);
    s.reading_open(Vec::new());
    fake_closed(&mut s, "Inkwell Cartographers v12 c120.cbz", 40);
    assert!(s.reading_banner() && s.now_reading().is_none(), "closed: the question in the banner only");
    assert!(!s.ctl_reading().unwrap().open);
    s.dismiss_reading();
    assert!(s.reading.current.is_none() && s.ctl_reading().is_none());
    fake_open(&mut s, "Okular", "/srv/manga/Paper Wolves of Osaka/c013.cbz", None);
    assert_eq!(s.now_reading().map(|r| r.read.source.as_str()), Some("Okular"));
}

/// Live test: v01 left after 6 s with YACReader's read flag still set from before: not read.
#[test]
fn a_stale_read_flag_is_not_read_to_the_end() {
    let mut s = state();
    let o = kotora_core::manga::yacreader::Opened {
        id: 1,
        rel_path: "/Inkwell Cartographers v12 c120.cbz".into(),
        path: "/comics/Inkwell Cartographers v12 c120.cbz".into(),
        parsed: kotora_core::manga::name::parse_path("/Inkwell Cartographers v12 c120.cbz"),
        meta_series: None,
        current_page: 3,
        num_pages: 40,
        read_flag: true,
        last_opened: 0,
    };
    let read = Read::from_opened(&o);
    assert!(!read.finished);
    s.reading.open_now.push(read.path.clone());
    s.reading_seen(Seen { read, found: Found::List { media_id: 1, sure: true, why: String::new() }, candidates: Vec::new(), cover_url: None });
    assert!(!s.reading_banner());
}

/// Live test (Arch): v01 opened from the library, then v02 inside the viewer: its command line
/// still says v01; the log's newer line wins.
#[test]
fn the_viewer_log_beats_a_stale_comic_id() {
    use kotora_core::manga::openfile::Open;
    let yes = |_: &str| true;
    let v01 = Open { app: "YACReader".into(), path: "/m/All You Need Is Kill v01.cbz".into() };
    let mut open = vec![v01.clone(), Open { app: "Okular".into(), path: "/m/x c1.cbz".into() }];
    let used = viewer_from_log(&mut open, Some("/m/All You Need Is Kill v02.cbz".into()), Some("/m/old.cbz".into()), false, yes);
    assert_eq!(used.as_deref(), Some("/m/All You Need Is Kill v02.cbz"));
    assert_eq!(open.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(), ["/m/x c1.cbz", "/m/All You Need Is Kill v02.cbz"]);
    // No fresh line, a viewer from the library: the command line stands (old lines don't count).
    let mut open = vec![v01.clone()];
    assert_eq!(viewer_from_log(&mut open, None, Some("/m/old.cbz".into()), false, yes), None);
    assert_eq!(open, std::slice::from_ref(&v01));
    // Opened from the Finder (no comic in argv): the log's last line, also from before.
    let mut open = Vec::new();
    assert_eq!(viewer_from_log(&mut open, None, Some("/m/old.cbz".into()), true, yes).as_deref(), Some("/m/old.cbz"));
    // A file that is gone: nothing.
    let mut open = vec![v01];
    assert_eq!(viewer_from_log(&mut open, Some("/m/gone.cbz".into()), None, false, |_| false), None);
}

/// Live test: the match was logged twice per open (the open-file signal, then the database's
/// page). Once, and again only when something changed.
#[test]
fn the_match_is_logged_once_per_open() {
    let k = |page: Option<(i64, i64)>, finished: bool, found: &str| LogKey { path: "/m/Dragon Ball Z v12.cbz".into(), found: found.into(), finished, page };
    let first = k(None, false, "list entry 42 (sure: you picked it)");
    assert!(log_due(None, &first));
    assert!(!log_due(Some(&first), &k(Some((1, 187)), false, "list entry 42 (sure: you picked it)")), "the page arriving is no change");
    assert!(!log_due(Some(&k(Some((1, 187)), false, "x")), &k(None, false, "x")));
    assert!(log_due(Some(&k(Some((1, 187)), false, "x")), &k(Some((90, 187)), false, "x")), "the page moved");
    assert!(log_due(Some(&first), &k(None, true, "list entry 42 (sure: you picked it)")), "read to the end");
    assert!(log_due(Some(&first), &k(None, false, "not on the list: Dragon Ball (42)")), "another match");
    let other = LogKey { path: "/m/Dragon Ball Z v13.cbz".into(), ..first.clone() };
    assert!(log_due(Some(&first), &other), "another file");
}

// ------------------------------------------------------------------------------------ rereads

#[test]
fn a_completed_manga_read_again_asks_and_never_starts_by_itself() {
    let mut s = state();
    s.prefs.auto = true;
    fake(&mut s, "Cobalt Postman v04 c031.cbz", 14);
    assert!(s.reading_offer().is_none(), "everything is read already");
    assert_eq!(s.reread_offer().map(|(_, o)| o), Some(Offer { chapter: Some(31), volume: Some(3) }));
    assert!(!s.reading_banner(), "page 14 of 40: not yet");
    fake(&mut s, "Cobalt Postman v04 c031.cbz", 40);
    assert!(s.reading_banner() && s.reading.current.as_ref().unwrap().acted, "asked once");
    let e = entry(&s, 9);
    assert_eq!((e.status, e.chapters, e.volumes, e.rereading), (Status::Completed, 96, 10, false), "asking changes nothing, even with auto on");
    assert!(s.reading.current.as_ref().unwrap().marked.is_none());
    let r = s.ctl_reading().unwrap();
    assert_eq!((r.offer.as_deref(), r.reread), (Some("Ch 31 and Vol 3"), true));
}

#[test]
fn yes_starts_the_reread_there_and_undo_puts_it_back() {
    let mut s = state();
    fake(&mut s, "Cobalt Postman v04 c031.cbz", 40);
    let (title, o) = s.confirm_reading().unwrap();
    assert_eq!((title.as_str(), o.chapter), ("Cobalt Postman", Some(31)));
    let e = entry(&s, 9);
    assert_eq!((e.status, e.chapters, e.volumes, e.rereading, e.times_reread), (Status::Completed, 31, 3, true, 0));
    assert!(s.reading_banner(), "Undo stays");
    assert!(s.reread_offer().is_none(), "being reread now");
    s.undo_reading();
    let e = entry(&s, 9);
    assert_eq!((e.status, e.chapters, e.volumes, e.rereading, e.times_reread), (Status::Completed, 96, 10, false, 0));
    assert!(!s.reading_banner());
}

#[test]
fn during_the_reread_chapters_count_and_the_last_ends_it() {
    let mut s = state();
    fake(&mut s, "Cobalt Postman c001.cbz", 40);
    s.confirm_reading().unwrap();
    assert_eq!((entry(&s, 9).chapters, entry(&s, 9).volumes, entry(&s, 9).rereading), (1, 0, true));
    s.dismiss_reading();
    // +1 works, and a chapter read goes through the normal offer.
    s.manga_plus_one(9).unwrap();
    assert_eq!(entry(&s, 9).chapters, 2);
    fake(&mut s, "Cobalt Postman c003.cbz", 40);
    assert_eq!(s.reading_offer().map(|(_, o)| o.chapter), Some(Some(3)));
    s.confirm_reading().unwrap();
    assert_eq!(entry(&s, 9).chapters, 3);
    // The last chapter: the reread ends, one more in the count, still Completed.
    fake(&mut s, "Cobalt Postman c096.cbz", 40);
    s.confirm_reading().unwrap();
    let e = entry(&s, 9);
    assert_eq!((e.status, e.chapters, e.rereading, e.times_reread), (Status::Completed, 96, false, 1));
    // Undo of that last chapter puts the reread back, uncounted.
    s.undo_reading();
    let e = entry(&s, 9);
    assert_eq!((e.chapters, e.rereading, e.times_reread), (3, true, 0));
}

#[test]
fn a_reread_of_one_chapter_starts_and_ends_at_once() {
    let mut s = state();
    fake(&mut s, "Kumo Atelier.cbz", 40);
    assert!(s.reread_offer().is_none(), "reading, not completed");
    s.manga_edit(7, Change { status: Some(Status::Completed), ..Default::default() }).unwrap();
    s.reading.current = None;
    fake(&mut s, "Kumo Atelier.cbz", 40);
    assert_eq!(s.reread_offer().map(|(_, o)| o.chapter), Some(Some(1)), "a one-shot is chapter 1");
    s.confirm_reading().unwrap();
    let e = entry(&s, 7);
    assert_eq!((e.status, e.rereading, e.times_reread), (Status::Completed, false, 1), "the last chapter ends it, counted");
}

#[test]
fn not_now_to_a_reread_holds_and_a_manga_not_completed_is_never_asked() {
    let mut s = state();
    fake(&mut s, "Cobalt Postman c031.cbz", 40);
    assert!(s.dismiss_reading());
    assert!(!s.reading_banner());
    assert_eq!((entry(&s, 9).chapters, entry(&s, 9).rereading), (96, false));
    // Reading, on hold, dropped: no reread.
    fake(&mut s, "Ferris Wheel Diaries c010.cbz", 40);
    assert!(s.reread_offer().is_none());
    fake(&mut s, "Rust Belt Saints c010.cbz", 40);
    assert!(s.reread_offer().is_none());
    // A chapter past the known count is no reread.
    fake(&mut s, "Cobalt Postman c200.cbz", 40);
    assert!(s.reread_offer().is_none() && !s.reading_banner());
}
