//! Recognition tests. Every title and file name here is made up.

use super::*;
use crate::store::tests::item;
use crate::{ListItem, Status};

/// Made-up titles without numbers in them (a number in a title is its own test below).
const TITLES: [&str; 10] = [
    "Lanterns of Kirigaoka",
    "Starfall Delivery Service",
    "Kumo to Kaze no Ryokan",
    "Ghost of the Library Stacks",
    "The Ninth Shrine Maiden",
    "Harbor Lights Overtime",
    "Paper Crane Detective",
    "Mikan-iro no Sora",
    "Clockwork Tanuki",
    "Re:Bloom Academy",
];

/// Naming patterns seen in the wild, with `{T}` the title and `{E}` the two-digit episode.
const PATTERNS: [&str; 8] = [
    "[Hoshi] {T} - {E} [1080p].mkv",
    "[Hoshi] {T} - {E} (1080p) [A1B2C3D4].mkv",
    "{T} - {E}.mkv",
    "[Kumo-Subs]_{U}_-_{E}_[720p].mkv",
    "{T} E{E} [1080p].mp4",
    "[Tsuki] {T} {E} [BD 1080p HEVC FLAC].mkv",
    "{T} - {E}v2 [1080p].mkv",
    "[Hoshi] {T} - Episode {E} [1080p].mkv",
];

/// 240 names: every title, pattern and three episodes. Title and episode must come back.
#[test]
fn the_corpus_parses() {
    let mut fails = Vec::new();
    let mut n = 0;
    for t in TITLES {
        for pat in PATTERNS {
            for ep in [1u32, 6, 12] {
                let name = pat.replace("{T}", t).replace("{U}", &t.replace(' ', "_")).replace("{E}", &format!("{ep:02}"));
                n += 1;
                let p = parse(&format!("/media/anime/{name}")).unwrap_or_default();
                if normalize(&p.title) != normalize(t) || p.episode != Some(ep) {
                    fails.push(format!("{name} → {:?} ep {:?}", p.title, p.episode));
                }
            }
        }
    }
    assert!(n >= 200);
    assert!(fails.is_empty(), "{} of {n} wrong:\n{}", fails.len(), fails.join("\n"));
}

#[test]
fn seasons_groups_and_folders() {
    let p = parse("/v/[Hoshi] Starfall Delivery Service S2 - 03 [1080p].mkv").unwrap();
    assert_eq!((p.title.as_str(), p.season, p.episode), ("Starfall Delivery Service", Some(2), Some(3)));
    assert_eq!((p.group.as_deref(), p.resolution.as_deref()), (Some("Hoshi"), Some("1080p")));
    assert_eq!(p.key(), "starfall delivery service#2");

    let p = parse("/v/Starfall Delivery Service 2nd Season - 03.mkv").unwrap();
    assert_eq!((p.episode, p.key().as_str()), (Some(3), "starfall delivery service#2"));

    // No title in the name: the folder's.
    let p = parse("/media/anime/[Hoshi] Lanterns of Kirigaoka/06.mkv").unwrap();
    assert_eq!((p.title.as_str(), p.episode, p.group.as_deref()), ("Lanterns of Kirigaoka", Some(6), Some("Hoshi")));
    assert!(parse("/06.mkv").is_none());
    assert!(parse("").is_none());

    // A number in the title stays in it.
    let p = parse("/v/[Hoshi] Neon Ronin 2099 - 04 [1080p].mkv").unwrap();
    assert_eq!((p.title.as_str(), p.episode), ("Neon Ronin 2099", Some(4)));
}

