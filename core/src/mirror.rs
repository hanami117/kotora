//! The account mirror: one main service (the active one, `[account] service`) and the other
//! service as its mirror. Entries are matched by MyAnimeList id (AniList `idMal`).
//!
//! - **Turning it on** ([`compare`]) compares the two lists and shows a preview: only on the main
//!   list, only on the mirror, on both with other values, the same, and what can't be matched
//!   (no MyAnimeList id, or the mirror doesn't know the anime). The comparison is the XML import's
//!   (`listxml::plan`): the main list is read as if it were an imported file. The user picks once
//!   ([`Align`]): the main list wins, merge, or don't align. [`Store::start_mirror`] queues the
//!   result through each service's own sync queue and saves the link, in one transaction.
//! - **After that** every change Kotora makes to a main entry (an edit, an add, an import, a
//!   Discard or Take theirs in the main queue) is also queued for the mirror ([`follow`], called
//!   from the queue's store calls, inside their transaction). What goes is the change itself
//!   (the fields that moved), not the whole entry. An anime not on the mirror yet is added there
//!   with the main entry's values; when the mirror is AniList and the anime's AniList id isn't
//!   known yet, it waits ([`Store::mirror_waits`]) until the mirror's worker looks it up
//!   ([`map_waits`]). Edits made on the main service's website are mirrored at the next sync
//!   ([`after_main_sync`]).
//! - **Edits made on the mirror's website** are never overwritten: a mirror change whose entry
//!   changed there after Kotora's last look stops as a conflict in the mirror's queue (the
//!   queue's own check), and the mirror's sync ([`sync_mirror`]) lists entries changed, added or
//!   removed there as [`WebEdit`]s for the user to settle ([`Store::settle_web_edit`]).
//! - Remove from list on the main list removes the entry from the mirror too ([`follow_removal`]),
//!   through the mirror's queue with its conflict check; Undo (Discard) of the main removal, or
//!   adding the anime again, takes back the mirror's waiting removal. Turning the mirror on never
//!   removes anything: an entry only on the mirror stays there in every choice. A removal made on
//!   the main service's website is not mirrored (a sync can't tell it from a list that failed to
//!   load whole); one made on the mirror's website is offered as "Add back".
//! - A failure on one side never blocks the other: each service has its own queue and its own
//!   worker, and only the user's own entries and edits are sent.
//!
//! AniList's terms: Kotora keeps no list of its own from AniList data. The mirror's saved list is
//! the same client-side cache of the user's own account Kotora keeps for the active service (it
//! is how a change made on the website is told apart from Kotora's), and signing out deletes it.
//!
//! The link and the small lists of waits and website edits live in the store's `kv` table under
//! `mirror:` (no schema change). No threads and no clock here: callers pass `now`.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::listxml::{self, Action, Plan, StatusConflict, Verdict, XmlEntry, XmlList};
use crate::queue::{discard_tx, enqueue_tx, entry_values, import_tx, open_ops, remove_tx};
use crate::service::{ListService, ServiceError, ServiceId};
use crate::store::{Result, Store, StoreError};
use crate::{Change, Kind, ListItem, Media, Score, Scope, Status, Values};

const LINK: &str = "mirror:link";
const WAITS: &str = "mirror:waits";
const EDITS: &str = "mirror:edits";
/// When the mirror was turned on: website edits on the main list before it are not mirrored.
const SINCE: &str = "mirror:since";

/// Which service is the main list and which one mirrors it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub main: ServiceId,
    pub to: ServiceId,
}

impl Link {
    /// The mirror of `main`: the other service.
    pub fn of(main: ServiceId) -> Link {
        let to = if main == ServiceId::Mal { ServiceId::AniList } else { ServiceId::Mal };
        Link { main, to }
    }

    fn text(self) -> String {
        format!("{}>{}", self.main.name(), self.to.name())
    }

    fn parse(s: &str) -> Option<Link> {
        let (a, b) = s.split_once('>')?;
        let l = Link { main: ServiceId::from_name(a)?, to: ServiceId::from_name(b)? };
        (l.main != l.to).then_some(l)
    }
}

// ------------------------------------------------------------------------------------- values

/// Every list field of `v` as a change (status, progress, score, rewatch).
pub fn full(v: &Values) -> Change {
    Change { status: Some(v.status), progress: Some(v.progress), score: Some(v.score), rewatching: Some(v.rewatching), times_rewatched: Some(v.times_rewatched), volumes: None }
}

/// The change an add makes: status and progress, and the score and rewatch only when set (an
/// add never removes a score the other list has).
fn add_change(v: &Values) -> Change {
    Change {
        status: Some(v.status),
        progress: Some(v.progress),
        score: v.score.map(Some),
        rewatching: v.rewatching.then_some(true),
        times_rewatched: (v.times_rewatched > 0).then_some(v.times_rewatched),
        volumes: None,
    }
}

