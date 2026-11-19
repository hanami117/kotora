//! MangaDex against recorded, made-up answers (`fixtures/`): no test calls MangaDex.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::http::Response;

const SEARCH: &str = include_str!("fixtures/search.json");
const AGGREGATE: &str = include_str!("fixtures/aggregate.json");

/// A transport that answers from a list and keeps the asked URLs.
#[derive(Default)]
struct Rec {
    answers: RefCell<VecDeque<Result<Response, String>>>,
    seen: RefCell<Vec<String>>,
}

impl Transport for Rec {
    fn send(&self, req: &Request) -> Result<Response, String> {
        self.seen.borrow_mut().push(req.url.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err("no more answers".into()))
    }
}

fn ok(body: &str) -> Result<Response, String> {
    Ok(Response {
        status: 200,
        retry_after: None,
        body: body.into(),
    })
}

fn status(n: u16) -> Result<Response, String> {
    Ok(Response {
        status: n,
        retry_after: Some(1),
        body: String::new(),
    })
}

fn client(answers: Vec<Result<Response, String>>) -> Client<Rec> {
    Client::without_sleep(Rec {
        answers: RefCell::new(answers.into()),
        ..Rec::default()
    })
}

fn table() -> Table {
    Table {
        md_id: "x".into(),
        title: "Kami Chouchin".into(),
        volumes: parse_aggregate(AGGREGATE).unwrap(),
    }
}

