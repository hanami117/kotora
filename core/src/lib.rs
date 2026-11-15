//! Kotora's core: no UI and no platform code.
//!
//! Stage 1 holds only the list statuses the UI and `kot` share. The SQLite store, the services
//! (MyAnimeList first), the sync queue and recognition arrive in later stages (docs/ROADMAP.md).

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
    fn slugs_are_unique() {
        let mut s: Vec<_> = Status::ALL.iter().map(|s| s.slug()).collect();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), Status::ALL.len());
    }
}
