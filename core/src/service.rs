//! What the app needs from an online list service. MyAnimeList (`mal.rs`) and AniList
//! (`anilist.rs`) implement it; the store and the queue key everything by the service's name.

use serde::{Deserialize, Serialize};

use crate::{Change, Kind, ListItem, Media, Remote, Status};

/// The services Kotora can keep a list on. One is active at a time (`[account] service`); each
/// keeps its own rows in the store (the `service` column) and its own token.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ServiceId {
    #[default]
    Mal,
    AniList,
}

impl ServiceId {
    pub const ALL: [ServiceId; 2] = [ServiceId::Mal, ServiceId::AniList];

    /// The store's `service` column and `[account] service`: `mal`, `anilist`.
    pub fn name(self) -> &'static str {
        match self {
            ServiceId::Mal => "mal",
            ServiceId::AniList => "anilist",
        }
    }

    pub fn from_name(s: &str) -> Option<ServiceId> {
        ServiceId::ALL.into_iter().find(|v| v.name() == s.trim().to_ascii_lowercase())
    }

    /// "MyAnimeList", "AniList".
    pub fn title(self) -> &'static str {
        match self {
            ServiceId::Mal => "MyAnimeList",
            ServiceId::AniList => "AniList",
        }
    }

    /// An anime's (or manga's) page on the service's website.
    pub fn media_url(self, kind: Kind, id: u32) -> String {
        match self {
            ServiceId::Mal => format!("https://myanimelist.net/{}/{id}", kind.name()),
            ServiceId::AniList => format!("https://anilist.co/{}/{id}", kind.name()),
        }
    }

    /// The user's profile page.
    pub fn profile_url(self, user: &str) -> String {
        match self {
            ServiceId::Mal => format!("https://myanimelist.net/profile/{user}"),
            ServiceId::AniList => format!("https://anilist.co/user/{user}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// The service can't be reached (no network, timeout, 5xx). The saved list still works.
    Offline(String),
    /// No token, or the refresh was refused: the user must sign in again.
    SignInAgain,
    /// Still rate limited after our retries.
    RateLimited,
    /// Anything else, in plain words.
    Other(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::Offline(m) => write!(f, "can't reach the service: {m}"),
            ServiceError::SignInAgain => write!(f, "sign in again"),
            ServiceError::RateLimited => write!(f, "the service asked us to slow down"),
            ServiceError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// One anime a service search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub id: u32,
    pub title: String,
    /// The other titles (English, Japanese, synonyms).
    pub other_titles: Vec<String>,
}

/// One anime (or manga) of an online search, with what a result row shows (search and add).
/// For a manga, `episodes` is the chapter count and `air_status` the publishing status.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimeHit {
    pub kind: Kind,
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    /// The service's raw format: `tv`, `movie`, …
    pub format: String,
    /// `None`: unknown.
    pub episodes: Option<u32>,
    /// The service's raw airing status (`currently_airing`, …).
    pub air_status: String,
    /// The season it started: year and `winter` / `spring` / `summer` / `fall`.
    pub season: Option<(u16, String)>,
    /// The service's mean score, 1-10.
    pub mean: Option<f32>,
    /// A small cover (rows).
    pub cover_url: Option<String>,
    /// Manga: the volume count, when known.
    pub volumes: Option<u32>,
    /// Manga: the authors, "First Last".
    pub authors: Vec<String>,
    /// Manga: the first publication date (`2021-04-08`, or shorter).
    pub start_date: Option<String>,
}

impl AnimeHit {
    /// The media row an add saves (the cover is the small one).
    pub fn media(&self) -> Media {
        Media {
            id: self.id,
            title: self.title.clone(),
            title_en: self.title_en.clone(),
            title_ja: self.title_ja.clone(),
            synonyms: self.synonyms.clone(),
            episodes: self.episodes,
            format: self.format.clone(),
            air_status: self.air_status.clone(),
            cover_url: self.cover_url.clone(),
            start_date: if self.kind == Kind::Manga { self.start_date.clone() } else { None },
            broadcast: None,
            authors: self.authors.clone(),
            kind: self.kind,
            volumes: self.volumes,
            next_airing: None,
            id_mal: None,
        }
    }

    /// "Fall 2024".
    pub fn season_text(&self) -> Option<String> {
        season_text(self.season.as_ref())
    }
}

/// "Fall 2024" from (2024, "fall").
pub fn season_text(season: Option<&(u16, String)>) -> Option<String> {
    let (year, name) = season?;
    let mut name = name.clone();
    if let Some(first) = name.get(..1) {
        name = first.to_uppercase() + &name[1..];
    }
    Some(format!("{name} {year}"))
}

/// The user's list entry as the details page shows it (MAL `my_list_status`), with the dates the
/// list page doesn't keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MyEntry {
    pub status: String,
    pub progress: u32,
    pub score: Option<u8>,
    pub rewatching: bool,
    /// Finished rewatches (older saved details have none: 0).
    #[serde(default)]
    pub times_rewatched: u32,
    /// Manga: volumes read (0 for anime and older saved details).
    #[serde(default)]
    pub volumes: u32,
    /// `2024-10-03` (MAL may give only `2024-10` or `2024`).
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
}

impl MyEntry {
    /// `status` holds the anime name for both kinds (`watching` for a manga being read).
    pub fn status(&self) -> Option<Status> {
        Status::from_mal(&self.status)
    }
}

/// One author of a manga and what they did ("Story & Art", "Story", "Art").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Author {
    /// "First Last" (MAL may have only one of the two).
    pub name: String,
    pub role: String,
}

