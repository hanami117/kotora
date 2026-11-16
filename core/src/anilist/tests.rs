//! Recorded, made-up AniList answers (`fixtures/`): no test calls AniList.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::*;
use crate::score::ten;

/// A made-up JWT whose `exp` is 1800000000 (its signature is not real).
const JWT: &str = "eyJ0eXAiOiAiSldUIiwgImFsZyI6ICJSUzI1NiJ9.eyJhdWQiOiAiOTk5OTkiLCAianRpIjogIm1hZGUtdXAiLCAiaWF0IjogMTcwMDAwMDAwMCwgImV4cCI6IDE4MDAwMDAwMDAsICJzdWIiOiAiNDI0MjQyIiwgInNjb3BlcyI6IFtdfQ.bWFkZS11cC1zaWduYXR1cmUtbm90LXJlYWw";

const VIEWER_JSON: &str = include_str!("fixtures/viewer.json");
const LIST1: &str = include_str!("fixtures/list-chunk1.json");
const LIST2: &str = include_str!("fixtures/list-chunk2.json");
const DETAILS: &str = include_str!("fixtures/details.json");
const SEASON: &str = include_str!("fixtures/season.json");
const SEARCH: &str = include_str!("fixtures/search.json");

/// What a test transport saw: URL, bearer, JSON body.
type Seen = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

/// Answers from a script, in order, and records each request.
struct Script {
    answers: RefCell<VecDeque<Result<Response, String>>>,
    seen: Seen,
}

impl Transport for Script {
    fn send(&self, req: &Request) -> Result<Response, String> {
        let body = req.json.as_deref().map(|j| serde_json::from_str(j).unwrap()).unwrap_or(Value::Null);
        self.seen.lock().unwrap().push((req.url.clone(), req.bearer.clone(), body));
        self.answers.borrow_mut().pop_front().expect("an answer for each request")
    }
}

fn ok(body: &str) -> Result<Response, String> {
    Ok(Response { status: 200, retry_after: None, body: body.to_string() })
}

fn status(code: u16, body: &str) -> Result<Response, String> {
    Ok(Response { status: code, retry_after: None, body: body.to_string() })
}

fn client(answers: Vec<Result<Response, String>>) -> (Client<Script>, Seen, Arc<Mutex<Vec<Duration>>>) {
    let seen: Seen = Arc::default();
    let waits: Arc<Mutex<Vec<Duration>>> = Arc::default();
    let w = waits.clone();
    let c = Client::new(Script { answers: RefCell::new(answers.into()), seen: seen.clone() }).with_own_budget().with_sleep(move |d| w.lock().unwrap().push(d));
    (c, seen, waits)
}

fn data(body: &str) -> Value {
    serde_json::from_str::<Value>(body).unwrap()["data"].clone()
}

