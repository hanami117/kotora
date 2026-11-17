//! Made-up files only (`fixtures/animelist.xml`): no real list, no service.

use std::io::Write as _;

use super::*;
use crate::score::ten;
use crate::queue::{self, Outcome, Step};
use crate::service::{Details, ListService, MyEntry, ServiceError};
use crate::{Remote, Scope};

const FILE: &str = include_str!("fixtures/animelist.xml");

fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

fn media(id: u32, title: &str, episodes: Option<u32>) -> Media {
    Media { id, title: title.into(), episodes, format: "tv".into(), air_status: "finished_airing".into(), ..Media::default() }
}

fn item(m: Media, v: Values) -> ListItem {
    ListItem { media: m, status: v.status, progress: v.progress, score: v.score, rewatching: v.rewatching, times_rewatched: v.times_rewatched, volumes: 0, updated_at: 100 }
}

fn vals(status: Status, progress: u32, score: Option<Score>) -> Values {
    Values::new(status, progress, score)
}

fn by_id(file: &XmlList, id: u32) -> &XmlEntry {
    file.entries.iter().find(|e| e.mal_id == id).unwrap()
}

#[test]
fn reads_a_mal_export_plain_and_gzipped() {
    let plain = parse(FILE.as_bytes()).unwrap();
    let packed = parse(&gz(FILE.as_bytes())).unwrap();
    assert_eq!(plain, packed);
    assert_eq!(plain.user_name.as_deref(), Some("demo_user"));
    assert_eq!(plain.entries.len(), 9);
    let h = by_id(&plain, 1001);
    assert_eq!((h.title.as_str(), h.series_type.as_str(), h.episodes, h.watched, h.score), ("Harbor Lights", "TV", Some(12), 5, ten(7)));
    assert_eq!((h.start_date.as_deref(), h.finish_date.as_deref()), (Some("2026-10-02"), None), "0000-00-00 is no date");
    assert_eq!(h.status, Some(Status::Watching));
}

#[test]
fn edge_cases_read_safely() {
    let f = parse(FILE.as_bytes()).unwrap();
    // Score 0 is "not scored"; 11 is out of range.
    assert_eq!(by_id(&f, 1002).score, None);
    assert_eq!(by_id(&f, 1007).score, None);
    // Old numeric statuses; an unknown word is kept as written.
    assert_eq!(by_id(&f, 1007).status, Some(Status::Dropped));
    assert_eq!((by_id(&f, 1006).status, by_id(&f, 1006).raw_status.as_str()), (None, "Paused for now"));
    // A missing id is 0; CDATA with markup characters comes through.
    assert!(f.entries.iter().any(|e| e.mal_id == 0 && e.title == "No Id Given"));
    assert_eq!(by_id(&f, 1007).title, "Cats & Dogs <Special>");
    // update_on_import: 0 only where written; missing means yes.
    assert!(!by_id(&f, 1005).update_on_import);
    assert!(by_id(&f, 1007).update_on_import);
    // Progress past the episode count is cut to it.
    assert_eq!(by_id(&f, 1007).values().unwrap().progress, 2);
    // Unknown episode count (0): none.
    assert_eq!(by_id(&f, 1005).episodes, None);
}

#[test]
fn a_rewatch_reads_as_completed_with_the_rewatch_episode() {
    let f = parse(FILE.as_bytes()).unwrap();
    let v = by_id(&f, 1003).values().unwrap();
    assert_eq!(v, Values { status: Status::Completed, progress: 6, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 0 });
}

#[test]
fn statuses_in_every_spelling() {
    for (w, st) in [("Plan to Watch", Status::PlanToWatch), ("plan_to_watch", Status::PlanToWatch), ("On-Hold", Status::OnHold), ("on hold", Status::OnHold), ("6", Status::PlanToWatch), ("3", Status::OnHold), (" COMPLETED ", Status::Completed)] {
        assert_eq!(status_from_xml(w), Some(st), "{w}");
    }
    assert_eq!(status_from_xml("5"), None);
    for st in Status::ALL {
        assert_eq!(status_from_xml(status_to_xml(st)), Some(st));
    }
}

#[test]
fn not_a_list_export_says_so() {
    assert!(parse(b"<html><body>hi</body></html>").unwrap_err().contains("not a MyAnimeList list export"));
    assert!(parse(b"not xml at all").unwrap_err().contains("not valid XML"));
    assert!(parse(&[0x1f, 0x8b, 0, 1, 2]).unwrap_err().contains("gzipped"));
    let manga = "<myanimelist><myinfo><user_export_type>2</user_export_type></myinfo><manga><manga_mangadb_id>3</manga_mangadb_id></manga></myanimelist>";
    assert!(parse(manga.as_bytes()).unwrap_err().contains("manga list"));
}

