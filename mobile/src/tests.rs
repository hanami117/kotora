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

// ------------------------------------------------------------------- airing and notifications

mod airing_week {
    use kotora_core::airing::{Broadcast, SeasonAnime, WEEK};
    use kotora_core::airnotes::What;
    use kotora_core::mangaupdates::Sources;
    use kotora_core::time::Clock;

    use super::*;
    use crate::airing::{air_note, chapter_note, week};

    /// Saturday 2026-10-03 23:30 JST (14:30 UTC): episode 1.
    const EP1: i64 = 1_791_037_800;
    const JST: i64 = 9 * 3600;

    fn show(id: u32, title: &str, format: &str) -> SeasonAnime {
        SeasonAnime {
            id,
            title: title.into(),
            format: format.into(),
            episodes: Some(12),
            air_status: "currently_airing".into(),
            start_date: Some("2026-10-03".into()),
            broadcast: Broadcast::parse("saturday", Some("23:30")),
            members: Some(1000 - id),
            ..Default::default()
        }
    }

    fn tokyo(t: i64) -> Clock {
        Clock::at(t, JST)
    }

    #[test]
    fn the_week_has_seven_days_with_the_desktops_texts() {
        // Friday 2026-10-23 12:00 JST: episode 4 is tomorrow at 23:30.
        let now = EP1 + 3 * WEEK - 86_400 - 11 * 3600 - 1800;
        let shows = vec![show(1, "Hoshikuzu Relay", "tv"), show(2, "Kagerou Bakery", "ona"), SeasonAnime { broadcast: Broadcast::parse("monday", None), ..show(3, "No Time", "tv") }];
        let list = vec![item(1, "Hoshikuzu Relay", Status::Watching, 2, Some(12), 1)];
        let (days, untimed) = week(&shows, &list, Service::Mal, AiringType::All, AiringMine::All, now, &tokyo);
        assert_eq!(days.len(), 7);
        assert_eq!(days[0].label, "Today \u{b7} Friday Oct 23");
        assert!(days[0].today && days[0].slots.is_empty());
        assert_eq!(days[1].label, "Tomorrow \u{b7} Saturday Oct 24");
        assert_eq!(days[2].label, "Sunday Oct 25");
        let s = &days[1].slots;
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].id, s[0].episode, s[0].time.as_str(), s[0].mine), (1, 4, "23:30", true), "mine first at the same time");
        assert_eq!(s[0].note, "in 1 d 11 h");
        assert_eq!(s[0].to_watch.as_deref(), Some("~1 to watch"), "episode 3 is out, the user is at 2");
        assert_eq!((s[1].note.as_str(), s[1].format_label.as_str(), s[1].status), ("", "ONA", None), "not on the list: no countdown");
        assert_eq!(days[1].on_list, 1);
        assert_eq!(untimed, vec![AiringUntimed { id: 3, title: "No Time".into(), cover_url: None, why: "Mondays in Japan, time not known".into(), status: None }]);
        // The filters.
        let (days, _) = week(&shows, &list, Service::Mal, AiringType::Ona, AiringMine::All, now, &tokyo);
        assert_eq!(days[1].slots.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
        let (days, untimed) = week(&shows, &list, Service::Mal, AiringType::All, AiringMine::On, now, &tokyo);
        assert_eq!(days[1].slots.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1]);
        assert!(untimed.is_empty());
    }

    #[test]
    fn an_aired_slot_today_says_aired_and_a_progress_ahead_moves_it() {
        // Saturday 2026-10-24 23:40 JST: episode 4 aired ten minutes ago.
        let now = EP1 + 3 * WEEK + 600;
        let list = vec![item(1, "Hoshikuzu Relay", Status::Watching, 4, Some(12), 1)];
        let (days, _) = week(&[show(1, "Hoshikuzu Relay", "tv")], &list, Service::Mal, AiringType::All, AiringMine::All, now, &tokyo);
        assert_eq!((days[0].slots[0].episode, days[0].slots[0].note.as_str()), (4, "aired"));
        assert_eq!(days[0].slots[0].to_watch.as_deref(), Some("Caught up"));
        // Watched 5 already: the estimate moves forward, today's slot is episode 5.
        let list = vec![item(1, "Hoshikuzu Relay", Status::Watching, 5, Some(12), 1)];
        let (days, _) = week(&[show(1, "Hoshikuzu Relay", "tv")], &list, Service::Mal, AiringType::All, AiringMine::All, now, &tokyo);
        assert_eq!(days[0].slots[0].episode, 5);
    }

    #[test]
    fn notification_texts_and_switches_are_the_desktops() {
        let on = NotifySettings::default();
        assert_eq!(on, NotifySettings { started_airing: true, new_episode: true, new_chapter: false });
        let ptw = item(4, "Neko Tantei Rin", Status::PlanToWatch, 0, None, 1);
        let n = air_note(&ptw, What::Started, on).unwrap();
        assert_eq!((n.id.as_str(), n.summary.as_str(), n.body.as_str()), ("kotora-4-started", "Neko Tantei Rin started airing", "It\u{2019}s on your Plan to watch list."));
        let w = item(1, "Hoshikuzu Relay", Status::Watching, 6, Some(12), 1);
        let n = air_note(&w, What::Episode(7), on).unwrap();
        assert_eq!((n.id.as_str(), n.kind, n.summary.as_str()), ("kotora-1-ep7", NoteKind::Episode, "Hoshikuzu Relay \u{b7} Ep 7 is out"));
        assert_eq!(n.body, "Estimated from the broadcast time. You\u{2019}re at episode 6.");
        assert_eq!(air_note(&w, What::Episode(6), on), None, "already watched");
        assert_eq!(air_note(&w, What::Started, on), None, "not Plan to watch any more");
        let off = NotifySettings { started_airing: false, new_episode: false, new_chapter: false };
        assert_eq!((air_note(&ptw, What::Started, off), air_note(&w, What::Episode(7), off)), (None, None));
        let mut m = item(9, "Kaze no Shoten", Status::Watching, 70, None, 1);
        m.media.kind = Kind::Manga;
        let both = Sources { mu: true, md: true };
        let n = chapter_note(&m, 72, "Ch 72", both).unwrap();
        assert_eq!((n.id.as_str(), n.summary.as_str(), n.body.as_str()), ("kotora-m9-ch72", "New chapter of Kaze no Shoten: Ch 72", "2 chapters to read. Release data: MangaUpdates, MangaDex."));
        assert_eq!(chapter_note(&m, 71, "Ch 71", both).unwrap().body, "Release data: MangaUpdates, MangaDex.");
        assert_eq!(chapter_note(&m, 70, "Ch 70", both), None);
    }

    #[test]
    fn settings_are_kept_and_signed_out_there_is_nothing() {
        let dir = scratch("notify");
        let (k, _) = open(&dir, ids());
        assert_eq!(k.air_notes(), Ok(vec![]));
        assert_eq!(k.chapter_notes(), Ok(vec![]));
        assert!(matches!(k.airing_week(AiringType::All, AiringMine::All), Err(KotoraError::NotSignedIn)));
        let s = NotifySettings { started_airing: false, new_episode: true, new_chapter: true };
        k.set_notify_settings(s).unwrap();
        assert_eq!(k.notify_settings(), s);
        // Signed in with nothing saved: an empty, stale week (the page asks for the season).
        sign_in_fake(&k, Service::Mal);
        fill(&k);
        let w = k.airing_week(AiringType::All, AiringMine::All).unwrap();
        assert!(w.stale && w.fetched_at.is_none() && w.shows == 0 && w.service_title == "MyAnimeList");
        // Chapters: Manga is off, so no request is made.
        assert_eq!(k.chapter_notes(), Ok(vec![]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_season_shows_and_an_episode_out_is_announced_once() {
        let dir = scratch("air-notes");
        let (k, _) = open(&dir, ids());
        sign_in_fake(&k, Service::Mal);
        let now = kotora_core::time::now_unix();
        // A show whose episode came out an hour ago (by the weekly estimate, in JST).
        let at = now - 3600;
        let c = Clock::at(at, JST);
        let (y, m, d) = kotora_core::time::civil_from_days(c.day - 14);
        let days = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
        let slot = format!("{:02}:{:02}", c.minute / 60, c.minute % 60);
        let mut it = item(1, "Hoshikuzu Relay", Status::Watching, 2, Some(12), 1);
        it.media.start_date = Some(format!("{y}-{m:02}-{d:02}"));
        it.media.broadcast = Broadcast::parse(days[usize::from(c.weekday)], Some(&slot));
        k.store().replace_list("mal", &[it.clone()], 1_000).unwrap();
        let notes = k.air_notes().unwrap();
        assert_eq!(notes.iter().map(|n| n.summary.as_str()).collect::<Vec<_>>(), vec!["Hoshikuzu Relay \u{b7} Ep 3 is out"]);
        assert_eq!(k.air_notes(), Ok(vec![]), "once");
        // The season saved by an earlier fetch shows in the week.
        let cur = kotora_core::airing::Season::of(now);
        let sa = kotora_core::airing::SeasonAnime::from_item(&it);
        k.store().save_season("mal", cur, &[sa], now, &[]).unwrap();
        let w = k.airing_week(AiringType::All, AiringMine::On).unwrap();
        assert_eq!(w.shows, 1);
        assert_eq!(w.fetched_at, Some(now));
        assert!(w.stale, "the previous season isn't saved");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
