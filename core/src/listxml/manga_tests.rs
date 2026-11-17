//! The manga side of the list file: made-up data only (`fixtures/mangalist.xml`).

use std::collections::{HashMap, HashSet};

use super::*;
use crate::queue::{self, Outcome, Step};
use crate::score::ten;
use crate::service::{Details, ListService, MyEntry, ServiceError};
use crate::{Remote, Scope};

const FILE: &str = include_str!("fixtures/mangalist.xml");
const ANIME_FILE: &str = include_str!("fixtures/animelist.xml");
const MANGA: Scope = Scope::new("mal", Kind::Manga);

fn manga(id: u32, title: &str, chapters: Option<u32>, volumes: Option<u32>) -> Media {
    Media { kind: Kind::Manga, id, title: title.into(), episodes: chapters, volumes, format: "manga".into(), ..Media::default() }
}

fn item(m: Media, status: Status, chapters: u32, volumes: u32, score: Option<Score>) -> ListItem {
    ListItem { media: m, status, progress: chapters, score, rewatching: false, times_rewatched: 0, volumes, updated_at: 100 }
}

impl XmlEntry {
    fn blank_manga() -> XmlEntry {
        XmlEntry {
            kind: Kind::Manga,
            mal_id: 0,
            title: String::new(),
            series_type: String::new(),
            episodes: None,
            volumes: None,
            raw_status: String::new(),
            status: None,
            watched: 0,
            volumes_read: 0,
            score: None,
            start_date: None,
            finish_date: None,
            times_watched: 0,
            rewatching: false,
            rewatching_ep: 0,
            update_on_import: true,
        }
    }
}

fn read(file: &XmlList, id: u32) -> &XmlEntry {
    file.entries.iter().find(|e| e.mal_id == id).unwrap()
}

fn parse_manga(bytes: &[u8]) -> Result<XmlList, String> {
    parse_kind(bytes, Kind::Manga)
}

fn row(p: &Plan, id: u32) -> &PlanRow {
    p.rows.iter().find(|r| r.entry.mal_id == id && !matches!(r.verdict, Verdict::Unknown(ref w) if w.contains("Twice"))).unwrap()
}

#[test]
fn reads_a_manga_export() {
    let f = parse_manga(FILE.as_bytes()).unwrap();
    assert_eq!((f.kind, f.entries.len(), f.other), (Kind::Manga, 8, 0));
    assert_eq!(f.user_name.as_deref(), Some("demo_user"));
    let e = read(&f, 2001);
    assert_eq!((e.title.as_str(), e.episodes, e.volumes, e.watched, e.volumes_read, e.score), ("Lantern Road", Some(40), Some(5), 12, 2, ten(7)));
    assert_eq!((e.start_date.as_deref(), e.finish_date.as_deref()), (Some("2026-09-01"), None));
    assert_eq!(e.status, Some(Status::Watching), "Reading is Kotora's Watching slot");
    assert_eq!(e.values().unwrap(), Values { volumes: 2, ..Values::new(Status::Watching, 12, ten(7)) });
    // Unknown counts (still publishing) are none; Plan to Read reads.
    let m = read(&f, 2003);
    assert_eq!((m.episodes, m.volumes, m.status), (None, None, Some(Status::PlanToWatch)));
    // A reread: Completed, rereading, once read.
    assert_eq!(read(&f, 2004).values().unwrap(), Values { status: Status::Completed, progress: 10, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 2 });
    assert_eq!(read(&f, 2005).status, None);
    assert!(!read(&f, 2006).update_on_import);
    // The same file gzipped.
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut e, FILE.as_bytes()).unwrap();
    assert_eq!(parse_manga(&e.finish().unwrap()).unwrap(), f);
}

#[test]
fn counts_past_the_known_totals_are_cut() {
    let e = XmlEntry { status: Some(Status::Watching), episodes: Some(10), volumes: Some(2), watched: 30, volumes_read: 9, ..XmlEntry::blank_manga() };
    let v = e.values().unwrap();
    assert_eq!((v.progress, v.volumes), (10, 2));
}

