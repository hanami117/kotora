//! anime-relations tests. The Re:Zero rules and ids are the real ones from the file (public
//! data); the other titles and ids are made up.

use super::*;
use crate::recognize::{Cached, Found, from_search_picked, on_list, parse, redirect};
use crate::service::SearchHit;
use crate::store::tests::item;
use crate::{ListItem, Status};

const HEAD: &str = "# comment\n::meta\n- version: 1.3.0\n- last_modified: 2026-01-02\n\n::rules\n";

fn rel(rules: &str) -> Relations {
    Relations::parse(&format!("{HEAD}{rules}")).unwrap()
}

#[test]
fn reads_meta_ids_ranges_and_titles() {
    let r = rel("# Starfall Delivery Service -> ~ 2nd Season\n- 9001|501|7001:13-24 -> 9002|502|7002:1-12!\n");
    assert_eq!((r.version.as_str(), r.last_modified.as_str(), r.skipped), ("1.3.0", "2026-01-02", 0));
    assert_eq!(r.rules.len(), 2, "! adds the destination's own rule");
    let a = &r.rules[0];
    assert_eq!(a.from, Ids { mal: Some(9001), kitsu: Some(501), anilist: Some(7001) });
    assert_eq!((a.from_eps, a.to_eps), (Range { first: 13, last: Some(24) }, Range { first: 1, last: Some(12) }));
    assert_eq!((a.from_title.as_deref(), a.to_title.as_deref()), (Some("Starfall Delivery Service"), Some("Starfall Delivery Service 2nd Season")));
    let b = &r.rules[1];
    assert_eq!((b.from.mal, b.to.mal, b.from_eps.first), (Some(9002), Some(9002), 13));
    assert_eq!(b.from_title.as_deref(), Some("Starfall Delivery Service 2nd Season"));
    assert_eq!(r.mal(9001, 13).map(|x| (x.to, x.episode)), Some((9002, 1)));
    assert_eq!(r.mal(9001, 24).map(|x| (x.to, x.episode)), Some((9002, 12)));
    assert_eq!(r.mal(9002, 15).map(|x| (x.to, x.episode)), Some((9002, 3)));
    assert_eq!(r.mal(9001, 12), None, "inside the first season");
    assert_eq!(r.mal(9001, 25), None, "past the rule");
    assert_eq!(r.mal(9002, 3), None, "the second season's own numbering");
}

#[test]
fn unknowns_tildes_open_ranges_and_many_to_one() {
    let r = rel(concat!(
        "- 9010|?|?:0 -> ~|~|~:1\n",                   // episode 0 is episode 1 of the same
        "- 9011|511|?:171-? -> 9012|512|?:1-?!\n",     // open range
        "- 9013|?|?:1-4 -> ~|?|?:1\n",                 // four parts, one episode
        "- ?|?|7020:14-25 -> ?|?|7021:1-12!\n",        // AniList only: not for MAL
        "- 9014|?|?:5-8 -> ~:1-4\n",                   // a bare ~ for all ids
    ));
    assert_eq!(r.skipped, 0);
    assert_eq!(r.rules[0].to, Ids { mal: Some(9010), kitsu: None, anilist: None });
    assert_eq!(r.mal(9010, 0).map(|x| (x.to, x.episode)), Some((9010, 1)));
    assert_eq!(r.mal(9011, 200).map(|x| (x.to, x.episode)), Some((9012, 30)));
    assert_eq!(r.mal(9012, 171).map(|x| (x.to, x.episode)), Some((9012, 1)));
    assert_eq!(r.mal(9013, 3).map(|x| (x.to, x.episode)), Some((9013, 1)));
    assert_eq!(r.mal(9013, 1), None, "the same episode: nothing moves");
    assert_eq!(r.mal(9014, 6).map(|x| (x.to, x.episode)), Some((9014, 2)));
    assert_eq!(r.rules.iter().filter(|x| x.from.anilist == Some(7020)).count(), 1);
    assert_eq!(r.mal_rules(), 5, "the AniList-only rule has no MAL ids");
}

