//! The API without a network: a fake platform (tokens in memory), a scratch folder, made-up
//! lists put straight into the store.

use std::collections::HashMap;
use std::sync::Mutex;

use kotora_core::{Media, Values};

use super::*;

#[derive(Default)]
struct FakePlatform {
    tokens: Mutex<HashMap<String, Vec<u8>>>,
}

impl Platform for FakePlatform {
    fn utc_offset(&self, _at_unix: i64) -> i32 {
        9 * 3600
    }
    fn log(&self, _level: LogLevel, _message: String) {}
    fn token_load(&self, service: String) -> Option<Vec<u8>> {
        self.tokens.lock().unwrap().get(&service).cloned()
    }
    fn token_save(&self, service: String, bytes: Vec<u8>) -> bool {
        self.tokens.lock().unwrap().insert(service, bytes);
        true
    }
    fn token_delete(&self, service: String) {
        self.tokens.lock().unwrap().remove(&service);
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kotora-mobile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn open(dir: &std::path::Path, ids: ClientIds) -> (Arc<Kotora>, Arc<FakePlatform>) {
    let platform = Arc::new(FakePlatform::default());
    let paths = Paths {
        config_dir: dir.join("files").display().to_string(),
        data_dir: dir.join("nobackup").display().to_string(),
        cache_dir: dir.join("cache").display().to_string(),
    };
    (Kotora::new(paths, ids, platform.clone()).unwrap(), platform)
}

fn ids() -> ClientIds {
    ClientIds { mal: "fake-mal-id".into(), anilist: "fake-anilist-id".into() }
}

fn sign_in_fake(k: &Kotora, service: Service) {
    let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
    k.tokens.save(service.name(), &t).unwrap();
    k.store().set_account(service.name(), "made-up-user").unwrap();
}

fn item(id: u32, title: &str, status: Status, progress: u32, episodes: Option<u32>, updated_at: i64) -> ListItem {
    ListItem {
        media: Media {
            kind: Kind::Anime,
            id,
            title: title.into(),
            title_en: None,
            title_ja: None,
            synonyms: Vec::new(),
            episodes,
            format: "tv".into(),
            air_status: "currently_airing".into(),
            cover_url: None,
            start_date: None,
            broadcast: None,
            volumes: None,
            authors: Vec::new(),
            next_airing: None,
            id_mal: None,
        },
        status,
        progress,
        score: kotora_core::Score::from_ten(8),
        rewatching: false,
        times_rewatched: 0,
        volumes: 0,
        updated_at,
    }
}

fn fill(k: &Kotora) {
    let list = [
        item(1, "Hoshikuzu Relay", Status::Watching, 6, Some(12), 300),
        item(2, "Kagerou Bakery", Status::Watching, 12, Some(13), 200),
        item(3, "Ashita no Lantern", Status::Watching, 24, Some(25), 100),
        ListItem { rewatching: true, ..item(5, "Kitsune Postman", Status::Completed, 3, Some(12), 40) },
        item(4, "Neko Tantei Rin", Status::PlanToWatch, 0, None, 50),
    ];
    k.store().replace_list("mal", &list, 1_000).unwrap();
}

#[test]
fn nothing_works_signed_out_and_tokens_go_to_the_platform() {
    let dir = scratch("signed-out");
    let (k, platform) = open(&dir, ids());
    assert_eq!(k.accounts(), vec![]);
    assert_eq!(k.active_service(), None);
    assert_eq!(k.list(MediaKind::Anime, None, Sort::Updated), Err(KotoraError::NotSignedIn));
    sign_in_fake(&k, Service::Mal);
    assert!(platform.tokens.lock().unwrap().contains_key("mal"), "the token is in the vault");
    assert!(!dir.join("nobackup/tokens.json").exists(), "and never in a file");
    assert_eq!(k.active_service(), Some(Service::Mal));
    assert_eq!(k.accounts()[0].user_name, "made-up-user");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_counts_sort_and_status() {
    let dir = scratch("list");
    let (k, _) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);
    let watching = k.list(MediaKind::Anime, Some(ListStatus::Watching), Sort::Updated).unwrap();
    assert_eq!(watching.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2, 3, 5], "a rewatch shows under Watching");
    assert_eq!(watching[0].score.as_deref(), Some("8"));
    assert_eq!(watching[0].total, Some(12));
    let by_title = k.list(MediaKind::Anime, None, Sort::Title).unwrap();
    assert_eq!(by_title[0].title, "Ashita no Lantern");
    let counts = k.counts(MediaKind::Anime).unwrap();
    assert_eq!(counts[0], StatusCount { status: ListStatus::Watching, count: 4 });
    assert_eq!(counts[2], StatusCount { status: ListStatus::Completed, count: 1 }, "and under Completed");
    assert_eq!(counts[1], StatusCount { status: ListStatus::PlanToWatch, count: 1 });
    assert_eq!(k.list(MediaKind::Manga, None, Sort::Updated).unwrap(), vec![]);
    assert_eq!(k.synced_at(MediaKind::Anime), Some(1_000));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plus_one_queues_completes_and_undoes() {
    let dir = scratch("plus");
    let (k, _) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);

    let e = k.plus_one(MediaKind::Anime, 1).unwrap();
    assert_eq!(e.message, "Hoshikuzu Relay: episode 7 saved");
    let r = k.entry(MediaKind::Anime, 1).unwrap().unwrap();
    assert_eq!((r.progress, r.waiting), (7, true));
    assert_eq!(k.waiting_count(), 1);
    k.undo(e).unwrap();
    let r = k.entry(MediaKind::Anime, 1).unwrap().unwrap();
    assert_eq!(r.progress, 6);
    assert_eq!(k.waiting_count(), 0, "back to where it started: nothing to send");

    // The last episode completes the entry.
    let e = k.plus_one(MediaKind::Anime, 2).unwrap();
    assert!(e.message.ends_with("episode 13 saved, completed"), "{}", e.message);
    assert_eq!(k.entry(MediaKind::Anime, 2).unwrap().unwrap().status, ListStatus::Completed);
    k.undo(e).unwrap();
    let r = k.entry(MediaKind::Anime, 2).unwrap().unwrap();
    assert_eq!((r.status, r.progress), (ListStatus::Watching, 12));

    // At the last episode: refused. Not on the list: NotFound.
    k.plus_one(MediaKind::Anime, 3).unwrap();
    assert!(matches!(k.plus_one(MediaKind::Anime, 3), Err(KotoraError::Other { .. })));
    assert_eq!(k.plus_one(MediaKind::Anime, 99), Err(KotoraError::NotFound));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sign_out_takes_the_list_and_the_token() {
    let dir = scratch("out");
    let (k, platform) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);
    k.plus_one(MediaKind::Anime, 1).unwrap();
    k.sign_out(Service::Mal).unwrap();
    assert!(platform.tokens.lock().unwrap().is_empty());
    assert_eq!(k.accounts(), vec![]);
    assert!(k.store().load_list("mal").unwrap().is_empty());
    assert_eq!(k.waiting_count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sign_in_start_uses_the_app_redirect_and_needs_a_client_id() {
    let dir = scratch("start");
    let (k, _) = open(&dir, ClientIds { mal: "fake-mal-id".into(), anilist: " ".into() });
    let p = k.sign_in_start(Service::Mal).unwrap();
    assert!(p.url.starts_with("https://myanimelist.net/v1/oauth2/authorize?"));
    assert!(p.url.contains("redirect_uri=io.github.hanami117.kotora%3A%2F%2Foauth%2Fmal"), "{}", p.url);
    assert!(p.url.contains(&format!("state={}", p.state)));
    assert!(p.url.contains("code_challenge_method=plain"));
    assert_eq!(p.verifier.len(), 64);
    assert!(!k.has_client_id(Service::AniList));
    assert_eq!(k.sign_in_start(Service::AniList), Err(KotoraError::NoClientId));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_mal_redirect_is_checked() {
    let ok = "io.github.hanami117.kotora://oauth/mal?code=abc%2Fdef&state=s1";
    assert_eq!(mal_code(ok, "s1").unwrap(), "abc/def");
    assert!(matches!(mal_code(ok, "other"), Err(KotoraError::SignInFailed { .. })));
    assert!(matches!(mal_code(ok, ""), Err(KotoraError::SignInFailed { .. })));
    let denied = "io.github.hanami117.kotora://oauth/mal?error=access_denied&state=s1";
    assert_eq!(mal_code(denied, "s1"), Err(KotoraError::SignInFailed { message: "You cancelled the sign-in on MyAnimeList.".into() }));
    assert!(mal_code("io.github.hanami117.kotora://oauth/mal?state=s1", "s1").is_err());
}

#[test]
fn a_pasted_anilist_answer_that_is_not_a_token_is_refused_before_any_request() {
    let dir = scratch("al");
    let (k, _) = open(&dir, ids());
    let r = k.sign_in_finish(PendingSignIn { service: Service::AniList, url: String::new(), state: String::new(), verifier: String::new() }, "io.github.hanami117.kotora://oauth/anilist#error=access_denied".into());
    assert!(matches!(r, Err(KotoraError::SignInFailed { .. })), "{r:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn values_round_trip_for_undo() {
    // `Values` is what Undo rebuilds from the Edit's fields.
    let v = Values { status: Status::Completed, progress: 3, score: None, rewatching: true, times_rewatched: 2, volumes: 0 };
    let back = Change { status: Some(v.status), progress: Some(v.progress), rewatching: Some(v.rewatching), times_rewatched: Some(v.times_rewatched), ..Default::default() };
    assert_eq!(back.apply(Values::new(Status::Watching, 9, None)).progress, 3);
}

#[test]
fn the_editor_steps_like_the_desktop_and_saves_one_change() {
    let dir = scratch("editor");
    let (k, _) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);

    let ed = k.editor(MediaKind::Anime, 2).unwrap();
    assert_eq!((ed.progress, ed.total, ed.status), (12, Some(13), ListStatus::Watching));
    let up = editor_step(ed.clone(), 1);
    assert_eq!((up.progress, up.status), (13, ListStatus::Completed), "the last episode completes it");
    let back = editor_step(up.clone(), -1);
    assert_eq!((back.progress, back.status), (12, ListStatus::Watching), "and stepping back undoes that");
    assert_eq!(editor_step(up, 5).progress, 13, "never past the count");
    let done = editor_set_status(ed.clone(), ListStatus::Completed);
    assert_eq!(done.progress, 13, "Completed fills the progress");

    assert_eq!(k.save_editor(ed.clone()).unwrap(), None, "nothing changed: nothing queued");
    let scale = k.score_scale().unwrap();
    assert_eq!(scale.choices.len(), 10);
    assert_eq!(scale.choices[6], ScoreChoice { points: 70, label: "7".into() });
    let msg = k.save_editor(EntryEditor { score: Some(90), times_rewatched: 1, ..done }).unwrap().unwrap();
    assert!(msg.starts_with("Kagerou Bakery: "), "{msg}");
    let r = k.entry(MediaKind::Anime, 2).unwrap().unwrap();
    assert_eq!((r.status, r.progress, r.score.as_deref(), r.times_rewatched, r.waiting), (ListStatus::Completed, 13, Some("9"), 1, true));
    assert_eq!(k.waiting_count(), 1, "one change");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rewatch_starts_stops_and_the_last_episode_counts_it() {
    let dir = scratch("rewatch");
    let (k, _) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);
    assert!(k.start_rewatch(MediaKind::Anime, 1).is_err(), "only a completed anime");
    assert!(k.stop_rewatch(MediaKind::Anime, 1).is_err());

    // Kitsune Postman is being rewatched: stop puts the progress at the last episode.
    let e = k.stop_rewatch(MediaKind::Anime, 5).unwrap();
    let r = k.entry(MediaKind::Anime, 5).unwrap().unwrap();
    assert_eq!((r.rewatching, r.progress, r.status), (false, 12, ListStatus::Completed));
    k.undo(e).unwrap();
    assert!(k.entry(MediaKind::Anime, 5).unwrap().unwrap().rewatching);
    k.stop_rewatch(MediaKind::Anime, 5).unwrap();

    let e = k.start_rewatch(MediaKind::Anime, 5).unwrap();
    assert_eq!(e.message, "Kitsune Postman: rewatch started");
    let r = k.entry(MediaKind::Anime, 5).unwrap().unwrap();
    assert_eq!((r.rewatching, r.progress, r.status), (true, 0, ListStatus::Completed));
    // The editor to the last episode ends it, one more in the count (the core's rule).
    let ed = k.editor(MediaKind::Anime, 5).unwrap();
    k.save_editor(EntryEditor { progress: 12, ..ed }).unwrap().unwrap();
    let r = k.entry(MediaKind::Anime, 5).unwrap().unwrap();
    assert_eq!((r.rewatching, r.times_rewatched, r.progress), (false, 1, 12));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_queue_lists_changes_and_discard_puts_the_entry_back() {
    let dir = scratch("queue");
    let (k, _) = open(&dir, ids());
    sign_in_fake(&k, Service::Mal);
    fill(&k);
    k.plus_one(MediaKind::Anime, 1).unwrap();
    k.plus_one(MediaKind::Anime, 1).unwrap();
    let q = k.queue_items();
    assert_eq!(q.len(), 1);
    assert_eq!((q[0].state, q[0].edits, q[0].what.as_str(), q[0].service), (QueueState::Waiting, 2, "Progress 6 \u{2192} 8", Service::Mal));
    assert_eq!(k.conflict_count(), 0);
    k.discard(Service::Mal, q[0].id).unwrap();
    assert_eq!(k.entry(MediaKind::Anime, 1).unwrap().unwrap().progress, 6);
    assert!(k.queue_items().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_conflict_shows_both_sides_in_the_desktop_words() {
    use kotora_core::queue::{Op, OpState};
    let old = Values { status: Status::Watching, progress: 6, score: None, rewatching: false, times_rewatched: 0, volumes: 0 };
    let theirs = Values { progress: 9, score: kotora_core::score::ten(7), ..old };
    let op = Op {
        id: 1,
        kind: Kind::Anime,
        media_id: 1,
        title: "Hoshikuzu Relay".into(),
        episodes: Some(12),
        change: Change { progress: Some(7), score: Some(kotora_core::score::ten(8)), ..Default::default() },
        old,
        base_updated_at: 0,
        created_at: 0,
        edited_at: 0,
        edits: 1,
        state: OpState::Conflict,
        attempts: 0,
        next_try: 0,
        error: None,
        sent_at: None,
        remote: Some(kotora_core::Remote { values: theirs, updated_at: 5 }),
        added: false,
        removed: false,
    };
    assert_eq!(edit::conflict_values(&op, &op.mine(), ScoreFormat::Point10), "Progress 7 / 12 \u{b7} Score 8");
    assert_eq!(edit::conflict_values(&op, &theirs, ScoreFormat::Point10), "Progress 9 / 12 \u{b7} Score 7");
    assert_eq!(edit::remote_text(Kind::Anime, op.remote.as_ref().unwrap(), Some(12), ScoreFormat::Point10), "9 / 12, Watching, score 7");
}
