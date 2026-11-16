use std::collections::{HashMap, VecDeque};

use super::*;
use crate::{ListItem, Media};

/// A made-up service: its own copy of the list, scripted errors, and a log of every call.
#[derive(Default)]
struct Fake {
    remote: HashMap<u32, Remote>,
    /// Errors to answer with, one per call, before answering normally.
    errors: VecDeque<ServiceError>,
    calls: Vec<String>,
    /// The `updated_at` the next write gets.
    clock: i64,
}

impl ListService for Fake {
    fn name(&self) -> &'static str {
        "mal"
    }
    fn title(&self) -> &'static str {
        "MyAnimeList"
    }
    fn user_name(&mut self) -> std::result::Result<String, ServiceError> {
        Ok("demo_user".into())
    }
    fn fetch_list(&mut self) -> std::result::Result<Vec<ListItem>, ServiceError> {
        Ok(vec![])
    }
    fn get_entry(&mut self, id: u32) -> std::result::Result<Option<Remote>, ServiceError> {
        self.calls.push(format!("get {id}"));
        if let Some(e) = self.errors.pop_front() {
            return Err(e);
        }
        Ok(self.remote.get(&id).copied())
    }
    fn update_entry(&mut self, id: u32, change: &Change) -> std::result::Result<Remote, ServiceError> {
        self.calls.push(format!("patch {id}"));
        if let Some(e) = self.errors.pop_front() {
            return Err(e);
        }
        self.clock += 1;
        let old = self.remote.get(&id).map_or(Values::new(Status::PlanToWatch, 0, None), |r| r.values);
        let r = Remote { values: change.apply(old), updated_at: self.clock };
        self.remote.insert(id, r);
        Ok(r)
    }
}

fn v(status: Status, progress: u32, score: Option<u8>) -> Values {
    Values::new(status, progress, score)
}

fn item(id: u32, progress: u32, updated_at: i64) -> ListItem {
    ListItem {
        media: Media {
            id,
            title: format!("Show {id}"),
            title_en: None,
            title_ja: None,
            synonyms: vec![],
            episodes: Some(12),
            format: "tv".into(),
            air_status: "finished_airing".into(),
            cover_url: None,
            start_date: None,
            broadcast: None,
            authors: Vec::new(),
            kind: Default::default(),
            volumes: None,
        },
        status: Status::Watching,
        progress,
        score: Some(7),
        rewatching: false,
        times_rewatched: 0,
        updated_at,
        volumes: 0,
    }
}

/// A store with shows 1 and 2 (progress 3, updated at 100) and a service that agrees.
fn setup() -> (Store, Fake) {
    let mut s = Store::open_memory().unwrap();
    s.replace_list("mal", &[item(1, 3, 100), item(2, 3, 100)], 100).unwrap();
    let mut f = Fake { clock: 1000, ..Default::default() };
    for id in [1, 2] {
        f.remote.insert(id, Remote { values: v(Status::Watching, 3, Some(7)), updated_at: 100 });
    }
    (s, f)
}

fn progress(n: u32) -> Change {
    Change { progress: Some(n), ..Default::default() }
}

fn entry(s: &Store, id: u32) -> ListItem {
    s.load_item("mal", id).unwrap().unwrap()
}

#[test]
fn an_edit_shows_at_once_and_waits_in_the_queue() {
    let (mut s, _) = setup();
    let id = s.enqueue("mal", 1, progress(4), 500).unwrap().unwrap();
    let e = entry(&s, 1);
    assert_eq!((e.progress, e.updated_at), (4, 500));
    let q = s.queue("mal", 500).unwrap();
    assert_eq!(q.len(), 1);
    let op = &q[0];
    assert_eq!((op.id, op.media_id, op.title.as_str(), op.state, op.base_updated_at), (id, 1, "Show 1", OpState::Queued, 100));
    assert_eq!(op.old, v(Status::Watching, 3, Some(7)));
    assert_eq!(op.mine(), v(Status::Watching, 4, Some(7)));
    assert_eq!(s.open_count("mal").unwrap(), 1);
    assert!(s.enqueue("mal", 99, progress(1), 500).is_err(), "not on the list");
}

