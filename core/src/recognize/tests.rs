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
    on_list(&parse(name).unwrap(), &list(), &None)
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
    assert_eq!(on_list(&p, &list(), &Some((4, "user".into(), String::new()))), Some(Found::List { media_id: 4, sure: true, why: "you picked it".into() }));
    assert_eq!(on_list(&p, &list(), &Some((0, "ignore".into(), String::new()))), Some(Found::Ignored));
    // A pick that left the list counts for nothing.
    assert_eq!(on_list(&p, &list(), &Some((99, "user".into(), String::new()))), None);
}

#[test]
fn search_hits_on_and_off_the_list() {
    let hit = |id, t: &str| SearchHit { id, title: t.into(), other_titles: vec![] };
    assert_eq!(from_search(&[hit(77, "Elsewhere"), hit(2, "Hoshizora Yuubin")], &list()), Found::List { media_id: 2, sure: false, why: "found by a MyAnimeList search".into() });
    assert_eq!(from_search(&[hit(77, "Elsewhere")], &list()), Found::NotOnList { media_id: 77, title: "Elsewhere".into() });
    assert_eq!(from_search(&[], &list()), Found::Nothing);
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