#[test]
fn normalising_and_seasons_in_titles() {
    assert_eq!(normalize("Re:Bloom  Academy!"), "re bloom academy");
    assert_eq!(normalize("Kirigaoka's Lanterns"), "kirigaokas lanterns");
    assert_eq!(normalize("ほしぞら郵便"), "ほしぞら郵便");
    assert_eq!(split_season("starfall delivery service 2nd season"), ("starfall delivery service".into(), Some(2)));
    assert_eq!(split_season("starfall delivery service season 3"), ("starfall delivery service".into(), Some(3)));
    assert_eq!(split_season("starfall delivery service s2"), ("starfall delivery service".into(), Some(2)));
    assert_eq!(split_season("starfall delivery service ii"), ("starfall delivery service".into(), Some(2)));
    assert_eq!(split_season("neon ronin 2099"), ("neon ronin 2099".into(), None));
    assert_eq!(split_season("season"), ("season".into(), None));
    assert_eq!(split_season("season 2"), ("season 2".into(), None));
    assert_eq!(split_season("starfall delivery service third season"), ("starfall delivery service".into(), Some(3)));
    assert_eq!(split_season("starfall delivery service 2nd season the long night"), ("starfall delivery service the long night".into(), Some(2)));
    assert_eq!(split_season("ほしぞら郵便 第3期"), ("ほしぞら郵便".into(), Some(3)));
    assert_eq!(split_season("ほしぞら郵便第3期"), ("ほしぞら郵便".into(), Some(3)));
    assert!(similarity("lanterns of kirigaoka", "lanterns of kirigaoka") == 1.0);
    assert!(similarity("lanterns of kirigaoka", "lantern of kirigaoka") > 0.9);
    assert!(similarity("lanterns of kirigaoka", "clockwork tanuki") < 0.3);
    assert_eq!(similarity("", "a"), 0.0);
}

fn list() -> Vec<ListItem> {
    let mut a = item(1, "Lanterns of Kirigaoka", Status::Watching, 5, 0);
    a.media.episodes = Some(12);
    let mut b = item(2, "Hoshizora Yuubin", Status::Watching, 1, 0);
    b.media.title_en = Some("Starfall Delivery Service".into());
    b.media.episodes = Some(12);
    let mut c = item(3, "Hoshizora Yuubin 2nd Season", Status::PlanToWatch, 0, 0);
    c.media.title_en = Some("Starfall Delivery Service Season 2".into());
    c.media.episodes = Some(12);
    let mut d = item(4, "Clockwork Tanuki", Status::Completed, 1, 0);
    d.media.episodes = Some(1);
    vec![a, b, c, d]
}

fn found(name: &str) -> Option<Found> {
    on_list(&parse(name).unwrap(), &list(), &None, &Relations::default())
}

#[test]
fn matches_the_list_by_any_title_and_season() {
    assert_eq!(found("/v/[Hoshi] Lanterns of Kirigaoka - 06 [1080p].mkv"), Some(Found::List { media_id: 1, sure: true, why: "same title in your list".into() }));
    // The English title, season 1 and season 2.
    assert!(matches!(found("/v/Starfall Delivery Service - 02.mkv"), Some(Found::List { media_id: 2, sure: true, .. })));
    assert!(matches!(found("/v/Starfall Delivery Service S2 - 03.mkv"), Some(Found::List { media_id: 3, sure: true, .. })));
    assert!(matches!(found("/v/Hoshizora Yuubin 2nd Season - 03.mkv"), Some(Found::List { media_id: 3, sure: true, .. })));
    // A typo: close, so it asks.
    assert!(matches!(found("/v/Lantern of Kirigaoka - 06.mkv"), Some(Found::List { media_id: 1, sure: false, .. })));
    // Past the episode count: asks.
    assert!(matches!(found("/v/Lanterns of Kirigaoka - 13.mkv"), Some(Found::List { media_id: 1, sure: false, why }) if why.contains("past its 12")));
    // Nothing close.
    assert_eq!(found("/v/Paper Crane Detective - 01.mkv"), None);
}

