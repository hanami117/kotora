//! AniList manga against recorded, made-up answers (`fixtures/manga-*`): no test calls AniList.

use super::*;
use crate::anilist::manga::{self as al, save_manga_variables};
use crate::service::ListService;
use crate::store::Store;
use crate::sync::{MangaSync, sync_manga};

const LIST1: &str = include_str!("../fixtures/manga-list-chunk1.json");
const LIST2: &str = include_str!("../fixtures/manga-list-chunk2.json");
const SEARCH: &str = include_str!("../fixtures/manga-search.json");
const DETAILS: &str = include_str!("../fixtures/manga-details.json");

fn query_of(seen: &Seen, i: usize) -> String {
    seen.lock().unwrap()[i].2["query"].as_str().unwrap().to_string()
}

#[test]
fn the_manga_list_maps_chapters_volumes_statuses_formats_and_publishing() {
    let (items, more) = al::parse_list(data(LIST1)).unwrap();
    assert!(more);
    assert_eq!(items.len(), 2, "an entry in a custom list too is read once");
    let lanterns = &items[0];
    assert_eq!((lanterns.status, lanterns.progress, lanterns.volumes, lanterns.score, lanterns.rewatching, lanterns.updated_at), (Status::Watching, 21, 3, Score::from_points(85), false, 1_763_200_000));
    let m = &lanterns.media;
    assert_eq!((m.kind, m.id, m.title.as_str(), m.title_en.as_deref(), m.title_ja.as_deref()), (Kind::Manga, 190001, "Kami Chouchin", Some("Paper Lanterns"), Some("紙提灯")));
    assert_eq!((m.format.as_str(), m.air_status.as_str(), m.episodes, m.volumes, m.start_date.as_deref()), ("manga", "currently_publishing", None, None, Some("2021-04-08")));
    assert!(m.is_publishing());
    assert_eq!(m.id_mal, Some(79001), "kept for the mirror");
    assert_eq!((m.broadcast, m.next_airing), (None, None));
    let garden = &items[1];
    assert_eq!((garden.status, garden.rewatching, garden.times_rewatched, garden.score, garden.progress), (Status::Completed, true, 1, None, 4), "REPEATING is a reread of a completed manga");
    assert_eq!((garden.media.format.as_str(), garden.media.air_status.as_str(), garden.media.episodes, garden.media.volumes), ("manhwa", "finished", Some(120), Some(12)));
    assert_eq!(garden.media.cover_url.as_deref(), Some("https://s4.anilist.example.invalid/media/manga/cover/medium/bx190002.jpg"));

    let (items, more) = al::parse_list(data(LIST2)).unwrap();
    assert!(!more);
    let got: Vec<_> = items.iter().map(|i| (i.media.id, i.status, i.score, i.volumes, i.media.format.as_str(), i.media.air_status.as_str(), i.media.id_mal)).collect();
    assert_eq!(
        got,
        [
            (190003, Status::PlanToWatch, None, 0, "manhua", "not_yet_published", None),
            (190004, Status::OnHold, Score::from_points(42), 5, "light_novel", "on_hiatus", Some(79004)),
            (190005, Status::Dropped, Score::from_points(6), 0, "one_shot", "discontinued", Some(79005)),
            (190006, Status::Completed, Score::from_points(100), 8, "manhua", "finished", Some(79006)),
        ]
    );
    assert!(items[1].media.is_novel());
    assert_eq!(items[3].times_rewatched, 2);
    // The labels are MyAnimeList's.
    let labels: Vec<String> = ["manga", "manhwa", "manhua", "light_novel", "one_shot"].iter().map(|f| crate::mal::manga_type_label(f)).collect();
    assert_eq!(labels, ["Manga", "Manhwa", "Manhua", "Light novel", "One-shot"]);
}