/// The same list values (anime: volumes don't count; scores compared on the 1-10 scale, so a
/// MyAnimeList 7 and an AniList 73 are the same).
pub fn same(a: &Values, b: &Values) -> bool {
    full(a).without_same_ten(b).is_empty()
}

/// Values as the `kv` lists keep them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub status: String,
    pub progress: u32,
    /// 100-point scale (as the store keeps it).
    pub score: Option<Score>,
    pub rewatching: bool,
    pub times: u32,
}

impl From<Values> for Saved {
    fn from(v: Values) -> Saved {
        Saved { status: v.status.mal_name().to_string(), progress: v.progress, score: v.score, rewatching: v.rewatching, times: v.times_rewatched }
    }
}

impl Saved {
    pub fn values(&self) -> Values {
        Values {
            status: Status::from_mal(&self.status).unwrap_or(Status::PlanToWatch),
            progress: self.progress,
            score: self.score,
            rewatching: self.rewatching,
            times_rewatched: self.times,
            volumes: 0,
        }
    }
}

// ------------------------------------------------------------------------------- the kv lists

/// An anime added to (or changed on) the main list whose mirror id isn't known yet: the
/// mirror's worker looks it up ([`map_waits`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wait {
    pub mal_id: u32,
    pub title: String,
    pub at: i64,
    /// The lookup found nothing: why it isn't mirrored.
    #[serde(default)]
    pub error: Option<String>,
}

/// An entry changed, added or removed on the mirror's website since Kotora last looked, now
/// different from the main list. Nothing was overwritten; the user settles it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebEdit {
    pub mal_id: u32,
    pub title: String,
    /// The main entry (`None`: not on the main list: added on the mirror).
    pub main_id: Option<u32>,
    pub main: Option<Saved>,
    /// The mirror's entry (`None`: removed there).
    pub mirror_id: Option<u32>,
    pub mirror: Option<Saved>,
    /// When the mirror changed (its `updated_at`), or when the removal was seen.
    pub at: i64,
}

/// How a [`WebEdit`] is settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    /// The main list's values go to the mirror (or the entry is added back there).
    UseMain,
    /// The mirror's values go to the main list (or the entry is added there).
    UseMirror,
    /// Both stay as they are; asked again only when the mirror changes again.
    Leave,
}