#[test]
fn bad_lines_are_skipped_and_bad_files_refused() {
    let r = rel("- 1|2|3:14-25 -> nonsense\n- 1|2:1 -> 4|5|6:1\n- 1|2|3:9-2 -> 4|5|6:1\njunk\n- 9001|?|?:13-24 -> 9002|?|?:1-12\n");
    assert_eq!((r.skipped, r.rules.len()), (4, 1));
    assert!(Relations::parse("::meta\n- version: 1.0.0\n").is_err(), "no rules section");
    assert!(Relations::parse("::meta\n- version: 2.0.0\n::rules\n").is_err(), "a version we don't know");
    assert!(Relations::parse("").is_err());
    // A blank line ends a comment's titles.
    let r = rel("# A -> B\n\n- 9001|?|?:13-24 -> 9002|?|?:1-12\n");
    assert_eq!(r.rules[0].to_title, None);
}

/// The built-in copy reads completely and is quick to read (once per run).
#[test]
fn the_bundled_file_reads() {
    let t = std::time::Instant::now();
    let r = Relations::bundled();
    let took = t.elapsed();
    assert_eq!(r.skipped, 0, "every rule line reads");
    assert!(r.version.starts_with("1.") && !r.last_modified.is_empty());
    assert!(r.mal_rules() > 500, "{} MAL rules", r.mal_rules());
    assert!(took.as_millis() < 200, "{took:?}");
    println!("anime-relations: {} rules ({} for MAL) read in {took:?}", r.rules.len(), r.mal_rules());
}

// The real Re:Zero rules (anime-relations.txt, last_modified 2026-10-19).
const REZERO: &str = "\
# Re:Zero kara Hajimeru Isekai Seikatsu -> ~ 2nd Season
- 31240|11209|21355:26-38 -> 39587|42198|108632:1-13!
# Re:Zero kara Hajimeru Isekai Seikatsu -> ~ 2nd Season Part 2
- 31240|11209|21355:39-50 -> 42203|43247|119661:1-12!
# Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season -> ~ Part 2
- 39587|42198|108632:14-25 -> 42203|43247|119661:1-12!
# Re:Zero kara Hajimeru Isekai Seikatsu -> ~ 3rd Season
- 31240|11209|21355:51-66 -> 54857|47235|163134:1-16!
";

fn rz(id: u32, title: &str, eps: u32, progress: u32) -> ListItem {
    let mut it = item(id, title, Status::Watching, progress, 0);
    it.media.title_en = None;
    it.media.synonyms = vec![];
    it.media.episodes = Some(eps);
    it
}

fn rezero_list(with_part2: bool) -> Vec<ListItem> {
    let mut v = vec![
        rz(31240, "Re:Zero kara Hajimeru Isekai Seikatsu", 25, 25),
        rz(39587, "Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season", 13, 13),
        rz(54857, "Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season", 16, 4),
    ];
    if with_part2 {
        v.push(rz(42203, "Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season Part 2", 12, 1));
    }
    v
}

/// The whole path: parse, match the list, redirect.
fn recognise(name: &str, items: &[ListItem], r: &Relations) -> (Found, Option<u32>) {
    let mut p = parse(name).unwrap();
    let found = on_list(&p, items, &None, r).unwrap_or(Found::Nothing);
    let found = redirect(found, &mut p, items, r);
    (found, p.episode)
}

