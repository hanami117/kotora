use super::*;
use crate::store::Store;
use crate::{Change, Kind, ListItem, Media};

pub(crate) fn item(kind: Kind, id: u32, status: Status, progress: u32, total: Option<u32>) -> ListItem {
    ListItem {
        media: Media { kind, id, title: format!("Title {id}"), episodes: total, format: "tv".into(), ..Media::default() },
        status,
        progress,
        score: None,
        rewatching: false,
        times_rewatched: 0,
        volumes: 0,
        updated_at: 1,
    }
}

const T: i64 = 1_790_000_000;

fn store(items: &[ListItem]) -> Store {
    let mut s = Store::open_memory().unwrap();
    s.replace_list("mal", items, 1).unwrap();
    s
}

fn units(s: &Store) -> Vec<(u32, u32, u32)> {
    s.activity("mal").unwrap().iter().map(|r| (r.media_id, r.from, r.to)).collect()
}

#[test]
fn a_new_store_knows_when_the_log_started() {
    let s = store(&[]);
    assert!(s.activity_since().unwrap().is_some_and(|t| t > T - 10 * 365 * 86_400));
}

#[test]
fn edits_that_raise_the_progress_are_logged() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Watching, 3, Some(12))]);
    s.enqueue("mal", 1, Change { progress: Some(4), ..Default::default() }, T).unwrap();
    s.enqueue("mal", 1, Change { progress: Some(6), ..Default::default() }, T + 60).unwrap();
    // A score is no activity.
    s.enqueue("mal", 1, Change { score: Some(crate::score::ten(8)), ..Default::default() }, T + 70).unwrap();
    assert_eq!(units(&s), [(1, 3, 4), (1, 4, 6)]);
    let rows = s.activity("mal").unwrap();
    assert_eq!((rows[0].at, rows[1].at), (T, T + 60));
}

#[test]
fn undo_takes_the_activity_back() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Watching, 3, Some(12))]);
    s.enqueue("mal", 1, Change { progress: Some(4), ..Default::default() }, T).unwrap();
    s.enqueue("mal", 1, Change { progress: Some(6), ..Default::default() }, T + 60).unwrap();
    s.enqueue("mal", 1, Change { progress: Some(5), ..Default::default() }, T + 90).unwrap();
    assert_eq!(units(&s), [(1, 3, 4), (1, 4, 5)]);
    // Discard: back to 3, nothing left.
    let id = s.queue("mal", T + 100).unwrap()[0].id;
    s.discard("mal", id, T + 100).unwrap();
    assert_eq!(units(&s), []);
}

#[test]
fn an_old_correction_keeps_what_was_logged() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Watching, 3, Some(12))]);
    s.enqueue("mal", 1, Change { progress: Some(5), ..Default::default() }, T).unwrap();
    s.enqueue("mal", 1, Change { progress: Some(4), ..Default::default() }, T + 3 * 86_400).unwrap();
    assert_eq!(units(&s), [(1, 3, 5)]);
}

#[test]
fn backlog_and_adds_count_as_they_should() {
    let mut s = store(&[item(Kind::Anime, 1, Status::PlanToWatch, 0, Some(12))]);
    // Plan to watch to Completed: watched before, not now.
    s.enqueue("mal", 1, Change { status: Some(Status::Completed), progress: Some(12), ..Default::default() }, T).unwrap();
    assert_eq!(units(&s), []);
    // Added at episode 5: that one episode.
    let m = Media { id: 2, title: "Two".into(), episodes: Some(12), ..Media::default() };
    s.add_to_list("mal", &m, Status::Watching, Some(5), T).unwrap();
    assert_eq!(units(&s), [(2, 4, 5)]);
    // Added as Completed: nothing.
    let m = Media { id: 3, title: "Three".into(), episodes: Some(12), ..Media::default() };
    s.add_to_list("mal", &m, Status::Completed, None, T).unwrap();
    assert_eq!(units(&s), [(2, 4, 5)]);
}

#[test]
fn a_rewatch_that_starts_takes_nothing_back() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Watching, 11, Some(12))]);
    s.enqueue("mal", 1, Change { progress: Some(12), ..Default::default() }, T).unwrap();
    s.enqueue("mal", 1, Change { rewatching: Some(true), progress: Some(0), ..Default::default() }, T + 60).unwrap();
    s.enqueue("mal", 1, Change { progress: Some(1), ..Default::default() }, T + 120).unwrap();
    assert_eq!(units(&s), [(1, 11, 12), (1, 0, 1)]);
}

#[test]
fn manga_volumes_are_logged_too() {
    let mut s = Store::open_memory().unwrap();
    s.replace_list(("mal", Kind::Manga), &[item(Kind::Manga, 7, Status::Watching, 20, None)], 1).unwrap();
    s.enqueue(("mal", Kind::Manga), 7, Change { progress: Some(22), volumes: Some(3), ..Default::default() }, T).unwrap();
    let r = s.activity(("mal", Kind::Manga)).unwrap();
    assert_eq!((r[0].units(), r[0].volumes()), (2, 3));
    // Not in the anime log.
    assert!(s.activity("mal").unwrap().is_empty());
    // Volumes taken back, chapters kept.
    s.enqueue(("mal", Kind::Manga), 7, Change { volumes: Some(0), ..Default::default() }, T + 10).unwrap();
    let r = s.activity(("mal", Kind::Manga)).unwrap();
    assert_eq!((r[0].units(), r[0].volumes()), (2, 0));
}

#[test]
fn signing_out_keeps_the_log() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Watching, 3, Some(12))]);
    s.enqueue("mal", 1, Change { progress: Some(4), ..Default::default() }, T).unwrap();
    s.clear_service("mal").unwrap();
    assert_eq!(units(&s), [(1, 3, 4)]);
}

/// Review 2026-11-24 (A4): taking back a rewatch that just started (Undo puts the entry back at
/// its last episode, or Discard of the queued start) is no watching: nothing is logged.
#[test]
fn undoing_a_rewatch_start_logs_nothing() {
    let mut s = store(&[item(Kind::Anime, 1, Status::Completed, 12, Some(12))]);
    // Undo after the start was sent: the old values queued as a new edit.
    s.enqueue("mal", 1, Change { rewatching: Some(true), progress: Some(1), ..Default::default() }, T).unwrap();
    s.conn.execute("UPDATE pending_op SET state = 'done', sent_at = ?1", [T + 1]).unwrap();
    s.enqueue("mal", 1, Change { rewatching: Some(false), progress: Some(12), ..Default::default() }, T + 60).unwrap();
    assert_eq!(units(&s), [], "an Undo of a rewatch start counts no episodes");
    // Discard of the queued start.
    let mut s = store(&[item(Kind::Anime, 2, Status::Completed, 12, Some(12))]);
    let id = s.enqueue("mal", 2, Change { rewatching: Some(true), progress: Some(1), ..Default::default() }, T).unwrap().unwrap();
    s.discard("mal", id, T + 60).unwrap();
    assert_eq!(units(&s), [], "a Discard of a rewatch start counts no episodes");
    // A rewatch that ends at the last episode still counts it.
    let mut s = store(&[item(Kind::Anime, 3, Status::Completed, 12, Some(12))]);
    s.enqueue("mal", 3, Change { rewatching: Some(true), progress: Some(11), ..Default::default() }, T).unwrap();
    s.enqueue("mal", 3, Change { rewatching: Some(false), progress: Some(12), times_rewatched: Some(1), ..Default::default() }, T + 60).unwrap();
    assert_eq!(units(&s), [(3, 11, 12)]);
}
