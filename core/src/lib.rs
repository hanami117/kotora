//! Kotora's core: no UI and no platform code.
//!
//! Stage 3: the data model, the SQLite store (the local cache of the user's list), the sync queue
//! (`queue.rs`), paths, the config file, the token file, and the MyAnimeList client (list, one
//! entry, `PATCH my_list_status`). Recognition arrives in stage 4 (docs/ROADMAP.md). Nothing here
//! touches a UI or starts a thread of its own.

pub mod config;
pub mod covers;
pub mod loopback;
pub mod mal;
pub mod model;
pub mod paths;
pub mod queue;
pub mod ratelimit;
pub mod service;
pub mod store;
pub mod time;
pub mod tokens;

pub use model::{Change, ListItem, Media, Remote, Values};

/// A list status. Kotora's own enum, mapped per service later (MAL `plan_to_watch` / `on_hold`,
/// AniList `PLANNING` / `PAUSED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Watching,
    PlanToWatch,
    Completed,
    OnHold,
    Dropped,
}

impl Status {
    /// Sidebar order.
    pub const ALL: [Status; 5] = [Status::Watching, Status::PlanToWatch, Status::Completed, Status::OnHold, Status::Dropped];

    /// The name the UI and `kot` show.
    pub fn label(self) -> &'static str {
        match self {
            Status::Watching => "Watching",
            Status::PlanToWatch => "Plan to watch",
            Status::Completed => "Completed",
            Status::OnHold => "On hold",
            Status::Dropped => "Dropped",
        }
    }

    /// MyAnimeList's `list_status.status` value.
    pub fn mal_name(self) -> &'static str {
        match self {
            Status::Watching => "watching",
            Status::PlanToWatch => "plan_to_watch",
            Status::Completed => "completed",
            Status::OnHold => "on_hold",
            Status::Dropped => "dropped",
        }
    }

    pub fn from_mal(s: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|st| st.mal_name() == s)
    }

    /// A stable lower-case name (scene names, the wire).
    pub fn slug(self) -> &'static str {
        match self {
            Status::Watching => "watching",
            Status::PlanToWatch => "plan",
            Status::Completed => "completed",
            Status::OnHold => "on-hold",
            Status::Dropped => "dropped",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mal_names_round_trip() {
        for st in Status::ALL {
            assert_eq!(Status::from_mal(st.mal_name()), Some(st));
        }
        assert_eq!(Status::from_mal("nope"), None);
    }

    #[test]
    fn slugs_are_unique() {
        let mut s: Vec<_> = Status::ALL.iter().map(|s| s.slug()).collect();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), Status::ALL.len());
    }
}
