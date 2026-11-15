//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one (CLAUDE.md).

use kotora_core::Status;

use kotora_core::queue::OpState;
use kotora_core::recognize::Found;

use crate::model::{Auth, Filter, Page, Sort, State};

/// A scene: its name and the change it makes to the fake first state.
pub type Scene = (&'static str, fn(&mut State));

pub const SCENES: &[Scene] = &[
    // The main list, as `mockup/shots/list.png`.
    ("list", |_| {}),
    // Real covers (made up here) on the rows that have one; two rows have none and keep the tile.
    ("list-covers", fake_covers),
    ("list-watching", |s| s.go(Page::List(Status::Watching))),
    ("list-plan", |s| s.go(Page::List(Status::PlanToWatch))),
    ("list-completed", |s| s.go(Page::List(Status::Completed))),
    ("list-on-hold", |s| s.go(Page::List(Status::OnHold))),
    // Dropped holds nothing in the fake list: the empty state.
    ("list-dropped", |s| s.go(Page::List(Status::Dropped))),
    ("list-airing", |s| s.set_filter(Filter::Airing)),
    ("list-new-episode", |s| s.set_filter(Filter::NewEpisode)),
    // The network is down: the saved list shows, with the alert and the sidebar line.
    ("list-offline", |s| {
        s.offline = true;
        s.error = Some("Can't reach MyAnimeList. Showing your saved list.".into());
    }),
    // A sync is running over a saved list.
    ("list-syncing", |s| s.syncing = true),
    // Search and sort on the list.
    ("list-search", |s| {
        s.query.replace_text("harbor");
        s.go(Page::List(Status::Completed));
        s.refresh();
    }),
    ("list-search-none", |s| {
        s.query.replace_text("zzzz");
        s.refresh();
    }),
    ("list-sort-title", |s| s.set_sort(Sort::Title)),
    ("list-sort-score", |s| s.set_sort(Sort::Score)),
    // The token was refused: the saved list shows, with a Sign in button.
    ("list-sign-in-again", |s| s.auth = Auth::Expired),
    // First run: no Client ID in config.toml.
    ("no-client-id", |s| signed_out(s, Auth::NoClientId)),
    // The guide with a pasted ID that isn't one (the inline error), and with the URL copied.
    ("no-client-id-error", |s| {
        signed_out(s, Auth::NoClientId);
        s.client_id.draft.replace_text("kotora-app-secret-from-mal");
        s.save_client_id();
        s.client_id.copied = true;
    }),
    // Saved: Sign in is enabled at once, and the page says the ID was saved.
    ("client-id-saved", |s| {
        signed_out(s, Auth::SignedOut);
        s.client_id.saved = true;
    }),
    ("signed-out", |s| signed_out(s, Auth::SignedOut)),
    ("signing-in", |s| signed_out(s, Auth::SigningIn)),
    ("sign-in-failed", |s| {
        signed_out(s, Auth::SignedOut);
        s.error = Some("MyAnimeList refused the sign-in. Try again.".into());
    }),
    // Every status empty (a new account).
    ("empty", |s| {
        s.entries.clear();
        s.refresh();
        s.ops.clear();
        s.queue = 0;
        s.now = None;
    }),
    // The first sync with nothing saved yet.
    ("loading", |s| {
        s.entries.clear();
        s.refresh();
        s.loading = true;
        s.syncing = true;
        s.ops.clear();
        s.queue = 0;
        s.now = None;
    }),
    // The inline editor under a row (Lanterns of Kirigaoka), as opened by a press on it.
    ("list-editor", |s| s.toggle_editor(0)),
    // The editor after picking Completed (progress fills) and a score of 9.
    ("list-editor-changed", |s| {
        s.toggle_editor(4);
        s.editor_status(Status::Completed);
        s.editor_score(Some(9));
    }),
    // `+1` pressed on a synced row: the number moves at once and the row waits to sync.
    ("list-plus-one", |s| {
        let _ = s.plus_one(4);
    }),
    // An entry with no episode count (still airing): `?` and no upper bound.
    ("list-editor-unknown-count", |s| {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == 6) {
            e.episodes = None;
        }
        s.toggle_editor(6);
    }),
    // Now Playing: episode 6 of Lanterns of Kirigaoka in mpv, 68 % in, a sure match (no banner yet).
    ("now-playing-page", |s| s.go(Page::NowPlaying)),
    ("now-playing-paused", |s| {
        if let Some(n) = &mut s.now {
            n.playing = false;
        }
        s.go(Page::NowPlaying);
    }),
    // It passed the threshold: the banner asks on top of every page.
    ("now-playing-offer", reached),
    ("now-playing-offer-page", |s| {
        reached(s);
        s.go(Page::NowPlaying);
    }),
    // "Update automatically" on: recorded at once, Undo in the banner.
    ("now-playing-recorded", |s| {
        s.prefs.auto = true;
        reached(s);
    }),
    // A close title, not the same: "Not sure", and the banner says so.
    ("now-playing-unsure", |s| {
        unsure(s);
        s.go(Page::NowPlaying);
    }),
    ("now-playing-unsure-offer", |s| {
        unsure(s);
        reached(s);
    }),
    // "Wrong anime…": the closest titles on the list.
    ("now-playing-picking", |s| s.wrong_anime()),
    // One MyAnimeList search found it, but it isn't on the list.
    ("now-playing-not-on-list", |s| {
        playing_file(s, "[Hoshi] Velvet Orchard Chronicle - 02 (1080p).mkv");
        if let Some(n) = &mut s.now {
            n.found = Found::NotOnList { media_id: 9001, title: "Velvet Orchard Chronicle".into() };
        }
        s.go(Page::NowPlaying);
    }),
    // Nothing on the list, nothing found.
    ("now-playing-unknown", |s| {
        playing_file(s, "holiday_2026_beach.mp4");
        if let Some(n) = &mut s.now {
            n.found = Found::Nothing;
            n.candidates.clear();
        }
        s.go(Page::NowPlaying);
    }),
    // No player plays anything.
    ("now-playing-empty", |s| {
        s.now = None;
        s.go(Page::NowPlaying);
    }),
    // The queue as in the mockup: a conflict, two waiting, one failed, two sent today.
    ("queue-page", |s| s.go(Page::Queue)),
    ("queue-empty", |s| {
        s.ops.clear();
        s.queue = 0;
        s.go(Page::Queue);
    }),
    // No network: the queue keeps everything, with Try now.
    ("queue-offline", |s| {
        s.offline = true;
        s.error = Some("Can't reach MyAnimeList. Showing your saved list.".into());
        if let Some(op) = s.ops.iter_mut().find(|o| o.id == 2) {
            op.error = Some("can't reach the service: no answer from MyAnimeList".into());
            op.attempts = 2;
        }
        s.go(Page::Queue);
    }),
    // The sign-in expired (401, refresh refused): the queue is kept, with a Sign in button.
    ("queue-sign-in-again", |s| {
        s.auth = Auth::Expired;
        s.go(Page::Queue);
    }),
    // A conflict where the entry was removed on MyAnimeList.
    ("queue-conflict-removed", |s| {
        if let Some(op) = s.ops.iter_mut().find(|o| o.state == OpState::Conflict) {
            op.remote = None;
        }
        s.go(Page::Queue);
    }),
    // One change on its way, nothing else open.
    ("queue-sending", |s| {
        s.ops.retain(|o| o.id == 2 || o.state == OpState::Done);
        if let Some(op) = s.ops.iter_mut().find(|o| o.id == 2) {
            op.state = OpState::Sending;
        }
        s.queue = 1;
        s.go(Page::Queue);
    }),
    ("settings-page", |s| s.go(Page::Settings)),
    // "Update automatically" on, threshold 90 %, keep-higher off.
    ("settings-auto-on", |s| {
        s.prefs.auto = true;
        s.prefs.threshold = 90;
        s.prefs.keep_higher = false;
        s.go(Page::Settings);
    }),
    // Two players switched off, one with a problem.
    ("settings-players-off", |s| {
        s.prefs.players.vlc = false;
        s.prefs.players.iina = false;
        s.prefs.players.other_mpris = false;
        s.players = vec![kotora_detect::SourceStatus {
            switch: kotora_detect::Switch::Mpv,
            via: kotora_detect::Via::MpvIpc,
            line: "folder of /run/user/1000/mpv.sock not found".into(),
            led: kotora_detect::Led::Problem,
        }];
        s.now = None;
        s.go(Page::Settings);
    }),
    // Settings → Accounts: Change pressed on the Client ID, with a wrong paste.
    ("settings-client-id-change", |s| {
        s.client_id.editing = true;
        s.client_id.draft.replace_text("abc123");
        s.save_client_id();
        s.go(Page::Settings);
    }),
    // No Client ID yet: Settings offers to set one.
    ("settings-no-client-id", |s| {
        signed_out(s, Auth::NoClientId);
        s.client_id.hint = None;
        s.go(Page::Settings);
    }),
    ("settings-signed-out", |s| {
        signed_out(s, Auth::SignedOut);
        s.go(Page::Settings);
    }),
];

