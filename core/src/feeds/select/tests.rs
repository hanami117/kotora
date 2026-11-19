//! Selection tests: made-up titles, ids and releases only.

use super::*;
use crate::store::tests::item as base_item;

fn entry(id: u32, title: &str, status: Status, progress: u32) -> ListItem {
    let mut it = base_item(id, title, status, progress, 0);
    it.media.synonyms.clear();
    it.media.title_en = None;
    it
}

fn list() -> Vec<ListItem> {
    vec![
        entry(1, "Lanterns of Kirigaoka", Status::Watching, 7),
        entry(2, "Starfall Delivery Service", Status::PlanToWatch, 0),
        entry(3, "Paper Crane Detective", Status::OnHold, 2),
        entry(4, "Clockwork Tanuki", Status::Completed, 12),
        entry(5, "Harbor Lights Overtime", Status::Dropped, 3),
    ]
}

fn it(title: &str) -> Item {
    Item { guid: title.into(), title: title.into(), seeders: Some(10), ..Item::default() }
}

fn hashed(title: &str, hash: &str) -> Item {
    Item { info_hash: Some(hash.into()), ..it(title) }
}

struct World {
    list: Vec<ListItem>,
    picks: HashMap<String, (u32, String, String)>,
    relations: Relations,
    aired: HashMap<u32, u32>,
    filter: Filter,
    series: HashMap<u32, SeriesFilter>,
    marks: Marks,
}

impl World {
    fn new() -> World {
        World { list: list(), picks: HashMap::new(), relations: Relations::default(), aired: HashMap::new(), filter: Filter::default(), series: HashMap::new(), marks: Marks::default() }
    }
    fn select(&self, items: &[Item]) -> Selection {
        let items: Vec<(i64, Item)> = items.iter().cloned().map(|i| (1, i)).collect();
        let ctx = Ctx { list: &self.list, picks: &self.picks, relations: &self.relations, aired: &self.aired, filter: &self.filter, series: &self.series, marks: &self.marks };
        select(&items, &ctx)
    }
}

fn rows(s: &Selection) -> Vec<(u32, (u32, u32), Vec<usize>)> {
    s.rows.iter().map(|r| (r.media_id, r.episodes, r.releases.clone())).collect()
}

#[test]
fn which_lists_count() {
    let w = World::new();
    let s = w.select(&[
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"),
        it("[Hoshi] Paper Crane Detective - 03 [1080p]"),
        it("[Hoshi] Clockwork Tanuki - 13 [1080p]"),
        it("[Hoshi] Harbor Lights Overtime - 04 [1080p]"),
        it("[Hoshi] Ghost of the Library Stacks - 01 [1080p]"),
    ]);
    // Watching and On hold count; Completed, Dropped and titles off the list don't.
    assert_eq!(rows(&s), vec![(1, (8, 8), vec![0]), (3, (3, 3), vec![1])]);
    assert_eq!(s.new_count(), 2);
    let mut rw = entry(4, "Clockwork Tanuki", Status::Completed, 4);
    rw.rewatching = true;
    let mut w = World::new();
    w.list[3] = rw;
    assert_eq!(rows(&w.select(&[it("[Hoshi] Clockwork Tanuki - 05 [1080p]")])), vec![(4, (5, 5), vec![0])], "a rewatch past its progress");
}