#[test]
fn an_anime_file_is_refused_by_the_manga_import_and_the_other_way_round() {
    let err = parse_manga(ANIME_FILE.as_bytes()).unwrap_err();
    assert!(err.contains("\u{201c}Import anime list\u{2026}\u{201d}"), "names the anime item: {err}");
    let err = parse(FILE.as_bytes()).unwrap_err();
    assert!(err.contains("\u{201c}Import manga list\u{2026}\u{201d}"), "names the manga item: {err}");
    assert!(export_refusal(ServiceId::AniList).is_some_and(|w| w.contains("Export anime list\u{2026}") && w.contains("Export manga list\u{2026}")));
    // A file with both: each import reads its own kind and counts the other.
    let both = String::from("<myanimelist><anime><series_animedb_id>1</series_animedb_id><my_status>Watching</my_status></anime><manga><manga_mangadb_id>2</manga_mangadb_id><my_status>Reading</my_status></manga></myanimelist>");
    let (a, m) = (parse(both.as_bytes()).unwrap(), parse_manga(both.as_bytes()).unwrap());
    assert_eq!((a.entries.len(), a.other, m.entries.len(), m.other), (1, 1, 1, 1));
    // An empty list of either kind is not a mix-up.
    assert!(parse_manga(b"<myanimelist><myinfo/></myanimelist>").unwrap().entries.is_empty());
}

fn mal_store() -> Store {
    let mut s = Store::open_memory().unwrap();
    s.set_account("mal", "demo_user").unwrap();
    let a = item(manga(2001, "Lantern Road", Some(40), Some(5)), Status::Watching, 12, 2, ten(7));
    let mut b = item(manga(2004, "Winter Again", Some(10), Some(2)), Status::Completed, 10, 2, ten(9));
    b.rewatching = true;
    b.times_rewatched = 1;
    let c = item(manga(2003, "Moth <Garden> ]]> Two", None, None), Status::PlanToWatch, 0, 0, None);
    s.replace_list(MANGA, &[a, b, c], 100).unwrap();
    // An anime with the same id as a manga stays out of the manga file.
    s.replace_list("mal", &[ListItem { media: Media { id: 2001, title: "Not A Manga".into(), ..Media::default() }, ..item(manga(0, "", None, None), Status::Watching, 1, 0, None) }], 100).unwrap();
    let mine = MyEntry { status: "completed".into(), progress: 10, score: ten(9), rewatching: true, times_rewatched: 1, volumes: 2, start_date: Some("2025-01".into()), finish_date: Some("2025-03-30".into()) };
    s.save_details("mal", &Details { id: 2004, kind: Kind::Manga, title: "Winter Again".into(), mine: Some(mine), ..Details::default() }, 100).unwrap();
    s
}

#[test]
fn export_writes_the_manga_list_and_reads_back_the_same() {
    let s = mal_store();
    let ex = export_kind(&s, ServiceId::Mal, Kind::Manga).unwrap();
    assert_eq!((ex.written, ex.skipped.len()), (3, 0), "the anime with a manga's id is not in it");
    let x = &ex.xml;
    for want in [
        "<user_export_type>2</user_export_type>",
        "<user_total_manga>3</user_total_manga>",
        "<user_total_reading>1</user_total_reading>",
        "<user_total_plantoread>1</user_total_plantoread>",
        "<manga_mangadb_id>2001</manga_mangadb_id>",
        "<manga_title><![CDATA[Lantern Road]]></manga_title>",
        "<manga_volumes>5</manga_volumes>",
        "<manga_chapters>40</manga_chapters>",
        "<my_read_volumes>2</my_read_volumes>",
        "<my_read_chapters>12</my_read_chapters>",
        "<my_status>Reading</my_status>",
        "<my_status>Plan to Read</my_status>",
        "<my_times_read>1</my_times_read>",
        "<my_rereading>1</my_rereading>",
        "<my_start_date>2025-01-00</my_start_date>",
        "<my_finish_date>2025-03-30</my_finish_date>",
        "<update_on_import>1</update_on_import>",
    ] {
        assert!(x.contains(want), "missing {want}");
    }
    assert!(!x.contains("<anime>") && !x.contains("series_"), "no anime fields in a manga file");
    // Moth Garden's title holds ]]>: the CDATA is split, and it reads back whole.
    let back = parse_manga(x.as_bytes()).unwrap();
    assert_eq!(read(&back, 2003).title, "Moth <Garden> ]]> Two");
    for it in s.load_list(MANGA).unwrap() {
        assert_eq!(read(&back, it.media.id).values(), Some(it.values()), "{}", it.media.title);
    }
    // Importing the file into the same list finds nothing to do.
    let p = plan(&back, "mal", &s.load_list(MANGA).unwrap(), &HashMap::new());
    assert_eq!((p.kind, p.counts()), (Kind::Manga, Counts { same: 3, ..Counts::default() }));
    // The anime export is untouched by the manga list.
    assert_eq!(export(&s, ServiceId::Mal).unwrap().written, 1);
}