/// The fake episode passed the threshold.
fn reached(s: &mut State) {
    let (key, file) = match &mut s.now {
        Some(n) => {
            n.position = Some(1166.0);
            (n.key.clone(), n.file.clone())
        }
        None => return,
    };
    s.now_reached(&key, &file);
}

fn unsure(s: &mut State) {
    playing_file(s, "Lantern of Kirigaoka - 06.mkv");
    if let Some(n) = &mut s.now {
        n.found = Found::List { media_id: 0, sure: false, why: "close to a title in your list (95%)".into() };
    }
}

/// The fake player shows another file.
fn playing_file(s: &mut State, file: &str) {
    if let Some(n) = &mut s.now {
        n.file = file.into();
        n.parsed = kotora_core::recognize::parse(file);
    }
}

fn fake_covers(s: &mut State) {
    s.covers.fake();
    for e in s.entries.iter_mut().filter(|e| e.id % 5 != 2) {
        e.cover_url = Some(format!("https://cdn.example.invalid/{}.jpg", e.id));
    }
}

/// Nobody is signed in: no account, no list, no queue.
fn signed_out(s: &mut State, auth: Auth) {
    s.auth = auth;
    s.account = None;
    s.entries.clear();
    s.refresh();
    s.synced_at = None;
    s.ops.clear();
    s.queue = 0;
    s.now = None;
}