fn kv_get(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

fn kv_set(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute("INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

fn link_of(c: &Connection) -> Result<Option<Link>> {
    Ok(kv_get(c, LINK)?.as_deref().and_then(Link::parse))
}

fn list_of<T: for<'de> Deserialize<'de>>(c: &Connection, key: &str) -> Result<Vec<T>> {
    Ok(kv_get(c, key)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

fn set_list<T: Serialize>(c: &Connection, key: &str, list: &[T]) -> Result<()> {
    if list.is_empty() {
        c.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        return Ok(());
    }
    kv_set(c, key, &serde_json::to_string(list).map_err(|e| StoreError(e.to_string()))?)
}

fn drop_wait(c: &Connection, mal_id: u32) -> Result<()> {
    let mut w: Vec<Wait> = list_of(c, WAITS)?;
    let n = w.len();
    w.retain(|x| x.mal_id != mal_id);
    if w.len() != n { set_list(c, WAITS, &w) } else { Ok(()) }
}

fn push_wait(c: &Connection, mal_id: u32, title: &str, now: i64) -> Result<()> {
    let mut w: Vec<Wait> = list_of(c, WAITS)?;
    match w.iter_mut().find(|x| x.mal_id == mal_id) {
        // A new change asks again, even after a lookup that found nothing.
        Some(x) => x.error = None,
        None => w.push(Wait { mal_id, title: title.to_string(), at: now, error: None }),
    }
    set_list(c, WAITS, &w)
}

fn drop_edit(c: &Connection, mal_id: u32) -> Result<()> {
    let mut e: Vec<WebEdit> = list_of(c, EDITS)?;
    let n = e.len();
    e.retain(|x| x.mal_id != mal_id);
    if e.len() != n { set_list(c, EDITS, &e) } else { Ok(()) }
}

// ---------------------------------------------------------------------------------- ids

/// A media's MyAnimeList id on `service`: its own id on MyAnimeList, `idMal` elsewhere.
pub fn mal_id_of(service: ServiceId, m: &Media) -> Option<u32> {
    match service {
        ServiceId::Mal => Some(m.id).filter(|id| *id > 0),
        ServiceId::AniList => m.id_mal.filter(|id| *id > 0),
    }
}

/// The MyAnimeList id of a saved anime of `service`.
fn mal_id_tx(c: &Connection, service: ServiceId, id: u32) -> Result<Option<u32>> {
    if service == ServiceId::Mal {
        return Ok(Some(id));
    }
    Ok(c.query_row("SELECT id_mal FROM media WHERE service = ?1 AND kind = 'anime' AND id = ?2", params![service.name(), id], |r| r.get::<_, Option<u32>>(0))
        .optional()?
        .flatten()
        .filter(|id| *id > 0))
}

/// The id on `service` of the anime with MyAnimeList id `mal`, when known (an entry first).
fn id_on(c: &Connection, service: ServiceId, mal: u32) -> Result<Option<u32>> {
    if service == ServiceId::Mal {
        return Ok(Some(mal));
    }
    Ok(c.query_row(
        "SELECT m.id FROM media m WHERE m.service = ?1 AND m.kind = 'anime' AND m.id_mal = ?2
         ORDER BY EXISTS (SELECT 1 FROM entry e WHERE e.service = m.service AND e.kind = m.kind AND e.media_id = m.id) DESC, m.id LIMIT 1",
        params![service.name(), mal],
        |r| r.get(0),
    )
    .optional()?)
}

/// The saved media row (the fields an add needs).
fn media_row(c: &Connection, service: ServiceId, id: u32) -> Result<Option<Media>> {
    Ok(c.query_row(
        "SELECT title, episodes, format, air_status, cover_url, id_mal FROM media WHERE service = ?1 AND kind = 'anime' AND id = ?2",
        params![service.name(), id],
        |r| {
            Ok(Media {
                kind: Kind::Anime,
                id,
                title: r.get(0)?,
                episodes: r.get(1)?,
                format: r.get(2)?,
                air_status: r.get(3)?,
                cover_url: r.get(4)?,
                id_mal: r.get(5)?,
                ..Media::default()
            })
        },
    )
    .optional()?)
}

/// A MyAnimeList media row for an anime known from the other service: its id and title only
/// (the next MyAnimeList sync, or `sync::complete_media`, fills in the rest from MyAnimeList).
fn mal_media(mal: u32, title: &str, episodes: Option<u32>) -> Media {
    Media { kind: Kind::Anime, id: mal, title: title.to_string(), episodes, ..Media::default() }
}

// ------------------------------------------------------------------------------------ follow

/// Called inside every store call that changes a main entry (`queue.rs`): the same change is
/// queued for the mirror. `before`: the main entry before the call (`None`: it wasn't on the
/// list). Does nothing without a link, for the mirror itself, and for manga.
pub(crate) fn follow(tx: &Connection, sc: Scope, media_id: u32, before: Option<Values>, now: i64) -> Result<()> {
    if sc.kind != Kind::Anime {
        return Ok(());
    }
    let Some(link) = link_of(tx)? else { return Ok(()) };
    if sc.service != link.main.name() {
        return Ok(());
    }
    let Some(mal) = mal_id_tx(tx, link.main, media_id)? else { return Ok(()) };
    let after = entry_values(tx, sc, media_id)?.map(|(v, _)| v);
    if before.is_none() && after.is_none() {
        return Ok(());
    }
    let to = Scope::new(link.to.name(), Kind::Anime);
    let Some(after) = after else {
        // The entry left the main list (a discarded add): what it queued for the mirror and
        // that still waits goes with it (only the main list's changes queue there).
        drop_wait(tx, mal)?;
        if let Some(mid) = id_on(tx, link.to, mal)? {
            for op in open_ops(tx, to, mid)? {
                if op.state != crate::queue::OpState::Sending {
                    discard_tx(tx, to.service, &op)?;
                }
            }
        }
        return Ok(());
    };
    let change = match before {
        Some(b) => full(&after).without_same(&b),
        None => add_change(&after),
    };
    if change.is_empty() {
        return Ok(());
    }
    let title: String = tx.query_row("SELECT title FROM media WHERE service = ?1 AND kind = 'anime' AND id = ?2", params![sc.service, media_id], |r| r.get(0)).optional()?.unwrap_or_default();
    let episodes: Option<u32> = tx.query_row("SELECT episodes FROM media WHERE service = ?1 AND kind = 'anime' AND id = ?2", params![sc.service, media_id], |r| r.get(0)).optional()?.flatten();
    match id_on(tx, link.to, mal)? {
        Some(mid) if entry_values(tx, to, mid)?.is_some() => {
            // A score that is the same on the mirror's scale doesn't go (a MyAnimeList 7 and an
            // AniList 73).
            let change = match entry_values(tx, to, mid)? {
                Some((cur, _)) => change.without_same_ten(&cur),
                None => change,
            };
            if !change.is_empty() {
                enqueue_tx(tx, to, mid, change, now)?;
            }
        }
        Some(mid) => {
            // Not on the mirror: added there with the main entry's values.
            let media = match link.to {
                ServiceId::Mal => media_row(tx, link.to, mid)?.unwrap_or_else(|| mal_media(mal, &title, episodes)),
                ServiceId::AniList => match media_row(tx, link.to, mid)? {
                    Some(m) => m,
                    None => return push_wait(tx, mal, &title, now),
                },
            };
            add_to_mirror(tx, to.service, &media, &after, now)?;
        }
        None => push_wait(tx, mal, &title, now)?,
    }
    Ok(())
}

/// Called inside [`Store::remove_from_list`] for a main entry: the mirror's entry is removed
/// too, through the mirror's own queue (its conflict check stops the removal when the entry
/// changed on the mirror's website since Kotora's last look). Not on the mirror: an add that
/// waits for its AniList id is dropped, nothing else happens. Does nothing without a link, for
/// the mirror itself, and for manga.
pub(crate) fn follow_removal(tx: &Connection, sc: Scope, media_id: u32, now: i64) -> Result<()> {
    if sc.kind != Kind::Anime {
        return Ok(());
    }
    let Some(link) = link_of(tx)? else { return Ok(()) };
    if sc.service != link.main.name() {
        return Ok(());
    }
    let Some(mal) = mal_id_tx(tx, link.main, media_id)? else { return Ok(()) };
    drop_wait(tx, mal)?;
    drop_edit(tx, mal)?;
    let to = Scope::new(link.to.name(), Kind::Anime);
    if let Some(mid) = id_on(tx, link.to, mal)?
        && entry_values(tx, to, mid)?.is_some()
    {
        remove_tx(tx, to, mid, now)?;
    }
    Ok(())
}

/// Puts `media` on the mirror's list with `v` (an add, then the score and rewatch).
fn add_to_mirror(tx: &Connection, service: &str, media: &Media, v: &Values, now: i64) -> Result<()> {
    import_tx(tx, service, &[Action::Add { media: Box::new(media.clone()), values: *v }], now, false)?;
    Ok(())
}

/// The main entry with MyAnimeList id `mal`: its id and values.
fn main_entry(tx: &Connection, link: Link, mal: u32) -> Result<Option<(u32, Values)>> {
    let Some(id) = id_on(tx, link.main, mal)? else { return Ok(None) };
    Ok(entry_values(tx, Scope::new(link.main.name(), Kind::Anime), id)?.map(|(v, _)| (id, v)))
}

// ----------------------------------------------------------------------------------- store

impl Store {
    /// The mirror link, when the mirror is on.
    pub fn mirror_link(&self) -> Result<Option<Link>> {
        link_of(&self.conn)
    }

    /// Anime whose mirror id is still being looked up, or that the mirror doesn't have.
    pub fn mirror_waits(&self) -> Result<Vec<Wait>> {
        list_of(&self.conn, WAITS)
    }

    /// Entries changed on the mirror's website, waiting for the user.
    pub fn web_edits(&self) -> Result<Vec<WebEdit>> {
        list_of(&self.conn, EDITS)
    }

    /// Turns the mirror on: the alignment's changes go into each service's queue (`mirror`
    /// first, then `main`, whose changes then need nothing more on the mirror), and the link is
    /// saved, in one transaction. Returns the queued change ids (mirror, main).
    pub fn start_mirror(&mut self, link: Link, mirror: &[Action], main: &[Action], now: i64) -> Result<(Vec<i64>, Vec<i64>)> {
        let tx = self.conn.transaction()?;
        let a = import_tx(&tx, link.to.name(), mirror, now, false)?;
        kv_set(&tx, LINK, &link.text())?;
        kv_set(&tx, SINCE, &now.to_string())?;
        set_list::<Wait>(&tx, WAITS, &[])?;
        set_list::<WebEdit>(&tx, EDITS, &[])?;
        let b = import_tx(&tx, link.main.name(), main, now, true)?;
        tx.commit()?;
        Ok((a, b))
    }

    /// Turns the mirror off: nothing more is queued for it, and its changes that still wait
    /// (not one being sent) are dropped, their entries back as they were. Its saved list stays,
    /// as after a switch of services (signing out deletes it). Returns how many were dropped.
    pub fn stop_mirror(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let Some(link) = link_of(&tx)? else { return Ok(0) };
        let ids: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM pending_op WHERE service = ?1 AND state IN ('queued', 'conflict', 'failed') ORDER BY id")?;
            st.query_map([link.to.name()], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
        };
        let mut n = 0;
        for id in ids {
            // An earlier discard of the same entry may have taken it already.
            let Some(op) = crate::queue::find_op(&tx, id)? else { continue };
            discard_tx(&tx, link.to.name(), &op)?;
            n += 1;
        }
        for key in [LINK, WAITS, EDITS, SINCE] {
            tx.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// The link goes, and nothing else (a switch of services: the old mirror's waiting
    /// changes are the now active list's own and go out with it).
    pub fn unlink_mirror(&self) -> Result<()> {
        for key in [LINK, WAITS, EDITS, SINCE] {
            self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        }
        Ok(())
    }

    /// Settles a website edit. `main_media`: for "use the mirror's" on an anime not on the main
    /// list when the main list is AniList, its AniList media (looked up by the caller).
    pub fn settle_web_edit(&mut self, mal_id: u32, how: Settle, main_media: Option<&Media>, now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let Some(link) = link_of(&tx)? else { return Err(StoreError("the mirror is off".into())) };
        let edits: Vec<WebEdit> = list_of(&tx, EDITS)?;
        let Some(e) = edits.into_iter().find(|e| e.mal_id == mal_id) else { return Err(StoreError("that difference is settled already".into())) };
        let to = Scope::new(link.to.name(), Kind::Anime);
        let main_sc = Scope::new(link.main.name(), Kind::Anime);
        match how {
            Settle::Leave => {}
            Settle::UseMain => {
                let Some((_, main)) = main_entry(&tx, link, mal_id)? else {
                    return Err(StoreError(format!("it isn\u{2019}t on your {} list; Kotora can\u{2019}t remove it from {}", link.main.title(), link.to.title())));
                };
                match id_on(&tx, link.to, mal_id)? {
                    Some(mid) if entry_values(&tx, to, mid)?.is_some() => {
                        let (cur, _) = entry_values(&tx, to, mid)?.unwrap_or((main, 0));
                        enqueue_tx(&tx, to, mid, full(&main).without_same(&cur), now)?;
                    }
                    Some(mid) if link.to == ServiceId::Mal || media_row(&tx, link.to, mid)?.is_some() => {
                        let media = media_row(&tx, link.to, mid)?.unwrap_or_else(|| mal_media(mal_id, &e.title, None));
                        add_to_mirror(&tx, to.service, &media, &main, now)?;
                    }
                    _ => push_wait(&tx, mal_id, &e.title, now)?,
                }
            }
            Settle::UseMirror => {
                let Some(theirs) = e.mirror.as_ref().map(Saved::values) else {
                    return Err(StoreError(format!("it was removed from {}; Kotora can\u{2019}t remove it from {}", link.to.title(), link.main.title())));
                };
                match main_entry(&tx, link, mal_id)? {
                    Some((id, cur)) => {
                        enqueue_tx(&tx, main_sc, id, full(&theirs).without_same(&cur), now)?;
                    }
                    None => {
                        let media = match (link.main, main_media) {
                            (_, Some(m)) => m.clone(),
                            (ServiceId::Mal, None) => mal_media(mal_id, &e.title, None),
                            (ServiceId::AniList, None) => return Err(StoreError(format!("{} doesn\u{2019}t know this anime", link.main.title()))),
                        };
                        import_tx(&tx, main_sc.service, &[Action::Add { media: Box::new(media), values: theirs }], now, false)?;
                    }
                }
            }
        }
        drop_edit(&tx, mal_id)?;
        tx.commit()?;
        Ok(())
    }

    /// The mirror's anime for waiting MyAnimeList ids, as the mirror's worker found them: each
    /// goes on the mirror's list with the main entry's values; ids it doesn't know are marked.
    /// Returns how many were queued.
    pub fn mirror_mapped(&mut self, asked: &[u32], found: &[Media], now: i64) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let Some(link) = link_of(&tx)? else { return Ok(0) };
        let mut waits: Vec<Wait> = list_of(&tx, WAITS)?;
        let mut n = 0;
        for w in waits.iter_mut().filter(|w| asked.contains(&w.mal_id)) {
            match found.iter().find(|m| m.id_mal == Some(w.mal_id)) {
                Some(m) => {
                    if let Some((_, v)) = main_entry(&tx, link, w.mal_id)? {
                        let to = Scope::new(link.to.name(), Kind::Anime);
                        match entry_values(&tx, to, m.id)? {
                            Some((cur, _)) => {
                                enqueue_tx(&tx, to, m.id, add_change(&v).without_same_ten(&cur), now)?;
                            }
                            None => add_to_mirror(&tx, to.service, m, &v, now)?,
                        }
                        n += 1;
                    }
                    w.mal_id = 0;
                }
                None => w.error = Some(format!("Not on {}", link.to.title())),
            }
        }
        waits.retain(|w| w.mal_id != 0);
        set_list(&tx, WAITS, &waits)?;
        tx.commit()?;
        Ok(n)
    }

    /// One main entry changed outside the queue (its website, at a sync): the change is
    /// mirrored like an edit made here.
    pub fn mirror_follow(&mut self, main_id: u32, before: Option<Values>, now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let Some(link) = link_of(&tx)? else { return Ok(()) };
        follow(&tx, Scope::new(link.main.name(), Kind::Anime), main_id, before, now)?;
        tx.commit()?;
        Ok(())
    }

    fn save_web_edits(&self, add: Vec<WebEdit>, clear: &HashSet<u32>) -> Result<usize> {
        let mut all: Vec<WebEdit> = list_of(&self.conn, EDITS)?;
        all.retain(|e| !clear.contains(&e.mal_id) && !add.iter().any(|a| a.mal_id == e.mal_id));
        let n = add.len();
        all.extend(add);
        all.sort_by_key(|e| std::cmp::Reverse(e.at));
        set_list(&self.conn, EDITS, &all)?;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------------- syncs

/// After a sync of the main list: entries changed on the main service's website since the last
/// sync (`old`: the saved list before it) and after the mirror was turned on are mirrored.
/// Entries with a change still waiting are left alone (the queue's own conflict check covers
/// them). A sync with no saved list before it mirrors nothing (it would be the whole list).
/// Returns how many were followed.
pub fn after_main_sync(store: &mut Store, service: &str, old: &[ListItem], now: i64) -> Result<usize> {
    let Some(link) = store.mirror_link()?.filter(|l| l.main.name() == service) else { return Ok(0) };
    if old.is_empty() {
        return Ok(0);
    }
    let since: i64 = store.kv_get(SINCE)?.and_then(|v| v.parse().ok()).unwrap_or(0);
    let sc = Scope::new(link.main.name(), Kind::Anime);
    let before: HashMap<u32, &ListItem> = old.iter().map(|it| (it.media.id, it)).collect();
    let open: HashSet<u32> = store.queue(sc.service, now)?.into_iter().filter(|o| o.state.is_open()).map(|o| o.media_id).collect();
    let mut n = 0;
    for it in store.load_list(sc)? {
        if open.contains(&it.media.id) {
            continue;
        }
        let was = before.get(&it.media.id);
        if it.updated_at <= since || was.is_some_and(|w| it.updated_at <= w.updated_at) {
            continue;
        }
        store.mirror_follow(it.media.id, was.map(|w| w.values()), now)?;
        n += 1;
    }
    Ok(n)
}

/// What a mirror sync did.
#[derive(Debug, PartialEq, Eq)]
pub struct MirrorSync {
    /// Entries on the mirror's list.
    pub items: usize,
    /// New website edits found.
    pub web_edits: usize,
}

/// Reads the mirror's list, saves it, and lists what changed on the mirror's website since the
/// last look and now differs from the main list ([`WebEdit`]). The first sync only saves.
pub fn sync_mirror(service: &mut dyn ListService, store: &mut Store, now: i64) -> std::result::Result<MirrorSync, ServiceError> {
    let other = |e: StoreError| ServiceError::Other(format!("can\u{2019}t save the mirror\u{2019}s list: {e}"));
    let Some(link) = store.mirror_link().map_err(other)? else { return Ok(MirrorSync { items: 0, web_edits: 0 }) };
    if service.name() != link.to.name() {
        return Err(ServiceError::Other(format!("{} is not the mirror", service.title())));
    }
    let to = Scope::new(link.to.name(), Kind::Anime);
    let had = store.synced_at(to).map_err(other)?.is_some();
    let old = store.load_list(to).map_err(other)?;
    let fetched = service.fetch_list()?;
    store.replace_list(to, &fetched, now).map_err(other)?;
    let web_edits = if had { find_web_edits(store, link, &old, &fetched, now).map_err(other)? } else { 0 };
    Ok(MirrorSync { items: fetched.len(), web_edits })
}

fn find_web_edits(store: &Store, link: Link, old: &[ListItem], fetched: &[ListItem], now: i64) -> Result<usize> {
    let open = |service: ServiceId| -> Result<HashSet<u32>> { Ok(store.queue(service.name(), now)?.into_iter().filter(|o| o.state.is_open()).map(|o| o.media_id).collect()) };
    let (open_main, open_mirror) = (open(link.main)?, open(link.to)?);
    let main: HashMap<u32, ListItem> = store.load_list(link.main.name())?.into_iter().filter_map(|it| Some((mal_id_of(link.main, &it.media)?, it))).collect();
    let was: HashMap<u32, &ListItem> = old.iter().map(|it| (it.media.id, it)).collect();
    let now_ids: HashSet<u32> = fetched.iter().map(|it| it.media.id).collect();
    let mut found = Vec::new();
    let mut settled = HashSet::new();
    for f in fetched {
        let Some(mal) = mal_id_of(link.to, &f.media) else { continue };
        let m = main.get(&mal);
        if open_mirror.contains(&f.media.id) || m.is_some_and(|m| open_main.contains(&m.media.id)) {
            continue;
        }
        if m.is_some_and(|m| same(&m.values(), &f.values())) {
            // Equal again (changed the same way on both sites): nothing left to settle.
            settled.insert(mal);
            continue;
        }
        if was.get(&f.media.id).is_some_and(|w| f.updated_at <= w.updated_at) {
            continue;
        }
        found.push(WebEdit {
            mal_id: mal,
            title: m.map_or_else(|| f.media.title.clone(), |m| m.media.title.clone()),
            main_id: m.map(|m| m.media.id),
            main: m.map(|m| m.values().into()),
            mirror_id: Some(f.media.id),
            mirror: Some(f.values().into()),
            at: f.updated_at,
        });
    }
    for o in old.iter().filter(|o| !now_ids.contains(&o.media.id) && !open_mirror.contains(&o.media.id)) {
        let Some(mal) = mal_id_of(link.to, &o.media) else { continue };
        let Some(m) = main.get(&mal) else { continue };
        if open_main.contains(&m.media.id) {
            continue;
        }
        found.push(WebEdit { mal_id: mal, title: m.media.title.clone(), main_id: Some(m.media.id), main: Some(m.values().into()), mirror_id: None, mirror: None, at: now });
    }
    store.save_web_edits(found, &settled)
}

/// The most ids one [`map_waits`] call looks up (one AniList request holds 50).
pub const MAP_MAX: usize = 50;

/// Looks up the mirror's id of anime waiting for it (AniList by MyAnimeList id, one batched
/// request) and queues them. Returns how many were queued.
pub fn map_waits(service: &mut dyn ListService, store: &mut Store, now: i64) -> std::result::Result<usize, ServiceError> {
    let other = |e: StoreError| ServiceError::Other(e.to_string());
    let ids: Vec<u32> = store.mirror_waits().map_err(other)?.into_iter().filter(|w| w.error.is_none()).map(|w| w.mal_id).take(MAP_MAX).collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let found = service.media_by_mal_ids(&ids)?;
    store.mirror_mapped(&ids, &found, now).map_err(other)
}

// --------------------------------------------------------------------------------- preview

/// The user's one choice when turning the mirror on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The mirror becomes a copy: entries only on the main list are added there, and entries on
    /// both take the main list's values. Entries only on the mirror stay.
    MainWins,
    /// Add what is missing on each side; for different values keep the one further along (the
    /// import's rules), asking about unclear statuses one by one.
    Merge,
    /// Nothing now; only edits made from now on are mirrored.
    DontAlign,
}

/// An entry on both lists with other values, as a merge settles it.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeRow {
    pub mal_id: u32,
    pub title: String,
    pub main_id: u32,
    pub mirror_id: u32,
    pub main: Values,
    pub mirror: Values,
    pub episodes: Option<u32>,
    /// The main entry's change when the main status stays (or the clear winner's).
    pub update: Change,
    /// A status the merge can't settle alone: `mine` is the main list's, `file` the mirror's.
    pub conflict: Option<StatusConflict>,
    /// The main entry's change when the user takes the mirror's status.
    pub with_mirror_status: Change,
}

impl MergeRow {
    /// The values both sides end with.
    pub fn merged(&self, mirror_status: bool) -> Values {
        let c = if self.conflict.is_some() && mirror_status { self.with_mirror_status } else { self.update };
        c.apply(self.main)
    }
}

/// An entry only on the mirror.
#[derive(Clone, Debug, PartialEq)]
pub struct OnlyMirror {
    pub mal_id: u32,
    pub item: ListItem,
    /// The main service's media for it (MyAnimeList: always; AniList: when it knows the id).
    pub main_media: Option<Media>,
}

/// The two lists compared.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub link: Link,
    /// The main list against the mirror's, as an import would see it: New = only on the main
    /// list, Different, Same, Unknown = can't be mirrored (and why).
    pub plan: Plan,
    /// The Different rows, for a merge.
    pub merge: Vec<MergeRow>,
    pub only_mirror: Vec<OnlyMirror>,
    /// Titles of mirror entries without a MyAnimeList id (never matched).
    pub unmatched_mirror: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewCounts {
    pub only_main: usize,
    pub only_mirror: usize,
    pub different: usize,
    pub same: usize,
    /// Not mirrored: no MyAnimeList id, or unknown on the other side.
    pub unmatched: usize,
    pub conflicts: usize,
}

impl Preview {
    pub fn counts(&self) -> PreviewCounts {
        let c = self.plan.counts();
        PreviewCounts {
            only_main: c.new,
            only_mirror: self.only_mirror.len(),
            different: c.different,
            same: c.same,
            unmatched: c.unknown + self.unmatched_mirror.len(),
            conflicts: self.merge.iter().filter(|r| r.conflict.is_some()).count(),
        }
    }
}

/// The main list as an import file: each entry under its MyAnimeList id (0: none).
fn as_file(link: Link, main: &[ListItem]) -> XmlList {
    let entries = main
        .iter()
        .map(|it| XmlEntry {
            kind: Kind::Anime,
            volumes_read: 0,
            volumes: None,
            mal_id: mal_id_of(link.main, &it.media).unwrap_or(0),
            title: it.media.title.clone(),
            series_type: String::new(),
            episodes: it.media.episodes,
            raw_status: it.status.mal_name().to_string(),
            status: Some(it.status),
            watched: it.progress,
            score: it.score,
            start_date: None,
            finish_date: None,
            times_watched: it.times_rewatched,
            rewatching: it.rewatching,
            rewatching_ep: it.progress,
            update_on_import: true,
        })
        .collect();
    XmlList { kind: Kind::Anime, user_name: None, entries, other: 0 }
}

/// MyAnimeList ids only on the main list (the mirror must look them up when it is AniList).
pub fn ids_only_on_main(link: Link, main: &[ListItem], mirror: &[ListItem]) -> Vec<u32> {
    Plan::ids_to_map(&as_file(link, main), mirror, link.to.name())
}

/// MyAnimeList ids only on the mirror (the main service must look them up when it is AniList).
pub fn ids_only_on_mirror(link: Link, main: &[ListItem], mirror: &[ListItem]) -> Vec<u32> {
    let on_main: HashSet<u32> = main.iter().filter_map(|it| mal_id_of(link.main, &it.media)).collect();
    let mut seen = HashSet::new();
    mirror.iter().filter_map(|it| mal_id_of(link.to, &it.media)).filter(|id| !on_main.contains(id) && seen.insert(*id)).collect()
}

/// Compares the main list with the mirror's. `mirror_media`: the mirror's media for MyAnimeList
/// ids only on the main list (AniList mirror); `main_media`: the main service's media for ids
/// only on the mirror (AniList main). Pure: tests drive it.
pub fn compare(link: Link, main: &[ListItem], mirror: &[ListItem], mirror_media: &HashMap<u32, Media>, main_media: &HashMap<u32, Media>) -> Preview {
    let file = as_file(link, main);
    let mut plan = listxml::plan(&file, link.to.name(), mirror, mirror_media);
    plan.service = link.to.name().to_string();
    let main_by_mal: HashMap<u32, &ListItem> = main.iter().filter_map(|it| Some((mal_id_of(link.main, &it.media)?, it))).collect();
    let mut merge = Vec::new();
    for r in plan.rows.iter().filter(|r| r.verdict == Verdict::Different) {
        let (Some(mirror_id), Some(theirs), Some(m)) = (r.target, r.current, main_by_mal.get(&r.entry.mal_id)) else { continue };
        let mine = m.values();
        let episodes = m.media.episodes.or(r.media.as_ref().and_then(|x| x.episodes));
        let (update, conflict, with_mirror_status) = listxml::resolve(&mine, &theirs, episodes);
        merge.push(MergeRow { mal_id: r.entry.mal_id, title: m.media.title.clone(), main_id: m.media.id, mirror_id, main: mine, mirror: theirs, episodes, update, conflict, with_mirror_status });
    }
    let mut only_mirror = Vec::new();
    let mut unmatched_mirror = Vec::new();
    let mut seen = HashSet::new();
    for it in mirror {
        match mal_id_of(link.to, &it.media) {
            None => unmatched_mirror.push(it.media.title.clone()),
            Some(mal) if !main_by_mal.contains_key(&mal) && seen.insert(mal) => {
                let main_media = match link.main {
                    ServiceId::Mal => Some(mal_media(mal, &it.media.title, it.media.episodes)),
                    ServiceId::AniList => main_media.get(&mal).cloned(),
                };
                only_mirror.push(OnlyMirror { mal_id: mal, item: it.clone(), main_media });
            }
            Some(_) => {}
        }
    }
    Preview { link, plan, merge, only_mirror, unmatched_mirror }
}

/// The changes `align` makes: (for the mirror, for the main list). `mirror_status`: MyAnimeList
/// ids of merge conflicts settled for the mirror's status (the rest keep the main list's).
pub fn align_actions(p: &Preview, align: Align, mirror_status: &HashSet<u32>) -> (Vec<Action>, Vec<Action>) {
    let mut to = Vec::new();
    let mut main = Vec::new();
    if align == Align::DontAlign {
        return (to, main);
    }
    // Only on the main list: added to the mirror, with the main values.
    to.extend(listxml::actions(&p.plan, listxml::Mode::NewOnly, &HashSet::new()));
    for r in &p.merge {
        let target = match align {
            Align::MainWins => r.main,
            _ => r.merged(mirror_status.contains(&r.mal_id)),
        };
        let c = full(&target).without_same_ten(&r.mirror);
        if !c.is_empty() {
            to.push(Action::Update { kind: Kind::Anime, media_id: r.mirror_id, change: c });
        }
        if align == Align::Merge {
            let c = full(&target).without_same_ten(&r.main);
            if !c.is_empty() {
                main.push(Action::Update { kind: Kind::Anime, media_id: r.main_id, change: c });
            }
        }
    }
    if align == Align::Merge {
        for o in &p.only_mirror {
            if let Some(m) = &o.main_media {
                main.push(Action::Add { media: Box::new(m.clone()), values: o.item.values() });
            }
        }
    }
    (to, main)
}

#[cfg(test)]
mod tests;