#[test]
fn an_anilist_manga_list_is_never_exported() {
    let s = Store::open_memory().unwrap();
    let err = export_kind(&s, ServiceId::AniList, Kind::Manga).unwrap_err();
    assert!(err.contains("AniList") && err.contains("terms"), "{err}");
}

#[test]
fn file_names_say_manga() {
    assert_eq!(default_file_name_of(Kind::Manga, 1_763_251_200), "mangalist-2025-11-16.xml");
    assert_eq!(default_file_name(1_763_251_200), "animelist-2025-11-16.xml");
}

/// 2001 Reading 12 ch / 2 vol (file: 12 / 2, score 7) → same once scored; here ch 8 / vol 3,
/// no score → different: chapters from the file, volumes kept (higher), score fills;
/// 2002 Reading 24 ch (file: Completed 24 / 3) → the further status wins, volumes filled;
/// 2004 Dropped (file: Completed, rereading) → a status conflict.
fn list() -> Vec<ListItem> {
    vec![
        item(manga(2001, "Lantern Road", Some(40), Some(5)), Status::Watching, 8, 3, None),
        item(manga(2002, "Salt and Wind", Some(24), Some(3)), Status::Watching, 24, 1, None),
        item(manga(2004, "Winter Again", Some(10), Some(2)), Status::Dropped, 4, 1, None),
        item(manga(2006, "Leave Me Be", Some(30), Some(4)), Status::OnHold, 2, 0, None),
    ]
}

#[test]
fn the_manga_preview_sorts_entries_and_the_higher_count_wins() {
    let f = parse_manga(FILE.as_bytes()).unwrap();
    let p = plan(&f, "mal", &list(), &HashMap::new());
    assert_eq!(row(&p, 2003).verdict, Verdict::New);
    assert_eq!(row(&p, 2003).media.as_ref().unwrap().kind, Kind::Manga);
    assert_eq!(row(&p, 2001).verdict, Verdict::Different);
    assert_eq!(row(&p, 2001).update, Change { progress: Some(12), score: Some(ten(7)), ..Change::default() }, "more chapters from the file, the list's 3 volumes stay, the score fills");
    assert!(row(&p, 2001).conflict.is_none());
    // 2002: Completed is further along than Reading: its totals fill in.
    assert_eq!(row(&p, 2002).update.status, Some(Status::Completed));
    assert_eq!(row(&p, 2002).update.volumes, Some(3));
    // 2004: Dropped against Completed: asked; taking the file's status takes its counts.
    assert_eq!(row(&p, 2004).conflict, Some(StatusConflict { mine: Status::Dropped, file: Status::Completed }));
    assert_eq!(row(&p, 2004).with_file_status.status, Some(Status::Completed));
    assert_eq!(row(&p, 2004).with_file_status.volumes, Some(2), "the file's volumes are higher than 1");
    // 2006 asked not to be updated; 2005 and the unnumbered ones can't be used.
    assert!(row(&p, 2006).update.is_empty());
    assert!(matches!(&row(&p, 2005).verdict, Verdict::Unknown(w) if w.contains("Waiting")));
    let unknown: Vec<&Verdict> = p.rows.iter().map(|r| &r.verdict).filter(|v| matches!(v, Verdict::Unknown(_))).collect();
    assert_eq!(unknown.len(), 3, "odd status, no id, twice");
    let c = p.counts();
    assert_eq!((c.new, c.different, c.conflicts), (1, 4, 1));
}