#[test]
fn formats_and_publishing_words() {
    use al::{format_word, publishing_word};
    assert_eq!(format_word(Some("MANGA"), Some("JP")), "manga");
    assert_eq!(format_word(Some("MANGA"), None), "manga");
    assert_eq!(format_word(Some("MANGA"), Some("KR")), "manhwa");
    assert_eq!(format_word(Some("MANGA"), Some("TW")), "manhua");
    assert_eq!(format_word(Some("NOVEL"), Some("KR")), "light_novel");
    assert_eq!(format_word(Some("ONE_SHOT"), Some("JP")), "one_shot");
    assert_eq!(format_word(None, Some("JP")), "");
    assert_eq!(publishing_word(Some("RELEASING")), "currently_publishing");
    assert_eq!(publishing_word(None), "");
}

#[test]
fn the_manga_list_reads_every_chunk_of_type_manga_after_one_viewer() {
    let (dir, tokens) = token_dir("manga-list", Some(i64::MAX));
    let (c, seen, _) = client(vec![ok(VIEWER_JSON), ok(LIST1), ok(LIST2)]);
    let mut al = AniList::new(c, tokens);
    let items = al.fetch_manga_list().unwrap();
    assert_eq!(items.len(), 6);
    assert!(items.iter().all(|i| i.media.kind == Kind::Manga));
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert!(query_of(&seen, 1).contains("type: MANGA") && query_of(&seen, 1).contains("progressVolumes"));
    assert!(!query_of(&seen, 1).contains("staff"), "no staff in the list: it would cost AniList far more");
    assert_eq!(seen.lock().unwrap()[1].2["variables"], json!({ "user": 424242, "chunk": 1 }));
    assert!(seen.lock().unwrap().iter().all(|(url, bearer, _)| url == "https://graphql.anilist.co" && bearer.as_deref() == Some(JWT)));
    let _ = std::fs::remove_dir_all(dir);
}

/// The anime sync reads the Viewer; the manga list right after reuses it (one request less).
#[test]
fn the_anime_then_manga_sync_asks_who_once() {
    let (dir, tokens) = token_dir("manga-both", Some(i64::MAX));
    let anime_empty = r#"{"data":{"MediaListCollection":{"hasNextChunk":false,"lists":[]}}}"#;
    let (c, seen, _) = client(vec![ok(VIEWER_JSON), ok(anime_empty), ok(LIST2)]);
    let mut al = AniList::new(c, tokens);
    assert!(al.fetch_list().unwrap().is_empty());
    assert_eq!(al.fetch_manga_list().unwrap().len(), 4);
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert!(query_of(&seen, 1).contains("type: ANIME") && query_of(&seen, 2).contains("type: MANGA"));
    let _ = std::fs::remove_dir_all(dir);
}

