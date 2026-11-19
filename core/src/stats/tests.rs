use super::*;
use crate::activity::Row;
use crate::activity::tests::item;
use crate::time::days_from_civil;

/// UTC+1, no DST.
fn plus_one(_: i64) -> i64 {
    3600
}

/// Thursday 2026-11-19, 12:00 local (UTC+1).
fn now() -> i64 {
    days_from_civil(2026, 11, 19) * 86_400 + 11 * 3600
}

fn at(y: i64, m: i64, d: i64, hour: i64) -> i64 {
    days_from_civil(y, m, d) * 86_400 + hour * 3600 - 3600
}

fn row(media_id: u32, at: i64, from: u32, to: u32) -> Row {
    Row {
        media_id,
        at,
        from,
        to,
        vol_from: 0,
        vol_to: 0,
    }
}

fn store() -> Store {
    let mut s = Store::open_memory().unwrap();
    let mut a = item(Kind::Anime, 1, Status::Watching, 6, Some(12));
    a.media.air_status = "currently_airing".into();
    a.score = crate::score::ten(8);
    let mut b = item(Kind::Anime, 2, Status::Completed, 1, Some(1));
    b.media.format = "movie".into();
    b.times_rewatched = 1;
    b.score = crate::Score::from_points(73);
    let c = item(Kind::Anime, 3, Status::PlanToWatch, 0, Some(24));
    s.replace_list("mal", &[a, b, c], 1).unwrap();
    s
}

#[test]
fn the_list_totals_count_rewatches_and_guess_lengths() {
    let s = store();
    let st = s.stats("mal", now(), &plus_one).unwrap();
    let l = &st.list;
    assert_eq!(l.titles, 3);
    assert_eq!(l.statuses, [1, 1, 1, 0, 0]);
    assert_eq!(l.scores[7], 1);
    assert_eq!(l.scores[6], 1, "a 73 is a 7");
    // 6 episodes + the movie twice.
    assert_eq!(l.units, 8);
    assert_eq!(l.minutes, 6 * 24 + 2 * 90);
    assert_eq!(l.guessed, 2);
    assert_eq!(l.airing.iter().map(|t| t.media_id).collect::<Vec<_>>(), [1]);
    assert!(!st.has_activity());
}

#[test]
fn saved_details_give_lengths_genres_and_studios() {
    let s = store();
    let d = |id: u32, minutes: u32, genres: &[&str], studios: &[&str]| crate::service::Details {
        id,
        title: format!("Title {id}"),
        minutes: Some(minutes),
        genres: genres.iter().map(|g| g.to_string()).collect(),
        studios: studios.iter().map(|g| g.to_string()).collect(),
        ..Default::default()
    };
    s.save_details("mal", &d(1, 23, &["Action", "Drama"], &["Studio A"]), 1)
        .unwrap();
    s.save_details("mal", &d(2, 120, &["Drama"], &["Studio A", "Studio B"]), 1)
        .unwrap();
    // Plan to watch doesn't count for genres.
    s.save_details("mal", &d(3, 24, &["Comedy"], &["Studio C"]), 1)
        .unwrap();
    let l = s.stats("mal", now(), &plus_one).unwrap().list;
    assert_eq!(l.minutes, 6 * 23 + 2 * 120);
    assert_eq!(l.guessed, 0);
    assert_eq!(l.with_details, 2);
    assert_eq!(l.watched, 2);
    assert_eq!(
        l.genres,
        [("Drama".to_string(), 2), ("Action".to_string(), 1)]
    );
    assert_eq!(
        l.makers,
        [("Studio A".to_string(), 2), ("Studio B".to_string(), 1)]
    );
}

#[test]
fn the_log_gives_periods_days_streaks_and_hours() {
    let s = store();
    // Mon Nov 16 21:00, Tue 17 22:00 (2), Wed 18 nothing, Thu 19 09:00 (1); Oct 30 and last year.
    for r in [
        row(1, at(2025, 12, 31, 20), 0, 1),
        row(1, at(2026, 10, 30, 20), 1, 2),
        row(1, at(2026, 11, 16, 21), 2, 3),
        row(1, at(2026, 11, 17, 22), 3, 5),
        row(2, at(2026, 11, 19, 9), 0, 1),
    ] {
        s.log_activity("mal", r).unwrap();
    }
    let st = s.stats("mal", now(), &plus_one).unwrap();
    let l = &st.log;
    assert_eq!(st.today, days_from_civil(2026, 11, 19));
    assert_eq!(
        l.week,
        Period {
            units: 4,
            volumes: 0,
            minutes: 3 * 24 + 90,
            titles: 2
        }
    );
    assert_eq!(l.month.units, 4);
    assert_eq!(l.year.units, 5);
    assert_eq!(l.all.units, 6);
    assert_eq!(l.days.len(), DAYS);
    assert_eq!(*l.days.last().unwrap(), 1);
    assert_eq!(l.days[DAYS - 3], 2);
    assert_eq!(l.days[DAYS - 2], 0);
    // Today only (yesterday had nothing); Nov 16-17 was the best run.
    assert_eq!((l.streak, l.best_streak), (1, 2));
    assert_eq!(l.weekdays[1], 2, "Tuesday");
    assert_eq!(l.hours[22], 2);
    assert_eq!(
        l.lately
            .iter()
            .map(|t| (t.media_id, t.units))
            .collect::<Vec<_>>(),
        [(1, 4), (2, 1)]
    );
    assert_eq!(
        st.since,
        Some(at(2025, 12, 31, 20)).min(s.activity_since().unwrap())
    );
}

#[test]
fn a_streak_waits_for_today() {
    let s = store();
    s.log_activity("mal", row(1, at(2026, 11, 17, 20), 0, 1))
        .unwrap();
    s.log_activity("mal", row(1, at(2026, 11, 18, 20), 1, 2))
        .unwrap();
    assert_eq!(s.stats("mal", now(), &plus_one).unwrap().log.streak, 2);
}

#[test]
fn words() {
    assert_eq!(duration(45), "45 min");
    assert_eq!(duration(320), "5 h 20 min");
    assert_eq!(duration(120), "2 h");
    assert_eq!(duration(3 * 1440 + 4 * 60 + 5), "3 d 4 h");
    assert_eq!(grouped(1_234_567), "1,234,567");
    assert_eq!(date_text(days_from_civil(2026, 11, 19)), "Nov 19, 2026");
    let s = store();
    let t = text(&s.stats("mal", now(), &plus_one).unwrap(), &plus_one);
    assert!(t.contains("Episodes watched 8"), "{t}");
    assert!(t.contains("Nothing watched in Kotora yet"), "{t}");
}
