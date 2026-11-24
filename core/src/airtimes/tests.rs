//! No network: `fetch` is a closure that records what it was asked.

use super::*;
use crate::airing::{Broadcast, Timetable};
use crate::store::tests::item;

const NOW: i64 = 1_795_000_000;

fn show(id: u32, status: Status, air: &str) -> ListItem {
    let mut it = item(id, &format!("Show {id}"), status, 0, 1);
    it.media.air_status = air.into();
    it.media.start_date = Some("2026-10-03".into());
    it.media.broadcast = Broadcast::parse("saturday", Some("23:30"));
    it
}

fn list() -> Vec<ListItem> {
    vec![
        show(10, Status::Watching, "currently_airing"),
        show(11, Status::PlanToWatch, "not_yet_aired"),
        show(12, Status::Completed, "currently_airing"),
        show(13, Status::Watching, "finished_airing"),
        show(14, Status::Dropped, "currently_airing"),
        show(15, Status::OnHold, "currently_airing"),
    ]
}

fn next(episode: u32, at: i64) -> Option<NextAiring> {
    Some(NextAiring { episode, at })
}

/// Answers every id: id 10 has episode 5 in a day, the others nothing.
fn answer(ids: &[u32]) -> std::result::Result<Vec<(u32, Option<NextAiring>)>, AirFail> {
    Ok(ids.iter().map(|&id| (id, if id == 10 { next(5, NOW + 86_400) } else { None })).collect())
}

#[test]
fn only_airing_watching_and_plan_to_watch_are_asked_for() {
    assert_eq!(wanted(&list()), vec![10, 11]);
    let mut manga = show(16, Status::Watching, "currently_airing");
    manga.media.kind = Kind::Manga;
    assert_eq!(wanted(&[manga]), Vec::<u32>::new());
}

#[test]
fn fresh_times_are_not_asked_again() {
    let st = Store::open_memory().unwrap();
    let mut asked: Vec<Vec<u32>> = Vec::new();
    let mut fetch = |ids: &[u32]| {
        asked.push(ids.to_vec());
        answer(ids)
    };
    let r = refresh(&st, &list(), NOW, &mut fetch);
    assert_eq!(r, Outcome::Updated(HashMap::from([(10, NextAiring { episode: 5, at: NOW + 86_400 })])));
    // Within MAX_AGE: nothing asked, nothing changed (11's "no data" is kept too).
    assert_eq!(refresh(&st, &list(), NOW + MAX_AGE - 1, &mut fetch), Outcome::Unchanged);
    // Older: both again, in one request.
    assert!(matches!(refresh(&st, &list(), NOW + MAX_AGE, &mut fetch), Outcome::Updated(_)));
    assert_eq!(asked, vec![vec![10, 11], vec![10, 11]]);
}

#[test]
fn an_aired_episode_asks_for_the_next_one_soon_after() {
    let st = Store::open_memory().unwrap();
    let at = NOW + 3600;
    let mut fetch = |ids: &[u32]| Ok(ids.iter().map(|&id| (id, next(5, at))).collect());
    refresh(&st, &list(), NOW, &mut fetch);
    let c = Cache::load(&st);
    assert_eq!(c.due(&[10], at + AIRED_GAP - 1), Vec::<u32>::new());
    assert_eq!(c.due(&[10], at + AIRED_GAP), vec![10]);
}

#[test]
fn many_shows_go_fifty_a_request_with_a_cap() {
    let st = Store::open_memory().unwrap();
    let many: Vec<ListItem> = (1..=260).map(|id| show(id, Status::Watching, "currently_airing")).collect();
    let sizes = std::cell::RefCell::new(Vec::new());
    let mut fetch = |ids: &[u32]| {
        sizes.borrow_mut().push(ids.len());
        answer(ids)
    };
    refresh(&st, &many, NOW, &mut fetch);
    assert_eq!(*sizes.borrow(), vec![50, 50, 50, 50], "at most MAX_REQUESTS");
    // The rest the next time.
    refresh(&st, &many, NOW + 60, &mut fetch);
    assert_eq!(sizes.borrow()[4..], [50, 10]);
}

#[test]
fn a_429_holds_every_request_for_its_retry_after() {
    let st = Store::open_memory().unwrap();
    let mut calls = 0;
    let mut slow = |_: &[u32]| {
        calls += 1;
        Err(AirFail::Wait(120))
    };
    assert_eq!(refresh(&st, &list(), NOW, &mut slow), Outcome::Failed);
    assert_eq!(refresh(&st, &list(), NOW + 119, &mut slow), Outcome::Unchanged, "held");
    assert_eq!(calls, 1);
    let mut fine = answer;
    assert!(matches!(refresh(&st, &list(), NOW + 120, &mut fine), Outcome::Updated(_)));
    // An absurd Retry-After is cut.
    let st = Store::open_memory().unwrap();
    refresh(&st, &list(), NOW, &mut |_: &[u32]| Err(AirFail::Wait(u64::MAX)));
    assert_eq!(Cache::load(&st).hold_until, NOW + MAX_HOLD);
}