#[test]
fn the_cache_wins_and_ignores() {
    let p = parse("/v/Paper Crane Detective - 01.mkv").unwrap();
    assert_eq!(on_list(&p, &list(), &Some((4, "user".into(), String::new())), &Relations::default()), Some(Found::List { media_id: 4, sure: true, why: "you picked it".into() }));
    assert_eq!(on_list(&p, &list(), &Some((0, "ignore".into(), String::new())), &Relations::default()), Some(Found::Ignored));
    // A pick that left the list counts for nothing.
    assert_eq!(on_list(&p, &list(), &Some((99, "user".into(), String::new())), &Relations::default()), None);
}

#[test]
fn search_hits_on_and_off_the_list() {
    let hit = |id, t: &str| SearchHit { id, title: t.into(), other_titles: vec![] };
    let p = parse("/v/Hoshi - 03.mkv").unwrap();
    assert_eq!(from_search(&p, &[hit(77, "Elsewhere"), hit(2, "Hoshizora Yuubin")], &list()), Found::List { media_id: 2, sure: false, why: "found by a MyAnimeList search".into() });
    assert_eq!(from_search(&p, &[hit(77, "Elsewhere")], &list()), Found::NotOnList { media_id: 77, title: "Elsewhere".into() });
    assert_eq!(from_search(&p, &[], &list()), Found::Nothing);
}

#[test]
fn offers_only_a_new_episode() {
    let l = list();
    assert_eq!(offer(&l[0], Some(6)), Some(6));
    assert_eq!(offer(&l[0], Some(5)), None, "already counted");
    assert_eq!(offer(&l[0], Some(13)), None, "past the count");
    assert_eq!(offer(&l[0], None), None);
    assert_eq!(offer(&l[0], Some(0)), None);
    // A movie with no number: episode 1, but it's already at 1.
    assert_eq!(offer(&l[3], None), None);
    let mut movie = l[3].clone();
    movie.progress = 0;
    assert_eq!(offer(&movie, None), Some(1));
    // Rewatching offers again.
    let mut re = l[0].clone();
    re.rewatching = true;
    assert_eq!(offer(&re, Some(3)), Some(3));
}

#[test]
fn a_record_two_or_more_ahead_skips_episodes() {
    assert_eq!(skipped(5, 6), None, "the next episode");
    assert_eq!(skipped(5, 5), None);
    assert_eq!(skipped(5, 3), None, "a rewatch going back");
    assert_eq!(skipped(5, 7), Some((6, 6)));
    assert_eq!(skipped(0, 4), Some((1, 3)));
    assert_eq!(skipped(u32::MAX, 3), None);
}

/// Upstream anitomy's own test data, when given: `KOTORA_ANITOMY_DATA=/path/to/data.json
/// cargo test -p kotora-core anitomy_upstream -- --ignored --nocapture`. Prints how many file names
/// get the title and episode upstream expects. Not in the repo (it holds real release names).
#[test]
#[ignore]
fn anitomy_upstream() {
    let Some(path) = std::env::var_os("KOTORA_ANITOMY_DATA") else { return };
    let text = std::fs::read_to_string(path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let cases = v.as_array().cloned().unwrap_or_default();
    let (mut ok, mut n) = (0, 0);
    for c in &cases {
        let Some(name) = c.get("file_name").and_then(|x| x.as_str()) else { continue };
        let want_title = c.get("anime_title").and_then(|x| x.as_str()).unwrap_or_default();
        let want_ep = c.get("episode_number").and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_array().and_then(|a| a.last()).and_then(|e| e.as_str()).map(str::to_string)));
        n += 1;
        let p = parse_name(name);
        let ep_ok = match &want_ep {
            Some(e) => p.episode == last_number(e),
            None => true,
        };
        if p.title == want_title && ep_ok {
            ok += 1;
        }
    }
    println!("anitomy upstream data: {ok} of {n} with the expected title and episode");
}