/// `sync_manga` with AniList: off asks nothing; on saves the manga under AniList's scope, apart
/// from the anime and from MyAnimeList, with the MyAnimeList ids kept.
#[test]
fn sync_manga_works_on_anilist() {
    let (dir, tokens) = token_dir("manga-sync", Some(i64::MAX));
    let (c, seen, _) = client(vec![ok(VIEWER_JSON), ok(LIST1), ok(LIST2)]);
    let mut al = AniList::new(c, tokens);
    let mut store = Store::open_memory().unwrap();
    assert_eq!(sync_manga(&mut al, &mut store, 100), MangaSync::Off);
    assert!(seen.lock().unwrap().is_empty(), "Manga off: no request");
    store.set_manga_enabled(true).unwrap();
    let MangaSync::Synced { items, synced_at } = sync_manga(&mut al, &mut store, 100) else { panic!("not synced") };
    assert_eq!((items.len(), synced_at), (6, 100));
    let saved = store.load_list(("anilist", Kind::Manga)).unwrap();
    let garden = saved.iter().find(|i| i.media.id == 190002).unwrap();
    assert_eq!((garden.media.id_mal, garden.media.format.as_str(), garden.media.volumes, garden.rewatching, garden.times_rewatched), (Some(79002), "manhwa", Some(12), true, 1));
    assert_eq!(saved.iter().find(|i| i.media.id == 190001).unwrap().volumes, 3);
    assert!(store.load_list("anilist").unwrap().is_empty(), "the anime list is not touched");
    assert!(store.load_list(("mal", Kind::Manga)).unwrap().is_empty(), "MyAnimeList's manga are apart");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_manga_change_sends_chapters_volumes_and_anilists_names() {
    let vars = |c: Change| save_manga_variables(7, &c, None);
    assert_eq!(vars(Change { progress: Some(22), ..Default::default() }), json!({ "mediaId": 7, "progress": 22 }), "chapters are progress");
    assert_eq!(vars(Change { volumes: Some(4), ..Default::default() }), json!({ "mediaId": 7, "progressVolumes": 4 }));
    assert_eq!(vars(Change { volumes: Some(0), ..Default::default() }), json!({ "mediaId": 7, "progressVolumes": 0 }), "0 volumes is a value too");
    assert_eq!(vars(Change { status: Some(Status::PlanToWatch), ..Default::default() }), json!({ "mediaId": 7, "status": "PLANNING" }), "Plan to read");
    assert_eq!(vars(Change { status: Some(Status::Watching), ..Default::default() })["status"], "CURRENT", "Reading");
    assert_eq!(vars(Change { status: Some(Status::Dropped), ..Default::default() })["status"], "DROPPED");
    // Completed with the last chapter and volume, as the editor sends it.
    let media = Media { kind: Kind::Manga, id: 7, episodes: Some(120), volumes: Some(12), ..Default::default() };
    assert_eq!(vars(Change::to_status_of(Status::Completed, &media)), json!({ "mediaId": 7, "status": "COMPLETED", "progress": 120, "progressVolumes": 12 }));
    // An add: status, chapters, volumes.
    assert_eq!(
        vars(Change { status: Some(Status::Watching), progress: Some(3), volumes: Some(1), ..Default::default() }),
        json!({ "mediaId": 7, "status": "CURRENT", "progress": 3, "progressVolumes": 1 })
    );
}

/// A reread is AniList's REPEATING, with `repeat` the reread count, as for an anime rewatch.
#[test]
fn rereading_starts_as_repeating_and_ends_with_one_more_repeat() {
    let vars = |c: Change, cur: Option<&Values>| save_manga_variables(7, &c, cur);
    let done = Values { times_rewatched: 1, volumes: 12, ..Values::new(Status::Completed, 120, ten(8)) };
    let start = Change::start_rewatch(&done).unwrap();
    assert_eq!(vars(start, None), json!({ "mediaId": 7, "status": "REPEATING", "progress": 0 }));
    let rereading = start.apply(done);
    assert!(rereading.rewatching);
    let end = Change { progress: Some(120), ..Default::default() }.completes(&Values { progress: 119, ..rereading }, Some(120));
    assert_eq!(vars(end, None), json!({ "mediaId": 7, "status": "COMPLETED", "progress": 120, "repeat": 2 }));
    // A reread count set by hand.
    assert_eq!(vars(Change { times_rewatched: Some(3), ..Default::default() }, None), json!({ "mediaId": 7, "repeat": 3 }));
    // Stop rereading alone: COMPLETED only when AniList has it REPEATING.
    let off = Change { rewatching: Some(false), ..Default::default() };
    assert_eq!(vars(off, Some(&rereading)), json!({ "mediaId": 7, "status": "COMPLETED" }));
    assert_eq!(vars(off, Some(&done)), json!({ "mediaId": 7 }));
}

/// Scores go as `scoreRaw`, the 100-point value, whatever the user's format: AniList shows it in
/// theirs. A score picked in each format comes back the same in that format.
#[test]
fn manga_scores_in_every_score_format() {
    let picks = [(ScoreFormat::Point100, 73, 73), (ScoreFormat::Point10Decimal, 75, 75), (ScoreFormat::Point10, 8, 80), (ScoreFormat::Point5, 4, 80), (ScoreFormat::Point3, 3, 85), (ScoreFormat::Point3, 1, 35)];
    for (format, value, raw) in picks {
        let score = format.score_of(value);
        let v = save_manga_variables(7, &Change { score: Some(score), ..Default::default() }, None);
        assert_eq!(v, json!({ "mediaId": 7, "scoreRaw": raw }), "{format:?} {value}");
        // Read back as AniList sends it (POINT_100): the same value in the user's format.
        let back = score100(raw as f32).unwrap();
        assert_eq!(format.value(back) as u32, value, "{format:?}");
    }
    assert_eq!(save_manga_variables(7, &Change { score: Some(None), ..Default::default() }, None), json!({ "mediaId": 7, "scoreRaw": 0 }), "0 removes the score");
    // The list query asks for the 100-point scale whatever the user's format.
    assert!(manga_entry_fields().contains("score(format: POINT_100)"));
}

fn manga_entry_fields() -> String {
    let (mut c, seen, _) = client(vec![ok(r#"{"data":{"Media":{"id":5,"mediaListEntry":null}}}"#)]);
    c.fetch_manga_entry(JWT, 5).unwrap();
    query_of(&seen, 0)
}

#[test]
fn save_sends_the_manga_mutation_and_returns_the_saved_entry() {
    let saved = r#"{"data":{"SaveMediaListEntry":{"id":8001,"mediaId":190001,"status":"CURRENT","progress":22,"progressVolumes":4,"repeat":0,"score":85,"updatedAt":1763300000}}}"#;
    let (mut c, seen, _) = client(vec![ok(saved)]);
    let r = c.save_manga_entry(JWT, 190001, &Change { progress: Some(22), volumes: Some(4), ..Default::default() }).unwrap();
    assert_eq!((r.values.progress, r.values.volumes, r.values.score, r.updated_at), (22, 4, Score::from_points(85), 1_763_300_000));
    let q = query_of(&seen, 0);
    assert!(q.starts_with("mutation") && q.contains("progressVolumes: $progressVolumes"));
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "mediaId": 190001, "progress": 22, "progressVolumes": 4 }));
    let (mut c, seen, _) = client(vec![]);
    assert!(c.save_manga_entry(JWT, 1, &Change::default()).is_err(), "nothing to send, nothing sent");
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn stopping_a_reread_alone_reads_the_manga_entry_first() {
    let repeating = r#"{"data":{"Media":{"id":190002,"mediaListEntry":{"id":8002,"mediaId":190002,"status":"REPEATING","progress":4,"progressVolumes":0,"repeat":1,"score":0,"updatedAt":1763100000}}}}"#;
    let saved = r#"{"data":{"SaveMediaListEntry":{"id":8002,"mediaId":190002,"status":"COMPLETED","progress":4,"progressVolumes":0,"repeat":1,"score":0,"updatedAt":1763400000}}}"#;
    let (mut c, seen, _) = client(vec![ok(repeating), ok(saved)]);
    let r = c.save_manga_entry(JWT, 190002, &Change { rewatching: Some(false), ..Default::default() }).unwrap();
    assert_eq!((r.values.status, r.values.rewatching), (Status::Completed, false));
    assert!(query_of(&seen, 0).contains("type: MANGA"));
    assert_eq!(seen.lock().unwrap()[1].2["variables"], json!({ "mediaId": 190002, "status": "COMPLETED" }));
}

#[test]
fn reads_one_manga_entry_or_none() {
    let entry = r#"{"data":{"Media":{"id":190002,"mediaListEntry":{"id":8002,"mediaId":190002,"status":"REPEATING","progress":4,"progressVolumes":2,"repeat":1,"score":70,"updatedAt":1763100000}}}}"#;
    let gone = r#"{"errors":[{"message":"Not Found.","status":404}],"data":{"Media":null}}"#;
    let (mut c, seen, _) = client(vec![ok(entry), ok(r#"{"data":{"Media":{"id":5,"mediaListEntry":null}}}"#), status(404, gone)]);
    let r = c.fetch_manga_entry(JWT, 190002).unwrap().unwrap();
    assert_eq!((r.values.status, r.values.rewatching, r.values.times_rewatched, r.values.progress, r.values.volumes, r.values.score), (Status::Completed, true, 1, 4, 2, Score::from_points(70)));
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "id": 190002 }));
    assert!(query_of(&seen, 0).contains("Media(id: $id, type: MANGA)"));
    assert_eq!(c.fetch_manga_entry(JWT, 5).unwrap(), None);
    assert_eq!(c.fetch_manga_entry(JWT, 6).unwrap(), None, "no such manga: not on the list");
}

#[test]
fn delete_finds_the_manga_entry_id_then_removes_it() {
    let entry = r#"{"data":{"Media":{"id":190005,"mediaListEntry":{"id":8005,"mediaId":190005,"status":"DROPPED","progress":2,"progressVolumes":null,"repeat":0,"score":6,"updatedAt":1760000000}}}}"#;
    let (mut c, seen, _) = client(vec![
        ok(entry),
        ok(r#"{"data":{"DeleteMediaListEntry":{"deleted":true}}}"#),
        ok(r#"{"data":{"Media":{"id":3,"mediaListEntry":null}}}"#),
        ok(entry),
        ok(r#"{"data":{"DeleteMediaListEntry":{"deleted":false}}}"#),
    ]);
    c.delete_manga_entry(JWT, 190005).unwrap();
    assert!(query_of(&seen, 0).contains("type: MANGA"));
    assert!(query_of(&seen, 1).starts_with("mutation") && query_of(&seen, 1).contains("DeleteMediaListEntry"));
    assert_eq!(seen.lock().unwrap()[1].2["variables"], json!({ "id": 8005 }), "the entry's id, not the manga's");
    c.delete_manga_entry(JWT, 3).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 3, "not on the list: nothing to delete");
    assert!(c.delete_manga_entry(JWT, 190005).is_err(), "AniList said no");
}

/// The queue's `*_of(Manga)` calls reach the manga methods (never the anime ones).
#[test]
fn the_kind_helpers_reach_the_manga_calls() {
    let (dir, tokens) = token_dir("manga-of", Some(i64::MAX));
    let entry = r#"{"data":{"Media":{"id":190005,"mediaListEntry":{"id":8005,"mediaId":190005,"status":"DROPPED","progress":2,"repeat":0,"score":6,"updatedAt":1760000000}}}}"#;
    let saved = r#"{"data":{"SaveMediaListEntry":{"id":8005,"mediaId":190005,"status":"CURRENT","progress":3,"progressVolumes":1,"repeat":0,"score":6,"updatedAt":1763000000}}}"#;
    let (c, seen, _) = client(vec![ok(entry), ok(saved), ok(entry), ok(r#"{"data":{"DeleteMediaListEntry":{"deleted":true}}}"#), ok(SEARCH), ok(DETAILS)]);
    let mut al = AniList::new(c, tokens);
    assert_eq!(al.get_entry_of(Kind::Manga, 190005).unwrap().unwrap().values.progress, 2);
    assert_eq!(al.update_entry_of(Kind::Manga, 190005, &Change { status: Some(Status::Watching), progress: Some(3), volumes: Some(1), ..Default::default() }).unwrap().values.volumes, 1);
    al.delete_entry_of(Kind::Manga, 190005).unwrap();
    assert_eq!(al.browse_of(Kind::Manga, "lantern", 10).unwrap()[0].kind, Kind::Manga);
    assert_eq!(al.details_of(Kind::Manga, 190002).unwrap().kind, Kind::Manga);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 6);
    assert!(seen.iter().all(|(_, _, b)| !b["query"].as_str().unwrap().contains("type: ANIME")), "no anime request");
    drop(seen);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn manga_search_reads_rows_with_authors_and_skips_short_queries() {
    let (mut c, seen, _) = client(vec![ok(SEARCH)]);
    assert!(c.browse_manga(JWT, " ab ", 10).unwrap().is_empty());
    assert!(seen.lock().unwrap().is_empty(), "under 3 characters nothing is asked");
    let hits = c.browse_manga(JWT, "lantern", 40).unwrap();
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "q": "lantern", "n": 25 }));
    let q = query_of(&seen, 0);
    assert!(q.contains("type: MANGA") && q.contains("isAdult: false"));
    assert_eq!(hits.len(), 2, "a row without an id is left out");
    let h = &hits[0];
    assert_eq!((h.kind, h.id, h.title.as_str(), h.format.as_str(), h.episodes, h.mean), (Kind::Manga, 190001, "Kami Chouchin", "manga", None, Some(8.2)));
    assert_eq!((h.air_status.as_str(), h.start_date.as_deref(), h.season.clone()), ("currently_publishing", Some("2021-04-08"), None));
    assert_eq!(h.authors, ["Aoi Hoshimiya"], "the translator is not an author");
    let novel = &hits[1];
    assert_eq!((novel.format.as_str(), novel.volumes, novel.authors.clone()), ("light_novel", Some(9), vec!["Mio Tanabe".to_string(), "Sora Ikeda".to_string()]));
    // An add from a search row keeps the kind, the counts and the authors.
    let m = novel.media();
    assert_eq!((m.kind, m.volumes, m.start_date.as_deref()), (Kind::Manga, Some(9), Some("2019")));
}

#[test]
fn manga_details_read_authors_counts_and_the_users_entry() {
    let (mut c, seen, _) = client(vec![ok(DETAILS)]);
    let d = c.manga_details(JWT, 190002).unwrap();
    assert!(query_of(&seen, 0).contains("Media(id: $id, type: MANGA)") && query_of(&seen, 0).contains("staff("));
    assert_eq!((d.kind, d.id, d.title.as_str(), d.title_en.as_deref(), d.title_ja.as_deref()), (Kind::Manga, 190002, "Cheolui Jeongwon", Some("Iron Garden"), Some("철의 정원")));
    assert_eq!((d.format.as_str(), d.episodes, d.volumes, d.air_status.as_str()), ("manhwa", Some(120), Some(12), "finished"));
    assert_eq!((d.start_date.as_deref(), d.end_date.as_deref(), d.season.clone()), (Some("2018-02-05"), Some("2022-09-20"), None));
    let authors: Vec<(&str, &str)> = d.authors.iter().map(|a| (a.name.as_str(), a.role.as_str())).collect();
    assert_eq!(authors, [("Min-seo Park", "Story & Art (ch. 1-40)"), ("Ji-ho Han", "Art")], "story and art roles only, one line a person");
    assert!(d.serialization.is_empty(), "AniList has no magazine");
    assert_eq!((d.studios.len(), d.source.as_str(), d.minutes), (0, "", None));
    assert_eq!(d.genres, ["Drama", "Fantasy"]);
    assert_eq!((d.rank, d.popularity, d.members, d.mean), (Some(310), Some(420), Some(15400), Some(7.9)), "all-time ranks only");
    assert_eq!(d.synopsis, "A gardener grows iron flowers.\n\n(Source: made up) & more");
    assert_eq!(d.cover_url.as_deref(), Some("https://s4.anilist.example.invalid/media/manga/cover/medium/bx190002.jpg"));
    let mine = d.mine.clone().unwrap();
    assert_eq!(
        (mine.status.as_str(), mine.progress, mine.volumes, mine.score, mine.rewatching, mine.times_rewatched, mine.start_date.as_deref(), mine.finish_date.as_deref()),
        ("completed", 4, 0, Score::from_points(70), true, 1, Some("2023-01-02"), Some("2023-06"))
    );
    let m = d.media();
    assert_eq!((m.kind, m.volumes, m.authors.clone()), (Kind::Manga, Some(12), vec!["Min-seo Park".to_string(), "Ji-ho Han".to_string()]));
    let (mut c, _, _) = client(vec![ok(r#"{"data":{"Media":null}}"#)]);
    assert!(c.manga_details(JWT, 1).is_err());
}

#[test]
fn manga_calls_share_the_rate_budget_and_the_429_rule() {
    let r429 = Ok(Response { status: 429, retry_after: Some(9), body: r#"{"errors":[{"message":"Too Many Requests.","status":429}]}"#.into() });
    let (mut c, seen, waits) = client(vec![r429, ok(SEARCH)]);
    assert_eq!(c.browse_manga(JWT, "lantern", 5).unwrap().len(), 2);
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(waits.lock().unwrap().iter().any(|w| *w >= Duration::from_secs(8)), "waited for Retry-After");
}
