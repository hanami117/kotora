//! Named states for headless renders (`--scene NAME`), filled with fake, deterministic data.
//! `--list-scenes` lists them. Every view and state gets one (CLAUDE.md).

use kotora_core::Status;

use crate::model::{Filter, Page, State};

/// A scene: its name and the change it makes to the fake first state.
pub type Scene = (&'static str, fn(&mut State));

pub const SCENES: &[Scene] = &[
    // The main list, as `mockup/shots/list.png`.
    ("list", |_| {}),
    ("list-watching", |s| s.go(Page::List(Status::Watching))),
    ("list-plan", |s| s.go(Page::List(Status::PlanToWatch))),
    ("list-completed", |s| s.go(Page::List(Status::Completed))),
    ("list-on-hold", |s| s.go(Page::List(Status::OnHold))),
    // Dropped holds nothing in the fake list: the empty state.
    ("list-dropped", |s| s.go(Page::List(Status::Dropped))),
    ("list-airing", |s| s.filter = Filter::Airing),
    ("list-new-episode", |s| s.filter = Filter::NewEpisode),
    ("list-offline", |s| s.offline = true),
    // Every status empty (a new account).
    ("empty", |s| {
        s.entries.clear();
        s.queue = 0;
        s.now = None;
    }),
    // The first sync with nothing saved yet.
    ("loading", |s| {
        s.entries.clear();
        s.loading = true;
        s.queue = 0;
        s.now = None;
    }),
    ("now-playing-page", |s| s.go(Page::NowPlaying)),
    ("queue-page", |s| s.go(Page::Queue)),
    ("settings-page", |s| s.go(Page::Settings)),
];

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
                assert!(rt.find("top").is_some(), "{name}: page title");
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

    /// The type column hides below 1100 pt.
    #[test]
    fn narrow_window_drops_the_type_column() {
        let wide = render("list", (1280.0, 800.0));
        let narrow = render("list", (1000.0, 700.0));
        assert!(wide.find("thead/type").is_some());
        assert!(narrow.find("thead/type").is_none());
    }
}