/// One anime's or manga's information page (MAL `GET /anime/{id}`, `GET /manga/{id}`). Saved
/// as JSON in the store's `details` table for titles on the list or recently shown (see
/// `Store::save_details`). A manga's chapter count is `episodes`, its publishing status
/// `air_status`; `season`, `studios`, `source` and `minutes` stay empty for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    /// Older saved details have none: anime.
    pub kind: Kind,
    pub id: u32,
    pub title: String,
    pub title_en: Option<String>,
    pub title_ja: Option<String>,
    pub synonyms: Vec<String>,
    pub format: String,
    pub episodes: Option<u32>,
    pub air_status: String,
    /// `2024-10-04` (or a shorter date).
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub season: Option<(u16, String)>,
    pub studios: Vec<String>,
    pub genres: Vec<String>,
    /// `manga`, `original`, `light_novel`, …
    pub source: String,
    /// Minutes per episode.
    pub minutes: Option<u32>,
    pub mean: Option<f32>,
    pub rank: Option<u32>,
    pub popularity: Option<u32>,
    pub members: Option<u32>,
    pub synopsis: String,
    /// The large cover (the details page draws it at 200 x 284).
    pub cover_url: Option<String>,
    /// The small cover (rows), for an add from the details page.
    pub cover_small: Option<String>,
    /// The user's entry when the anime is on the list.
    pub mine: Option<MyEntry>,
    /// Manga: the volume count, when known.
    pub volumes: Option<u32>,
    /// Manga: who wrote and drew it.
    pub authors: Vec<Author>,
    /// Manga: the magazines or labels it ran in.
    pub serialization: Vec<String>,
    /// The exact next airing, when the service gives it (AniList).
    pub next_airing: Option<crate::airing::NextAiring>,
}

impl Details {
    /// The media row an add (or a refresh of the saved one) uses.
    pub fn media(&self) -> Media {
        Media {
            id: self.id,
            title: self.title.clone(),
            title_en: self.title_en.clone(),
            title_ja: self.title_ja.clone(),
            synonyms: self.synonyms.clone(),
            episodes: self.episodes,
            format: self.format.clone(),
            air_status: self.air_status.clone(),
            cover_url: self.cover_small.clone().or_else(|| self.cover_url.clone()),
            start_date: self.start_date.clone(),
            broadcast: None,
            authors: self.authors.iter().map(|a| a.name.clone()).collect(),
            kind: self.kind,
            volumes: self.volumes,
            next_airing: self.next_airing,
            id_mal: None,
        }
    }

    pub fn season_text(&self) -> Option<String> {
        season_text(self.season.as_ref())
    }
}

/// The signed-in user: name and profile picture URL (none set, or a service without one), and
/// what the Profile page shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct User {
    pub name: String,
    pub picture: Option<String>,
    pub profile: Profile,
}

/// The Profile page's facts from the service: when the user joined and their anime numbers.
/// Saved as JSON in the store's `kv` table (it goes with sign out).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// ISO date or date-time ("2019-03-02T10:20:30+00:00").
    pub joined: Option<String>,
    pub stats: Option<AnimeStats>,
}

/// The service's own count of the user's anime (MAL `anime_statistics`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimeStats {
    pub days: f64,
    pub episodes: u32,
    pub mean_score: f64,
    pub rewatched: u32,
    pub watching: u32,
    pub completed: u32,
    pub on_hold: u32,
    pub dropped: u32,
    pub plan_to_watch: u32,
}

impl AnimeStats {
    /// Entries with status `st`.
    pub fn count(&self, st: Status) -> u32 {
        match st {
            Status::Watching => self.watching,
            Status::Completed => self.completed,
            Status::OnHold => self.on_hold,
            Status::Dropped => self.dropped,
            Status::PlanToWatch => self.plan_to_watch,
        }
    }

    pub fn total(&self) -> u32 {
        Status::ALL.iter().map(|st| self.count(*st)).sum()
    }
}