#[test]
fn edits_merge_and_an_edit_back_cancels() {
    let (mut s, _) = setup();
    s.enqueue("mal", 1, progress(4), 500).unwrap();
    s.enqueue("mal", 1, progress(5), 501).unwrap();
    s.enqueue("mal", 1, Change { score: Some(Some(9)), ..Default::default() }, 502).unwrap();
    let q = s.queue("mal", 502).unwrap();
    assert_eq!(q.len(), 1, "one change for the entry");
    assert_eq!(q[0].change, Change { progress: Some(5), score: Some(Some(9)), status: None, ..Default::default() });
    assert_eq!(q[0].edits, 3);
    // Back to the old values: nothing left to send, and the entry is as before.
    assert_eq!(s.enqueue("mal", 1, Change { progress: Some(3), score: Some(Some(7)), status: None, ..Default::default() }, 503).unwrap(), None);
    assert_eq!(s.open_count("mal").unwrap(), 0);
    let e = entry(&s, 1);
    assert_eq!((e.progress, e.score, e.updated_at), (3, Some(7), 100));
    // An edit to the value it already has queues nothing.
    assert_eq!(s.enqueue("mal", 2, progress(3), 504).unwrap(), None);
}

#[test]
fn the_worker_sends_and_saves_the_services_answer() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(4), 500).unwrap();
    s.enqueue("mal", 2, Change { status: Some(Status::Completed), score: Some(None), progress: Some(12), ..Default::default() }, 501).unwrap();
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(step(&mut s, &mut f, 601).unwrap(), Step::Handled { media_id: 2, outcome: Outcome::Sent });
    assert_eq!(step(&mut s, &mut f, 602).unwrap(), Step::Idle);
    assert_eq!(f.calls, ["get 1", "patch 1", "get 2", "patch 2"]);
    assert_eq!(f.remote[&2].values, v(Status::Completed, 12, None));
    let e = entry(&s, 2);
    assert_eq!((e.status, e.progress, e.score, e.updated_at), (Status::Completed, 12, None, 1002));
    assert_eq!(s.open_count("mal").unwrap(), 0);
    let q = s.queue("mal", 700).unwrap();
    assert_eq!(q.iter().map(|o| (o.media_id, o.state)).collect::<Vec<_>>(), [(2, OpState::Done), (1, OpState::Done)], "sent, newest first");
    // A day later the sent list is empty, and the start-up prune forgets them.
    assert!(s.queue("mal", 601 + 86_401).unwrap().is_empty());
    s.queue_start("mal", 601 + 86_401).unwrap();
    let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM pending_op", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn a_change_made_elsewhere_is_a_conflict_and_nothing_is_overwritten() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, Change { score: Some(Some(9)), ..Default::default() }, 500).unwrap();
    // The phone set the score to 6 after our base.
    f.remote.insert(1, Remote { values: v(Status::Watching, 3, Some(6)), updated_at: 400 });
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Conflict });
    assert_eq!(f.calls, ["get 1"], "no PATCH");
    let op = s.queue("mal", 600).unwrap().remove(0);
    assert_eq!(op.state, OpState::Conflict);
    assert_eq!(op.remote.unwrap().values, v(Status::Watching, 3, Some(6)));
    assert_eq!(entry(&s, 1).score, Some(9), "the list still shows the user's edit");
    assert_eq!(step(&mut s, &mut f, 601).unwrap(), Step::Idle, "a conflict waits for the user");
    assert_eq!(s.open_count("mal").unwrap(), 1);

    // Keep mine: sent over the service's version.
    s.keep_mine(op.id).unwrap();
    assert_eq!(step(&mut s, &mut f, 602).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(f.remote[&1].values.score, Some(9));
}