#[test]
fn the_new_rule() {
    let mut w = World::new();
    let s = w.select(&[
        it("[Hoshi] Lanterns of Kirigaoka - 07 [1080p]"),
        it("[Hoshi] Lanterns of Kirigaoka - 13 [1080p]"),
        it("[Hoshi] Starfall Delivery Service - 02 [1080p]"),
        it("[Hoshi] Starfall Delivery Service - 01 [1080p]"),
    ]);
    // At the progress: no. Past the count (12): no (and unsure). Plan to watch: episode 1 only.
    assert_eq!(rows(&s), vec![(2, (1, 1), vec![3])]);
    // The aired count caps it.
    w.aired.insert(1, 8);
    let s = w.select(&[it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"), it("[Hoshi] Lanterns of Kirigaoka - 09 [1080p]")]);
    assert_eq!(rows(&s), vec![(1, (8, 8), vec![0])]);
}

#[test]
fn batches_and_their_new_part() {
    let w = World::new();
    let s = w.select(&[it("[Kumo-Subs] Lanterns of Kirigaoka (01-12) [BD 1080p] [Batch]"), it("[Kumo-Subs] Starfall Delivery Service (01-12) [BD 1080p]")]);
    assert_eq!(rows(&s), vec![(1, (8, 12), vec![0]), (2, (1, 12), vec![1])]);
    let r = parse_release("Paper Crane Detective S01 1080p WEB-DL H.264-Nami");
    assert_eq!(new_part(&w.list[2], &r, None), Some((3, 12)), "a season pack: all of it after the progress");
    let mut w = World::new();
    w.filter.batches = Batches::Hide;
    let s = w.select(&[it("[Kumo-Subs] Lanterns of Kirigaoka (01-12) [BD 1080p] [Batch]")]);
    assert_eq!((s.rows.len(), s.hidden[0].2.to_string()), (0, "batches hidden".to_string()));
    w.filter.batches = Batches::AfterFinished;
    // Id 1 is odd: finished airing in the test list.
    assert_eq!(w.select(&[it("[Kumo-Subs] Lanterns of Kirigaoka (01-12) [BD 1080p]")]).rows.len(), 1);
}

#[test]
fn one_row_per_episode_best_first() {
    let mut w = World::new();
    w.filter.groups_preferred = vec!["Tsuki".into(), "Hoshi".into()];
    let items = [
        it("[Hoshi] Lanterns of Kirigaoka - 08 [720p]"),
        Item { seeders: Some(500), ..it("[Other] Lanterns of Kirigaoka - 08 [1080p]") },
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"),
        it("[Tsuki] Lanterns of Kirigaoka - 08 [720p]"),
        it("[Hoshi] Lanterns of Kirigaoka - 08v2 [1080p]"),
    ];
    // Group order, then resolution, then version; seeders last.
    assert_eq!(rows(&w.select(&items)), vec![(1, (8, 8), vec![3, 4, 2, 0, 1])]);
    w.filter.groups_preferred.clear();
    assert_eq!(rows(&w.select(&items))[0].2, vec![4, 1, 2, 0, 3]);
}

#[test]
fn the_same_release_in_two_feeds_counts_once() {
    let w = World::new();
    let a = hashed("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]", "aa");
    let s = w.select(&[a.clone(), Item { guid: "other".into(), ..a }]);
    assert_eq!(rows(&s), vec![(1, (8, 8), vec![0])]);
}

#[test]
fn filters_say_why() {
    let mut w = World::new();
    w.filter = Filter {
        groups_never: vec!["Bad".into()],
        exclude: vec!["dub".into(), "dual audio".into()],
        resolutions: vec![1080],
        min_seeders: Some(5),
        ten_bit: false,
        ..Filter::default()
    };
    let s = w.select(&[
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p] [Dub]"),
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p] [Dual-Audio]"),
        it("[bad] Lanterns of Kirigaoka - 08 [1080p]"),
        it("[Hoshi] Lanterns of Kirigaoka - 08 [720p]"),
        Item { seeders: Some(1), ..it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]") },
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p HEVC 10bit]"),
        // "Dubbed" is not the word "dub".
        it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p] Dubbed"),
    ]);
    let why: Vec<String> = s.hidden.iter().map(|(_, _, h)| h.to_string()).collect();
    assert_eq!(why, ["exclude word \u{201c}dub\u{201d}", "exclude word \u{201c}dual audio\u{201d}", "group Bad is never shown", "resolution not chosen", "only 1 seeders", "10-bit not allowed"]);
    assert_eq!(rows(&s), vec![(1, (8, 8), vec![6])]);
    // Include words: all of them.
    w.filter = Filter { include: vec!["multi sub".into()], ..Filter::default() };
    let s = w.select(&[it("[Hoshi] Lanterns of Kirigaoka - 08 [Multi-Sub]"), it("[Hoshi] Lanterns of Kirigaoka - 08 [Sub]")]);
    assert_eq!((rows(&s), s.hidden[0].2.to_string()), (vec![(1, (8, 8), vec![0])], "no \u{201c}multi sub\u{201d} in the name".to_string()));
}

#[test]
fn per_series_filters_inherit() {
    let mut w = World::new();
    w.filter.groups_only = vec!["Hoshi".into()];
    w.series.insert(3, SeriesFilter { groups_only: Some(vec!["Tsuki".into()]), ..SeriesFilter::default() });
    let s = w.select(&[it("[Tsuki] Lanterns of Kirigaoka - 08"), it("[Tsuki] Paper Crane Detective - 03"), it("[Hoshi] Paper Crane Detective - 03")]);
    assert_eq!(rows(&s), vec![(3, (3, 3), vec![1])]);
    assert_eq!(s.hidden.iter().map(|(i, m, _)| (*i, *m)).collect::<Vec<_>>(), [(0, 1), (2, 3)]);
    let f = Filter::default().with(&SeriesFilter { feeds: Some(Some(vec![2])), ..SeriesFilter::default() });
    assert_eq!(hidden(&f, &it("x"), &parse_release("x - 01"), &w.list[0], 1), Some(Hidden::FeedNotUsed));
}