#[test]
fn a_failure_keeps_the_times_already_kept() {
    let st = Store::open_memory().unwrap();
    refresh(&st, &list(), NOW, &mut answer);
    let later = NOW + MAX_AGE;
    let mut down = |_: &[u32]| Err(AirFail::Failed);
    assert_eq!(refresh(&st, &list(), later, &mut down), Outcome::Failed);
    let c = Cache::load(&st);
    assert_eq!(c.usable(later).get(&10), next(5, NOW + 86_400).as_ref());
    assert!(c.held(later + FAIL_HOLD - 1) && !c.held(later + FAIL_HOLD));
}

#[test]
fn old_times_give_way_to_the_estimate() {
    let st = Store::open_memory().unwrap();
    refresh(&st, &list(), NOW, &mut answer);
    let c = Cache::load(&st);
    assert!(c.usable(NOW + KEEP - 1).contains_key(&10));
    assert!(c.usable(NOW + KEEP).is_empty());
}

#[test]
fn shows_no_longer_wanted_are_forgotten() {
    let st = Store::open_memory().unwrap();
    refresh(&st, &list(), NOW, &mut answer);
    let mut l = list();
    l[0].status = Status::Completed;
    assert_eq!(refresh(&st, &l, NOW + 60, &mut |_: &[u32]| panic!("nothing due")), Outcome::Unchanged);
    assert!(Cache::load(&st).usable(NOW + 60).is_empty());
}

#[test]
fn the_saved_myanimelist_list_carries_the_times() {
    let mut st = Store::open_memory().unwrap();
    let now = crate::time::now_unix();
    st.replace_list("mal", &list(), now).unwrap();
    st.replace_list("anilist", &list(), now).unwrap();
    refresh(&st, &st.load_list("mal").unwrap(), now, &mut |ids: &[u32]| Ok(ids.iter().map(|&id| (id, if id == 10 { next(5, now + 86_400) } else { None })).collect()));
    let mal = st.load_list("mal").unwrap();
    let ten = mal.iter().find(|it| it.media.id == 10).unwrap();
    assert_eq!(ten.media.next_airing, next(5, now + 86_400));
    assert!(mal.iter().filter(|it| it.media.id != 10).all(|it| it.media.next_airing.is_none()), "no data: the estimate");
    assert_eq!(st.load_item("mal", 10).unwrap().unwrap().media.next_airing, next(5, now + 86_400));
    // Exact beats the slot; the others keep the estimate.
    assert_eq!(Timetable::of_media(&ten.media).map(|t| t.next(now)), Some(Some((5, now + 86_400))));
    let eleven = mal.iter().find(|it| it.media.id == 11).unwrap();
    assert_eq!(Timetable::of_media(&eleven.media), Timetable::new("2026-10-03", Broadcast::parse("saturday", Some("23:30")).unwrap(), Some(12), None));
    // Another service's list is its own.
    assert!(st.load_list("anilist").unwrap().iter().all(|it| it.media.next_airing.is_none()));
    // Sign out forgets them.
    st.clear_service("mal").unwrap();
    assert_eq!(Cache::load(&st), Cache::default());
}

#[test]
fn with_no_times_kept_the_saved_list_reads_as_before() {
    // The phones never look times up: what the list saved stays as it was.
    let mut st = Store::open_memory().unwrap();
    let mut l = list();
    l[0].media.next_airing = next(3, NOW);
    st.replace_list("mal", &l, NOW).unwrap();
    assert_eq!(st.load_item("mal", 10).unwrap().unwrap().media.next_airing, next(3, NOW));
    assert!(st.load_list("mal").unwrap().iter().filter(|it| it.media.id != 10).all(|it| it.media.next_airing.is_none()));
}

#[test]
fn a_new_episode_note_follows_the_exact_time() {
    let mut st = Store::open_memory().unwrap();
    let now = crate::time::now_unix();
    let mut l = list();
    l[0].progress = 4;
    st.replace_list("mal", &l, now).unwrap();
    // Episode 5 aired an hour ago, at a time the weekly slot doesn't say.
    let aired = now - 3600;
    refresh(&st, &l, now, &mut |ids: &[u32]| Ok(ids.iter().map(|&id| (id, if id == 10 { next(5, aired) } else { None })).collect()));
    let it = st.load_item("mal", 10).unwrap().unwrap();
    assert_eq!(crate::airnotes::new_episode(&it, now), Some(5));
    assert_eq!(crate::airnotes::new_episode(&it, aired - 60), None, "not before it airs");
}
