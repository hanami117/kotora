//! MangaUpdates against recorded, made-up answers (`fixtures/`): no test calls MangaUpdates.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::http::Response;

const SEARCH: &str = include_str!("fixtures/search.json");
const SEARCH_AMBIGUOUS: &str = include_str!("fixtures/search-ambiguous.json");
const SEARCH_NONE: &str = include_str!("fixtures/search-none.json");
const SERIES: &str = include_str!("fixtures/series.json");
const WEEKLY: &str = include_str!("fixtures/releases-weekly.json");
const BONE: &str = include_str!("fixtures/releases-bone.json");
const NIGHT: &str = include_str!("fixtures/releases-night.json");
const VOLUMES: &str = include_str!("fixtures/releases-volumes.json");
const SERIES_WEEKLY: &str = include_str!("fixtures/series-weekly.json");
const SERIES_FROZEN: &str = include_str!("fixtures/series-frozen.json");
const FROZEN: &str = include_str!("fixtures/releases-frozen.json");
const SERIES_COMPLETE: &str = include_str!("fixtures/series-complete.json");
const COMPLETE: &str = include_str!("fixtures/releases-complete.json");
const SERIES_HIATUS: &str = include_str!("fixtures/series-hiatus.json");

fn record(body: &str) -> Series {
    parse_series(body).unwrap().2
}

/// A transport that answers from a list and keeps what was asked.
#[derive(Default)]
struct Rec {
    answers: RefCell<VecDeque<Result<Response, String>>>,
    seen: RefCell<Vec<(String, Option<String>)>>,
}

impl Transport for Rec {
    fn send(&self, req: &Request) -> Result<Response, String> {
        self.seen.borrow_mut().push((format!("{} {}", req.method, req.url), req.json.clone()));
        self.answers.borrow_mut().pop_front().unwrap_or(Err("no more answers".into()))
    }
}

fn ok(body: &str) -> Result<Response, String> {
    Ok(Response { status: 200, retry_after: None, body: body.into() })
}

fn status(n: u16) -> Result<Response, String> {
    Ok(Response { status: n, retry_after: Some(1), body: String::new() })
}

fn client(answers: Vec<Result<Response, String>>) -> Client<Rec> {
    Client::without_sleep(Rec { answers: RefCell::new(answers.into()), ..Rec::default() })
}

/// 2026-11-17 12:00 UTC, a Tuesday.
const NOW: i64 = 1_794_916_800;

fn day(s: &str) -> i64 {
    crate::time::parse_date(s).unwrap() * DAY
}

fn days(list: &[&str]) -> Vec<i64> {
    list.iter().map(|d| day(d)).collect()
}

/// Release days every `gap` days, the last `ago` days before NOW, `n` of them.
fn every(gap: i64, n: usize, ago: i64) -> Vec<i64> {
    let last = (NOW / DAY - ago) * DAY;
    (0..n as i64).rev().map(|k| last - k * gap * DAY).collect()
}

#[test]
fn numbers_in_odd_chapter_names() {
    assert_eq!(number_in("74"), Some(74.0));
    assert_eq!(number_in("79.1"), Some(79.1));
    assert_eq!(number_in("Bone 67"), Some(67.0));
    assert_eq!(number_in("Night 259"), Some(259.0));
    assert_eq!(number_in("10-12"), Some(12.0));
    assert_eq!(number_in("c.5"), Some(5.0));
    assert_eq!(number_in("12a"), Some(12.0));
    assert_eq!(number_in("Extra"), None);
    assert_eq!(number_in(""), None);
    assert_eq!(number_in("Oneshot"), None);
    assert_eq!(number_in("5."), Some(5.0));
    assert_eq!(chapter_label("74"), "Ch 74");
    assert_eq!(chapter_label("79.1"), "Ch 79.1");
    assert_eq!(chapter_label("Bone 67"), "Bone 67");
}

#[test]
fn links_to_series_ids() {
    let id = 55_099_564_912;
    assert_eq!(series_id_from_link("https://www.mangaupdates.com/series/pb8uwds/one-piece"), Some(id));
    assert_eq!(series_id_from_link("mangaupdates.com/series/pb8uwds"), Some(id));
    assert_eq!(series_id_from_link("  HTTPS://WWW.MANGAUPDATES.COM/series/PB8UWDS/x?y=1 "), Some(id));
    assert_eq!(series_id_from_link("https://api.mangaupdates.com/v1/series/55099564912"), Some(id));
    assert_eq!(series_id_from_link("55099564912"), Some(id));
    assert_eq!(series_url(id), "https://www.mangaupdates.com/series/pb8uwds");
    assert_eq!(series_id_from_link(&series_url(81_020_304_051)), Some(81_020_304_051));
    // Old-style and other links, and junk: no id.
    assert_eq!(series_id_from_link("https://www.mangaupdates.com/series.html?id=33"), None);
    assert_eq!(series_id_from_link("https://mangadex.org/title/abc"), None);
    assert_eq!(series_id_from_link("one piece"), None);
    assert_eq!(series_id_from_link("0"), None);
    assert_eq!(series_id_from_link(""), None);
}

