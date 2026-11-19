//! Tests of the Taiga port. Made-up titles, ids and releases only.

use super::*;
use crate::store::tests::item as base_item;

fn entry(id: u32, title: &str, status: Status, progress: u32) -> ListItem {
    let mut it = base_item(id, title, status, progress, 0);
    it.media.synonyms.clear();
    it.media.title_en = None;
    it
}

fn list() -> Vec<ListItem> {
    // Id 2 and 4 are airing in the test list (even ids); 1 and 3 finished.
    vec![
        entry(1, "Lanterns of Kirigaoka", Status::Watching, 7),
        entry(2, "Starfall Delivery Service", Status::PlanToWatch, 0),
        entry(3, "Paper Crane Detective", Status::Dropped, 2),
        entry(4, "Clockwork Tanuki", Status::PlanToWatch, 0),
    ]
}

fn it(title: &str) -> Item {
    Item {
        guid: title.into(),
        title: title.into(),
        magnet: Some(format!("magnet:?xt=urn:btih:{:040x}", title.len())),
        ..Item::default()
    }
}

fn run(
    titles: &[&str],
    filters: &[Filter],
    archive: &Archive,
    available: &dyn Fn(u32, u32) -> bool,
) -> Vec<Entry> {
    let l = list();
    let items: Vec<(i64, Item)> = titles.iter().map(|t| (0, it(t))).collect();
    let mut e = examine(&items, &l, &HashMap::new(), &Relations::default());
    let w = World {
        list: &l,
        available,
    };
    run_filters(&mut e, filters, true, archive, &w);
    e
}

fn states(e: &[Entry]) -> Vec<(&str, ItemState)> {
    e.iter().map(|x| (x.item.title.as_str(), x.state)).collect()
}

#[test]
fn presets_are_taigas() {
    let p = presets();
    assert_eq!(p.len(), 10);
    let names: Vec<&str> = p
        .iter()
        .filter(|p| p.is_default)
        .map(|p| p.filter.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Select currently watching",
            "Select airing anime in plan to watch",
            "Discard dropped",
            "Discard and deactivate not-in-list anime",
            "Discard watched and available episodes",
            "Prefer high-resolution files"
        ]
    );
    assert_eq!(default_filters().len(), 6);
}

#[test]
fn the_default_filters() {
    let none = |_: u32, _: u32| false;
    let e = run(
        &[
            "[Hoshi] Lanterns of Kirigaoka - 08 [1080p]",
            "[Hoshi] Lanterns of Kirigaoka - 08 [720p]",
            "[Hoshi] Lanterns of Kirigaoka - 06 [1080p]",
            "[Hoshi] Starfall Delivery Service - 01 [1080p]",
            "[Hoshi] Clockwork Tanuki - 01 [1080p]",
            "[Hoshi] Paper Crane Detective - 03 [1080p]",
            "[Hoshi] Ghost of the Library Stacks - 01 [1080p]",
        ],
        &default_filters(),
        &Archive::default(),
        &none,
    );
    let s: HashMap<&str, ItemState> = states(&e).into_iter().collect();
    // Watching, new, 1080p: selected; its 720p loses to the 1080p preference.
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"],
        ItemState::Selected
    );
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08 [720p]"],
        ItemState::DiscardedNormal
    );
    // Watched already.
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 06 [1080p]"],
        ItemState::DiscardedNormal
    );
    // Plan to watch: selected only while airing (id 2 airs, id 4 doesn't... both even/odd rule: 4 is even).
    assert_eq!(
        s["[Hoshi] Starfall Delivery Service - 01 [1080p]"],
        ItemState::Selected
    );
    assert_eq!(
        s["[Hoshi] Clockwork Tanuki - 01 [1080p]"],
        ItemState::Selected
    );
    // Dropped: discarded; not on the list: deactivated.
    assert_eq!(
        s["[Hoshi] Paper Crane Detective - 03 [1080p]"],
        ItemState::DiscardedNormal
    );
    assert_eq!(
        s["[Hoshi] Ghost of the Library Stacks - 01 [1080p]"],
        ItemState::DiscardedInactive
    );
    // Selected first (Taiga's sort).
    assert_eq!(e[0].state, ItemState::Selected);
    assert!(
        e.iter()
            .find(|x| x.item.title.contains("- 08 [1080p]"))
            .unwrap()
            .new_episode
    );
}

#[test]
fn availability_and_archive_discard() {
    let have_8 = |id: u32, ep: u32| id == 1 && ep == 8;
    let e = run(
        &[
            "[Hoshi] Lanterns of Kirigaoka - 08 [1080p]",
            "[Hoshi] Lanterns of Kirigaoka - 09 [1080p]",
        ],
        &default_filters(),
        &Archive::default(),
        &have_8,
    );
    let s: HashMap<&str, ItemState> = states(&e).into_iter().collect();
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"],
        ItemState::DiscardedNormal
    );
    let mut a = Archive::default();
    a.add("[Hoshi] Lanterns of Kirigaoka - 09 [1080p]");
    let e = run(
        &["[Hoshi] Lanterns of Kirigaoka - 09 [1080p]"],
        &default_filters(),
        &a,
        &|_, _| false,
    );
    assert_eq!(e[0].state, ItemState::DiscardedNormal);
    a.add("x");
    a.add("x");
    assert_eq!(a.trimmed(1), ["x"]);
}