#[test]
fn the_volume_table_skips_none_and_words_and_keeps_decimals() {
    let vols = parse_aggregate(AGGREGATE).unwrap();
    let got: Vec<(f64, f64)> = vols.iter().map(|v| (v.volume, v.last_chapter)).collect();
    // "none", "Extra" and volume 6 (its only chapter is "Bonus") are left out; volume 2's
    // "none" chapter too; chapters as an array (volume 5) are read; 10.5 lives in volume 3.
    assert_eq!(
        got,
        [
            (1.0, 5.0),
            (2.0, 10.0),
            (3.0, 12.0),
            (5.0, 24.0),
            (5.5, 24.5)
        ]
    );
    // No volumes at all: an empty array.
    assert!(
        parse_aggregate(r#"{"result":"ok","volumes":[]}"#)
            .unwrap()
            .is_empty()
    );
    assert!(parse_aggregate("not json").is_err());
}

/// Volumes read = the highest volume whose last chapter is read; gaps and specials don't break it.
#[test]
fn volumes_from_chapters_read() {
    let t = table();
    let v = |ch: u32| volumes_for(&t, ch);
    assert_eq!(
        (v(0), v(4), v(5), v(9), v(10)),
        (0, 0, 1, 1, 2),
        "a volume counts once its last chapter is read"
    );
    assert_eq!(
        (v(11), v(12), v(23)),
        (2, 3, 3),
        "10.5 is in volume 3: chapter 12 ends it"
    );
    assert_eq!(
        v(24),
        5,
        "volume 4 is missing on MangaDex: chapter 24 ends volume 5"
    );
    assert_eq!(v(25), 5, "volume 5.5 (24.5) counts as 5");
    assert_eq!(v(500), 5, "past the table: its last volume");
    assert_eq!(
        volumes_for(
            &Table {
                md_id: String::new(),
                title: String::new(),
                volumes: vec![]
            },
            40
        ),
        0
    );
}

#[test]
fn search_reads_links_and_lookup_matches_the_mal_or_anilist_id() {
    let hits = parse_search(SEARCH).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(
        (hits[1].mal, hits[1].al, hits[1].title.as_str()),
        (Some(79001), Some(190001), "Kami Chouchin")
    );
    assert_eq!((hits[2].mal, hits[2].al), (None, None), "links: null");
    // MyAnimeList id: the second hit (the first links elsewhere), then its aggregate.
    let mut c = client(vec![ok(SEARCH), ok(AGGREGATE)]);
    let found = c
        .lookup(Key::Mal(79001), &["Kami Chouchin".into()])
        .unwrap();
    let Lookup::Found(t) = found else {
        panic!("not found")
    };
    assert_eq!(t.md_id, "bbbbbbbb-0000-4000-8000-000000000002");
    let seen = c.transport.seen.borrow();
    assert!(
        seen[0].starts_with("https://api.mangadex.org/manga?title=Kami%20Chouchin&limit=10"),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains("contentRating%5B%5D=pornographic"));
    assert_eq!(
        seen[1],
        "https://api.mangadex.org/manga/bbbbbbbb-0000-4000-8000-000000000002/aggregate"
    );
    drop(seen);
    // AniList id.
    let mut c = client(vec![ok(SEARCH), ok(AGGREGATE)]);
    assert!(matches!(
        c.lookup(Key::AniList(190001), &["Kami Chouchin".into()])
            .unwrap(),
        Lookup::Found(_)
    ));
    // Nobody links to it: the next title is searched, then it is not found (no aggregate asked).
    let mut c = client(vec![ok(SEARCH), ok(SEARCH)]);
    assert_eq!(
        c.lookup(
            Key::Mal(12345),
            &[
                "Kami Chouchin".into(),
                "kami chouchin".into(),
                "Paper Lanterns".into()
            ]
        )
        .unwrap(),
        Lookup::NotFound
    );
    assert_eq!(
        c.transport.seen.borrow().len(),
        2,
        "the same title (any case) is searched once"
    );
    // Found, but no volume numbers: not found.
    let mut c = client(vec![ok(SEARCH), ok(r#"{"result":"ok","volumes":[]}"#)]);
    assert_eq!(
        c.lookup(Key::Mal(79001), &["Kami".into()]).unwrap(),
        Lookup::NotFound
    );
}

#[test]
fn offline_rate_limits_and_errors_say_so() {
    let mut c = client(vec![Err("dns".into())]);
    assert!(matches!(c.search("Kami"), Err(ServiceError::Offline(_))));
    // A 429 is waited out once, then asked again.
    let mut c = client(vec![status(429), ok(SEARCH)]);
    assert_eq!(c.search("Kami").unwrap().len(), 3);
    let mut c = client(vec![status(429), status(429)]);
    assert_eq!(c.search("Kami"), Err(ServiceError::RateLimited));
    let mut c = client(vec![status(503)]);
    assert!(matches!(c.search("Kami"), Err(ServiceError::Other(_))));
    let mut c = client(vec![]);
    assert!(
        c.search("  ").unwrap().is_empty(),
        "nothing asked for an empty title"
    );
}

/// Saved answers: fresh ones are used without asking, old ones and a refresh ask again; a
/// series not found is kept a shorter time.
#[test]
fn the_cache_saves_lookups_per_series() {
    let dir = std::env::temp_dir().join(format!("kotora-mangadex-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::new(dir.clone());
    let titles = vec!["Kami Chouchin".to_string()];
    let mut c = client(vec![ok(SEARCH), ok(AGGREGATE)]);
    let (l, asked) = find(&mut c, &cache, Key::Mal(79001), &titles, 1_000, false).unwrap();
    assert!(asked && matches!(l, Lookup::Found(_)));
    let mut c = client(vec![]);
    let (l2, asked) = find(
        &mut c,
        &cache,
        Key::Mal(79001),
        &titles,
        1_000 + FOUND_TTL - 1,
        false,
    )
    .unwrap();
    assert!(!asked && l2 == l, "fresh: from the cache, nothing asked");
    assert!(
        cache.get(Key::Mal(79001), 1_000 + FOUND_TTL).is_none(),
        "30 days on: asked again"
    );
    assert!(
        cache.get(Key::AniList(79001), 1_000).is_none(),
        "keys apart"
    );
    let mut c = client(vec![ok(SEARCH), ok(AGGREGATE)]);
    assert!(
        find(&mut c, &cache, Key::Mal(79001), &titles, 1_001, true)
            .unwrap()
            .1,
        "refresh asks"
    );
    cache.put(Key::Mal(5), &Lookup::NotFound, 1_000);
    assert_eq!(
        cache.get(Key::Mal(5), 1_000 + MISSING_TTL - 1),
        Some(Lookup::NotFound)
    );
    assert_eq!(cache.get(Key::Mal(5), 1_000 + MISSING_TTL), None);
    let _ = std::fs::remove_dir_all(&dir);
}

fn manga(id: u32, title: &str, chapters: u32, volumes: u32, total: Option<u32>) -> crate::ListItem {
    crate::ListItem {
        media: crate::Media {
            kind: crate::Kind::Manga,
            id,
            title: title.into(),
            volumes: total,
            ..Default::default()
        },
        status: crate::Status::Watching,
        progress: chapters,
        score: None,
        rewatching: false,
        times_rewatched: 0,
        volumes,
        updated_at: 1,
    }
}

/// The preview: volumes that go up, those already right or higher (never lowered), those not
/// on MangaDex; capped at the known total.
#[test]
fn the_plan_never_lowers_volumes() {
    let t = Lookup::Found(table());
    let rows = vec![
        (manga(1, "Salt Road", 24, 0, None), Some(t.clone())),
        (manga(2, "Ink Harbor", 12, 3, None), Some(t.clone())),
        (manga(3, "Moth Garden", 5, 4, None), Some(t.clone())),
        (manga(4, "Kite Post", 30, 0, None), Some(Lookup::NotFound)),
        (manga(5, "Lantern Ends", 40, 1, Some(4)), Some(t.clone())),
        (manga(6, "Not Asked", 40, 0, None), None),
    ];
    let p = plan_fill(&rows);
    let fill: Vec<(u32, u32, u32)> = p.fill.iter().map(|r| (r.media_id, r.from, r.to)).collect();
    assert_eq!(
        fill,
        [(5, 1, 4), (1, 0, 5)],
        "sorted by title; the total caps it (5 -> 4)"
    );
    let ids = |rows: &[FillRow]| {
        rows.iter()
            .map(|r| (r.media_id, r.from, r.to))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&p.same), [(2, 3, 3)], "already what MangaDex says");
    assert_eq!(
        ids(&p.higher),
        [(3, 4, 1)],
        "higher than MangaDex: left alone, never lowered"
    );
    assert_eq!(p.not_found, ["Kite Post"]);
    let actions = fill_actions(&p);
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|a| matches!(a, crate::listxml::Action::Update { kind: crate::Kind::Manga, change, .. } if change.volumes.is_some() && change.progress.is_none() && change.status.is_none())), "volumes only");
}

#[test]
fn keys_titles_and_who_is_looked_up() {
    use crate::service::ServiceId;
    let mut m = manga(190001, "Kami Chouchin", 3, 0, None).media;
    assert_eq!(
        key_of(ServiceId::AniList, &m),
        Key::AniList(190001),
        "AniList without idMal"
    );
    m.id_mal = Some(79001);
    assert_eq!(key_of(ServiceId::AniList, &m), Key::Mal(79001));
    assert_eq!(
        key_of(ServiceId::Mal, &manga(79001, "K", 3, 0, None).media),
        Key::Mal(79001)
    );
    m.title_en = Some("Paper Lanterns".into());
    assert_eq!(titles_of(&m), ["Kami Chouchin", "Paper Lanterns"]);
    assert!(
        !wants_lookup(&manga(1, "a", 0, 0, None)),
        "no chapters read"
    );
    assert!(
        !wants_lookup(&manga(1, "a", 40, 4, Some(4))),
        "all volumes already"
    );
    assert!(
        wants_lookup(&manga(1, "a", 40, 1, Some(4))) && wants_lookup(&manga(1, "a", 3, 0, None))
    );
}

/// Applied as an import: one transaction through the queue, volumes only, Undo takes it back;
/// with the mirror on it is mirrored like any import.
#[test]
fn the_fill_goes_through_the_queue_with_undo() {
    let mut s = crate::store::Store::open_memory().unwrap();
    let scope = crate::Scope::new("mal", crate::Kind::Manga);
    s.replace_list(
        scope,
        &[
            manga(1, "Salt Road", 24, 0, None),
            manga(2, "Ink Harbor", 12, 3, None),
        ],
        10,
    )
    .unwrap();
    let p = plan_fill(&[(
        manga(1, "Salt Road", 24, 0, None),
        Some(Lookup::Found(table())),
    )]);
    let ids = s.import("mal", &fill_actions(&p), 20).unwrap();
    assert_eq!(ids.len(), 1);
    let it = s.load_item(scope, 1).unwrap().unwrap();
    assert_eq!(
        (it.volumes, it.progress),
        (5, 24),
        "volumes up, chapters untouched"
    );
    let op = &s.queue("mal", 20).unwrap()[0];
    assert_eq!(
        (op.kind, op.change),
        (
            crate::Kind::Manga,
            crate::Change {
                volumes: Some(5),
                ..Default::default()
            }
        )
    );
    assert_eq!(s.undo_import("mal", &ids, 30).unwrap(), 1);
    assert_eq!(
        s.load_item(scope, 1).unwrap().unwrap().volumes,
        0,
        "Undo: back as it was"
    );
    assert_eq!(s.open_count("mal").unwrap(), 0);
}

// ------------------------------------------------------------- Publishing's chapter feed

const FEED: &str = include_str!("fixtures/feed-external.json");
const SEARCH_LINKED: &str = include_str!("fixtures/search-linked.json");
const MANGA: &str = include_str!("fixtures/manga.json");

#[test]
fn the_feed_keeps_official_external_chapters_and_prefers_english() {
    let f = parse_feed(FEED).unwrap();
    assert_eq!(f.len(), 22, "the unavailable chapter is left out");
    let odd = f.iter().find(|c| c.lang == "xx").unwrap();
    assert_eq!((odd.chapter.as_str(), odd.external), ("401", true));
    assert!(
        f.iter().any(|c| !c.external && c.chapter.is_empty()),
        "a fan upload with no chapter number"
    );
    // English chapters (enough of them): the odd-numbered language and others drop out.
    let r = feed_releases(&f);
    assert!(r.iter().all(|x| x.chapter != "401"));
    assert_eq!(
        r.iter().filter_map(|x| x.chapter_no()).fold(0.0, f64::max),
        368.0
    );
    let s = crate::mangaupdates::summarize(
        &r,
        crate::time::parse_rfc3339("2026-11-17T12:00:00Z").unwrap(),
    );
    assert_eq!(
        (s.latest.as_deref(), s.cadence.pace),
        (Some("368"), crate::mangaupdates::Pace::Weekly)
    );
    assert_eq!(
        s.latest_at,
        crate::time::parse_rfc3339("2026-11-10T15:00:00Z")
    );
    // Few English chapters: every language counts.
    let es: Vec<FeedChapter> = f.iter().filter(|c| c.lang != "en").cloned().collect();
    assert!(feed_releases(&es).iter().any(|x| x.chapter == "401"));
    assert!(parse_feed("{}").is_err());
}

#[test]
fn only_a_series_linking_to_the_id_is_taken() {
    // Three hits with the title; only the one whose links.mal is 101 counts.
    let mut c = client(vec![ok(SEARCH_LINKED)]);
    let h = c
        .find_linked(Key::Mal(101), &["Goal Line Furnace".to_string()])
        .unwrap()
        .unwrap();
    assert_eq!(h.id, "eeeeeeee-0000-4000-8000-000000000005");
    // No hit links to it: none, never the title match.
    let mut c = client(vec![ok(SEARCH_LINKED), ok(SEARCH_LINKED)]);
    assert_eq!(
        c.find_linked(
            Key::Mal(7),
            &["Goal Line Furnace".to_string(), "GLF".to_string()]
        )
        .unwrap(),
        None
    );
    // AniList ids work too.
    let mut c = client(vec![ok(SEARCH_LINKED)]);
    assert!(
        c.find_linked(Key::AniList(9101), &["Goal Line Furnace".to_string()])
            .unwrap()
            .is_some()
    );
    // The feed request asks for external chapters and empty pages, newest readable first.
    let mut c = client(vec![ok(FEED)]);
    c.feed("eeeeeeee-0000-4000-8000-000000000005").unwrap();
    let url = c.transport.seen.borrow()[0].clone();
    assert!(
        url.starts_with(
            "https://api.mangadex.org/manga/eeeeeeee-0000-4000-8000-000000000005/feed?"
        )
    );
    for p in [
        "includeExternalUrl=1",
        "includeEmptyPages=1",
        "order%5BreadableAt%5D=desc",
        "contentRating%5B%5D=pornographic",
    ] {
        assert!(url.contains(p), "{p}");
    }
    let mut c = client(vec![ok(MANGA)]);
    assert_eq!(
        c.title_of("eeeeeeee-0000-4000-8000-000000000005")
            .unwrap()
            .1,
        "Goal Line Furnace"
    );
}

#[test]
fn mangadex_links() {
    let id = "eeeeeeee-0000-4000-8000-000000000005";
    assert_eq!(
        md_id_from_link(&format!(
            "https://mangadex.org/title/{id}/goal-line-furnace"
        ))
        .as_deref(),
        Some(id)
    );
    assert_eq!(
        md_id_from_link(&format!("mangadex.org/title/{}", id.to_uppercase())).as_deref(),
        Some(id)
    );
    assert_eq!(md_id_from_link(id).as_deref(), Some(id));
    assert_eq!(md_id_from_link("https://mangadex.org/chapter/abc"), None);
    assert_eq!(
        md_id_from_link("https://www.mangaupdates.com/series/pb8uwds"),
        None
    );
    assert_eq!(md_url(id), format!("https://mangadex.org/title/{id}"));
}