/// A MAL store with three anime; one has saved details with the list dates.
fn mal_store() -> Store {
    let mut s = Store::open_memory().unwrap();
    s.set_account("mal", "demo_user").unwrap();
    let a = item(media(1001, "Harbor Lights", Some(12)), vals(Status::Watching, 5, ten(7)));
    let mut b = item(media(1003, "Starfall Academy", Some(24)), Values { rewatching: true, times_rewatched: 1, ..vals(Status::Completed, 6, ten(9)) });
    b.media.format = "ona".into();
    let c = item(media(1008, "Tea & <Tide> ]]> Finale", None), vals(Status::PlanToWatch, 0, None));
    s.replace_list("mal", &[a, b, c], 100).unwrap();
    let mine = MyEntry { status: "completed".into(), progress: 6, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 0, start_date: Some("2025-01".into()), finish_date: Some("2025-03-30".into()) };
    s.save_details("mal", &Details { id: 1003, title: "Starfall Academy".into(), format: "ona".into(), mine: Some(mine), ..Details::default() }, 100).unwrap();
    s
}

#[test]
fn export_writes_the_list_and_reads_back_the_same() {
    let s = mal_store();
    let ex = export(&s, ServiceId::Mal).unwrap();
    assert_eq!((ex.written, ex.skipped.len()), (3, 0));
    let x = &ex.xml;
    for want in [
        "<user_name>demo_user</user_name>",
        "<user_total_anime>3</user_total_anime>",
        "<user_total_plantowatch>1</user_total_plantowatch>",
        "<series_type>ONA</series_type>",
        "<my_start_date>2025-01-00</my_start_date>",
        "<my_finish_date>2025-03-30</my_finish_date>",
        "<my_rewatching>1</my_rewatching>",
        "<my_rewatching_ep>6</my_rewatching_ep>",
        "<my_status>Plan to Watch</my_status>",
        "<update_on_import>1</update_on_import>",
    ] {
        assert!(x.contains(want), "missing {want}");
    }
    // No dates saved: MAL's empty date. Nothing from the catalogue but the format's fields.
    assert!(x.contains("<my_start_date>0000-00-00</my_start_date>"));
    for not in ["cover", "synopsis", "genres", "anilist"] {
        assert!(!x.to_lowercase().contains(not), "{not}");
    }
    let back = parse(x.as_bytes()).unwrap();
    assert_eq!(back.entries.len(), 3);
    assert_eq!(by_id(&back, 1008).title, "Tea & <Tide> ]]> Finale", "CDATA split around ]]>");
    // Each entry's values come back as they were.
    for it in s.load_list("mal").unwrap() {
        assert_eq!(by_id(&back, it.media.id).values(), Some(it.values()), "{}", it.media.title);
    }
    // And an import of it into the same list finds nothing to do.
    let p = plan(&back, "mal", &s.load_list("mal").unwrap(), &HashMap::new());
    assert_eq!(p.counts(), Counts { same: 3, ..Counts::default() });
}

#[test]
fn an_anilist_list_is_never_exported() {
    let mut s = Store::open_memory().unwrap();
    let m = Media { id_mal: Some(1001), ..media(171001, "Harbor Lights", Some(12)) };
    s.replace_list("anilist", &[item(m, vals(Status::Watching, 5, None))], 100).unwrap();
    let err = export(&s, ServiceId::AniList).unwrap_err();
    assert!(err.contains("AniList") && err.contains("terms"), "{err}");
    assert!(export_refusal(ServiceId::Mal).is_none());
}

#[test]
fn file_names_carry_the_date() {
    assert_eq!(default_file_name(1_763_251_200), "animelist-2025-11-16.xml");
}