#[test]
fn take_theirs_drops_the_change_and_shows_the_services_values() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, Change { status: Some(Status::OnHold), ..Default::default() }, 500).unwrap();
    f.remote.insert(1, Remote { values: v(Status::Dropped, 4, None), updated_at: 400 });
    step(&mut s, &mut f, 600).unwrap();
    let id = s.queue("mal", 600).unwrap()[0].id;
    s.take_theirs("mal", id).unwrap();
    assert_eq!(s.open_count("mal").unwrap(), 0);
    let e = entry(&s, 1);
    assert_eq!((e.status, e.progress, e.score, e.updated_at), (Status::Dropped, 4, None, 400));
}

#[test]
fn a_higher_progress_wins_on_its_own_and_a_lower_one_asks() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(6), 500).unwrap();
    s.enqueue("mal", 2, progress(4), 500).unwrap();
    f.remote.insert(1, Remote { values: v(Status::Watching, 5, Some(7)), updated_at: 400 });
    f.remote.insert(2, Remote { values: v(Status::Watching, 5, Some(7)), updated_at: 400 });
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 2, outcome: Outcome::Conflict });
    assert_eq!(f.remote[&1].values.progress, 6);
}

#[test]
fn with_the_setting_off_a_higher_progress_asks_too() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(6), 500).unwrap();
    f.remote.insert(1, Remote { values: v(Status::Watching, 5, Some(7)), updated_at: 400 });
    assert_eq!(step_with(&mut s, &mut f, 600, false).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Conflict });
}

#[test]
fn values_already_on_the_service_count_as_sent() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(6), 500).unwrap();
    f.remote.insert(1, Remote { values: v(Status::Watching, 6, Some(7)), updated_at: 450 });
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::AlreadyThere });
    assert_eq!(f.calls, ["get 1"]);
    assert_eq!(entry(&s, 1).updated_at, 450);
}

#[test]
fn no_network_backs_off_and_tries_again_later() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(4), 500).unwrap();
    f.errors.push_back(ServiceError::Offline("no answer".into()));
    let st = step(&mut s, &mut f, 600).unwrap();
    assert!(matches!(&st, Step::Handled { outcome: Outcome::Retry(m), .. } if m.contains("can't reach")), "{st:?}");
    let op = s.queue("mal", 600).unwrap().remove(0);
    assert_eq!((op.state, op.attempts, op.next_try), (OpState::Queued, 1, 630));
    assert!(op.error.is_some());
    assert_eq!(step(&mut s, &mut f, 610).unwrap(), Step::Wait(20), "not due yet: no request");
    assert_eq!(f.calls.len(), 1);
    f.errors.push_back(ServiceError::RateLimited);
    step(&mut s, &mut f, 630).unwrap();
    assert_eq!(s.queue("mal", 630).unwrap()[0].next_try, 690, "60 s after the second try");
    // Sync now: no waiting.
    s.retry_now("mal").unwrap();
    assert_eq!(step(&mut s, &mut f, 631).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
}

#[test]
fn backoff_doubles_up_to_fifteen_minutes() {
    assert_eq!((1..=8).map(backoff).collect::<Vec<_>>(), [30, 60, 120, 240, 480, 900, 900, 900]);
    assert_eq!(backoff(0), 30);
    assert_eq!(backoff(u32::MAX), MAX_BACKOFF);
}