#[test]
fn a_list_that_is_further_along_keeps_its_counts() {
    let f = parse_manga(FILE.as_bytes()).unwrap();
    let ahead = vec![item(manga(2001, "Lantern Road", Some(40), Some(5)), Status::Watching, 30, 4, ten(7))];
    let p = plan(&f, "mal", &ahead, &HashMap::new());
    assert_eq!(row(&p, 2001).verdict, Verdict::Different);
    assert!(row(&p, 2001).update.is_empty(), "nothing to send");
}

/// A made-up service with a manga list only.
#[derive(Default)]
struct Shelf {
    manga: HashMap<u32, Remote>,
    clock: i64,
}

impl ListService for Shelf {
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
    fn get_entry(&mut self, _: u32) -> Result<Option<Remote>, ServiceError> {
        panic!("an anime was asked for")
    }
    fn update_entry(&mut self, _: u32, _: &Change) -> Result<Remote, ServiceError> {
        panic!("an anime was sent")
    }
    fn get_manga_entry(&mut self, id: u32) -> Result<Option<Remote>, ServiceError> {
        Ok(self.manga.get(&id).copied())
    }
    fn update_manga_entry(&mut self, id: u32, change: &Change) -> Result<Remote, ServiceError> {
        self.clock += 1;
        let old = self.manga.get(&id).map_or(Values::new(Status::PlanToWatch, 0, None), |r| r.values);
        let r = Remote { values: change.apply(old), updated_at: self.clock + 1000 };
        self.manga.insert(id, r);
        Ok(r)
    }
}

fn shelf_store() -> (Store, Shelf) {
    let mut s = Store::open_memory().unwrap();
    let l = list();
    s.replace_list(MANGA, &l, 100).unwrap();
    let mut f = Shelf::default();
    for it in &l {
        f.manga.insert(it.media.id, Remote { values: it.values(), updated_at: 100 });
    }
    (s, f)
}

#[test]
fn a_manga_import_goes_through_the_queue_and_undo_takes_it_back() {
    let (mut s, mut f) = shelf_store();
    let file = parse_manga(FILE.as_bytes()).unwrap();
    let p = plan(&file, "mal", &s.load_list(MANGA).unwrap(), &HashMap::new());
    assert_eq!(actions(&p, Mode::NewOnly, &HashSet::new()).len(), 1, "only 2003 is new");
    let acts = actions(&p, Mode::NewAndDifferent, &HashSet::from([2004]));
    assert!(acts.iter().all(|a| match a {
        Action::Add { media, .. } => media.kind == Kind::Manga,
        Action::Update { kind, .. } => *kind == Kind::Manga,
    }));
    assert_eq!(acts.len(), 4, "add 2003, update 2001, 2002 and 2004 (the file's status)");
    let ids = s.import("mal", &acts, 500).unwrap();
    assert_eq!(ids.len(), 4);
    // The list shows it at once; the service has nothing yet.
    let e = s.load_item(MANGA, 2001).unwrap().unwrap();
    assert_eq!((e.progress, e.volumes, e.score), (12, 3, ten(7)));
    assert_eq!(s.load_item(MANGA, 2004).unwrap().unwrap().status, Status::Completed);
    assert!(s.load_item(MANGA, 2003).unwrap().is_some());
    assert!(!f.manga.contains_key(&2003));
    assert!(s.queue("mal", 500).unwrap().iter().all(|o| o.kind == Kind::Manga));
    // One goes out; undo takes back the three that still wait.
    let Step::Handled { kind, outcome, .. } = queue::step(&mut s, &mut f, 600).unwrap() else { panic!("nothing sent") };
    assert_eq!((kind, outcome), (Kind::Manga, Outcome::Sent));
    assert_eq!(s.undo_import("mal", &ids, 1_000).unwrap(), 3);
    assert_eq!(s.open_count("mal").unwrap(), 0);
    assert!(s.load_item(MANGA, 2003).unwrap().is_none(), "the add is gone again");
    assert_eq!(s.load_item(MANGA, 2004).unwrap().unwrap().status, Status::Dropped);
}