/// A made-up series of three seasons: season 1 and 2 completed, season 3 being watched. The
/// files name only the start of the title ("Yakusou") and the season.
fn series() -> Vec<ListItem> {
    let mut s1 = item(11, "Yakusou no Sasayaki", Status::Completed, 24, 0);
    s1.media.title_en = Some("The Herbalist Whispers".into());
    s1.media.episodes = Some(24);
    let mut s2 = item(12, "Yakusou no Sasayaki 2nd Season", Status::Completed, 24, 0);
    s2.media.title_en = Some("The Herbalist Whispers Season 2".into());
    s2.media.episodes = Some(24);
    let mut s3 = item(13, "Yakusou no Sasayaki: Kouhen", Status::Watching, 6, 0);
    s3.media.title_en = Some("The Herbalist Whispers Season 3".into());
    s3.media.title_ja = Some("ヤクソウノササヤキ 第3期".into());
    s3.media.synonyms = vec![];
    s3.media.episodes = Some(12);
    let mut other = list();
    other.extend([s1, s2, s3]);
    other
}

fn picked(id: u32) -> Cached {
    Some((id, "user".into(), String::new()))
}

#[test]
fn a_season_3_file_finds_the_season_3_entry() {
    let items = series();
    let p = parse("/v/[Hoshi] Yakusou - S03E07.mkv").unwrap();
    assert_eq!((p.season, p.key().as_str()), (Some(3), "yakusou#3"));
    // Only the English title and the Japanese one name season 3; the earlier seasons score 0.
    let c = candidates(&p, &items);
    assert_eq!(c[0].media_id, 13);
    assert!(c.iter().all(|x| x.media_id == 13 || x.score < UNSURE_AT), "{c:?}");
    let f = on_list(&p, &items, &None, &Relations::default());
    assert_eq!(f, Some(Found::List { media_id: 13, sure: true, why: "season 3 of a title in your list that starts with \u{201c}Yakusou\u{201d}".into() }));
    assert_eq!(offer(&items.iter().find(|i| i.media.id == 13).unwrap().clone(), p.episode), Some(7));
    // The full title with the season works the same; season 2 is the 2nd Season.
    let p2 = parse("/v/The Herbalist Whispers S2 - 05.mkv").unwrap();
    assert!(matches!(on_list(&p2, &items, &None, &Relations::default()), Some(Found::List { media_id: 12, sure: true, .. })));
}

#[test]
fn a_pick_of_season_1_does_not_take_a_season_3_file() {
    let items = series();
    let p = parse("/v/[Hoshi] Yakusou - S03E07.mkv").unwrap();
    // An old pick for this very key ("yakusou#3") of the completed season 1.
    let f = on_list(&p, &items, &picked(11), &Relations::default());
    assert!(
        matches!(&f, Some(Found::List { media_id: 13, sure: true, why }) if why.contains("earlier pick, Yakusou no Sasayaki, is another season")),
        "{f:?}"
    );
    // A pick for season 1 is kept under season 1's key: never looked up for this file.
    assert_ne!(parse("/v/[Hoshi] Yakusou - 07.mkv").unwrap().key(), p.key());
}