#[test]
fn sign_in_again_keeps_the_queue() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(4), 500).unwrap();
    f.errors.push_back(ServiceError::SignInAgain);
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::SignInAgain);
    let op = s.queue("mal", 600).unwrap().remove(0);
    assert_eq!((op.state, op.attempts), (OpState::Queued, 0));
    // The same when the read goes through and the write is refused.
    let f2 = Fake { remote: f.remote.clone(), ..Default::default() };
    struct WriteRefused(Fake);
    impl ListService for WriteRefused {
        fn name(&self) -> &'static str {
            "mal"
        }
        fn title(&self) -> &'static str {
            "MyAnimeList"
        }
        fn user_name(&mut self) -> std::result::Result<String, ServiceError> {
            Ok(String::new())
        }
        fn fetch_list(&mut self) -> std::result::Result<Vec<ListItem>, ServiceError> {
            Ok(vec![])
        }
        fn get_entry(&mut self, id: u32) -> std::result::Result<Option<Remote>, ServiceError> {
            self.0.get_entry(id)
        }
        fn update_entry(&mut self, _: u32, _: &Change) -> std::result::Result<Remote, ServiceError> {
            Err(ServiceError::SignInAgain)
        }
    }
    let mut w = WriteRefused(f2);
    assert_eq!(step(&mut s, &mut w, 601).unwrap(), Step::SignInAgain);
    assert_eq!(s.open_count("mal").unwrap(), 1);
    assert_eq!(entry(&s, 1).progress, 4);
}

#[test]
fn a_refused_change_fails_with_the_services_words_until_edited_or_discarded() {
    let (mut s, mut f) = setup();
    s.enqueue("mal", 1, progress(13), 500).unwrap();
    f.errors.extend([ServiceError::Other("MyAnimeList answered HTTP 400 (out of range)".into())]);
    // The error comes on the read here; the same path holds for the write.
    let st = step(&mut s, &mut f, 600).unwrap();
    assert_eq!(st, Step::Handled { media_id: 1, outcome: Outcome::Failed("MyAnimeList answered HTTP 400 (out of range)".into()) });
    let op = s.queue("mal", 600).unwrap().remove(0);
    assert_eq!((op.state, op.error.as_deref()), (OpState::Failed, Some("MyAnimeList answered HTTP 400 (out of range)")));
    assert_eq!(step(&mut s, &mut f, 601).unwrap(), Step::Idle, "a failed change is not retried by itself");
    // Edit: a fresh start.
    s.enqueue("mal", 1, progress(12), 602).unwrap();
    let op = s.queue("mal", 602).unwrap().remove(0);
    assert_eq!((op.state, op.error, op.change.progress, op.edits), (OpState::Queued, None, Some(12), 2));
    // Discard: the entry goes back.
    s.discard("mal", op.id).unwrap();
    let e = entry(&s, 1);
    assert_eq!((e.progress, e.updated_at), (3, 100));
    assert_eq!(s.open_count("mal").unwrap(), 0);
}

#[test]
fn a_full_sync_keeps_waiting_changes_on_top() {
    let (mut s, _) = setup();
    s.enqueue("mal", 1, Change { progress: Some(5), score: Some(None), status: None, ..Default::default() }, 500).unwrap();
    // The fetched list still has the old values, and show 2 is gone from it.
    s.replace_list("mal", &[item(1, 3, 100)], 700).unwrap();
    let e = entry(&s, 1);
    assert_eq!((e.progress, e.score, e.updated_at), (5, None, 500));
    s.enqueue("mal", 2, progress(1), 701).unwrap_err();
    // A change for an entry removed elsewhere keeps its title and asks.
    s.replace_list("mal", &[item(2, 3, 100)], 800).unwrap();
    let q = s.queue("mal", 800).unwrap();
    assert_eq!(q[0].title, "Show 1");
    let mut f = Fake::default();
    assert_eq!(step(&mut s, &mut f, 900).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Conflict });
    assert_eq!(s.queue("mal", 900).unwrap()[0].remote, None, "removed there");
    // Keep mine puts it back on the list; take theirs would have dropped it.
    s.keep_mine(q[0].id).unwrap();
    assert_eq!(step(&mut s, &mut f, 901).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(f.remote[&1].values.progress, 5);
}

#[test]
fn take_theirs_on_a_removed_entry_drops_it() {
    let (mut s, _) = setup();
    s.enqueue("mal", 1, progress(5), 500).unwrap();
    let mut f = Fake::default();
    step(&mut s, &mut f, 600).unwrap();
    let id = s.queue("mal", 600).unwrap()[0].id;
    s.take_theirs("mal", id).unwrap();
    assert!(s.load_item("mal", 1).unwrap().is_none());
}