#[test]
fn prefer_new_versions_and_fansub() {
    let p = presets();
    let versions = p
        .iter()
        .find(|p| p.filter.name == "Prefer new versions")
        .unwrap()
        .filter
        .clone();
    let e = run(
        &[
            "[Hoshi] Lanterns of Kirigaoka - 08 [1080p]",
            "[Hoshi] Lanterns of Kirigaoka - 08v2 [1080p]",
        ],
        &[versions],
        &Archive::default(),
        &|_, _| false,
    );
    let s: HashMap<&str, ItemState> = states(&e).into_iter().collect();
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08v2 [1080p]"],
        ItemState::Blank
    );
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"],
        ItemState::DiscardedNormal
    );

    // A fansub filter for one anime is strong: the others of that anime go.
    let mut filters = Vec::new();
    assert!(set_fansub_filter(
        &mut filters,
        1,
        "Lanterns of Kirigaoka",
        "Tsuki",
        ""
    ));
    assert_eq!(fansub_groups(&filters, 1), ["Tsuki"]);
    let e = run(
        &[
            "[Hoshi] Lanterns of Kirigaoka - 08 [1080p]",
            "[Tsuki] Lanterns of Kirigaoka - 08 [1080p]",
            "[Hoshi] Starfall Delivery Service - 01",
        ],
        &filters,
        &Archive::default(),
        &|_, _| false,
    );
    let s: HashMap<&str, ItemState> = states(&e).into_iter().collect();
    assert_eq!(
        s["[Tsuki] Lanterns of Kirigaoka - 08 [1080p]"],
        ItemState::Selected
    );
    assert_eq!(
        s["[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"],
        ItemState::DiscardedNormal
    );
    assert_eq!(
        s["[Hoshi] Starfall Delivery Service - 01"],
        ItemState::Blank
    );
    // Changing and taking it back.
    assert!(set_fansub_filter(
        &mut filters,
        1,
        "Lanterns of Kirigaoka",
        "Kumo",
        "720p"
    ));
    assert_eq!((filters.len(), filters[0].conditions.len()), (1, 2));
    assert!(set_fansub_filter(
        &mut filters,
        1,
        "Lanterns of Kirigaoka",
        "",
        ""
    ));
    assert!(filters.is_empty());
}

#[test]
fn conditions_and_operators() {
    let l = list();
    let w = World {
        list: &l,
        available: &|_, _| false,
    };
    let items = vec![(
        0,
        Item {
            size: Some(2 * 1024 * 1024 * 1024),
            description: "Multi-Sub | 1080p".into(),
            ..it("[Hoshi] Lanterns of Kirigaoka - 08 [1920x1080 HEVC]")
        },
    )];
    let e = &examine(&items, &l, &HashMap::new(), &Relations::default())[0];
    let c = |element, op, value: &str| {
        evaluate(
            &Condition {
                element,
                op,
                value: value.into(),
            },
            e,
            &w,
        )
    };
    assert!(c(Element::FileSize, Operator::GreaterThan, "1 GB"));
    assert!(!c(Element::FileSize, Operator::GreaterThan, "3 GiB"));
    assert!(c(
        Element::EpisodeVideoResolution,
        Operator::Equals,
        "1080p"
    ));
    assert!(c(
        Element::EpisodeVideoResolution,
        Operator::GreaterThanOrEqual,
        "720p"
    ));
    assert!(c(Element::EpisodeVideoType, Operator::Contains, "hevc"));
    assert!(c(Element::FileDescription, Operator::Contains, "multi-sub"));
    assert!(c(Element::EpisodeGroup, Operator::Equals, "hoshi"));
    assert!(c(
        Element::EpisodeNumber,
        Operator::GreaterThan,
        "%watched%"
    ));
    assert!(c(Element::UserStatus, Operator::Equals, "1"));
    assert!(c(Element::MetaId, Operator::Equals, "1"));
    assert!(c(Element::FileTitle, Operator::BeginsWith, "[Hoshi]"));
    assert!(
        !c(Element::FileTitle, Operator::BeginsWith, "[hoshi]"),
        "BeginsWith is case-sensitive, as Taiga's StartsWith"
    );
    assert!(c(Element::FileTitle, Operator::NotContains, "720p"));
    assert!(c(Element::FileCategory, Operator::Equals, "Anime"));
    assert_eq!(
        condition_text(
            &Condition {
                element: Element::UserStatus,
                op: Operator::Equals,
                value: "4".into()
            },
            &|_| None
        ),
        "Anime watching status is \u{201c}Dropped\u{201d}"
    );
}

