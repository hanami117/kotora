//! Kotora's core: no UI and no platform code.
//!
//! The data model, the SQLite store (the local cache of the user's list), the sync queue
//! (`queue.rs`), recognition (`recognize.rs`: file name → anime on the list), paths, the config
//! file, the token file, and the MyAnimeList client (list, one entry, `PATCH my_list_status`,
//! search, seasons), the Airing timetable (`airing.rs`), the update check (`update.rs`). Nothing here touches a UI or starts a thread of its own.

pub mod airing;
pub mod airnotes;
pub mod config;
pub mod covers;
pub mod library;
pub mod loopback;
pub mod mal;
pub mod manga;
pub mod model;
pub mod paths;
pub mod queue;
pub mod ratelimit;
pub mod recognize;
pub mod service;
pub mod store;
pub mod time;
pub mod tokens;
pub mod update;

pub use model::{Change, ListItem, Media, Remote, Values};

/// What a list holds: anime or manga. Part of every key in the store (service + kind + id): MAL
/// anime and manga ids overlap. Manga counts chapters where anime counts episodes (`progress`,
/// `episodes`), and volumes besides. Light novels are manga on MAL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Anime,
    Manga,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Anime, Kind::Manga];

    /// The `kind` column and the wire: `anime`, `manga`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Anime => "anime",
            Kind::Manga => "manga",
        }
    }

    /// Unknown names are anime (rows from before manga have no other).
    pub fn from_name(s: &str) -> Kind {
        if s == "manga" { Kind::Manga } else { Kind::Anime }
    }

    /// "Anime", "Manga".
    pub fn label(self) -> &'static str {
        match self {
            Kind::Anime => "Anime",
            Kind::Manga => "Manga",
        }
    }

    /// What one step of progress is: "episode", "chapter".
    pub fn unit(self) -> &'static str {
        match self {
            Kind::Anime => "episode",
            Kind::Manga => "chapter",
        }
    }
}

/// One list of one service: what the store's list and queue calls work on. A plain service name
/// (`"mal"`) is its anime list, as before manga, so `store.load_list("mal")` still means anime;
/// `("mal", Kind::Manga)` is the manga list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope<'a> {
    pub service: &'a str,
    pub kind: Kind,
}

impl<'a> Scope<'a> {
    pub const fn new(service: &'a str, kind: Kind) -> Scope<'a> {
        Scope { service, kind }
    }
}

impl<'a> From<&'a str> for Scope<'a> {
    fn from(service: &'a str) -> Self {
        Scope { service, kind: Kind::Anime }
    }
}

impl<'a> From<&'a String> for Scope<'a> {
    fn from(service: &'a String) -> Self {
        Scope { service, kind: Kind::Anime }
    }
}

impl<'a> From<(&'a str, Kind)> for Scope<'a> {
    fn from((service, kind): (&'a str, Kind)) -> Self {
        Scope { service, kind }
    }
}

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

    /// The name the UI and `kot` show for a list of `kind`: Reading and Plan to read for manga
    /// (the same statuses and colours as Watching and Plan to watch).
    pub fn label_in(self, kind: Kind) -> &'static str {
        match (kind, self) {
            (Kind::Manga, Status::Watching) => "Reading",
            (Kind::Manga, Status::PlanToWatch) => "Plan to read",
            _ => self.label(),
        }
    }

    /// MyAnimeList's status value in a list of `kind` (manga: `reading`, `plan_to_read`). The
    /// store keeps the anime names for both kinds; only the MAL client uses these.
    pub fn mal_name_in(self, kind: Kind) -> &'static str {
        match (kind, self) {
            (Kind::Manga, Status::Watching) => "reading",
            (Kind::Manga, Status::PlanToWatch) => "plan_to_read",
            _ => self.mal_name(),
        }
    }

    pub fn from_mal_in(kind: Kind, s: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|st| st.mal_name_in(kind) == s)
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
    fn manga_names_round_trip_and_differ_only_where_mal_does() {
        for st in Status::ALL {
            assert_eq!(Status::from_mal_in(Kind::Manga, st.mal_name_in(Kind::Manga)), Some(st));
            assert_eq!(st.mal_name_in(Kind::Anime), st.mal_name());
        }
        assert_eq!(Status::from_mal_in(Kind::Manga, "watching"), None);
        assert_eq!(Status::from_mal_in(Kind::Manga, "plan_to_read"), Some(Status::PlanToWatch));
        assert_eq!(Status::Watching.label_in(Kind::Manga), "Reading");
        assert_eq!(Status::OnHold.label_in(Kind::Manga), "On hold");
        assert_eq!(Status::PlanToWatch.label_in(Kind::Anime), "Plan to watch");
    }

    #[test]
    fn a_service_name_is_its_anime_list() {
        assert_eq!(Scope::from("mal"), Scope::new("mal", Kind::Anime));
        assert_eq!(Scope::from(("mal", Kind::Manga)).kind, Kind::Manga);
        assert_eq!(Kind::from_name("manga"), Kind::Manga);
        assert_eq!(Kind::from_name(""), Kind::Anime);
    }

    #[test]
    fn slugs_are_unique() {
        let mut s: Vec<_> = Status::ALL.iter().map(|s| s.slug()).collect();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), Status::ALL.len());
    }
}