#[test]
fn an_edit_while_sending_waits_behind_and_starts_from_the_new_state() {
    let (mut s, mut f) = setup();
    let first = s.enqueue("mal", 1, progress(4), 500).unwrap().unwrap();
    s.set_state(first, OpState::Sending).unwrap();
    let second = s.enqueue("mal", 1, progress(5), 501).unwrap().unwrap();
    assert_ne!(first, second, "the change on its way is not touched");
    assert_eq!(s.queue("mal", 501).unwrap()[1].old.progress, 4);
    // The first lands: the service answers progress 4 with a new time.
    s.set_state(first, OpState::Queued).unwrap();
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(entry(&s, 1).progress, 5, "the second change still shows");
    let q = s.queue("mal", 600).unwrap();
    assert_eq!(q[0].base_updated_at, f.remote[&1].updated_at, "rebased on our own write");
    assert_eq!(step(&mut s, &mut f, 601).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::Sent });
    assert_eq!(f.remote[&1].values.progress, 5);
}

#[test]
fn a_change_caught_mid_send_goes_again_at_start() {
    let (mut s, mut f) = setup();
    let id = s.enqueue("mal", 1, progress(4), 500).unwrap().unwrap();
    s.set_state(id, OpState::Sending).unwrap();
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Idle);
    s.queue_start("mal", 600).unwrap();
    // It did reach the service before the crash: found there, not sent twice.
    f.remote.insert(1, Remote { values: v(Status::Watching, 4, Some(7)), updated_at: 550 });
    assert_eq!(step(&mut s, &mut f, 601).unwrap(), Step::Handled { media_id: 1, outcome: Outcome::AlreadyThere });
}

#[test]
fn sign_out_forgets_the_queue() {
    let (mut s, _) = setup();
    s.enqueue("mal", 1, progress(4), 500).unwrap();
    s.clear_service("mal").unwrap();
    assert_eq!(s.open_count("mal").unwrap(), 0);
}

#[test]
fn the_schema_moves_up_from_version_1_and_keeps_the_saved_list() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::store::MIGRATIONS[0]).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute_batch(
        "INSERT INTO media (service, id, title, fetched_at) VALUES ('mal', 1, 'Old', 1);
         INSERT INTO entry (service, media_id, status, progress, updated_at) VALUES ('mal', 1, 'watching', 2, 5);",
    )
    .unwrap();
    let mut s = Store::from_connection(conn).unwrap();
    assert_eq!(s.schema_version().unwrap(), crate::store::MIGRATIONS.len());
    assert_eq!(entry(&s, 1).progress, 2);
    s.enqueue("mal", 1, progress(3), 9).unwrap();
}

#[test]
fn changes_read_plainly() {
    let old = v(Status::Watching, 3, Some(7));
    assert_eq!(describe(&progress(5), &old), "Progress 3 \u{2192} 5");
    assert_eq!(describe(&Change { status: Some(Status::Completed), score: Some(Some(8)), progress: None, ..Default::default() }, &old), "Status \u{2192} Completed, score 7 \u{2192} 8");
    assert_eq!(describe(&Change { score: Some(None), ..Default::default() }, &old), "Score removed");
    assert_eq!(describe(&Change { score: Some(Some(6)), ..Default::default() }, &v(Status::Watching, 0, None)), "Score 6");
}

fn new_media(id: u32, episodes: Option<u32>) -> Media {
    Media { id, title: format!("New {id}"), title_en: None, title_ja: None, synonyms: vec![], episodes, format: "tv".into(), air_status: "finished_airing".into(), cover_url: None, start_date: None, broadcast: None, ..Default::default() }
}