#[test]
fn rezero_season_2_numbered_1_to_25() {
    let r = rel(REZERO);
    let items = rezero_list(true);
    // Episodes 1-13 are the 2nd Season's own.
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu S2 - 13 [1080p].mkv", &items, &r);
    assert_eq!((f, ep), (Found::List { media_id: 39587, sure: true, why: "the title and season 2 match your list".into() }, Some(13)));
    // Episode 15 is episode 2 of Part 2, and still sure.
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu S2 - 15 [1080p].mkv", &items, &r);
    assert_eq!(ep, Some(2));
    assert_eq!(
        f,
        Found::List { media_id: 42203, sure: true, why: "episode 15 of Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season is episode 2 of Part 2".into() }
    );
    let (f, ep) = recognise("/v/Re Zero kara Hajimeru Isekai Seikatsu 2nd Season - 25.mkv", &items, &r);
    assert!(matches!(f, Found::List { media_id: 42203, sure: true, .. }) && ep == Some(12));
    // Part 2 not on the list: still found, as "not on your list", with the title from the file.
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu S2 - 15 [1080p].mkv", &rezero_list(false), &r);
    assert_eq!((f, ep), (Found::NotOnList { media_id: 42203, title: "Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season Part 2".into() }, Some(2)));
    // Without the rules, episode 15 is past the 2nd Season's 13: unsure, nothing moves.
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu S2 - 15 [1080p].mkv", &items, &Relations::default());
    assert!(matches!(f, Found::List { media_id: 39587, sure: false, .. }) && ep == Some(15));
}

#[test]
fn rezero_absolute_numbers_and_season_3() {
    let r = rel(REZERO);
    let items = rezero_list(true);
    // Numbered on from the first season.
    let (f, ep) = recognise("/v/Re Zero kara Hajimeru Isekai Seikatsu - 30.mkv", &items, &r);
    assert_eq!((f, ep), (Found::List { media_id: 39587, sure: true, why: "episode 30 of Re:Zero kara Hajimeru Isekai Seikatsu is episode 5 of 2nd Season".into() }, Some(5)));
    // A season 3 release, numbered by its own episodes or on from season 1.
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu (Season 03) - 05 [1080p].mkv", &items, &r);
    assert_eq!((f, ep), (Found::List { media_id: 54857, sure: true, why: "the title and season 3 match your list".into() }, Some(5)));
    for name in ["/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu S03E05 [1080p].mkv", "/v/[Hoshi] Re Zero (Season 03)/Re Zero kara Hajimeru Isekai Seikatsu - S03E05 [1080p].mkv"] {
        let (f, ep) = recognise(name, &items, &r);
        assert!(matches!(f, Found::List { media_id: 54857, sure: true, .. }) && ep == Some(5), "{name}: {f:?} {ep:?}");
    }
    let (f, ep) = recognise("/v/[Hoshi] Re Zero kara Hajimeru Isekai Seikatsu (Season 03) - 55 [1080p].mkv", &items, &r);
    assert_eq!(ep, Some(5));
    assert!(matches!(&f, Found::List { media_id: 54857, sure: true, why } if why.contains("is its episode 5")), "{f:?}");
}

/// "S2E03" and absolute "15" are the same episode when the first season has 12.
#[test]
fn season_episode_and_absolute_numbering_agree() {
    let r = rel("# Lanterns of Kirigaoka -> ~ 2nd Season\n- 9101|?|?:13-24 -> 9102|?|?:1-12!\n");
    let mut a = rz(9101, "Lanterns of Kirigaoka", 12, 12);
    a.media.title_en = Some("Lanterns of Kirigaoka".into());
    let b = rz(9102, "Lanterns of Kirigaoka 2nd Season", 12, 2);
    let items = vec![a, b];
    for name in ["/v/Lanterns of Kirigaoka S02E03 [1080p].mkv", "/v/[Hoshi] Lanterns of Kirigaoka - 15 [1080p].mkv", "/v/Lanterns of Kirigaoka S2 - 15.mkv"] {
        let (f, ep) = recognise(name, &items, &r);
        assert!(matches!(f, Found::List { media_id: 9102, sure: true, .. }) && ep == Some(3), "{name}: {f:?} {ep:?}");
    }
}