/// The MAL list the import tests compare with:
/// 1001 Watching 3 (file: 5, score 7) → different, the higher progress wins;
/// 1004 Watching 2 (file: On hold 4) → a status conflict;
/// 1002 Completed 1, no score (file: same) → same;
/// 1005 Watching 1 (file: Plan to watch, update_on_import 0) → different, but not updated;
/// 1007 Completed 2 score 8 (file: Dropped) → conflict.
fn import_list() -> Vec<ListItem> {
    vec![
        item(media(1001, "Harbor Lights", Some(12)), vals(Status::Watching, 3, None)),
        item(media(1004, "Quiet Garden", Some(13)), vals(Status::Watching, 2, ten(8))),
        item(media(1002, "Paper Moon Express", Some(1)), vals(Status::Completed, 1, None)),
        item(media(1005, "Ember Tide", None), vals(Status::Watching, 1, None)),
        item(media(1007, "Cats and Dogs", Some(2)), vals(Status::Completed, 2, ten(8))),
    ]
}

fn row(p: &Plan, mal_id: u32) -> &PlanRow {
    p.rows.iter().find(|r| r.entry.mal_id == mal_id && !matches!(r.verdict, Verdict::Unknown(ref w) if w.contains("Twice"))).unwrap()
}

#[test]
fn the_preview_sorts_entries_into_new_different_same_unknown() {
    let f = parse(FILE.as_bytes()).unwrap();
    let p = plan(&f, "mal", &import_list(), &HashMap::new());
    assert_eq!(row(&p, 1003).verdict, Verdict::New);
    assert_eq!(row(&p, 1001).verdict, Verdict::Different);
    assert_eq!(row(&p, 1002).verdict, Verdict::Same);
    assert_eq!(row(&p, 1004).verdict, Verdict::Different);
    assert!(matches!(&row(&p, 1006).verdict, Verdict::Unknown(w) if w.contains("Paused for now")));
    let unknown: Vec<String> = p.rows.iter().filter_map(|r| if let Verdict::Unknown(w) = &r.verdict { Some(w.clone()) } else { None }).collect();
    assert!(unknown.iter().any(|w| w == "No MyAnimeList id"));
    assert!(unknown.iter().any(|w| w == "Twice in the file"));
    let c = p.counts();
    assert_eq!((c.new, c.different, c.same, c.unknown, c.conflicts), (1, 4, 1, 3, 2));
    // 1005 asked not to be updated: different, nothing to send.
    assert_eq!(c.to_update, 3);
    assert!(row(&p, 1005).update.is_empty() && row(&p, 1005).conflict.is_none());
}

#[test]
fn merge_rules_higher_progress_empty_scores_and_status_conflicts() {
    let f = parse(FILE.as_bytes()).unwrap();
    let p = plan(&f, "mal", &import_list(), &HashMap::new());
    // Same status: the higher progress, and a score where there was none.
    assert_eq!(row(&p, 1001).update, Change { progress: Some(5), score: Some(ten(7)), ..Change::default() });
    // Watching vs On hold: asked. Keeping mine keeps the score too; the file's status brings
    // its progress.
    let r = row(&p, 1004);
    assert_eq!(r.conflict, Some(StatusConflict { mine: Status::Watching, file: Status::OnHold }));
    assert!(r.update.is_empty());
    assert_eq!(r.with_file_status, Change { status: Some(Status::OnHold), progress: Some(4), ..Change::default() });
    // Completed vs Dropped: asked; the existing score stays either way.
    let r = row(&p, 1007);
    assert_eq!(r.conflict, Some(StatusConflict { mine: Status::Completed, file: Status::Dropped }));
    assert_eq!(r.with_file_status.score, None);
}

#[test]
fn the_status_further_along_wins_without_asking() {
    let file = XmlList { entries: vec![XmlEntry { mal_id: 9, status: Some(Status::Completed), raw_status: "Completed".into(), watched: 3, ..blank() }], ..XmlList::default() };
    // Plan to watch → Completed: the file's, at the last episode.
    let p = plan(&file, "mal", &[item(media(9, "Nine", Some(12)), vals(Status::PlanToWatch, 0, None))], &HashMap::new());
    assert_eq!((p.rows[0].conflict, p.rows[0].update.status, p.rows[0].update.progress), (None, Some(Status::Completed), Some(12)));
    // Completed on the list, Watching in the file: the list stays.
    let file = XmlList { entries: vec![XmlEntry { status: Some(Status::Watching), raw_status: "Watching".into(), ..file.entries[0].clone() }], ..XmlList::default() };
    let p = plan(&file, "mal", &[item(media(9, "Nine", Some(12)), vals(Status::Completed, 12, None))], &HashMap::new());
    assert!(p.rows[0].update.is_empty() && p.rows[0].conflict.is_none());
    assert_eq!(p.counts().to_update, 0);
}