/// The user's own list: read it, read one entry, change one entry.
pub trait ListService {
    /// Machine name, the `service` column of the store: `mal`.
    fn name(&self) -> &'static str;
    /// Display name: "MyAnimeList".
    fn title(&self) -> &'static str;
    /// The signed-in user's name.
    fn user_name(&mut self) -> Result<String, ServiceError>;
    /// The signed-in user with the profile picture and the Profile page's facts, in one
    /// request where the service allows.
    fn user(&mut self) -> Result<User, ServiceError> {
        Ok(User { name: self.user_name()?, ..Default::default() })
    }
    /// The whole list.
    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError>;
    /// The service's copy of one entry now; `None` when it is not on the list.
    fn get_entry(&mut self, media_id: u32) -> Result<Option<Remote>, ServiceError>;
    /// Sends one change and returns the entry as the service saved it.
    fn update_entry(&mut self, media_id: u32, change: &Change) -> Result<Remote, ServiceError>;
    /// Removes one anime from the list. Nothing in the app asks for it yet.
    fn delete_entry(&mut self, _media_id: u32) -> Result<(), ServiceError> {
        Err(ServiceError::Other(format!("{} can't remove entries in Kotora yet", self.title())))
    }
    /// A title search (recognition's one search for a file not matched on the list).
    fn search(&mut self, _query: &str) -> Result<Vec<SearchHit>, ServiceError> {
        Ok(Vec::new())
    }
    /// A title search for the search bar: up to `limit` results with what a row shows.
    fn browse(&mut self, _query: &str, _limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        Ok(Vec::new())
    }
    /// One anime's information page, with the user's entry if it is on the list.
    fn details(&mut self, _media_id: u32) -> Result<Details, ServiceError> {
        Err(ServiceError::Other("this service has no details page".into()))
    }
    /// The whole manga list (only asked once the user turned Manga on: see `Store::manga_enabled`).
    fn fetch_manga_list(&mut self) -> Result<Vec<ListItem>, ServiceError> {
        Err(ServiceError::Other(format!("{} has no manga lists in Kotora", self.title())))
    }
    /// The service's copy of one manga entry; `None` when it is not on the list.
    fn get_manga_entry(&mut self, _media_id: u32) -> Result<Option<Remote>, ServiceError> {
        Err(ServiceError::Other(format!("{} has no manga lists in Kotora", self.title())))
    }
    /// Sends one change of a manga entry.
    fn update_manga_entry(&mut self, _media_id: u32, _change: &Change) -> Result<Remote, ServiceError> {
        Err(ServiceError::Other(format!("{} has no manga lists in Kotora", self.title())))
    }
    /// A manga title search for the search bar.
    fn browse_manga(&mut self, _query: &str, _limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        Ok(Vec::new())
    }
    /// One manga's information page, with the user's entry if it is on the list.
    fn manga_details(&mut self, _media_id: u32) -> Result<Details, ServiceError> {
        Err(ServiceError::Other("this service has no manga pages".into()))
    }
    /// [`ListService::fetch_list`] or [`ListService::fetch_manga_list`].
    fn fetch_list_of(&mut self, kind: Kind) -> Result<Vec<ListItem>, ServiceError> {
        match kind {
            Kind::Anime => self.fetch_list(),
            Kind::Manga => self.fetch_manga_list(),
        }
    }
    /// One entry of a list of `kind`.
    fn get_entry_of(&mut self, kind: Kind, media_id: u32) -> Result<Option<Remote>, ServiceError> {
        match kind {
            Kind::Anime => self.get_entry(media_id),
            Kind::Manga => self.get_manga_entry(media_id),
        }
    }
    /// One change of an entry of a list of `kind`.
    fn update_entry_of(&mut self, kind: Kind, media_id: u32, change: &Change) -> Result<Remote, ServiceError> {
        match kind {
            Kind::Anime => self.update_entry(media_id, change),
            Kind::Manga => self.update_manga_entry(media_id, change),
        }
    }
    /// The search bar's search in `kind`.
    fn browse_of(&mut self, kind: Kind, query: &str, limit: u32) -> Result<Vec<AnimeHit>, ServiceError> {
        match kind {
            Kind::Anime => self.browse(query, limit),
            Kind::Manga => self.browse_manga(query, limit),
        }
    }
    /// The information page of an anime or a manga.
    fn details_of(&mut self, kind: Kind, media_id: u32) -> Result<Details, ServiceError> {
        match kind {
            Kind::Anime => self.details(media_id),
            Kind::Manga => self.manga_details(media_id),
        }
    }
    /// One page of a season's anime, most members first (`page` from 0), and whether more
    /// pages follow.
    fn season(&mut self, _season: crate::airing::Season, _page: u32) -> Result<(Vec<crate::airing::SeasonAnime>, bool), ServiceError> {
        Err(ServiceError::Other("this service has no season lists".into()))
    }
}