#[test]
fn size_per_episode() {
    let mut w = World::new();
    w.filter.max_size = Some(2_000_000_000);
    let big = Item { size: Some(30_000_000_000), ..it("[Kumo-Subs] Lanterns of Kirigaoka (01-12) [BD 1080p]") };
    let ok = Item { size: Some(12_000_000_000), ..it("[Kumo-Subs] Lanterns of Kirigaoka (01-12) [BD 720p]") };
    let s = w.select(&[big, ok]);
    assert_eq!((rows(&s), s.hidden[0].2.clone()), (vec![(1, (8, 12), vec![1])], Hidden::TooLarge));
}

#[test]
fn unsure_matches_never_count() {
    let w = World::new();
    let s = w.select(&[it("[Hoshi] Lanterns of Kirigaok - 08 [1080p]")]);
    assert_eq!((s.rows.len(), s.unsure.len(), s.unsure[0].media_id), (0, 1, 1));
}

#[test]
fn picks_are_shared_with_files() {
    // A pick made for a file ("Wrong anime…") holds for the feed's release of the same title.
    let mut w = World::new();
    let file = crate::recognize::parse("/v/[Hoshi] Kirigaoka - 08 [1080p].mkv").unwrap();
    w.picks.insert(file.key(), (1, "user".into(), "Lanterns of Kirigaoka".into()));
    assert_eq!(rows(&w.select(&[it("[Other] Kirigaoka - 09 [720p]")])), vec![(1, (9, 9), vec![0])]);
    w.picks.insert(file.key(), (0, "ignore".into(), String::new()));
    assert!(w.select(&[it("[Other] Kirigaoka - 09 [720p]")]).rows.is_empty());
}

#[test]
fn marks() {
    let mut w = World::new();
    let v1 = it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]");
    let v2 = it("[Hoshi] Lanterns of Kirigaoka - 08v2 [1080p]");
    let nine = it("[Hoshi] Lanterns of Kirigaoka - 09 [1080p]");
    w.marks.got.insert((1, 8, 8), (None, None));
    // Got: its other releases hide; a v2 shows as "v2 out" and isn't counted as new.
    let s = w.select(&[v1.clone(), nine.clone()]);
    assert_eq!(rows(&s), vec![(1, (9, 9), vec![1])]);
    let s = w.select(&[v1.clone(), v2.clone(), nine.clone()]);
    assert_eq!((rows(&s), s.rows[0].version_out, s.new_count()), (vec![(1, (8, 8), vec![1]), (1, (9, 9), vec![2])], true, 1));
    w.marks.got.insert((1, 8, 8), (None, Some(2)));
    assert_eq!(w.select(&[v2]).rows.len(), 0);
    // A got batch covers its episodes; hidden hides; "not this series" drops that title.
    w.marks.got.insert((1, 9, 12), (None, None));
    assert!(w.select(std::slice::from_ref(&nine)).rows.is_empty());
    let mut w = World::new();
    w.marks.hidden.insert((1, 9, 9));
    assert!(w.select(std::slice::from_ref(&nine)).rows.is_empty());
    let mut w = World::new();
    w.marks.not_this.insert((1, parse_release(&nine.title).parsed.key()));
    assert!(w.select(&[nine]).rows.is_empty());
}

#[test]
fn absolute_numbering_moves_with_relations() {
    let mut w = World::new();
    w.list.push(entry(9002, "Starfall Delivery Service 2nd Season", Status::Watching, 1));
    w.relations = Relations::parse("::meta\n- version: 1.3.0\n- last_modified: 2026-01-02\n\n::rules\n- 9001|501|7001:13-24 -> 9002|502|7002:1-12!\n").unwrap();
    // "S2 - 15" numbers on from season 1: the rule for 9002 makes it its episode 3.
    let s = w.select(&[it("[Hoshi] Starfall Delivery Service S2 - 15 [1080p]")]);
    assert_eq!(rows(&s), vec![(9002, (3, 3), vec![0])]);
}