fn blank() -> XmlEntry {
    XmlEntry {
        mal_id: 0,
        title: String::new(),
        series_type: String::new(),
        episodes: None,
        raw_status: String::new(),
        status: None,
        watched: 0,
        score: None,
        start_date: None,
        finish_date: None,
        times_watched: 0,
        rewatching: false,
        rewatching_ep: 0,
        update_on_import: true,
    }
}

#[test]
fn anilist_matches_by_idmal_and_unknown_ids_are_listed() {
    let f = parse(FILE.as_bytes()).unwrap();
    // On the AniList list: 1001 as AniList 171001.
    let list = vec![item(Media { id_mal: Some(1001), ..media(171001, "Harbor Lights", Some(12)) }, vals(Status::Watching, 3, None))];
    let ids = Plan::ids_to_map(&f, &list, "anilist");
    assert_eq!(ids, vec![1002, 1003, 1004, 1005, 1007], "not 1001 (on the list), not the unknown ones, no repeats");
    let mut mapped = HashMap::new();
    mapped.insert(1003, Media { id_mal: Some(1003), ..media(173003, "Hoshifuri Gakuen", Some(24)) });
    let p = plan(&f, "anilist", &list, &mapped);
    assert_eq!((row(&p, 1001).verdict.clone(), row(&p, 1001).target), (Verdict::Different, Some(171001)));
    assert_eq!((row(&p, 1003).verdict.clone(), row(&p, 1003).target), (Verdict::New, Some(173003)));
    assert_eq!(row(&p, 1003).title(), "Hoshifuri Gakuen", "AniList's title for AniList media");
    assert_eq!(row(&p, 1002).verdict, Verdict::Unknown("Not on AniList".into()));
}

/// A made-up service that holds whatever it is sent.
#[derive(Default)]
struct Fake {
    remote: HashMap<u32, Remote>,
    clock: i64,
}

impl ListService for Fake {
    fn name(&self) -> &'static str {
        "mal"
    }
    fn title(&self) -> &'static str {
        "MyAnimeList"
    }
    fn user_name(&mut self) -> Result<String, ServiceError> {
        Ok("demo_user".into())
    }
    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        Ok(vec![])
    }
    fn get_entry(&mut self, id: u32) -> Result<Option<Remote>, ServiceError> {
        Ok(self.remote.get(&id).copied())
    }
    fn update_entry(&mut self, id: u32, change: &Change) -> Result<Remote, ServiceError> {
        self.clock += 1;
        let old = self.remote.get(&id).map_or(Values::new(Status::PlanToWatch, 0, None), |r| r.values);
        let r = Remote { values: change.apply(old), updated_at: self.clock };
        self.remote.insert(id, r);
        Ok(r)
    }
}

fn import_store() -> (Store, Fake) {
    let mut s = Store::open_memory().unwrap();
    let list = import_list();
    s.replace_list("mal", &list, 100).unwrap();
    let mut f = Fake { clock: 1000, ..Fake::default() };
    for it in &list {
        f.remote.insert(it.media.id, Remote { values: it.values(), updated_at: 100 });
    }
    (s, f)
}

#[test]
fn an_import_goes_through_the_queue_and_undo_takes_it_back() {
    let (mut s, mut f) = import_store();
    let file = parse(FILE.as_bytes()).unwrap();
    let p = plan(&file, "mal", &s.load_list("mal").unwrap(), &HashMap::new());
    // New only: one add (the rewatch), nothing else.
    let acts = actions(&p, Mode::NewOnly, &HashSet::new());
    assert_eq!(acts.len(), 1);
    // New and different, taking the file's status for 1004 only.
    let acts = actions(&p, Mode::NewAndDifferent, &HashSet::from([1004]));
    assert_eq!(acts.len(), 3, "add 1003, update 1001 and 1004; 1007 keeps mine (nothing to send), 1005 is not updated");
    let ids = s.import("mal", &acts, 500).unwrap();
    assert_eq!(ids.len(), 3);
    // The list shows it at once; the service has nothing yet.
    let e = s.load_item("mal", 1003).unwrap().unwrap();
    assert_eq!(e.values(), Values { status: Status::Completed, progress: 6, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 0 });
    assert_eq!(s.load_item("mal", 1004).unwrap().unwrap().status, Status::OnHold);
    assert_eq!(s.load_item("mal", 1001).unwrap().unwrap().progress, 5);
    assert!(!f.remote.contains_key(&1003));
    let q = s.queue("mal", 500).unwrap();
    assert_eq!(q.len(), 3);
    let add = q.iter().find(|o| o.media_id == 1003).unwrap();
    assert!(add.added, "one change per entry: the add carries the rewatch");
    // One goes out; undo takes back the two that still wait.
    assert_eq!(queue::step(&mut s, &mut f, 600).unwrap(), Step::Handled { kind: Kind::Anime, media_id: 1001, outcome: Outcome::Sent });
    assert_eq!(s.undo_import("mal", &ids, 1_000).unwrap(), 2);
    assert!(s.load_item("mal", 1003).unwrap().is_none(), "the add is gone again");
    assert_eq!(s.load_item("mal", 1004).unwrap().unwrap().status, Status::Watching);
    assert_eq!(s.load_item("mal", 1001).unwrap().unwrap().progress, 5, "sent stays sent");
    assert_eq!(s.open_count("mal").unwrap(), 0);
    assert_eq!(s.undo_import("mal", &ids, 1_000).unwrap(), 0, "nothing left to undo");
}