#[test]
fn an_add_shows_at_once_and_is_sent_as_a_new_entry() {
    let (mut s, mut f) = setup();
    let id = s.add_to_list("mal", &new_media(7, Some(12)), Status::Watching, None, 500).unwrap().unwrap();
    let e = entry(&s, 7);
    assert_eq!((e.status, e.progress, e.score, e.media.title.as_str()), (Status::Watching, 0, None, "New 7"));
    let op = &s.queue("mal", 500).unwrap()[0];
    assert_eq!((op.id, op.added, op.change.status), (id, true, Some(Status::Watching)));
    assert_eq!(describe_op(op), "Add as Watching");
    // Not on the service yet: that is no conflict.
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 7, outcome: Outcome::Sent });
    assert_eq!(f.remote[&7].values.status, Status::Watching);
    assert_eq!(entry(&s, 7).updated_at, f.remote[&7].updated_at);
}

#[test]
fn completed_adds_start_at_the_last_episode() {
    let (mut s, _) = setup();
    s.add_to_list("mal", &new_media(7, Some(12)), Status::Completed, None, 500).unwrap();
    assert_eq!(entry(&s, 7).progress, 12);
    let op = &s.queue("mal", 500).unwrap()[0];
    assert_eq!(describe_op(op), "Add as Completed, progress 12");
    // Unknown count: 0.
    s.add_to_list("mal", &new_media(8, None), Status::Completed, None, 500).unwrap();
    assert_eq!(entry(&s, 8).progress, 0);
}

/// Now Playing's "Add as Watching at episode N": one change that adds and records.
#[test]
fn an_add_can_start_at_an_episode() {
    let (mut s, mut f) = setup();
    s.add_to_list("mal", &new_media(7, Some(12)), Status::Watching, Some(4), 500).unwrap();
    assert_eq!(entry(&s, 7).progress, 4);
    let q = s.queue("mal", 500).unwrap();
    assert_eq!(q.len(), 1, "one change");
    assert_eq!(describe_op(&q[0]), "Add as Watching, progress 4");
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 7, outcome: Outcome::Sent });
    assert_eq!((f.remote[&7].values.status, f.remote[&7].values.progress), (Status::Watching, 4));
    // Never past the count; an unknown count takes the episode as it is.
    s.add_to_list("mal", &new_media(8, Some(12)), Status::Watching, Some(30), 700).unwrap();
    assert_eq!(entry(&s, 8).progress, 12);
    s.add_to_list("mal", &new_media(9, None), Status::Watching, Some(30), 700).unwrap();
    assert_eq!(entry(&s, 9).progress, 30);
    // Already on the list (Watching at 3): status and progress as one edit (the same status
    // isn't sent again).
    s.add_to_list("mal", &new_media(1, Some(12)), Status::OnHold, Some(5), 700).unwrap();
    let op = s.queue("mal", 700).unwrap().into_iter().find(|o| o.media_id == 1).unwrap();
    assert_eq!((op.change.status, op.change.progress), (Some(Status::OnHold), Some(5)));
}

#[test]
fn edits_merge_into_an_add_and_keep_its_status() {
    let (mut s, _) = setup();
    s.add_to_list("mal", &new_media(7, Some(12)), Status::PlanToWatch, None, 500).unwrap();
    s.enqueue("mal", 7, progress(1), 501).unwrap();
    let q = s.queue("mal", 501).unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].change, Change { status: Some(Status::PlanToWatch), progress: Some(1), score: None, ..Default::default() }, "the status still goes");
    s.enqueue("mal", 7, progress(0), 502).unwrap();
    assert_eq!(s.open_count("mal").unwrap(), 1, "an edit back to 0 still leaves the add");
}

#[test]
fn discarding_an_add_takes_it_off_the_list() {
    let (mut s, _) = setup();
    let id = s.add_to_list("mal", &new_media(7, Some(12)), Status::Watching, None, 500).unwrap().unwrap();
    s.discard("mal", id).unwrap();
    assert!(s.load_item("mal", 7).unwrap().is_none());
    assert_eq!(s.open_count("mal").unwrap(), 0);
}