#[test]
fn a_pick_applies_to_its_season_only() {
    let items = series();
    let rel = Relations::default();
    // A pick of the season 3 entry for "yakusou#3" applies to season 3 files.
    let s3 = parse("/v/[Hoshi] Yakusou - S03E08.mkv").unwrap();
    assert_eq!(on_list(&s3, &items, &picked(13), &rel), Some(Found::List { media_id: 13, sure: true, why: "you picked it".into() }));
    // Season 1 files have their own key, so the cache has nothing for them: season 1 matches.
    let s1 = parse("/v/[Hoshi] Yakusou - S01E08.mkv").unwrap();
    assert_eq!(s1.key(), "yakusou#1");
    assert!(matches!(on_list(&s1, &items, &None, &rel), Some(Found::List { media_id: 11, .. })));
    // A pick of another season than the file's (no season 4 on the list here) waits for the
    // search: a hit naming season 4 beats it, and with none it is the user's call, kept.
    let s4 = parse("/v/[Hoshi] Yakusou - S04E01.mkv").unwrap();
    assert_eq!(on_list(&s4, &items, &picked(13), &rel), None);
    let hit = |id, t: &str| SearchHit { id, title: t.into(), other_titles: vec![] };
    let pick = from_search_picked(&s4, &[hit(11, "Yakusou no Sasayaki"), hit(91, "Yakusou no Sasayaki Movie")], &items, Some(13));
    assert_eq!(pick, Found::List { media_id: 13, sure: true, why: "you picked it".into() });
    let s4_hit = from_search_picked(&s4, &[hit(90, "Yakusou no Sasayaki 4th Season")], &items, Some(13));
    assert_eq!(s4_hit, Found::NotOnList { media_id: 90, title: "Yakusou no Sasayaki 4th Season".into() });
}

#[test]
fn files_without_a_season_keep_the_old_pick() {
    let items = series();
    let rel = Relations::default();
    // Picks from before this fix were saved as "title#1" for files without a season: they still
    // apply to such files, even a pick of season 3 (numbered on, the user's call).
    let p = parse("/v/[Hoshi] Yakusou - 07.mkv").unwrap();
    assert_eq!((p.season, p.key().as_str()), (None, "yakusou#1"));
    assert_eq!(on_list(&p, &items, &picked(13), &rel), Some(Found::List { media_id: 13, sure: true, why: "you picked it".into() }));
    assert_eq!(on_list(&p, &items, &picked(11), &rel), Some(Found::List { media_id: 11, sure: true, why: "you picked it".into() }));
    // Without a pick, a short title and no season: season 1, but only unsure.
    assert!(matches!(on_list(&p, &items, &None, &rel), Some(Found::List { media_id: 11, sure: false, .. })));
}

#[test]
fn a_search_prefers_the_files_season() {
    let items = series();
    let hit = |id, t: &str| SearchHit { id, title: t.into(), other_titles: vec![] };
    let p = parse("/v/[Hoshi] Yakusou - S04E01.mkv").unwrap();
    // Season 1 is on the list but season 4 is the file's: the hit off the list naming it wins.
    let hits = [hit(11, "Yakusou no Sasayaki"), hit(90, "Yakusou no Sasayaki 4th Season")];
    assert_eq!(from_search(&p, &hits, &items), Found::NotOnList { media_id: 90, title: "Yakusou no Sasayaki 4th Season".into() });
    // Nothing names season 4: the list's entry, unsure, saying why.
    let f = from_search(&p, &[hit(11, "Yakusou no Sasayaki"), hit(91, "Yakusou no Sasayaki Movie")], &items);
    assert_eq!(f, Found::List { media_id: 11, sure: false, why: "found by a MyAnimeList search, but the file says season 4".into() });
}

/// (name, title, season, episodes, batch, version).
type ReleaseRow = (&'static str, &'static str, Option<u32>, Option<(u32, u32)>, bool, Option<u32>);

