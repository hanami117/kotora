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
