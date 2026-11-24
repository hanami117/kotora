//! Exact air times for a MyAnimeList list. MyAnimeList gives only a weekly slot, so its Airing
//! times are estimates ([`crate::airing`]). AniList's public data has each show's next episode
//! and its exact time; for the airing shows of Watching and Plan to watch, Kotora asks for it
//! by MyAnimeList id (no sign-in) and keeps the answer here, in the store's `kv` (`mal:airtimes`,
//! so sign out deletes it).
//!
//! - Asked only from workers that already run (the list sync, the Airing page's season worker),
//!   and only for shows whose time is older than [`MAX_AGE`], or whose episode aired more than
//!   [`AIRED_GAP`] ago. Usually one request; at most [`MAX_REQUESTS`].
//! - A 429 holds every request off for its `Retry-After`; a failure for [`FAIL_HOLD`].
//! - A time is used for [`KEEP`] after it was fetched, then the slot estimate is back. With no
//!   time (AniList failed, has no data, or offline from the start) the estimate is used, as
//!   before: nothing is said.
//! - The saved MyAnimeList list carries the times ([`overlay`] in `Store::load_list`), so the
//!   Airing page, Home, `kot airing` and the new-episode notifications all see them.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::airing::NextAiring;
use crate::anilist::AirFail;
use crate::store::{Result, Store};
use crate::{Kind, ListItem, Status};

/// The `kv` key (under `mal:`, so sign out deletes it).
const KEY: &str = "mal:airtimes";
/// A time is asked for again after this long.
pub const MAX_AGE: i64 = 6 * 3600;
/// An episode that aired: its next one is asked for after this long (AniList moves on soon after
/// the airing; asking at once might get the old answer back).
pub const AIRED_GAP: i64 = 30 * 60;
/// A time is used at most this long after it was fetched (the weekly step from it is still
/// better than the slot, until a break or a delay it can't know of).
pub const KEEP: i64 = 14 * 86_400;
/// After a failure (no answer, an error), nothing is asked for this long.
pub const FAIL_HOLD: i64 = 10 * 60;
/// A `Retry-After` longer than this is cut to it.
const MAX_HOLD: i64 = 6 * 3600;
/// Ids asked at most per refresh, in requests of 50.
pub const MAX_REQUESTS: usize = 4;
const PER_REQUEST: usize = crate::anilist::MAL_IDS_PER_REQUEST;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Row {
    /// `None`: AniList knows no next episode (finished, not scheduled, not on AniList).
    next: Option<NextAiring>,
    /// When it was fetched (unix seconds).
    at: i64,
}

/// What is kept.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    times: BTreeMap<u32, Row>,
    /// No request before this (a 429's `Retry-After`, or a failure).
    hold_until: i64,
}

impl Cache {
    pub fn load(store: &Store) -> Cache {
        store.kv_get(KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    }

    fn save(&self, store: &Store) -> Result<()> {
        store.kv_set(KEY, &serde_json::to_string(self).unwrap_or_default())
    }

    /// The times to use at `now`, by MyAnimeList id.
    pub fn usable(&self, now: i64) -> HashMap<u32, NextAiring> {
        self.times.iter().filter(|(_, r)| now - r.at < KEEP).filter_map(|(id, r)| Some((*id, r.next?))).collect()
    }

    /// The ids of `wanted` to ask for at `now`.
    pub fn due(&self, wanted: &[u32], now: i64) -> Vec<u32> {
        wanted
            .iter()
            .copied()
            .filter(|id| match self.times.get(id) {
                None => true,
                Some(r) => now - r.at >= MAX_AGE || r.next.is_some_and(|n| n.at <= now - AIRED_GAP && r.at <= n.at),
            })
            .collect()
    }

    pub fn held(&self, now: i64) -> bool {
        now < self.hold_until
    }
}

/// The anime of a MyAnimeList list whose times are wanted: Watching and Plan to watch, airing
/// or not aired yet.
pub fn wanted(items: &[ListItem]) -> Vec<u32> {
    let mut v: Vec<u32> = items
        .iter()
        .filter(|it| it.media.kind == Kind::Anime && matches!(it.status, Status::Watching | Status::PlanToWatch))
        .filter(|it| matches!(it.media.air_status.as_str(), "currently_airing" | "not_yet_aired"))
        .map(|it| it.media.id)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// One answer of AniList: each id asked and its next airing, if any.
pub type Fetched = std::result::Result<Vec<(u32, Option<NextAiring>)>, AirFail>;

/// What a refresh did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to ask (all fresh, nothing airing), or held off: nothing changed.
    Unchanged,
    /// New answers were saved. The times now in use, by MyAnimeList id.
    Updated(HashMap<u32, NextAiring>),
    /// AniList didn't answer as it should: the times already kept stay, else the estimate.
    Failed,
}

/// One look: asks `fetch` (AniList's public data, [`crate::anilist::Client::next_airing_by_mal`])
/// for the times of `items` that are due, saves the answers and forgets the shows no longer
/// wanted (no hoarding). Never asks while a hold runs. Runs on a worker.
pub fn refresh(store: &Store, items: &[ListItem], now: i64, fetch: &mut dyn FnMut(&[u32]) -> Fetched) -> Outcome {
    let mut cache = Cache::load(store);
    let wanted = wanted(items);
    let before = cache.times.len();
    cache.times.retain(|id, _| wanted.binary_search(id).is_ok());
    let pruned = cache.times.len() != before;
    let due = cache.due(&wanted, now);
    if due.is_empty() || cache.held(now) {
        if pruned {
            let _ = cache.save(store);
        }
        return Outcome::Unchanged;
    }
    let due = &due[..due.len().min(MAX_REQUESTS * PER_REQUEST)];
    let mut got = 0;
    let mut failed = false;
    for chunk in due.chunks(PER_REQUEST) {
        match fetch(chunk) {
            Ok(rows) => {
                for (id, next) in rows {
                    if chunk.contains(&id) {
                        cache.times.insert(id, Row { next: next.filter(|n| n.episode > 0), at: now });
                        got += 1;
                    }
                }
            }
            Err(AirFail::Wait(secs)) => {
                cache.hold_until = now + i64::try_from(secs).unwrap_or(MAX_HOLD).clamp(1, MAX_HOLD);
                failed = true;
                break;
            }
            Err(AirFail::Failed) => {
                cache.hold_until = now + FAIL_HOLD;
                failed = true;
                break;
            }
        }
    }
    let _ = cache.save(store);
    match (got, failed) {
        (0, true) => Outcome::Failed,
        (0, false) => Outcome::Unchanged,
        _ => Outcome::Updated(cache.usable(now)),
    }
}

/// Puts the kept times on a MyAnimeList anime list (what `Store::load_list` returns). A show
/// without one keeps what was saved with the list (MyAnimeList saves none: the estimate is
/// used), so a list nobody looks up times for (the phones) reads as before.
pub fn overlay(items: &mut [ListItem], times: &HashMap<u32, NextAiring>) {
    for it in items.iter_mut().filter(|it| it.media.kind == Kind::Anime) {
        if let Some(t) = times.get(&it.media.id) {
            it.media.next_airing = Some(*t);
        }
    }
}

#[cfg(test)]
mod tests;