#[test]
fn search_answers_and_titles() {
    let hits = parse_search(SEARCH).unwrap();
    assert_eq!(hits.len(), 4);
    assert_eq!(hits[0], Hit { id: 81_020_304_051, title: "Salt & Gears".into(), hit_title: "Salt & Gears".into(), kind: "Manhwa".into(), year: Some(2021) });
    assert_eq!(parse_search(SEARCH_NONE).unwrap(), []);
    assert!(parse_search("<html>").is_err());
    assert_eq!(normalize("Salt &amp; Gears!"), normalize("salt and amp gears"), "entities are unescaped before");
    assert_eq!(normalize("Salt & Gears!"), "salt and gears");
    assert_eq!(normalize("  The  Quiet—Foundry "), "the quiet foundry");
    let (id, title, s) = parse_series(SERIES).unwrap();
    assert_eq!((id, title.as_str()), (81_020_304_051, "Salt & Gears"));
    assert_eq!(s, Series { latest_chapter: Some(74), status: "74 Chapters (Ongoing)".into(), completed: false, last_updated: 0, went_up_at: 0 });
}

#[test]
fn a_clear_title_is_a_sure_match_and_types_are_checked() {
    let hits = parse_search(SEARCH).unwrap();
    let m = best_match(&["Salt & Gears".into()], Some(2021), Form::Comic, &hits).unwrap();
    assert_eq!(m, Matched { id: 81_020_304_051, title: "Salt & Gears".into(), confidence: Confidence::Sure });
    // No year on our side: still the only comic with the title.
    assert_eq!(best_match(&["salt and gears".into()], None, Form::Comic, &hits).unwrap().confidence, Confidence::Sure);
    // The novel of the same name for a novel, never the artbook.
    let n = best_match(&["Salt & Gears".into()], None, Form::Novel, &hits).unwrap();
    assert_eq!((n.id, n.confidence), (81_020_304_052, Confidence::Sure));
    // The year far off: a guess the user checks.
    assert_eq!(best_match(&["Salt & Gears".into()], Some(2012), Form::Comic, &hits).unwrap().confidence, Confidence::Guess);
    // A different title is no match, however close.
    assert_eq!(best_match(&["Salt and Gear".into()], None, Form::Comic, &hits), None);
    assert_eq!(best_match(&[], None, Form::Comic, &hits), None);
}

#[test]
fn two_series_with_one_title_need_the_year_or_the_user() {
    let hits = parse_search(SEARCH_AMBIGUOUS).unwrap();
    // The year tells them apart (the third has no year: it may be either).
    let m = best_match(&["Moth Orchestra".into()], Some(2018), Form::Comic, &hits).unwrap();
    assert_eq!((m.id, m.confidence), (900_002, Confidence::Guess), "the yearless hit could be it too");
    // Without a year: a guess, the first one.
    let m = best_match(&["Moth Orchestra".into()], None, Form::Comic, &hits).unwrap();
    assert_eq!((m.id, m.confidence), (900_001, Confidence::Guess));
    // The two with years, no yearless one: the year decides for sure.
    let two = &hits[..2];
    let m = best_match(&["Moth Orchestra".into()], Some(2009), Form::Comic, two).unwrap();
    assert_eq!((m.id, m.confidence), (900_001, Confidence::Sure));
}

#[test]
fn find_searches_the_titles_in_turn() {
    let mut c = client(vec![ok(SEARCH_NONE), ok(SEARCH)]);
    let titles = vec!["Shio to Haguruma".to_string(), "Salt & Gears".to_string()];
    let m = c.find(&titles, Some(2021), Form::Comic).unwrap().unwrap();
    assert_eq!(m.id, 81_020_304_051);
    let seen = c.transport.seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, "POST https://api.mangaupdates.com/v1/series/search");
    let body: Value = serde_json::from_str(seen[1].1.as_deref().unwrap()).unwrap();
    assert_eq!(body["search"], "Salt & Gears");
    // Nothing anywhere: none, after at most three searches (same titles asked once).
    let mut c = client(vec![ok(SEARCH_NONE), ok(SEARCH_NONE), ok(SEARCH_NONE), ok(SEARCH_NONE)]);
    let many: Vec<String> = ["A", "a", "B", "C", "D"].iter().map(|s| s.to_string()).collect();
    assert_eq!(c.find(&many, None, Form::Comic).unwrap(), None);
    assert_eq!(c.transport.seen.borrow().len(), 2, "\"a\" is \"A\"; the first three titles only");
}

#[test]
fn releases_request_and_errors() {
    let mut c = client(vec![ok(WEEKLY)]);
    let r = c.releases(81_020_304_051).unwrap();
    assert_eq!(r.len(), 13, "the release with no date at all is left out");
    let seen = c.transport.seen.borrow();
    assert_eq!(seen[0].0, "POST https://api.mangaupdates.com/v1/releases/search");
    let body: Value = serde_json::from_str(seen[0].1.as_deref().unwrap()).unwrap();
    assert_eq!((body["search"].as_str(), body["search_type"].as_str(), body["orderby"].as_str(), body["asc"].as_str()), (Some("81020304051"), Some("series"), Some("date"), Some("desc")));
    drop(seen);
    // A 429 is waited out once; a second one is an error. Offline says so.
    let mut c = client(vec![status(429), ok(BONE)]);
    assert_eq!(c.releases(1).unwrap().len(), 5);
    let mut c = client(vec![status(429), status(429)]);
    assert!(matches!(c.releases(1), Err(ServiceError::RateLimited)));
    let mut c = client(vec![Err("dns".into())]);
    assert!(matches!(c.releases(1), Err(ServiceError::Offline(_))));
    let mut c = client(vec![status(404)]);
    assert!(matches!(c.series(1), Err(ServiceError::Other(_))));
    let mut c = client(vec![ok(SERIES)]);
    assert_eq!(c.series(81_020_304_051).unwrap().1, "Salt & Gears");
    assert_eq!(c.transport.seen.borrow()[0].0, "GET https://api.mangaupdates.com/v1/series/81020304051");
}

