//! The data model the store, the services and the UI share.

use crate::Status;

/// An anime, as far as Kotora keeps it: only media on the user's list (no hoarding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The service's id (MyAnimeList id in v0.1).
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    /// `None`: unknown (still airing with no count).
    pub episodes: Option<u32>,
    /// The service's raw format: `tv`, `movie`, `ova`, …
    pub format: String,
    /// The service's raw airing status: `currently_airing`, `finished_airing`, `not_yet_aired`
    /// (empty when unknown).
    pub air_status: String,
    pub cover_url: Option<String>,
}

impl Media {
    pub fn is_airing(&self) -> bool {
        self.air_status == "currently_airing"
    }

    /// Every title, for the search filter.
    pub fn all_titles(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.as_str())
            .chain(self.title_en.as_deref())
            .chain(self.title_ja.as_deref())
            .chain(self.synonyms.iter().map(String::as_str))
            .filter(|t| !t.is_empty())
    }
}

/// One entry of the user's list: the media and the list status. This is the cached remote entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub media: Media,
    pub status: Status,
    pub progress: u32,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
    pub rewatching: bool,
    /// The remote `updated_at`, unix seconds (the base for conflict checks in stage 3).
    pub updated_at: i64,
}

/// The list fields an edit can change: status, progress and score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Values {
    pub status: Status,
    pub progress: u32,
    /// 1-10; `None`: not scored.
    pub score: Option<u8>,
}

/// One edit, or several merged: only the fields that change (`None`: unchanged).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub status: Option<Status>,
    pub progress: Option<u32>,
    /// `Some(None)` removes the score.
    pub score: Option<Option<u8>>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.status.is_none() && self.progress.is_none() && self.score.is_none()
    }

    /// `later` on top of `self`: the newer value of each field wins.
    pub fn merge(self, later: Change) -> Change {
        Change { status: later.status.or(self.status), progress: later.progress.or(self.progress), score: later.score.or(self.score) }
    }

    /// Drops the fields that already hold their value in `v` (an edit back to where it was).
    pub fn without_same(self, v: &Values) -> Change {
        Change {
            status: self.status.filter(|s| *s != v.status),
            progress: self.progress.filter(|p| *p != v.progress),
            score: self.score.filter(|s| *s != v.score),
        }
    }

    pub fn apply(&self, v: Values) -> Values {
        Values { status: self.status.unwrap_or(v.status), progress: self.progress.unwrap_or(v.progress), score: self.score.unwrap_or(v.score) }
    }

    /// Whether `v` already holds every value of this change.
    pub fn is_in(&self, v: &Values) -> bool {
        self.without_same(v).is_empty()
    }
}

/// The service's copy of one entry (MAL `my_list_status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remote {
    pub values: Values,
    pub rewatching: bool,
    /// Unix seconds.
    pub updated_at: i64,
}

impl ListItem {
    pub fn values(&self) -> Values {
        Values { status: self.status, progress: self.progress, score: self.score }
    }
}
