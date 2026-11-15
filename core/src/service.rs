//! What the app needs from an online list service. MyAnimeList implements it in v0.1; AniList
//! can in v0.2 without the store or the queue changing.

use crate::ListItem;

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

/// Read access to the user's own list. Writes arrive in stage 3.
pub trait ListService {
    /// Machine name, the `service` column of the store: `mal`.
    fn name(&self) -> &'static str;
    /// Display name: "MyAnimeList".
    fn title(&self) -> &'static str;
    /// The signed-in user's name.
    fn user_name(&mut self) -> Result<String, ServiceError>;
    /// The whole list.
    fn fetch_list(&mut self) -> Result<Vec<ListItem>, ServiceError>;
}