#[test]
fn an_imported_manga_is_sent_with_its_chapters_and_volumes() {
    let (mut s, mut f) = shelf_store();
    let e = XmlEntry {
        kind: Kind::Manga,
        mal_id: 2010,
        title: "Fresh Ink".into(),
        episodes: Some(40),
        volumes: Some(5),
        status: Some(Status::Watching),
        raw_status: "Reading".into(),
        watched: 7,
        volumes_read: 2,
        score: ten(8),
        update_on_import: true,
        ..XmlEntry::blank_manga()
    };
    let file = XmlList { kind: Kind::Manga, entries: vec![e], ..XmlList::default() };
    let p = plan(&file, "mal", &s.load_list(MANGA).unwrap(), &HashMap::new());
    assert_eq!(row(&p, 2010).verdict, Verdict::New);
    s.import("mal", &actions(&p, Mode::NewOnly, &HashSet::new()), 500).unwrap();
    while let Step::Handled { .. } = queue::step(&mut s, &mut f, 600).unwrap() {}
    assert_eq!(f.manga[&2010].values, Values { volumes: 2, ..Values::new(Status::Watching, 7, ten(8)) });
    // A Completed add takes the known totals.
    let done = XmlEntry { mal_id: 2011, status: Some(Status::Completed), raw_status: "Completed".into(), watched: 0, volumes_read: 0, score: None, ..file.entries[0].clone() };
    let file = XmlList { kind: Kind::Manga, entries: vec![done], ..XmlList::default() };
    let p = plan(&file, "mal", &s.load_list(MANGA).unwrap(), &HashMap::new());
    s.import("mal", &actions(&p, Mode::NewOnly, &HashSet::new()), 700).unwrap();
    while let Step::Handled { .. } = queue::step(&mut s, &mut f, 800).unwrap() {}
    assert_eq!(f.manga[&2011].values, Values { volumes: 5, ..Values::new(Status::Completed, 40, None) });
}

/// A manga file into AniList: matched by `idMal`, the others looked up on AniList as manga
/// (`ListService::manga_by_mal_ids`), adds land on the AniList manga list with their MAL id.
#[test]
fn a_manga_import_into_anilist_matches_by_mal_id() {
    let f = parse_manga(FILE.as_bytes()).unwrap();
    // On the AniList manga list: MAL 2001 as AniList 192001.
    let on_list = item(Media { id_mal: Some(2001), ..manga(192001, "Lantern Road", Some(40), Some(5)) }, Status::Watching, 3, 3, None);
    let list = vec![on_list];
    let ids = Plan::ids_to_map(&f, &list, "anilist");
    assert!(!ids.contains(&2001) && ids.contains(&2003), "only the ids not on the list are looked up: {ids:?}");
    let mut mapped = HashMap::new();
    mapped.insert(2003, Media { id_mal: Some(2003), ..manga(192003, "Ink Harbor", Some(30), Some(4)) });
    let p = plan(&f, "anilist", &list, &mapped);
    assert_eq!((row(&p, 2001).verdict.clone(), row(&p, 2001).target), (Verdict::Different, Some(192001)));
    assert_eq!((row(&p, 2003).verdict.clone(), row(&p, 2003).target), (Verdict::New, Some(192003)));
    assert_eq!(row(&p, 2003).media.as_ref().map(|m| m.kind), Some(Kind::Manga));
    let add = actions(&p, Mode::NewOnly, &HashSet::new());
    let mut s = Store::open_memory().unwrap();
    s.import("anilist", &add, 500).unwrap();
    let it = s.load_item(Scope::new("anilist", Kind::Manga), 192003).unwrap().unwrap();
    assert_eq!((it.media.id_mal, it.media.kind), (Some(2003), Kind::Manga));
    assert!(s.load_item(Scope::new("anilist", Kind::Anime), 192003).unwrap().is_none(), "never an anime");
}