#[cfg(test)]
mod tests {
    use super::SCENES;
    use crate::model::State;
    use rune::{Env, Runtime};

    fn render(name: &str, size: (f32, f32)) -> Runtime<State> {
        let mut s = State::fake();
        (SCENES.iter().find(|(n, _)| *n == name).unwrap().1)(&mut s);
        let mut rt = Runtime::new(s, crate::ui::view, size, Env { system_dark: true, scale: 1.0 });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        rt
    }

    /// Every scene lays out with the sidebar and its page, at the default and the minimum size.
    #[test]
    fn every_scene_renders() {
        let mut names: Vec<&str> = SCENES.iter().map(|(n, _)| *n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SCENES.len(), "scene names are unique");
        for (name, _) in SCENES {
            for size in [(1280.0, 800.0), (960.0, 600.0)] {
                let rt = render(name, size);
                assert!(rt.find("sidebar").is_some(), "{name}: sidebar");
                assert!(rt.find("main").is_some(), "{name}: main");
                // The sign-in screen has no top bar.
                assert!(rt.find("top").is_some() || rt.find("welcome").is_some(), "{name}: page title");
            }
        }
    }

    #[test]
    fn list_shows_rows_and_states_show_theirs() {
        let rt = render("list", (1280.0, 800.0));
        assert!(rt.find("rows").is_some() && rt.find("thead").is_some() && rt.find("chips").is_some());
        assert!(rt.find("0").is_some(), "the first row (media id 0) is built");
        assert!(rt.find("np-mini").is_some());
        let rt = render("list-dropped", (1280.0, 800.0));
        assert!(rt.find("empty").is_some() && rt.find("rows").is_none());
        let rt = render("loading", (1280.0, 800.0));
        assert!(rt.find("loading").is_some() && rt.find("skel/3").is_some() && rt.find("rows").is_none());
    }

