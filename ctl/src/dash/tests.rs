use super::*;

const NOW: i64 = 1_800_000_000;

fn row(title: &str, progress: u32, episodes: Option<u32>, updated: i64) -> DashRow {
    DashRow { title: title.into(), progress, episodes, updated_at: updated, ..Default::default() }
}

fn fake() -> Dashboard {
    let disk = DashRow { on_disk: true, ..row("Lanterns of Kirigaoka", 5, Some(12), NOW - 100) };
    let soon = DashRow { air_at: Some(NOW + 2 * 86_400 + 3600), air_slot: "Wed 18:00".into(), ..row("Mecha Bento Club", 10, Some(12), NOW - 50) };
    let later = DashRow { air_at: Some(NOW + 5 * 3600), air_slot: "Mon 23:30".into(), ..row("Hoshizora", 3, None, NOW - 10) };
    let aired = DashRow { aired: true, ..row("Neon Ronin", 0, Some(24), NOW - 500) };
    let unknown = row("Velvet Orchard", 7, Some(13), NOW - 900);
    Dashboard { now_unix: NOW, now: None, rows: vec![unknown, aired, soon, later, disk], queue: 2, synced_at: Some(NOW - 300), offline: false }
}

fn lines(s: &str) -> Vec<&str> {
    s.lines().collect()
}

#[test]
fn plain_output_has_no_escape_codes_and_sorts_on_disk_then_soonest() {
    let text = render(&fake(), 80, false);
    assert!(!text.contains('\x1b'), "{text:?}");
    let l = lines(&text);
    assert_eq!(l[0], "WATCHING (5)");
    assert_eq!(l[1], "Lanterns of Kirigaoka  \u{2588}\u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}  5/12  \u{25b6} Ep 6 on disk");
    assert_eq!(l[2], "Hoshizora              \u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}   3/?  Mon 23:30 (in 5h)");
    assert_eq!(l[3], "Mecha Bento Club       \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2591}\u{2591} 10/12  Wed 18:00 (in 2d)");
    // Nothing aired and unknown go after, by most recently updated: Neon Ronin (500) before Velvet (900 ago).
    assert!(l[4].starts_with("Neon Ronin") && l[4].ends_with("aired \u{b7} not on disk"), "{}", l[4]);
    assert!(l[5].starts_with("Velvet Orchard") && l[5].ends_with("\u{2014}"), "{}", l[5]);
    assert_eq!(l[7], "2 changes waiting to sync \u{b7} last sync 5 min ago");
}

#[test]
fn every_line_fits_the_width_and_long_titles_are_cut() {
    let mut d = fake();
    d.rows[0].title = "A very long title that goes on and on and on, far past any terminal".into();
    for w in [30, 40, 56, 80, 120] {
        let text = render(&d, w, false);
        for line in text.lines() {
            assert!(width_of(line) <= w.max(30), "width {w}: {line:?} is {}", width_of(line));
        }
    }
    let text = render(&d, 80, false);
    assert!(text.contains("A very long title that goes on and on and on, far\u{2026}") || text.contains('\u{2026}'), "{text}");
    // A narrow terminal drops the bar.
    let narrow = render(&d, 40, false);
    assert!(!narrow.contains('\u{2588}') && !narrow.contains('\u{2591}'), "{narrow}");
}

#[test]
fn truncation_counts_wide_characters_as_two_columns() {
    assert_eq!(truncate("abcdef", 6), "abcdef");
    assert_eq!(truncate("abcdef", 4), "abc\u{2026}");
    assert_eq!(truncate("abcdef", 0), "");
    // Each kanji is two columns: 3 of them fit in 7 columns with the ellipsis (2+2+2 > 6).
    assert_eq!(truncate("小虎小虎小虎", 7), "小虎小\u{2026}");
    assert_eq!(width_of("小虎"), 4);
    assert_eq!(pad("小虎", 6), "小虎  ");
}

#[test]
fn colour_wraps_cells_and_leaves_the_text_alone() {
    let plain = render(&fake(), 80, false);
    let color = render(&fake(), 80, true);
    assert!(color.contains("\x1b[32m"));
    let stripped = {
        let mut out = String::new();
        let mut esc = false;
        for c in color.chars() {
            match (esc, c) {
                (false, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (false, c) => out.push(c),
                _ => {}
            }
        }
        out
    };
    assert_eq!(stripped, plain, "colour must not move a column");
}

#[test]
fn the_now_line_shows_player_title_episode_time_and_offer() {
    let n = NowInfo {
        player: "mpv".into(),
        title: "Lanterns of Kirigaoka".into(),
        episode: Some(6),
        playing: true,
        position: Some(968),
        duration: Some(1420),
        offer: Some(6),
        reached: true,
        ..Default::default()
    };
    let d = Dashboard { now: Some(n.clone()), ..fake() };
    let text = render(&d, 100, false);
    assert_eq!(lines(&text)[0], "\u{25b6} mpv \u{b7} Lanterns of Kirigaoka \u{b7} ep 6 \u{b7} 16:08/23:40 \u{b7} update to ep 6? kot confirm");
    assert_eq!(lines(&text)[1], "");
    let paused = Dashboard { now: Some(NowInfo { playing: false, reached: false, position: None, duration: None, percent: Some(40), ..n.clone() }), ..fake() };
    assert!(render(&paused, 100, false).starts_with("\u{23f8} mpv \u{b7} Lanterns of Kirigaoka \u{b7} ep 6 \u{b7} 40% \u{b7} will offer ep 6\n"));
    // A long title is cut to fit.
    let long = Dashboard { now: Some(NowInfo { title: "x".repeat(200), ..n }), ..fake() };
    assert!(width_of(render(&long, 60, false).lines().next().unwrap()) <= 60);
}

#[test]
fn an_empty_list_and_the_footer() {
    let d = Dashboard { now_unix: NOW, ..Default::default() };
    assert_eq!(render(&d, 80, false), "Nothing in Watching.\n\nqueue empty \u{b7} not synced yet\n");
    let d = Dashboard { now_unix: NOW, queue: 1, synced_at: Some(NOW - 7200), offline: true, ..Default::default() };
    assert!(render(&d, 80, false).ends_with("1 change waiting to sync \u{b7} last sync 2 h ago \u{b7} offline\n"));
}

#[test]
fn short_times() {
    assert_eq!(countdown(0), "now");
    assert_eq!(countdown(40 * 60), "in 40m");
    assert_eq!(countdown(5 * 3600), "in 5h");
    assert_eq!(countdown(2 * 86_400 + 3600), "in 2d");
    assert_eq!(ago(20), "just now");
    assert_eq!(ago(3 * 86_400), "3 d ago");
    assert_eq!(clock(59), "0:59");
    assert_eq!(clock(3725), "1:02:05");
}