#[test]
fn a_users_pick_and_a_search_hit_are_redirected_too() {
    let r = rel(REZERO);
    let items = rezero_list(true);
    let mut p = parse("/v/Rezero S2 - 16.mkv").unwrap();
    let f = redirect(Found::List { media_id: 39587, sure: true, why: "you picked it".into() }, &mut p, &items, &r);
    assert!(matches!(&f, Found::List { media_id: 42203, sure: true, why } if why.starts_with("you picked it; episode 16")), "{f:?}");
    let mut p = parse("/v/Rezero S2 - 16.mkv").unwrap();
    let f = redirect(Found::NotOnList { media_id: 39587, title: "Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season".into() }, &mut p, &items, &r);
    assert!(matches!(f, Found::List { media_id: 42203, sure: false, .. }) && p.episode == Some(3));
    // Nothing to redirect.
    let mut p = parse("/v/Rezero.mkv").unwrap();
    assert_eq!(redirect(Found::Nothing, &mut p, &items, &r), Found::Nothing);
}

/// The file from the live report: "[Anime Time] Re Zero Season 03 - 01.mkv", with only the
/// first two seasons on the list (season 1 completed) and an old pick of season 1 saved for this
/// very key ("re zero#3"). MyAnimeList and AniList ids: the same rules, the same answers.
#[test]
fn rezero_season_3_beats_an_old_pick_of_season_1() {
    let file = "/v/[Anime Time] Re Zero Season 03 - 01.mkv";
    let p = parse(file).unwrap();
    assert_eq!((p.title.as_str(), p.season, p.episode, p.group.as_deref()), ("Re Zero", Some(3), Some(1), Some("Anime Time")));
    assert_eq!(p.key(), "re zero#3");
    let hit = |id, t: &str| SearchHit { id, title: t.into(), other_titles: vec![] };
    for (anilist, s1, s2, s3) in [(false, 31240, 39587, 54857), (true, 21355, 108632, 163134)] {
        let mut r = rel(REZERO);
        r.anilist = anilist;
        let mut first = rz(s1, "Re:Zero kara Hajimeru Isekai Seikatsu", 25, 25);
        first.status = Status::Completed;
        first.media.title_en = Some("Re:ZERO -Starting Life in Another World-".into());
        first.media.synonyms = vec!["ReZero".into()];
        let mut items = vec![first, rz(s2, "Re:Zero kara Hajimeru Isekai Seikatsu 2nd Season", 13, 13)];
        let pick: Cached = Some((s1, "user".into(), String::new()));
        // The file names season 3 and season 1 is not it: the pick doesn't answer for the file.
        assert_eq!(on_list(&p, &items, &pick, &r), None, "anilist {anilist}");
        // The search finds season 3 off the list: "not on your list", so it can be added.
        let hits = [hit(s1, "Re:Zero kara Hajimeru Isekai Seikatsu"), hit(s3, "Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season")];
        assert_eq!(from_search_picked(&p, &hits, &items, Some(s1)), Found::NotOnList { media_id: s3, title: "Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season".into() });
        // Nothing found names season 3: the pick stands (the user's call).
        let hits = [hit(s1, "Re:Zero kara Hajimeru Isekai Seikatsu"), hit(99_999, "Re:Zero Movie")];
        assert_eq!(from_search_picked(&p, &hits, &items, Some(s1)), Found::List { media_id: s1, sure: true, why: "you picked it".into() });
        // Season 3 on the list: it wins, sure, and says the pick was passed over.
        items.push(rz(s3, "Re:Zero kara Hajimeru Isekai Seikatsu 3rd Season", 16, 0));
        let f = on_list(&p, &items, &pick, &r);
        assert!(matches!(&f, Some(Found::List { media_id, sure: true, why }) if *media_id == s3 && why.contains("earlier pick")), "{f:?}");
        // A file without a season keeps its pick (saved under "re zero#1").
        let plain = parse("/v/[Anime Time] Re Zero - 05.mkv").unwrap();
        assert_eq!(plain.key(), "re zero#1");
        assert_eq!(on_list(&plain, &items, &pick, &r), Some(Found::List { media_id: s1, sure: true, why: "you picked it".into() }));
    }
}