    #[test]
    fn sign_in_screens_show_their_parts() {
        let rt = render("no-client-id", (1280.0, 800.0));
        assert!(rt.find("welcome").is_some() && rt.find("no-client-id").is_some() && rt.find("check-again").is_some());
        for key in ["cid-step/1", "cid-step/2", "cid-step/3", "cid/open", "cid/copy", "cid/url", "cid/input", "cid/save"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("cid/error").is_none());
        assert!(render("no-client-id-error", (1280.0, 800.0)).find("cid/error").is_some());
        assert!(render("no-client-id", (960.0, 600.0)).find("cid/save").is_some(), "the guide fits a small window");
        let rt = render("client-id-saved", (1280.0, 800.0));
        assert!(rt.find("cid-saved").is_some() && rt.find("no-client-id").is_none());
        let rt = render("settings-page", (1280.0, 800.0));
        assert!(rt.find("set/cid-masked").is_some() && rt.find("set/cid-change").is_some() && rt.find("set/cid/input").is_none());
        let rt = render("settings-client-id-change", (1280.0, 800.0));
        assert!(rt.find("set/cid/input").is_some() && rt.find("set/cid/error").is_some() && rt.find("set/cid-cancel").is_some());
        assert!(render("settings-no-client-id", (1280.0, 800.0)).find("set/cid-change").is_some());
        let rt = render("no-client-id", (1280.0, 800.0));
        assert!(rt.find("rows").is_none() && rt.find("account").is_none());
        let rt = render("signed-out", (1280.0, 800.0));
        assert!(rt.find("welcome").is_some() && rt.find("sign-in").is_some() && rt.find("no-client-id").is_none());
        assert!(render("signing-in", (1280.0, 800.0)).find("cancel").is_some());
        assert!(render("sign-in-failed", (1280.0, 800.0)).find("welcome").is_some());
    }

    #[test]
    fn alerts_show_over_the_saved_list() {
        let rt = render("list-sign-in-again", (1280.0, 800.0));
        assert!(rt.find("alert/expired").is_some() && rt.find("alert/sign-in").is_some() && rt.find("rows").is_some());
        let rt = render("list-offline", (1280.0, 800.0));
        assert!(rt.find("alert/error").is_some() && rt.find("alert/retry").is_some() && rt.find("rows").is_some());
        assert!(render("list", (1280.0, 800.0)).find("alert/error").is_none());
    }

    #[test]
    fn search_with_no_match_says_so() {
        let rt = render("list-search-none", (1280.0, 800.0));
        assert!(rt.find("no-match").is_some() && rt.find("rows").is_none() && rt.find("search").is_some());
        assert!(render("list-search", (1280.0, 800.0)).find("rows").is_some());
    }

    /// Settings: the Accounts and About cards, side by side when wide, stacked when narrow.
    #[test]
    fn settings_has_accounts_and_about() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-page", size);
            for key in ["account-card", "acct/mal", "acct/anilist", "token-note", "sign-out", "about-card", "link/source"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        let rt = render("settings-signed-out", (1360.0, 860.0));
        assert!(rt.find("sign-in").is_some() && rt.find("sign-out").is_none());
    }