#[test]
fn weekly_releases_with_two_groups_extras_and_a_backfill() {
    let r = parse_releases(WEEKLY).unwrap();
    let s = summarize(&r, NOW);
    assert_eq!((s.latest.as_deref(), s.latest_no), (Some("74"), Some(74.0)));
    assert_eq!(s.latest_at, Some(1_794_790_800), "time added, past the release day");
    assert_eq!(s.cadence.pace, Pace::Weekly);
    assert_eq!(s.cadence.gap_days, 7);
    // Last on Sunday the 15th: next ~Sunday the 22nd.
    assert_eq!(s.cadence.next, Some(day("2026-11-22")));
    assert!(!s.cadence.overdue);
    assert_eq!(s.recent[0], ("Ch 74".to_string(), day("2026-11-15")));
    assert_eq!(s.recent[1], ("Ch 73".to_string(), day("2026-11-08")), "the second group's 73 on the 9th doesn't count");
    assert!(s.recent.iter().all(|(c, _)| c != "Ch 1-20"), "the backfill doesn't count");
    assert_eq!(s.recent[3], ("Ch 71.1".to_string(), day("2026-10-25")), "71 and 71.1 on one day: one release day, the higher");
    assert_eq!(behind(s.latest_no, 40), Some(34));
    assert_eq!(behind(s.latest_no, 80), Some(0), "read past MangaUpdates (raws): nothing behind");
    assert_eq!(behind(Some(79.1), 79), Some(0));
    assert_eq!(behind(None, 3), None);
}

#[test]
fn named_chapters_every_two_weeks() {
    let s = summarize(&parse_releases(BONE).unwrap(), NOW);
    assert_eq!((s.latest.as_deref(), s.latest_no), (Some("Bone 67"), Some(67.0)));
    assert_eq!(s.cadence.pace, Pace::Biweekly);
    assert_eq!(s.cadence.next, Some(day("2026-11-26")));
    assert_eq!(s.recent[0].0, "Bone 67");
}

#[test]
fn a_quiet_spell_is_a_hiatus_only_when_a_status_says_so() {
    let r = parse_releases(NIGHT).unwrap();
    // Monthly, nothing since May: by the gap alone only "not tracked", never a hiatus.
    let s = summarize(&r, NOW);
    assert_eq!(s.latest.as_deref(), Some("Night 259"));
    assert_eq!((s.cadence.pace, s.stale, s.run), (Pace::Untracked, true, Run::Ongoing));
    assert_eq!(s.cadence.next, None);
    assert_eq!(s.cadence.pace.label(), "Ongoing");
    // MangaUpdates' status says hiatus: on hiatus.
    let s = summarize_with(&r, Some(&record(SERIES_HIATUS)), ListRun::Unknown, NOW);
    assert_eq!((s.cadence.pace, s.run, s.latest.as_deref()), (Pace::Hiatus, Run::Hiatus, Some("Night 259")));
    // So does the list's own status (MyAnimeList / AniList), even with no record.
    assert_eq!(summarize_with(&r, None, ListRun::Hiatus, NOW).cadence.pace, Pace::Hiatus);
    // In June it was simply monthly.
    let june = day("2026-06-02");
    let s = summarize(&r, june);
    assert_eq!((s.cadence.pace, s.cadence.next), (Pace::Monthly, Some(day("2026-06-19"))));
}

#[test]
fn volumes_only_series() {
    let s = summarize(&parse_releases(VOLUMES).unwrap(), NOW);
    assert_eq!((s.latest.as_deref(), s.latest_no), (None, None));
    assert_eq!(s.volume_only.as_deref(), Some("9"));
    assert_eq!(s.recent[0], ("Vol 9".to_string(), day("2026-09-30")));
    // Three volumes four months apart: regular, every 4 months.
    assert_eq!(s.cadence.pace, Pace::Every(125));
    assert_eq!(s.cadence.pace.label(), "Every 4 months");
}

#[test]
fn cadence_weekly_biweekly_monthly() {
    let c = cadence_of(&every(7, 10, 2), NOW);
    assert_eq!((c.pace, c.gap_days, c.overdue), (Pace::Weekly, 7, false));
    assert_eq!(c.next, Some((NOW / DAY + 5) * DAY));
    let c = cadence_of(&every(14, 8, 3), NOW);
    assert_eq!((c.pace, c.next), (Pace::Biweekly, Some((NOW / DAY + 11) * DAY)));
    let c = cadence_of(&every(21, 6, 1), NOW);
    assert_eq!(c.pace, Pace::Triweekly);
    let c = cadence_of(&every(30, 6, 10), NOW);
    assert_eq!((c.pace, c.next), (Pace::Monthly, Some((NOW / DAY + 20) * DAY)));
    // Real months: 28 to 31 days apart.
    let c = cadence_of(&days(&["2026-05-19", "2026-06-16", "2026-07-21", "2026-08-18", "2026-09-15", "2026-10-20", "2026-11-17"]), NOW);
    assert_eq!(c.pace, Pace::Monthly);
    // A weekly series that skips a week now and then is still weekly.
    let mut d = every(7, 12, 1);
    d.remove(5);
    assert_eq!(cadence_of(&d, NOW).pace, Pace::Weekly);
    // Labels.
    assert_eq!(Pace::Weekly.label(), "Weekly");
    assert_eq!(Pace::Biweekly.label(), "Every 2 weeks");
    assert_eq!(Pace::Every(10).label(), "Every 10 days");
    assert_eq!(Pace::Every(42).label(), "Every 6 weeks");
    assert_eq!(Pace::Every(60).label(), "Every 2 months");
}