// ------------------------------------------------------------------------- the downloaded copy

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kotora-relations-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn newer_file() -> String {
    // The built-in rules with a later date (and one rule of our own).
    BUNDLED.replace(&format!("last_modified: {}", Relations::bundled().last_modified), "last_modified: 2999-01-01") + "\n- 9901|?|?:13-24 -> 9902|?|?:1-12\n"
}

#[test]
fn refresh_once_a_week_keeps_the_last_good_copy() {
    let dir = scratch("refresh");
    let t0 = 1_800_000_000;
    // Nothing downloaded yet: the built-in copy.
    assert_eq!(load(&dir).last_modified, Relations::bundled().last_modified);
    assert!(due(&dir, t0));
    // A newer file comes; it is saved with its tag and loaded.
    let got = refresh(&dir, t0, |etag| {
        assert_eq!(etag, None);
        Ok(Fetched::New { body: newer_file(), etag: Some("\"abc\"".into()) })
    })
    .unwrap();
    assert!(got.is_some());
    assert_eq!(load(&dir).last_modified, "2999-01-01");
    assert!(load(&dir).mal(9901, 14).is_some());
    // Not again within the week.
    assert!(!due(&dir, t0 + REFRESH_SECS - 1));
    assert!(refresh(&dir, t0 + 60, |_| panic!("no request within a week")).unwrap().is_none());
    // A week on: the tag goes along; 304 keeps the copy.
    let t1 = t0 + REFRESH_SECS;
    assert!(refresh(&dir, t1, |etag| {
        assert_eq!(etag, Some("\"abc\""));
        Ok(Fetched::Same)
    })
    .unwrap()
    .is_none());
    assert!(!due(&dir, t1 + 1));
    // A broken file is refused and the last good copy stays.
    let t2 = t1 + REFRESH_SECS;
    assert!(refresh(&dir, t2, |_| Ok(Fetched::New { body: "<html>rate limited</html>".into(), etag: None })).is_err());
    assert!(refresh(&dir, t2 + REFRESH_SECS, |_| Ok(Fetched::New { body: format!("{HEAD}- 1|?|?:2-3 -> 4|?|?:1-2\n"), etag: None })).is_err(), "too few rules");
    assert_eq!(load(&dir).last_modified, "2999-01-01");
    // No answer at all: tried again a day later, not a week.
    let t3 = t2 + 3 * REFRESH_SECS;
    assert!(refresh(&dir, t3, |_| Err("offline".into())).is_err());
    assert!(!due(&dir, t3 + RETRY_SECS - 1) && due(&dir, t3 + RETRY_SECS));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_older_or_broken_download_loses_to_the_built_in_copy() {
    let dir = scratch("older");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(cache_file(&dir), BUNDLED.replace(&Relations::bundled().last_modified, "2000-01-01")).unwrap();
    assert_eq!(load(&dir).last_modified, Relations::bundled().last_modified);
    std::fs::write(cache_file(&dir), "garbage").unwrap();
    assert_eq!(load(&dir).last_modified, Relations::bundled().last_modified);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_anilist_active_the_rules_use_anilist_ids() {
    // Made-up ids: MAL 9101 -> 9102, AniList 170101 -> 170102; MAL 170101 is another anime.
    let mut r = rel("- 9101|?|170101:13-24 -> 9102|?|170102:1-12\n- 170101|?|?:1 -> 170109|?|?:5\n");
    assert_eq!(r.redirect(9101, 14).map(|x| (x.to, x.episode)), Some((9102, 2)));
    assert_eq!(r.redirect(170101, 1).map(|x| x.to), Some(170109), "MyAnimeList's own rule");
    r.anilist = true;
    assert_eq!(r.redirect(170101, 14).map(|x| (x.to, x.episode)), Some((170102, 2)));
    assert_eq!(r.redirect(170101, 1), None, "a MyAnimeList id that happens to match is not used");
    assert_eq!(r.redirect(9101, 14), None);
}