    #[test]
    fn the_editor_opens_under_its_row() {
        let rt = render("list-editor", (1280.0, 800.0));
        for key in ["editor", "ed/status/completed", "ed/progress", "ed/plus", "ed/score/none", "ed/score/10", "ed/save", "ed/cancel"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(render("list", (1280.0, 800.0)).find("editor").is_none());
        // Narrow window: the editor still fits.
        assert!(render("list-editor", (960.0, 600.0)).find("ed/save").is_some());
    }

    #[test]
    fn the_queue_page_shows_each_kind_of_change() {
        let rt = render("queue-page", (1280.0, 800.0));
        for key in ["page/queue", "conflict/1", "conflict/1/keep", "conflict/1/take", "qh/waiting", "qh/failed", "qh/sent", "q/2", "q/4/edit", "q/4/discard", "q/5", "queue/sync-now"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("queue/offline").is_none() && rt.find("queue/expired").is_none());
        assert!(render("queue-empty", (1280.0, 800.0)).find("queue-empty").is_some());
        assert!(render("queue-offline", (1280.0, 800.0)).find("queue/try-now").is_some());
        let rt = render("queue-sign-in-again", (1280.0, 800.0));
        assert!(rt.find("queue/expired").is_some() && rt.find("queue/sign-in").is_some() && rt.find("conflict/1").is_some());
        assert!(render("queue-conflict-removed", (1280.0, 800.0)).find("conflict/1/take").is_some());
    }

    #[test]
    fn now_playing_shows_its_parts() {
        let rt = render("now-playing-page", (1280.0, 800.0));
        for key in ["page/now", "np-card", "np-bar", "np-file", "np-match", "np/update", "np/wrong", "np-players", "player/mpv", "np-mini"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        assert!(rt.find("banner").is_none(), "no banner before the threshold");
        for name in ["now-playing-offer", "now-playing-offer-page", "now-playing-unsure-offer"] {
            let rt = render(name, (1280.0, 800.0));
            for key in ["banner", "banner/update", "banner/not-now", "banner/wrong"] {
                assert!(rt.find(key).is_some(), "{name}: {key}");
            }
        }
        // The banner fits a narrow window too.
        assert!(render("now-playing-offer", (960.0, 600.0)).find("banner/update").is_some());
        let rt = render("now-playing-recorded", (1280.0, 800.0));
        assert!(rt.find("banner/undo").is_some() && rt.find("banner/update").is_none());
        assert!(render("now-playing-unsure", (1280.0, 800.0)).find("np/yes").is_some());
        let rt = render("now-playing-picking", (1280.0, 800.0));
        assert!(rt.find("np-picker").is_some() && rt.find("pick/0").is_some() && rt.find("pick/none").is_some());
        assert!(render("now-playing-not-on-list", (1280.0, 800.0)).find("np-not-on-list").is_some());
        let rt = render("now-playing-unknown", (1280.0, 800.0));
        assert!(rt.find("np/update").is_none() && rt.find("np-card").is_some());
        let rt = render("now-playing-empty", (1280.0, 800.0));
        assert!(rt.find("np-empty").is_some() && rt.find("np-mini").is_none() && rt.find("np-players").is_some());
    }

    #[test]
    fn settings_has_updates_and_players() {
        for size in [(1360.0, 860.0), (960.0, 600.0)] {
            let rt = render("settings-page", size);
            for key in ["updates-card", "set/auto", "set/threshold", "set/keep-higher", "set/asks-on", "players-card", "set/switch/mpv"] {
                assert!(rt.find(key).is_some(), "{key} at {size:?}");
            }
        }
        let rt = render("settings-auto-on", (1360.0, 860.0));
        assert!(rt.find("set/asks-off").is_some());
        assert!(render("settings-players-off", (1360.0, 860.0)).find("set/switch/vlc").is_some());
    }

    /// The type column hides below 1100 pt.
    #[test]
    fn narrow_window_drops_the_type_column() {
        let wide = render("list", (1280.0, 800.0));
        let narrow = render("list", (1000.0, 700.0));
        assert!(wide.find("thead/type").is_some());
        assert!(narrow.find("thead/type").is_none());
    }
}