#[test]
fn categories() {
    let c = |t: &str| category_of(&it(t), &parse_release(t));
    assert_eq!(
        c("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]"),
        Category::Anime
    );
    assert_eq!(
        c("[Hoshi] Lanterns of Kirigaoka (01-12) [1080p]"),
        Category::Batch
    );
    assert_eq!(
        c("[Hoshi] Lanterns of Kirigaoka - NCOP [1080p]"),
        Category::Other
    );
    assert_eq!(
        c("[Hoshi] Lanterns of Kirigaoka Vol.1 [Scans].zip"),
        Category::Other
    );
    let mut b = it("[Hoshi] Lanterns of Kirigaoka - 08 [1080p]");
    b.category = "Anime - Batch".into();
    assert_eq!(category_of(&b, &parse_release(&b.title)), Category::Batch);
}

#[test]
fn xml_round_trip_and_taiga_import() {
    let mut f = default_filters();
    f.push(discard_filter(3, "Paper \"Crane\""));
    f[0].anime_ids = vec![1, 2];
    let back = import_filters(&export_filters(&f)).unwrap();
    assert_eq!(back, f);
    // A list as Taiga writes it in settings.xml.
    let taiga = r#"<settings><rss><torrent><filter>
        <item action="prefer" match="all" option="default" enabled="true" name="[Fansub] Lanterns">
          <anime id="1" />
          <condition element="episode_group" operator="equals" value="Tsuki" />
        </item>
        <item action="discard" match="any" option="hide" enabled="false" name="Odd">
          <condition element="no_such_thing" operator="equals" value="x" />
          <condition element="file_title" operator="contains" value="RAW" />
        </item>
    </filter></torrent></rss></settings>"#;
    let t = import_filters(taiga).unwrap();
    assert_eq!(
        (
            t.len(),
            t[0].action,
            t[0].anime_ids.clone(),
            t[1].enabled,
            t[1].option,
            t[1].conditions.len()
        ),
        (2, Action::Prefer, vec![1], false, FilterOption::Hide, 1)
    );
    assert!(import_filters("<nope/>").is_err());
}

#[test]
fn download_order_by_anime_then_episode() {
    let mut e = run(
        &[
            "[Hoshi] Lanterns of Kirigaoka - 10 [1080p]",
            "[Hoshi] Lanterns of Kirigaoka - 09 [1080p]",
            "[Hoshi] Starfall Delivery Service - 01 [1080p]",
        ],
        &default_filters(),
        &Archive::default(),
        &|_, _| false,
    );
    let order: Vec<String> = download_order(&e, false, false)
        .iter()
        .map(|i| e[*i].episode_text())
        .collect();
    assert_eq!(order, ["9", "10", "1"]);
    let order: Vec<String> = download_order(&e, false, true)
        .iter()
        .map(|i| e[*i].episode_text())
        .collect();
    assert_eq!(order, ["10", "9", "1"]);
    e[0].state = ItemState::DiscardedNormal;
    assert_eq!(download_order(&e, false, false).len(), 2);
}

/// Every release of a watched episode is discarded by "Discard watched and available episodes",
/// whatever its resolution (a 1080p twin once stayed shown while its 720p one was discarded).
#[test]
fn watched_episode_twins_are_all_discarded() {
    let l = vec![
        entry(10, "Dragon Ball Super: Beerus", Status::Watching, 2),
        entry(11, "Dragon Ball Super", Status::Completed, 131),
    ];
    let titles = [
        "[Yameii] Dragon Ball Super: Beerus - S01E02 [English Dub] [CR WEB-DL 1080p H264 AAC] [C0A5F010]",
        "[Yameii] Dragon Ball Super: Beerus - S01E02 [English Dub] [CR WEB-DL 720p H264 AAC] [49E59E5E]",
        "[Erai-raws] Dragon Ball Super: Beerus - 02 [1080p CR WEB-DL AVC AAC][MultiSub]",
    ];
    let items: Vec<(i64, Item)> = titles
        .iter()
        .enumerate()
        .map(|(n, t)| {
            (
                0,
                Item {
                    guid: format!("g{n}"),
                    title: t.to_string(),
                    info_hash: Some(format!("{n:040x}")),
                    ..Item::default()
                },
            )
        })
        .collect();
    let mut e = examine(&items, &l, &HashMap::new(), &Relations::default());
    let w = World {
        list: &l,
        available: &|_, _| false,
    };
    run_filters(&mut e, &default_filters(), true, &Archive::default(), &w);
    assert_eq!(e.len(), 3);
    for x in &e {
        assert_eq!(
            (x.anime_id, x.release.episodes, x.state),
            (Some(10), Some((2, 2)), ItemState::DiscardedNormal),
            "{}",
            x.item.title
        );
        assert_eq!(x.why, "Discard watched and available episodes");
    }
}