#[test]
fn an_add_survives_a_full_sync_until_sent() {
    let (mut s, _) = setup();
    s.add_to_list("mal", &new_media(7, Some(12)), Status::OnHold, None, 500).unwrap();
    s.replace_list("mal", &[item(1, 3, 100), item(2, 3, 100)], 700).unwrap();
    let e = entry(&s, 7);
    assert_eq!((e.status, e.media.title.as_str()), (Status::OnHold, "New 7"));
}

#[test]
fn an_add_of_an_anime_on_the_list_changes_its_status() {
    let (mut s, _) = setup();
    s.add_to_list("mal", &new_media(1, Some(12)), Status::OnHold, None, 500).unwrap();
    let op = &s.queue("mal", 500).unwrap()[0];
    assert!(!op.added);
    assert_eq!(op.change, Change { status: Some(Status::OnHold), ..Default::default() });
    assert_eq!(entry(&s, 1).media.title, "Show 1", "the saved titles stay");
}

#[test]
fn an_add_finds_the_anime_already_on_the_service() {
    let (mut s, mut f) = setup();
    f.remote.insert(7, Remote { values: v(Status::Completed, 12, Some(9)), updated_at: 300 });
    s.add_to_list("mal", &new_media(7, Some(12)), Status::Watching, None, 500).unwrap();
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 7, outcome: Outcome::Conflict }, "nothing overwritten");
    // Same values there: already there.
    let (mut s, mut f) = setup();
    f.remote.insert(7, Remote { values: v(Status::Watching, 0, None), updated_at: 300 });
    s.add_to_list("mal", &new_media(7, Some(12)), Status::Watching, None, 500).unwrap();
    assert_eq!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { media_id: 7, outcome: Outcome::AlreadyThere });
}

/// A rewatch goes through the queue like any edit: start, count, finish in one change, and the
/// saved entry follows at once. Its fields survive the database round trip.
#[test]
fn a_rewatch_is_queued_and_sent() {
    let mut s = Store::open_memory().unwrap();
    s.replace_list("mal", &[ListItem { status: Status::Completed, progress: 12, times_rewatched: 1, ..item(1, 12, 300) }], 300).unwrap();
    let cur = s.load_item("mal", 1).unwrap().unwrap().values();
    let start = Change::start_rewatch(&cur).unwrap();
    s.enqueue("mal", 1, start, 400).unwrap();
    let e = s.load_item("mal", 1).unwrap().unwrap();
    assert_eq!((e.status, e.progress, e.rewatching, e.times_rewatched), (Status::Completed, 0, true, 1));
    let q = s.queue("mal", 400).unwrap();
    assert_eq!((q[0].change.rewatching, q[0].old.rewatching, q[0].old.times_rewatched), (Some(true), false, 1));
    assert_eq!(describe(&q[0].change, &q[0].old), "Progress 12 \u{2192} 0, rewatch started");
    // Episodes 1..12 merge into the waiting change; the last one ends the rewatch.
    for ep in 1..=12 {
        let cur = s.load_item("mal", 1).unwrap().unwrap().values();
        s.enqueue("mal", 1, Change { progress: Some(ep), ..Default::default() }.completes(&cur, Some(12)), 400 + i64::from(ep)).unwrap();
    }
    let e = s.load_item("mal", 1).unwrap().unwrap();
    assert_eq!((e.status, e.progress, e.rewatching, e.times_rewatched), (Status::Completed, 12, false, 2));
    let q = s.queue("mal", 500).unwrap();
    assert_eq!(q.len(), 1, "one change");
    assert_eq!(q[0].change, Change { times_rewatched: Some(2), ..Default::default() }, "back at 12 and not rewatching: only the count is left");
    assert_eq!(describe(&q[0].change, &q[0].old), "Rewatched twice");
    let mut f = Fake::default();
    f.remote.insert(1, Remote { values: Values { times_rewatched: 1, ..v(Status::Completed, 12, Some(8)) }, updated_at: 300 });
    assert!(matches!(step(&mut s, &mut f, 600).unwrap(), Step::Handled { outcome: Outcome::Sent, .. }));
    assert_eq!(f.remote[&1].values.times_rewatched, 2);
}
