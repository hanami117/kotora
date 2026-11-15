//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one (CLAUDE.md).

use kotora_core::Status;

use kotora_core::queue::OpState;

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
    ("now-playing-page", |s| s.go(Page::NowPlaying)),
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
    ("settings-signed-out", |s| {
        signed_out(s, Auth::SignedOut);
        s.go(Page::Settings);
    }),
];

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

    /// The type column hides below 1100 pt.
    #[test]
    fn narrow_window_drops_the_type_column() {
        let wide = render("list", (1280.0, 800.0));
        let narrow = render("list", (1000.0, 700.0));
        assert!(wide.find("thead/type").is_some());
        assert!(narrow.find("thead/type").is_none());
    }
}