/// A token file in a temp dir with a token good until 1800000000 (or `expires_at`).
fn token_dir(tag: &str, expires_at: Option<i64>) -> (std::path::PathBuf, TokenStore) {
    let dir = std::env::temp_dir().join(format!("kotora-al-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = TokenStore::new(dir.join("tokens.json"));
    if let Some(at) = expires_at {
        store.save_anilist(&Token { access_token: JWT.into(), refresh_token: String::new(), expires_at: at }).unwrap();
    }
    (dir, store)
}

#[test]
fn the_sign_in_page_asks_for_a_token_and_no_secret() {
    let url = authorize_url("12345");
    assert_eq!(url, "https://anilist.co/api/v2/oauth/authorize?client_id=12345&response_type=token");
    assert!(!url.contains("secret"));
}

#[test]
fn tokens_come_pasted_bare_or_in_the_redirect() {
    let now = 1_700_000_000;
    // Bare, from the PIN page: the end comes from the token itself.
    let t = parse_token_input(&format!("  {JWT}\n"), now).unwrap();
    assert_eq!((t.access_token.as_str(), t.expires_at, t.refresh_token.as_str()), (JWT, 1_800_000_000, ""));
    // The address the browser ended on, or only its fragment: `expires_in` wins.
    let t = parse_token_input(&format!("http://127.0.0.1:47321/callback#access_token={JWT}&token_type=Bearer&expires_in=31536000"), now).unwrap();
    assert_eq!((t.access_token.as_str(), t.expires_at), (JWT, now + 31_536_000));
    let t = parse_token_input(&format!("access_token={JWT}&token_type=Bearer"), now).unwrap();
    assert_eq!(t.expires_at, 1_800_000_000);
    // A refusal, nonsense, and an ended token.
    assert!(parse_token_input("http://127.0.0.1:47321/callback?error=access_denied", now).unwrap_err().contains("access_denied"));
    assert!(parse_token_input("", now).unwrap_err().contains("Paste"));
    assert!(parse_token_input("12345", now).unwrap_err().contains("isn't an AniList token"));
    assert!(parse_token_input("a.b c.d", now).is_err());
    assert!(parse_token_input(JWT, 1_900_000_000).unwrap_err().contains("expired"));
    // A token with no readable end lasts a year.
    let odd = format!("{}.{}.{}", "x".repeat(20), "!!".repeat(10), "y".repeat(20));
    assert_eq!(parse_token_input(&odd, now).unwrap().expires_at, now + YEAR);
}

#[test]
fn debug_hides_the_token() {
    let t = parse_token_input(JWT, 1_700_000_000).unwrap();
    assert!(!format!("{t:?}").contains("eyJ"));
}

#[test]
fn the_viewer_reads_with_avatar_join_date_and_statistics() {
    let (id, u) = parse_viewer(data(VIEWER_JSON)).unwrap();
    assert_eq!((id, u.name.as_str()), (424242, "demo_user"));
    assert_eq!(u.picture.as_deref(), Some("https://s4.anilist.example.invalid/user/avatar/large/b424242.png"));
    assert_eq!(u.profile.joined.as_deref(), Some("2019-03-02"));
    let s = u.profile.stats.unwrap();
    assert_eq!((s.watching, s.completed, s.on_hold, s.plan_to_watch, s.dropped, s.episodes), (2, 3, 1, 1, 0, 131), "a rewatch counts as completed");
    assert!((s.days - 2.183).abs() < 0.01 && (s.mean_score - 7.45).abs() < 0.001);
    assert_eq!(u.profile.score_format, ScoreFormat::Point5);
    // A mean in the user's own format still shows out of 10.
    let five = data(&VIEWER_JSON.replace("\"meanScore\":74.5", "\"meanScore\":3.5"));
    assert!((parse_viewer(five).unwrap().1.profile.stats.unwrap().mean_score - 7.0).abs() < 0.001);
    let none = data(&VIEWER_JSON.replace("\"mediaListOptions\":{\"scoreFormat\":\"POINT_5\"},", ""));
    assert_eq!(parse_viewer(none).unwrap().1.profile.score_format, ScoreFormat::Point10, "no format: 1-10");
    assert!(parse_viewer(json!({ "Viewer": null })).is_err());
}

#[test]
fn the_list_maps_statuses_scores_formats_and_airing() {
    let (items, more) = parse_list(data(LIST1)).unwrap();
    assert!(more);
    assert_eq!(items.len(), 2, "an entry in a custom list too is read once");
    let lantern = &items[0];
    assert_eq!((lantern.status, lantern.progress, lantern.score, lantern.rewatching, lantern.updated_at), (Status::Watching, 6, Score::from_points(85), false, 1_763_200_000));
    let m = &lantern.media;
    assert_eq!((m.id, m.title.as_str(), m.title_en.as_deref(), m.title_ja.as_deref()), (170001, "Kirigaoka no Lantern", Some("Lanterns of Kirigaoka"), Some("霧ヶ丘のランタン")));
    assert_eq!((m.format.as_str(), m.air_status.as_str(), m.episodes, m.start_date.as_deref()), ("tv", "currently_airing", Some(12), Some("2026-10-03")));
    assert_eq!(m.next_airing, Some(NextAiring { episode: 8, at: 1_795_271_400 }));
    assert_eq!(m.id_mal, Some(59001), "kept for mirroring later");
    assert_eq!(m.broadcast, Some(airing::Broadcast { day: 5, minute: Some(23 * 60 + 30) }), "Saturday 23:30 JST");
    assert_eq!(m.cover_url.as_deref(), Some("https://s4.anilist.example.invalid/media/anime/cover/small/bx170001.jpg"));
    let velvet = &items[1];
    assert_eq!((velvet.status, velvet.rewatching, velvet.times_rewatched, velvet.score), (Status::Completed, true, 1, None), "REPEATING is a rewatch of a completed show");
    assert_eq!((velvet.media.format.as_str(), velvet.media.air_status.as_str(), velvet.media.start_date.as_deref()), ("tv", "finished_airing", Some("2024-04")));
    assert_eq!(velvet.media.cover_url.as_deref(), Some("https://s4.anilist.example.invalid/media/anime/cover/medium/bx170002.jpg"));

    let (items, more) = parse_list(data(LIST2)).unwrap();
    assert!(!more);
    let got: Vec<_> = items.iter().map(|i| (i.media.id, i.status, i.score, i.media.format.as_str(), i.media.air_status.as_str())).collect();
    assert_eq!(
        got,
        [
            (170003, Status::PlanToWatch, None, "movie", "not_yet_aired"),
            (170004, Status::OnHold, Score::from_points(42), "ona", "currently_airing"),
            (170005, Status::Dropped, Score::from_points(6), "ova", "finished_airing"),
            (170006, Status::Completed, Score::from_points(100), "tv", "finished_airing"),
        ]
    );
    assert_eq!(items[0].media.start_date.as_deref(), Some("2027"));
    assert_eq!(items[3].times_rewatched, 2);
}

#[test]
fn scores_keep_all_100_points() {
    // The 73 → 7 rounding of 0.2.0 is gone: Kotora keeps what AniList has.
    let got = [0.0, 4.0, 44.6, 73.0, 100.0, 120.0, f32::NAN].map(|r| score100(r).map(Score::points));
    assert_eq!(got, [None, Some(4), Some(45), Some(73), Some(100), Some(100), None]);
}

#[test]
fn fetch_list_asks_who_then_reads_every_chunk_with_the_bearer_only_to_anilist() {
    let (dir, tokens) = token_dir("list", Some(i64::MAX));
    let (c, seen, _) = client(vec![ok(VIEWER_JSON), ok(LIST1), ok(LIST2)]);
    let mut al = AniList::new(c, tokens);
    let items = al.fetch_list().unwrap();
    assert_eq!(items.len(), 6);
    // The Profile refresh right after reuses the Viewer: no fourth request.
    assert_eq!(al.user().unwrap().name, "demo_user");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|(url, bearer, _)| url == "https://graphql.anilist.co" && bearer.as_deref() == Some(JWT)));
    assert!(seen[0].2["query"].as_str().unwrap().contains("Viewer"));
    assert_eq!(seen[1].2["variables"], json!({ "user": 424242, "chunk": 1 }));
    assert_eq!(seen[2].2["variables"]["chunk"], 2);
    assert!(seen[1].2["query"].as_str().unwrap().contains("type: ANIME"), "anime only");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_token_or_an_ended_one_means_sign_in_again_without_a_request() {
    for (tag, at) in [("none", None), ("ended", Some(1))] {
        let (dir, tokens) = token_dir(tag, at);
        let (c, seen, _) = client(vec![]);
        let mut al = AniList::new(c, tokens);
        assert_eq!(al.get_entry(1), Err(ServiceError::SignInAgain));
        assert_eq!(al.fetch_list(), Err(ServiceError::SignInAgain));
        assert!(seen.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_refused_token_means_sign_in_again() {
    let (dir, tokens) = token_dir("refused", Some(i64::MAX));
    let (c, _, _) = client(vec![status(400, r#"{"errors":[{"message":"Invalid token","status":400}],"data":null}"#), status(401, "")]);
    let mut al = AniList::new(c, tokens);
    assert_eq!(al.get_entry(1), Err(ServiceError::SignInAgain));
    assert_eq!(al.update_entry(1, &Change { progress: Some(2), ..Default::default() }), Err(ServiceError::SignInAgain));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_429_waits_for_retry_after_then_succeeds_and_endless_ones_give_up() {
    let r429 = |after| Ok(Response { status: 429, retry_after: after, body: r#"{"errors":[{"message":"Too Many Requests.","status":429}]}"#.into() });
    let (mut c, seen, waits) = client(vec![r429(Some(7)), ok(VIEWER_JSON)]);
    assert_eq!(c.viewer(JWT).unwrap().0, 424242);
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(waits.lock().unwrap().iter().any(|w| *w >= Duration::from_secs(6)), "waited for Retry-After: {:?}", waits.lock().unwrap());
    let (mut c, _, _) = client(vec![r429(None), r429(None), r429(None), r429(None)]);
    assert_eq!(c.viewer(JWT).unwrap_err(), ServiceError::RateLimited);
}

#[test]
fn errors_are_sorted_into_offline_and_other() {
    let (mut c, _, _) = client(vec![
        Err("dns".into()),
        status(502, "<html>"),
        status(403, r#"{"errors":[{"message":"The AniList API has been temporarily disabled.","status":403}]}"#),
        status(400, r#"{"errors":[{"message":"Validation error: status","status":400}]}"#),
        ok(r#"{"errors":[{"message":"Something odd"}],"data":null}"#),
    ]);
    assert!(matches!(c.viewer(JWT), Err(ServiceError::Offline(m)) if m.contains("no answer")));
    assert!(matches!(c.viewer(JWT), Err(ServiceError::Offline(m)) if m.contains("502")));
    assert!(matches!(c.viewer(JWT), Err(ServiceError::Offline(m)) if m.contains("403")), "maintenance: try again later");
    assert!(matches!(c.viewer(JWT), Err(ServiceError::Other(m)) if m == "AniList answered HTTP 400 (Validation error: status)"));
    assert!(matches!(c.viewer(JWT), Err(ServiceError::Other(m)) if m.contains("Something odd")));
}

#[test]
fn reads_one_entry_or_none() {
    let entry = r#"{"data":{"Media":{"id":170002,"mediaListEntry":{"id":9002,"mediaId":170002,"status":"REPEATING","progress":3,"repeat":1,"score":0,"updatedAt":1763100000}}}}"#;
    let (mut c, seen, _) = client(vec![ok(entry), ok(r#"{"data":{"Media":{"id":5,"mediaListEntry":null}}}"#), status(404, r#"{"errors":[{"message":"Not Found.","status":404}],"data":{"Media":null}}"#)]);
    let r = c.fetch_entry(JWT, 170002).unwrap().unwrap();
    assert_eq!((r.values.status, r.values.rewatching, r.values.times_rewatched, r.values.progress, r.updated_at), (Status::Completed, true, 1, 3, 1_763_100_000));
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "id": 170002 }));
    assert_eq!(c.fetch_entry(JWT, 5).unwrap(), None);
    assert_eq!(c.fetch_entry(JWT, 6).unwrap(), None, "no such anime: not on the list");
}

#[test]
fn a_change_sends_only_its_fields_with_anilists_names() {
    let vars = |c: Change| save_variables(7, &c, None);
    assert_eq!(vars(Change { progress: Some(5), ..Default::default() }), json!({ "mediaId": 7, "progress": 5 }));
    assert_eq!(vars(Change { status: Some(Status::OnHold), ..Default::default() }), json!({ "mediaId": 7, "status": "PAUSED" }));
    assert_eq!(vars(Change { status: Some(Status::PlanToWatch), ..Default::default() })["status"], "PLANNING");
    assert_eq!(vars(Change { score: Some(ten(8)), ..Default::default() }), json!({ "mediaId": 7, "scoreRaw": 80 }), "AniList shows it in the user's format");
    assert_eq!(vars(Change { score: Some(None), ..Default::default() }), json!({ "mediaId": 7, "scoreRaw": 0 }), "0 removes the score");
    // A rewatch starts as REPEATING at 0, and the last episode ends it with one more in the count.
    let done = Values { times_rewatched: 1, ..Values::new(Status::Completed, 12, ten(8)) };
    let start = Change::start_rewatch(&done).unwrap();
    assert_eq!(vars(start), json!({ "mediaId": 7, "status": "REPEATING", "progress": 0 }));
    let rewatching = start.apply(done);
    let end = Change { progress: Some(12), ..Default::default() }.completes(&Values { progress: 11, ..rewatching }, Some(12));
    assert_eq!(vars(end), json!({ "mediaId": 7, "status": "COMPLETED", "progress": 12, "repeat": 2 }));
    // The rewatch switched off alone: COMPLETED only when AniList has it REPEATING.
    let off = Change { rewatching: Some(false), ..Default::default() };
    assert_eq!(save_variables(7, &off, Some(&rewatching)), json!({ "mediaId": 7, "status": "COMPLETED" }));
    assert_eq!(save_variables(7, &off, Some(&done)), json!({ "mediaId": 7 }));
    // An add: status and progress.
    assert_eq!(vars(Change { status: Some(Status::Watching), progress: Some(3), ..Default::default() }), json!({ "mediaId": 7, "status": "CURRENT", "progress": 3 }));
}

#[test]
fn save_sends_the_mutation_and_returns_the_saved_entry() {
    let saved = r#"{"data":{"SaveMediaListEntry":{"id":9001,"mediaId":170001,"status":"CURRENT","progress":7,"repeat":0,"score":85,"updatedAt":1763300000}}}"#;
    let (mut c, seen, _) = client(vec![ok(saved)]);
    let r = c.save_entry(JWT, 170001, &Change { progress: Some(7), ..Default::default() }).unwrap();
    assert_eq!((r.values.progress, r.values.score, r.updated_at), (7, Score::from_points(85), 1_763_300_000));
    let seen = seen.lock().unwrap();
    assert!(seen[0].2["query"].as_str().unwrap().starts_with("mutation"));
    assert_eq!(seen[0].2["variables"], json!({ "mediaId": 170001, "progress": 7 }));
    let (mut c, _, _) = client(vec![]);
    assert!(c.save_entry(JWT, 1, &Change::default()).is_err(), "nothing to send, nothing sent");
}

#[test]
fn ending_a_rewatch_alone_reads_the_entry_first() {
    let repeating = r#"{"data":{"Media":{"id":170002,"mediaListEntry":{"id":9002,"mediaId":170002,"status":"REPEATING","progress":3,"repeat":1,"score":0,"updatedAt":1763100000}}}}"#;
    let saved = r#"{"data":{"SaveMediaListEntry":{"id":9002,"mediaId":170002,"status":"COMPLETED","progress":3,"repeat":1,"score":0,"updatedAt":1763400000}}}"#;
    let (mut c, seen, _) = client(vec![ok(repeating), ok(saved)]);
    let r = c.save_entry(JWT, 170002, &Change { rewatching: Some(false), ..Default::default() }).unwrap();
    assert_eq!((r.values.status, r.values.rewatching), (Status::Completed, false));
    assert_eq!(seen.lock().unwrap()[1].2["variables"], json!({ "mediaId": 170002, "status": "COMPLETED" }));
}

#[test]
fn delete_finds_the_entry_id_then_removes_it() {
    let entry = r#"{"data":{"Media":{"id":170005,"mediaListEntry":{"id":9005,"mediaId":170005,"status":"DROPPED","progress":1,"repeat":0,"score":6,"updatedAt":1760000000}}}}"#;
    let (mut c, seen, _) = client(vec![ok(entry), ok(r#"{"data":{"DeleteMediaListEntry":{"deleted":true}}}"#), ok(r#"{"data":{"Media":{"id":3,"mediaListEntry":null}}}"#)]);
    c.delete_entry(JWT, 170005).unwrap();
    assert_eq!(seen.lock().unwrap()[1].2["variables"], json!({ "id": 9005 }), "the entry's id, not the anime's");
    c.delete_entry(JWT, 3).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 3, "not on the list: nothing to delete");
}

#[test]
fn search_reads_rows_and_skips_short_queries() {
    let (mut c, seen, _) = client(vec![ok(SEARCH), ok(SEARCH)]);
    assert!(c.browse(JWT, " ab ", 10).unwrap().is_empty());
    let hits = c.browse(JWT, "velvet", 40).unwrap();
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "q": "velvet", "n": 25 }));
    assert!(seen.lock().unwrap()[0].2["query"].as_str().unwrap().contains("isAdult: false"));
    let h = &hits[0];
    assert_eq!((h.id, h.title.as_str(), h.format.as_str(), h.episodes, h.mean), (170002, "Velvet Orchard", "tv", Some(24), Some(6.6)));
    assert_eq!((h.season_text().as_deref(), h.title_ja.as_deref(), h.air_status.as_str()), (Some("Spring 2024"), Some("ベルベット果樹園"), "finished_airing"));
    let found = c.search(JWT, "velvet").unwrap();
    assert_eq!((found[0].id, found[0].other_titles.clone()), (170002, vec!["ベルベット果樹園".to_string(), "VelOr".to_string()]));
    assert_eq!(found[1].title, "Velvet Orchard: Harvest");
}

#[test]
fn details_read_every_field_and_the_users_entry() {
    let d = parse_details(data(DETAILS)).unwrap();
    assert_eq!((d.id, d.title.as_str(), d.format.as_str(), d.episodes, d.air_status.as_str()), (170001, "Kirigaoka no Lantern", "tv", Some(12), "currently_airing"));
    assert_eq!((d.season_text().as_deref(), d.source.as_str(), d.minutes, d.mean), (Some("Fall 2026"), "light_novel", Some(24), Some(8.1)));
    assert_eq!((d.rank, d.popularity, d.members), (Some(120), Some(300), Some(52011)), "all-time ranks only");
    assert_eq!(d.studios, ["Studio Hoshi"]);
    assert_eq!(d.genres, ["Fantasy", "Slice of Life"]);
    assert_eq!(d.synopsis, "A lamp-maker's apprentice\n\nfinds a town that only appears in fog. (Made up for tests.) & more");
    assert_eq!(d.cover_url.as_deref(), Some("https://s4.anilist.example.invalid/media/anime/cover/medium/bx170001.jpg"));
    assert_eq!(d.cover_small.as_deref(), Some("https://s4.anilist.example.invalid/media/anime/cover/small/bx170001.jpg"));
    let next = d.media().next_airing;
    let mine = d.mine.unwrap();
    assert_eq!((mine.status.as_str(), mine.progress, mine.score, mine.start_date.as_deref(), mine.finish_date), ("watching", 6, Score::from_points(85), Some("2026-10-04"), None));
    assert_eq!(next, Some(NextAiring { episode: 8, at: 1_795_271_400 }));
}

#[test]
fn a_season_page_reads_shows_mine_and_exact_airing() {
    let (shows, more) = parse_season(data(SEASON)).unwrap();
    assert!(more);
    assert_eq!(shows.len(), 2);
    assert_eq!((shows[0].members, shows[0].mine.clone(), shows[0].mean), (Some(52011), Some(("watching".to_string(), 6)), Some(8.1)));
    assert_eq!(shows[0].next_airing, Some(NextAiring { episode: 8, at: 1_795_271_400 }));
    assert_eq!(airing::Timetable::of(&shows[0]).map(|t| t.aired(1_795_271_400 - 60)), Some(7));
    assert_eq!((shows[1].air_status.as_str(), shows[1].start_date.as_deref(), shows[1].mine.clone()), ("not_yet_aired", Some("2026-12"), None));
    let (mut c, seen, _) = client(vec![ok(SEASON)]);
    c.season(JWT, airing::Season { year: 2026, quarter: 3 }, 1).unwrap();
    assert_eq!(seen.lock().unwrap()[0].2["variables"], json!({ "season": "FALL", "year": 2026, "page": 2 }));
}

#[test]
fn sign_in_checks_the_token_before_saving_it() {
    let (dir, tokens) = token_dir("signin", None);
    let path = tokens.path().to_path_buf();
    let token = parse_token_input(JWT, 1_700_000_000).unwrap();
    let (c, _, _) = client(vec![status(400, r#"{"errors":[{"message":"Invalid token","status":400}]}"#), ok(VIEWER_JSON)]);
    let mut al = AniList::new(c, TokenStore::new(path.clone()));
    assert_eq!(al.complete_sign_in(&token), Err(ServiceError::SignInAgain));
    assert_eq!(TokenStore::new(path.clone()).load_anilist(), None, "a refused token is never saved");
    assert_eq!(al.complete_sign_in(&token).unwrap().name, "demo_user");
    assert_eq!(TokenStore::new(path).load_anilist(), Some(token));
    let _ = std::fs::remove_dir_all(dir);
}