/// Release names as feeds give them (made up).
#[test]
fn release_names() {
    let table: [ReleaseRow; 12] = [
        ("[Hoshi] Lanterns of Kirigaoka - 08 [1080p][A1B2C3D4].mkv", "Lanterns of Kirigaoka", None, Some((8, 8)), false, None),
        ("[Hoshi] Lanterns of Kirigaoka - 08v2 (1080p HEVC 10bit) [WEB-DL]", "Lanterns of Kirigaoka", None, Some((8, 8)), false, Some(2)),
        ("[Kumo-Subs] Starfall Delivery Service (01-12) [BD 1080p x265 FLAC] [Batch]", "Starfall Delivery Service", None, Some((1, 12)), true, None),
        ("[Tsuki] Starfall Delivery Service S2 - 05 [720p] [ENG SUB]", "Starfall Delivery Service", Some(2), Some((5, 5)), false, None),
        ("Paper Crane Detective S02 1080p WEB-DL AAC2.0 H.264-Nami", "Paper Crane Detective", Some(2), None, true, None),
        ("[Hoshi] Clockwork Tanuki - 12 END [1080p]", "Clockwork Tanuki", None, Some((12, 12)), false, None),
        ("[Hoshi] Fate/Lantern Night - 03 [1080p]", "Fate/Lantern Night", None, Some((3, 3)), false, None),
        ("[Hoshi] Mikan-iro no Sora - Movie [1080p AV1]", "Mikan-iro no Sora", None, None, false, None),
        ("[Hoshi] Harbor Lights Overtime (Complete) [1080p]", "Harbor Lights Overtime", None, None, true, None),
        ("[Hoshi] Ghost of the Library Stacks - 01 [1080p][Multiple Subtitle][ENG][FRE]", "Ghost of the Library Stacks", None, Some((1, 1)), false, None),
        ("Re:Bloom Academy S01E07 1080p WEB H264-Grp", "Re:Bloom Academy", Some(1), Some((7, 7)), false, None),
        ("[Hoshi] Lanterns of Kirigaoka - 08 [1080p].torrent", "Lanterns of Kirigaoka", None, Some((8, 8)), false, None),
    ];
    for (name, title, season, eps, batch, version) in table {
        let r = parse_release(name);
        assert_eq!((r.parsed.title.as_str(), r.parsed.season, r.episodes, r.batch, r.version), (title, season, eps, batch, version), "{name}");
        assert_eq!(r.parsed.episode, eps.map(|e| e.1), "{name}");
    }
}

#[test]
fn release_details() {
    let r = parse_release("[Hoshi] Lanterns of Kirigaoka - 08v2 (1080p HEVC 10bit) [WEB-DL]");
    assert_eq!((r.codec, r.ten_bit, r.source.as_deref(), r.height()), (Some(Codec::Hevc), true, Some("WEB-DL"), Some(1080)));
    assert_eq!(r.parsed.group.as_deref(), Some("Hoshi"));
    let r = parse_release("[Kumo-Subs] Starfall Delivery Service (01-12) [BD 1080p x265 FLAC] [Batch]");
    assert!(r.covers(1) && r.covers(12) && !r.covers(13));
    assert_eq!((r.codec, r.audio.as_slice()), (Some(Codec::Hevc), &["FLAC".to_string()][..]));
    let r = parse_release("Paper Crane Detective S02 1080p WEB-DL AAC2.0 H.264-Nami");
    assert_eq!((r.codec, r.parsed.group.as_deref(), r.parsed.key().as_str()), (Some(Codec::Avc), Some("Nami"), "paper crane detective#2"));
    assert!(parse_release("[Hoshi] Clockwork Tanuki - 12 END [1080p]").last);
    assert!(!parse_release("[Hoshi] Clockwork Tanuki - 11 [1080p]").last);
    let r = parse_release("[Hoshi] Mikan-iro no Sora - Movie [1080p AV1]");
    assert!(r.movie && !r.batch);
    assert_eq!(r.codec, Some(Codec::Av1));
    let r = parse_release("[Hoshi] Ghost of the Library Stacks - 01 [1080p][Multiple Subtitle][ENG][FRE]");
    assert_eq!(r.languages, ["ENG", "FRE"]);
    assert_eq!(parse_release("[Hoshi] Lanterns of Kirigaoka - 08 [1080p][a1b2c3d4]").checksum.as_deref(), Some("A1B2C3D4"));
    // The same key as the file, so a pick made for the file holds for the feed.
    let file = parse("/v/[Hoshi] Lanterns of Kirigaoka - 08 [1080p].mkv").unwrap();
    assert_eq!(parse_release("[Other] Lanterns of Kirigaoka - 09 [720p]").parsed.key(), file.key());
}
