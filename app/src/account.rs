//! The active list service: MyAnimeList or AniList, one at a time (`[account] service`).
//!
//! Workers run on their own threads and ask [`name`] and [`open`] each time they start, so a
//! switch in Settings takes effect for the next sync, send, search or detection. Views read the
//! service from the `State` instead (`State::service`), so scenes and tests can set it alone.
//! Each service keeps its own rows in the store (the `service` column) and its own token.

use std::sync::atomic::{AtomicU8, Ordering};

use kotora_core::anilist::{self, AniList};
use kotora_core::config::Config;
use kotora_core::mal::{Client, Mal, UreqTransport};
use kotora_core::paths;
use kotora_core::service::{ListService, ServiceId};
use kotora_core::tokens::TokenStore;

static ACTIVE: AtomicU8 = AtomicU8::new(0);

/// A signed-in client of a service, for one worker.
pub type Service = Box<dyn ListService + Send>;

pub fn active() -> ServiceId {
    if ACTIVE.load(Ordering::Relaxed) == 1 { ServiceId::AniList } else { ServiceId::Mal }
}

pub fn set_active(id: ServiceId) {
    ACTIVE.store(u8::from(id == ServiceId::AniList), Ordering::Relaxed);
}

/// The store's name of the active service (`mal`, `anilist`).
pub fn name() -> &'static str {
    active().name()
}

/// Reads `[account] service` at start (MyAnimeList when none is chosen).
pub fn load() -> ServiceId {
    let id = Config::load(&paths::config_file()).map(|c| c.service_id()).unwrap_or_default();
    set_active(id);
    id
}

/// Whether a token of `id` is saved.
pub fn has_token(id: ServiceId) -> bool {
    TokenStore::new(paths::token_file()).load(id.name()).is_some()
}

/// A signed-in client of the active service for a short worker (sync, queue, search, details,
/// `kot add`): `None` when offline by choice (`KOTORA_OFFLINE`), signed out, or (MyAnimeList)
/// with no Client ID. Each service shares one rate budget across the app.
pub fn open() -> Option<Service> {
    open_for(active())
}

pub fn open_for(id: ServiceId) -> Option<Service> {
    if paths::offline_forced() {
        return None;
    }
    let tokens = TokenStore::new(paths::token_file());
    tokens.load(id.name())?;
    match id {
        ServiceId::Mal => {
            let cid = Config::load(&paths::config_file()).ok()?.mal_client_id?;
            Some(Box::new(Mal::new(Client::new(cid, UreqTransport::default()), tokens)))
        }
        // AniList needs its Client ID only to sign in: the token works alone.
        ServiceId::AniList => Some(Box::new(AniList::new(anilist::Client::new(UreqTransport::default()), tokens))),
    }
}