#[test]
fn cadence_irregular_overdue_hiatus_and_too_few() {
    // Gaps all over the place.
    let c = cadence_of(&days(&["2026-08-01", "2026-08-04", "2026-08-20", "2026-08-23", "2026-09-30", "2026-10-02", "2026-10-29", "2026-11-12"]), NOW);
    assert!(matches!(c.pace, Pace::Irregular(_)), "{c:?}");
    assert_eq!(c.next, None, "no date for an irregular series");
    assert!(c.sort_key() < i64::MAX, "it still sorts by its usual gap");
    // Weekly, but the last one 12 days ago: overdue, no next date, not a hiatus yet.
    let c = cadence_of(&every(7, 10, 12), NOW);
    assert_eq!((c.pace, c.overdue, c.next), (Pace::Weekly, true, None));
    // Weekly, nothing for 9 weeks: hiatus (8 weeks at the least for weeklies).
    assert_eq!(cadence_of(&every(7, 10, 63), NOW).pace, Pace::Hiatus);
    assert_eq!(cadence_of(&every(7, 10, 50), NOW).pace, Pace::Weekly);
    // Monthly: four months quiet is a hiatus, two is only late.
    assert_eq!(cadence_of(&every(30, 8, 125), NOW).pace, Pace::Hiatus);
    let c = cadence_of(&every(30, 8, 60), NOW);
    assert_eq!((c.pace, c.overdue), (Pace::Monthly, true));
    // Two releases: too few to tell, unless long ago.
    assert_eq!(cadence_of(&every(7, 2, 3), NOW).pace, Pace::Unknown);
    assert_eq!(cadence_of(&every(7, 2, 200), NOW).pace, Pace::Hiatus);
    assert_eq!(cadence_of(&[], NOW).pace, Pace::Unknown);
    assert_eq!(cadence_of(&[], NOW).sort_key(), i64::MAX);
    // Hiatus and unknown sort last; sooner sorts first.
    let soon = cadence_of(&every(7, 10, 6), NOW).sort_key();
    let later = cadence_of(&every(30, 6, 1), NOW).sort_key();
    assert!(soon < later && later < cadence_of(&every(7, 10, 63), NOW).sort_key());
}