#[test]
fn an_imported_add_is_sent_with_all_its_values() {
    let (mut s, mut f) = import_store();
    let file = parse(FILE.as_bytes()).unwrap();
    let p = plan(&file, "mal", &s.load_list("mal").unwrap(), &HashMap::new());
    s.import("mal", &actions(&p, Mode::NewOnly, &HashSet::new()), 500).unwrap();
    while let Step::Handled { .. } = queue::step(&mut s, &mut f, 600).unwrap() {}
    assert_eq!(f.remote[&1003].values, Values { status: Status::Completed, progress: 6, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 0 });
}

#[test]
fn an_import_into_anilist_keeps_the_mal_id() {
    let mut s = Store::open_memory().unwrap();
    let add = Action::Add { media: Box::new(Media { id_mal: Some(1003), ..media(173003, "Hoshifuri Gakuen", Some(24)) }), values: vals(Status::Watching, 4, ten(8)) };
    s.import("anilist", &[add], 500).unwrap();
    let it = s.load_item(Scope::new("anilist", Kind::Anime), 173003).unwrap().unwrap();
    assert_eq!((it.media.id_mal, it.progress, it.score), (Some(1003), 4, ten(8)));
    // A second preview finds it on the list by its MAL id.
    let file = XmlList { entries: vec![XmlEntry { mal_id: 1003, status: Some(Status::Watching), raw_status: "Watching".into(), watched: 4, score: ten(8), ..blank() }], ..XmlList::default() };
    assert_eq!(plan(&file, "anilist", &s.load_list("anilist").unwrap(), &HashMap::new()).rows[0].verdict, Verdict::Same);
}

/// A MAL XML file scores 1-10: read on the 100-point scale, written back rounded, and compared
/// on the 1-10 scale (an AniList 73 is the file's 7: the same, never rounded down to 70).
#[test]
fn file_scores_are_tens_and_a_73_is_the_files_7() {
    let file = XmlList {
        entries: vec![
            XmlEntry { mal_id: 1, status: Some(Status::Watching), raw_status: "Watching".into(), watched: 3, score: ten(7), ..blank() },
            XmlEntry { mal_id: 2, status: Some(Status::Watching), raw_status: "Watching".into(), watched: 3, score: ten(8), ..blank() },
        ],
        ..XmlList::default()
    };
    let al = |id: u32, points: u32| item(Media { id_mal: Some(id), ..media(id + 170_000, "A", Some(12)) }, vals(Status::Watching, 3, Score::from_points(points)));
    let p = plan(&file, "anilist", &[al(1, 73), al(2, 73)], &HashMap::new());
    assert_eq!(row(&p, 1).verdict, Verdict::Same);
    assert_eq!(row(&p, 2).verdict, Verdict::Different);
    assert!(row(&p, 2).update.is_empty(), "a score already there is kept");
    // Written: 73 is 7, 75 is 8, none is 0.
    let rows: Vec<ExportRow> = [73, 75, 0]
        .iter()
        .enumerate()
        .map(|(i, &pts)| ExportRow { item: item(media(i as u32 + 1, "A", Some(12)), vals(Status::Watching, 3, Score::from_points(pts))), start: None, finish: None })
        .collect();
    let x = write("demo_user", &rows);
    let back = parse(x.as_bytes()).unwrap();
    assert_eq!(back.entries.iter().map(|e| e.score).collect::<Vec<_>>(), [ten(7), ten(8), None]);
    assert!(x.contains("<my_score>7</my_score>") && x.contains("<my_score>0</my_score>"));
}
