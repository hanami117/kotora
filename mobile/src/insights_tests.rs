//! Statistics, settings and the volume fill's offline parts, without a network.

use std::collections::HashMap;

use kotora_core::{Media, Status};

use super::*;
use crate::{ClientIds, LogLevel, Paths, Platform};

#[derive(Default)]
struct Fake(Mutex<HashMap<String, Vec<u8>>>);

impl Platform for Fake {
    fn utc_offset(&self, _at: i64) -> i32 {
        crate::testenv::OFFSET
    }
    fn log(&self, _level: LogLevel, _message: String) {}
    fn token_load(&self, s: String) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(&s).cloned()
    }
    fn token_save(&self, s: String, b: Vec<u8>) -> bool {
        self.0.lock().unwrap().insert(s, b);
        true
    }
    fn token_delete(&self, s: String) {
        self.0.lock().unwrap().remove(&s);
    }
}

fn open(name: &str) -> Arc<Kotora> {
    let dir = std::env::temp_dir().join(format!("kotora-insights-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = |d: &str| dir.join(d).display().to_string();
    let k = Kotora::new(Paths { config_dir: p("files"), data_dir: p("nobackup"), cache_dir: p("cache") }, ClientIds { mal: "x".into(), anilist: "y".into() }, Arc::new(Fake::default())).unwrap();
    let t = kotora_core::tokens::Token { access_token: "fake".into(), refresh_token: "fake".into(), expires_at: i64::MAX / 2 };
    k.tokens.save("mal", &t).unwrap();
    k.store().set_account("mal", "made-up-user").unwrap();
    k
}

fn item(kind: Kind, id: u32, status: Status, progress: u32, total: Option<u32>) -> ListItem {
    ListItem {
        media: Media { kind, id, title: format!("Title {id}"), episodes: total, format: "tv".into(), air_status: "currently_airing".into(), ..Default::default() },
        status,
        progress,
        score: kotora_core::Score::from_ten(7),
        rewatching: false,
        times_rewatched: 0,
        volumes: 0,
        updated_at: 1,
    }
}

#[test]
fn stats_count_the_list_and_what_was_recorded() {
    let k = open("stats");
    let list = [item(Kind::Anime, 1, Status::Watching, 3, Some(12)), item(Kind::Anime, 2, Status::Completed, 12, Some(12)), item(Kind::Anime, 3, Status::PlanToWatch, 0, None)];
    k.store().replace_list("mal", &list, 1_000).unwrap();
    let before = k.stats(MediaKind::Anime).unwrap();
    assert_eq!(before.titles, 3);
    assert_eq!(before.units, 15);
    assert_eq!(before.statuses.len(), 5);
    assert_eq!(before.scores[6], 3, "three 7s");
    assert_eq!(before.week.units, 0);
    assert_eq!(before.airing.len(), 1, "Watching and airing");
    // A +1 is logged: this week, today, the streak.
    k.plus_one(MediaKind::Anime, 1).unwrap();
    let after = k.stats(MediaKind::Anime).unwrap();
    assert_eq!(after.week.units, 1);
    assert_eq!(*after.days.last().unwrap(), 1, "today");
    assert_eq!(after.streak, 1);
    assert_eq!(after.today, after.first_day + after.days.len() as i64 - 1);
    assert!(after.since_day.is_some_and(|d| d <= after.today));
    assert_eq!(after.lately.first().map(|t| t.units), Some(1));
    assert_eq!(k.stats(MediaKind::Manga).unwrap().titles, 0);
}

#[test]
fn the_desktops_words() {
    assert_eq!(stats_duration(65), "1 h 5 min");
    assert_eq!(stats_grouped(1234), "1,234");
    assert_eq!(stats_days(1), "1 day");
    assert_eq!(stats_date_text(0), "Jan 1, 1970");
    assert_eq!((stats_month(31), stats_weekday(0)), (2, 3));
}

#[test]
fn settings_use_the_desktops_keys() {
    let k = open("settings");
    let _config = crate::testenv::fresh_config();
    let s = k.settings();
    assert_eq!((s.theme.as_str(), s.intro, s.open_on.as_str(), s.start_mode.as_str()), ("system", true, "home", "last"));
    assert!(s.notify_new_episode && s.notify_started_airing && !s.notify_new_chapter, "the desktop's defaults");
    let s = k.set_setting(Setting::Theme { value: "dark".into() }).unwrap();
    assert_eq!(s.theme, "dark");
    let s = k.set_setting(Setting::Intro { on: false }).unwrap();
    assert!(!s.intro && s.theme == "dark", "one line at a time");
    k.set_setting(Setting::NotifyNewChapter { on: true }).unwrap();
    k.set_setting(Setting::ListKind { kind: MediaKind::Manga }).unwrap();
    k.set_setting(Setting::LastPage { value: "stats".into() }).unwrap();
    let s = k.set_setting(Setting::OpenOn { value: "last".into() }).unwrap();
    assert_eq!((s.open_on.as_str(), s.last_page.as_str(), s.list_kind, s.notify_new_chapter), ("last", "stats", MediaKind::Manga, true));
    let text = std::fs::read_to_string(paths::config_file()).unwrap();
    assert!(text.contains("[display]") && text.contains("theme = \"dark\"") && text.contains("[notifications]\nnew_chapter = true"), "{text}");
    assert!(k.set_setting(Setting::Theme { value: "neon".into() }).is_err(), "not a choice: never written");
    assert!(k.set_setting(Setting::StartMode { value: "both".into() }).is_err());
    assert_eq!(k.settings().theme, "dark");
}

#[test]
fn the_fill_looks_up_only_manga_short_of_volumes() {
    let k = open("fill");
    let mut m = item(Kind::Manga, 10, Status::Watching, 40, Some(100));
    m.media.volumes = Some(10);
    let mut done = item(Kind::Manga, 11, Status::Watching, 0, None);
    done.media.volumes = Some(3);
    k.store().replace_list("mal", &[item(Kind::Anime, 1, Status::Watching, 3, Some(12))], 1).unwrap();
    k.store().replace_list(Scope::new("mal", Kind::Manga), &[m, done], 1).unwrap();
    let f = k.volume_fill().unwrap();
    assert_eq!(f.candidates().unwrap(), 1, "chapters read, volumes short");
    assert!(f.apply().is_err(), "nothing looked up yet");
    f.cancel();
    assert_eq!(f.progress(), FillProgress::default());
    assert_eq!(mangadex_credit(), "Volume data: MangaDex");
}