#[test]
fn the_cache_keeps_matches_and_releases() {
    let dir = std::env::temp_dir().join(format!("kotora-mu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::new(dir.clone());
    let key = Key { service: crate::service::ServiceId::Mal, media_id: 2 };
    assert_eq!(cache.get_match(key), None);
    let m = Matched { id: 7, title: "Salt & Gears".into(), confidence: Confidence::Sure };
    cache.put_match(key, Some(&m), NOW);
    assert_eq!(cache.get_match(key), Some((Some(m.clone()), NOW)));
    assert!(match_fresh(Some(&m), NOW, NOW + MATCH_TTL - 1));
    assert!(!match_fresh(Some(&m), NOW, NOW + MATCH_TTL));
    assert!(!match_fresh(None, NOW, NOW + MISSING_TTL), "not found is asked again sooner");
    cache.put_match(Key { media_id: 3, ..key }, None, NOW);
    assert_eq!(cache.get_match(Key { media_id: 3, ..key }), Some((None, NOW)));
    let r = parse_releases(BONE).unwrap();
    cache.put_releases(7, &r, NOW);
    assert_eq!(cache.get_releases(7), Some((r, NOW)));
    assert!(releases_fresh(NOW, NOW + RELEASES_TTL - 1));
    assert!(!releases_fresh(NOW, NOW + RELEASES_TTL));
    assert!(!releases_fresh(NOW + 60, NOW), "a clock set back asks again");
    assert_eq!(key.kv(), "mal:mu-link:2", "under the service: sign out forgets it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_is_searched_for_a_manga() {
    let m = crate::Media { title: "Shio to Haguruma".into(), title_en: Some("Salt & Gears".into()), synonyms: vec!["S&G".into()], title_ja: Some("塩と歯車".into()), format: "manhwa".into(), ..Default::default() };
    assert_eq!(titles_of(&m), ["Shio to Haguruma", "Salt & Gears", "S&G", "塩と歯車"]);
    assert_eq!(form_of(&m), Form::Comic);
    assert_eq!(form_of(&crate::Media { format: "light_novel".into(), ..Default::default() }), Form::Novel);
    assert_eq!(year_of(Some("2019-04-02")), Some(2019));
    assert_eq!(year_of(Some("2019")), Some(2019));
    assert_eq!(year_of(None), None);
}

fn salt() -> crate::Media {
    crate::Media { kind: crate::Kind::Manga, id: 2, title: "Shio to Haguruma".into(), title_en: Some("Salt & Gears".into()), format: "manhwa".into(), start_date: Some("2021-03-01".into()), ..Default::default() }
}

fn scratch(name: &str) -> (Cache, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kotora-mu-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (Cache::new(dir.clone()), dir)
}

#[test]
fn fetch_searches_once_then_only_releases_and_only_when_stale() {
    let (cache, dir) = scratch("fetch");
    let key = Key { service: crate::service::ServiceId::Mal, media_id: 2 };
    assert!(stale(&cache, key, None, NOW));
    assert_eq!(saved(&cache, key, None), Info::default(), "never looked");
    // First time: two searches (romaji finds nothing), then the releases.
    let mut c = client(vec![ok(SEARCH_NONE), ok(SEARCH), ok(WEEKLY), ok(SERIES_WEEKLY)]);
    let i = fetch(&mut c, &cache, key, None, &salt(), NOW, false).unwrap();
    assert_eq!(i.matched.as_ref().map(|m| (m.id, m.confidence)), Some((81_020_304_051, Confidence::Sure)));
    assert_eq!((i.fetched_at, i.releases.len()), (Some(NOW), 13));
    assert_eq!(i.series.as_ref().and_then(|s| s.latest_chapter), Some(74), "the series record is read with the releases");
    assert_eq!(c.transport.seen.borrow().len(), 4);
    assert_eq!(c.transport.seen.borrow()[3].0, "GET https://api.mangaupdates.com/v1/series/81020304051");
    assert!(!stale(&cache, key, None, NOW + 60));
    // Within 4 hours: nothing asked. The saved files say the same offline.
    let mut c = client(vec![]);
    assert_eq!(fetch(&mut c, &cache, key, None, &salt(), NOW + 3600, false).unwrap(), i);
    assert_eq!(saved(&cache, key, None), i);
    // After 4 hours: only the releases and the record.
    assert!(stale(&cache, key, None, NOW + RELEASES_TTL));
    let mut c = client(vec![ok(WEEKLY), ok(SERIES_WEEKLY)]);
    fetch(&mut c, &cache, key, None, &salt(), NOW + RELEASES_TTL, false).unwrap();
    assert!(c.transport.seen.borrow()[0].0.ends_with("/releases/search"));
    // Refresh asks for them even when fresh, never a sure match again.
    let mut c = client(vec![ok(WEEKLY), ok(SERIES_WEEKLY)]);
    fetch(&mut c, &cache, key, None, &salt(), NOW + RELEASES_TTL + 60, true).unwrap();
    assert_eq!(c.transport.seen.borrow().len(), 2);
    // Releases saved before the record was read (an older Kotora): asked again at once.
    cache.put_releases(81_020_304_051, &parse_releases(WEEKLY).unwrap(), NOW + RELEASES_TTL + 60);
    assert!(stale(&cache, key, None, NOW + RELEASES_TTL + 120));
    // Not found: saved, not searched again for 3 days.
    let other = Key { media_id: 3, ..key };
    let mut c = client(vec![ok(SEARCH_NONE), ok(SEARCH_NONE)]);
    let i = fetch(&mut c, &cache, other, None, &salt(), NOW, false).unwrap();
    assert_eq!((i.matched, i.looked), (None, true));
    assert!(!stale(&cache, other, None, NOW + MISSING_TTL - 1));
    assert!(stale(&cache, other, None, NOW + MISSING_TTL));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picked_series_wins_and_is_kept_in_the_store() {
    let (cache, dir) = scratch("picked");
    let store = crate::store::Store::open_memory().unwrap();
    let key = Key { service: crate::service::ServiceId::AniList, media_id: 4 };
    assert_eq!(picked(&store, key), None);
    let m = Matched { id: 900_002, title: "Moth Orchestra".into(), confidence: Confidence::Picked };
    set_picked(&store, key, Some(&m)).unwrap();
    assert_eq!(picked(&store, key), Some(m.clone()));
    // No search: straight to its releases.
    let mut c = client(vec![ok(NIGHT), ok(SERIES_HIATUS)]);
    let i = fetch(&mut c, &cache, key, picked(&store, key), &salt(), NOW, false).unwrap();
    assert_eq!(i.matched, Some(m.clone()));
    assert!(c.transport.seen.borrow()[0].0.ends_with("/releases/search"));
    assert_eq!(saved(&cache, key, Some(m.clone())).releases.len(), 4);
    // Reset: forgotten (the next fetch searches again).
    set_picked(&store, key, None).unwrap();
    assert_eq!(picked(&store, key), None);
    // Sign out forgets picks too.
    set_picked(&store, key, Some(&m)).unwrap();
    let mut store = store;
    store.clear_service("anilist").unwrap();
    assert_eq!(picked(&store, key), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_chapter_is_announced_only_while_new_and_unread() {
    let s = summarize(&parse_releases(WEEKLY).unwrap(), NOW);
    let added = s.latest_added;
    assert_eq!(new_chapter(&s, 73, added + 60), Some(74));
    assert_eq!(new_chapter(&s, 40, added + 3600), Some(74), "behind by many: the newest only");
    assert_eq!(new_chapter(&s, 74, added + 60), None, "read already");
    assert_eq!(new_chapter(&s, 80, added + 60), None);
    assert_eq!(new_chapter(&s, 73, added + NEW_FOR), None, "old news");
    // Without a time added, the release day counts.
    let s = summarize(&parse_releases(BONE).unwrap(), NOW);
    assert_eq!(s.latest_added, 0);
    assert_eq!(new_chapter(&s, 66, day("2026-11-12") + 3600), Some(67));
    assert_eq!(new_chapter(&s, 66, NOW), None);
    // Volumes only: no chapters to announce.
    let s = summarize(&parse_releases(VOLUMES).unwrap(), day("2026-09-30"));
    assert_eq!(new_chapter(&s, 0, day("2026-09-30") + 60), None);
    // The announcement key is apart from anime episodes.
    assert_eq!(crate::airnotes::What::Chapter(74).key(), "mch74");
}

// ------------------------------------------------- stale histories (the live test's cases)

#[test]
fn a_simulpub_froze_the_scanlation_history_the_record_has_the_latest() {
    // Groups stopped at 272 two years ago; the record says 368, ongoing (an official simulpub).
    let r = parse_releases(FROZEN).unwrap();
    let rec = record(SERIES_FROZEN);
    let s = summarize_with(&r, Some(&rec), ListRun::Publishing, NOW);
    assert_eq!((s.latest.as_deref(), s.latest_no), (Some("368"), Some(368.0)));
    assert_eq!((s.from_record, s.stale, s.run), (true, true, Run::Ongoing));
    assert_eq!(s.latest_at, None, "no date is invented for the record's chapter");
    assert_eq!((s.cadence.pace, s.cadence.next), (Pace::Untracked, None), "no estimate from a frozen history");
    assert_eq!(s.cadence.sort_key(), i64::MAX);
    assert_eq!(behind(s.latest_no, 272), Some(96), "to read from the record, not 'caught up'");
    assert_eq!(behind(s.latest_no, 368), Some(0));
    // Without the record (a failed request), the old history still never says hiatus.
    assert_eq!(summarize_with(&r, None, ListRun::Publishing, NOW).cadence.pace, Pace::Untracked);
    // The record one or two chapters ahead of fresh weekly releases: the latest from the
    // record, the weekly estimate kept.
    let w = parse_releases(WEEKLY).unwrap();
    let ahead = Series { latest_chapter: Some(75), status: "8 Volumes (Ongoing)".into(), ..Series::default() };
    let s = summarize_with(&w, Some(&ahead), ListRun::Publishing, NOW);
    assert_eq!((s.latest_no, s.from_record, s.stale, s.cadence.pace), (Some(75.0), true, false, Pace::Weekly));
}

#[test]
fn finished_series_say_finished_with_the_count_to_read() {
    // MangaUpdates' status "(Complete)", releases ending "163-170 (end)" fourteen years ago.
    let r = parse_releases(COMPLETE).unwrap();
    let s = summarize_with(&r, Some(&record(SERIES_COMPLETE)), ListRun::Unknown, NOW);
    assert_eq!((s.run, s.cadence.pace, s.cadence.pace.label()), (Run::Finished, Pace::Finished, "Finished".to_string()));
    assert_eq!((s.latest.as_deref(), s.latest_no), (Some("163-170 (end)"), Some(170.0)));
    assert_eq!(chapter_label(s.latest.as_deref().unwrap()), "Ch 163-170 (end)");
    assert_eq!(behind(s.latest_no, 120), Some(50), "still counted");
    // The list says finished (MyAnimeList "finished", AniList FINISHED): finished, whatever
    // MangaUpdates' text (a 25-chapter series whose groups stopped 8 months ago).
    let ongoing = Series { latest_chapter: Some(25), status: "3 Volumes (Ongoing)".into(), ..Series::default() };
    let short: Vec<Release> = (0..6).map(|k| Release { chapter: (25 - k).to_string(), volume: None, day: day("2026-03-10") - k as i64 * 7 * DAY, added: 0 }).collect();
    let s = summarize_with(&short, Some(&ongoing), ListRun::from_word("finished"), NOW);
    assert_eq!((s.cadence.pace, behind(s.latest_no, 25)), (Pace::Finished, Some(0)));
    // The flags and words.
    assert_eq!(run_of(Some(&Series { completed: true, ..Series::default() }), ListRun::Unknown), Run::Finished);
    assert_eq!(run_of(Some(&Series { status: "Incomplete (Ongoing)".into(), ..Series::default() }), ListRun::Unknown), Run::Ongoing);
    assert_eq!(run_of(None, ListRun::from_word("discontinued")), Run::Finished);
    assert_eq!(run_of(Some(&record(SERIES_HIATUS)), ListRun::Finished), Run::Finished, "finished wins");
    assert_eq!(ListRun::from_word("currently_publishing"), ListRun::Publishing);
}

#[test]
fn a_normal_weekly_series_is_unchanged_by_its_record() {
    let s = summarize_with(&parse_releases(WEEKLY).unwrap(), Some(&record(SERIES_WEEKLY)), ListRun::Publishing, NOW);
    assert_eq!((s.latest.as_deref(), s.from_record, s.stale), (Some("74"), false, false));
    assert_eq!((s.cadence.pace, s.cadence.next), (Pace::Weekly, Some(day("2026-11-22"))));
    assert!(s.latest_at.is_some());
}

#[test]
fn the_record_jump_never_notifies_only_a_rise_between_two_looks() {
    let r = parse_releases(FROZEN).unwrap();
    // First look: 368 in the record, the user at 360: not "new" (no release, no rise seen).
    let first = went_up(None, record(SERIES_FROZEN), NOW);
    assert_eq!(first.went_up_at, 0);
    let s = summarize_with(&r, Some(&first), ListRun::Publishing, NOW);
    assert_eq!(new_chapter(&s, 360, NOW), None);
    // Same record four hours later: still nothing.
    let same = went_up(Some(&first), record(SERIES_FROZEN), NOW + RELEASES_TTL);
    assert_eq!(same.went_up_at, 0);
    // The record goes to 369 at the next look: chapter 369, once (the announcement key does once).
    let next = went_up(Some(&same), Series { latest_chapter: Some(369), ..record(SERIES_FROZEN) }, NOW + 2 * RELEASES_TTL);
    assert_eq!(next.went_up_at, NOW + 2 * RELEASES_TTL);
    let s = summarize_with(&r, Some(&next), ListRun::Publishing, NOW + 2 * RELEASES_TTL + 60);
    assert_eq!(new_chapter(&s, 368, NOW + 2 * RELEASES_TTL + 60), Some(369));
    assert_eq!(new_chapter(&s, 369, NOW + 2 * RELEASES_TTL + 60), None, "read already");
    // The rise is kept while unchanged, and gets old.
    let later = went_up(Some(&next), Series { latest_chapter: Some(369), ..record(SERIES_FROZEN) }, NOW + 3 * RELEASES_TTL);
    assert_eq!(later.went_up_at, next.went_up_at);
    let s = summarize_with(&r, Some(&later), ListRun::Publishing, next.went_up_at + NEW_FOR);
    assert_eq!(new_chapter(&s, 368, next.went_up_at + NEW_FOR), None);
}

#[test]
fn fetch_keeps_the_record_and_its_rise() {
    let (cache, dir) = scratch("record");
    let key = Key { service: crate::service::ServiceId::Mal, media_id: 9 };
    let m = Matched { id: 70_000_000_001, title: "Goal Line Furnace".into(), confidence: Confidence::Picked };
    let mut c = client(vec![ok(FROZEN), ok(SERIES_FROZEN)]);
    let i = fetch(&mut c, &cache, key, Some(m.clone()), &salt(), NOW, false).unwrap();
    assert_eq!(i.series.as_ref().map(|s| (s.latest_chapter, s.went_up_at)), Some((Some(368), 0)));
    assert_eq!(saved(&cache, key, Some(m.clone())).series, i.series);
    // 4 hours later the record says 369: the rise is noted.
    let up = SERIES_FROZEN.replace("\"latest_chapter\": 368", "\"latest_chapter\": 369");
    let mut c = client(vec![ok(FROZEN), ok(&up)]);
    let i = fetch(&mut c, &cache, key, Some(m.clone()), &salt(), NOW + RELEASES_TTL, false).unwrap();
    assert_eq!(i.series.as_ref().map(|s| (s.latest_chapter, s.went_up_at)), Some((Some(369), NOW + RELEASES_TTL)));
    // A record that can't be read keeps the saved one; offline stops the fetch.
    let mut c = client(vec![ok(FROZEN), status(500)]);
    let i = fetch(&mut c, &cache, key, Some(m.clone()), &salt(), NOW + 2 * RELEASES_TTL, false).unwrap();
    assert_eq!(i.series.as_ref().and_then(|s| s.latest_chapter), Some(369));
    let mut c = client(vec![ok(FROZEN), Err("dns".into())]);
    assert!(matches!(fetch(&mut c, &cache, key, Some(m), &salt(), NOW + 3 * RELEASES_TTL, false), Err(ServiceError::Offline(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

// --------------------------------------------------------- two sources: MangaDex beside MangaUpdates

const MD_FEED: &str = include_str!("../mangadex/fixtures/feed-external.json");
const MD_SEARCH: &str = include_str!("../mangadex/fixtures/search-linked.json");

fn md_client(answers: Vec<Result<Response, String>>) -> crate::mangadex::Client<Rec> {
    crate::mangadex::Client::without_sleep(Rec { answers: RefCell::new(answers.into()), ..Rec::default() })
}

/// The live test's case: MangaUpdates frozen at 272 (record too), read 362; MangaDex lists the
/// official weekly chapters up to 368.
fn frozen_both() -> Info {
    let r = parse_releases(FROZEN).unwrap();
    let rec = Series { latest_chapter: Some(272), status: "33 Volumes (Ongoing)".into(), ..Series::default() };
    let md = crate::mangadex::feed_releases(&crate::mangadex::parse_feed(MD_FEED).unwrap());
    Info {
        matched: Some(Matched { id: 1, title: "Goal Line Furnace".into(), confidence: Confidence::Sure }),
        looked: true,
        releases: r,
        series: Some(rec),
        fetched_at: Some(NOW - 60),
        md: MdSide { id: Some("eeeeeeee-0000-4000-8000-000000000005".into()), title: "Goal Line Furnace".into(), looked: true, releases: md, fetched_at: Some(NOW - 60), external: true, ..MdSide::default() },
        ..Info::default()
    }
}

#[test]
fn auto_takes_the_source_with_the_higher_latest_chapter() {
    let i = frozen_both();
    let (s, used) = i.best(NOW, ListRun::Publishing, Sources::default()).unwrap();
    assert_eq!((used, s.latest.as_deref(), s.cadence.pace), (Source::MangaDex, Some("368"), Pace::Weekly));
    assert_eq!(behind(s.latest_no, 362), Some(6));
    // The user's choice wins while its source has data.
    let pinned = Info { pref: Source::MangaUpdates, ..i.clone() };
    assert_eq!(pinned.best(NOW, ListRun::Publishing, Sources::default()).unwrap().1, Source::MangaUpdates);
    // A source turned off is never used; its data stays saved.
    let (s, used) = i.best(NOW, ListRun::Publishing, Sources { mu: true, md: false }).unwrap();
    assert_eq!((used, s.latest.as_deref()), (Source::MangaUpdates, Some("272")));
    let pick_md = Info { pref: Source::MangaDex, ..i.clone() };
    assert_eq!(pick_md.best(NOW, ListRun::Publishing, Sources { mu: true, md: false }).unwrap().1, Source::MangaUpdates, "the choice falls back");
    assert_eq!(i.best(NOW, ListRun::Publishing, Sources { mu: false, md: true }).unwrap().1, Source::MangaDex);
    // Equal chapters: the dated one (releases) wins over a record's undated chapter.
    let tie = Info { series: Some(Series { latest_chapter: Some(368), ..Series::default() }), ..i.clone() };
    assert_eq!(tie.best(NOW, ListRun::Publishing, Sources::default()).unwrap().1, Source::MangaDex);
    assert_eq!(Source::from_word("md"), Source::MangaDex);
    assert_eq!(Source::from_word("nonsense"), Source::Auto);
    assert_eq!(i.checked_at(), Some(NOW - 60));
}

#[test]
fn found_and_not_found_across_sources() {
    let on = Sources::default();
    let i = Info { looked: true, md: MdSide { looked: true, ..MdSide::default() }, ..Info::default() };
    assert!(i.not_found(on));
    let half = Info { looked: true, ..Info::default() };
    assert!(!half.not_found(on), "MangaDex not looked up yet");
    assert!(half.not_found(Sources { mu: true, md: false }));
    let md_only = Info { looked: true, md: MdSide { id: Some("x".into()), looked: true, ..MdSide::default() }, ..Info::default() };
    assert!(md_only.found(on) && !md_only.found(Sources { mu: true, md: false }));
}

#[test]
fn md_fetch_links_by_id_then_reads_the_feed_and_caches() {
    let (cache, dir) = scratch("md");
    let key = Key { service: crate::service::ServiceId::Mal, media_id: 101 };
    let media = crate::Media { kind: crate::Kind::Manga, id: 101, title: "Goal Line Furnace".into(), ..Default::default() };
    assert!(md_stale(&cache, key, None, NOW));
    let mut c = md_client(vec![ok(MD_SEARCH), ok(MD_FEED)]);
    let s = md_fetch(&mut c, &cache, key, None, &media, NOW, false).unwrap();
    assert_eq!((s.id.as_deref(), s.looked, s.external, s.picked), (Some("eeeeeeee-0000-4000-8000-000000000005"), true, true, false));
    assert_eq!(s.fetched_at, Some(NOW));
    assert!(!md_stale(&cache, key, None, NOW + 60));
    assert_eq!(md_saved(&cache, key, None), s);
    // Within 4 hours nothing is asked; after, only the feed.
    let mut c = md_client(vec![]);
    assert_eq!(md_fetch(&mut c, &cache, key, None, &media, NOW + 3600, false).unwrap(), s);
    let mut c = md_client(vec![ok(MD_FEED)]);
    md_fetch(&mut c, &cache, key, None, &media, NOW + RELEASES_TTL, false).unwrap();
    // No series links to it: looked, none; asked again after 3 days.
    let other = Key { media_id: 7, ..key };
    let media7 = crate::Media { id: 7, ..media.clone() };
    let mut c = md_client(vec![ok(MD_SEARCH)]);
    let s = md_fetch(&mut c, &cache, other, None, &media7, NOW, false).unwrap();
    assert_eq!((s.id, s.looked), (None, true));
    assert!(!md_stale(&cache, other, None, NOW + MISSING_TTL - 1));
    // A picked series: no search.
    let picked = Some(("eeeeeeee-0000-4000-8000-000000000005".to_string(), "Goal Line Furnace".to_string()));
    let mut c = md_client(vec![ok(MD_FEED)]);
    let s = md_fetch(&mut c, &cache, other, picked, &media7, NOW, false).unwrap();
    assert!(s.picked && s.id.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn picks_and_the_source_choice_are_kept_per_manga() {
    let store = crate::store::Store::open_memory().unwrap();
    let key = Key { service: crate::service::ServiceId::Mal, media_id: 3 };
    assert_eq!(source_pref(&store, key), Source::Auto);
    set_source_pref(&store, key, Source::MangaDex).unwrap();
    assert_eq!(source_pref(&store, key), Source::MangaDex);
    set_source_pref(&store, key, Source::Auto).unwrap();
    assert_eq!(source_pref(&store, key), Source::Auto);
    set_md_picked(&store, key, Some(("eeeeeeee-0000-4000-8000-000000000005", "Goal Line Furnace"))).unwrap();
    assert_eq!(md_picked(&store, key).map(|p| p.1), Some("Goal Line Furnace".to_string()));
    set_md_picked(&store, key, None).unwrap();
    assert_eq!(md_picked(&store, key), None);
}
