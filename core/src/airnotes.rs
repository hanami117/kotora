//! Airing notifications (Settings → Notifications): what to announce, decided from the saved
//! list. No network of its own: the list sync and the details page refresh the saved airing
//! status, and the episode times are the Airing page's estimate ([`Timetable`]).
//!
//! - **Started airing**: a Plan to watch show whose airing status went from `not_yet_aired` to
//!   `currently_airing` since Kotora last looked (`air_seen`).
//! - **New episode**: a Watching show whose next episode's estimated slot has passed, in the
//!   last [`RECENT`] seconds (no flood of old episodes after a long break), and past the user's
//!   progress.
//!
//! Each is announced once (`announced`), whatever the switches say: a switch turned on later
//! never brings old ones back.

use std::collections::HashMap;

use rusqlite::params;

use crate::airing::Timetable;
use crate::store::{Result, Store};
use crate::{ListItem, Status};

/// An episode whose slot passed longer ago than this is not announced (two days: a Mac asleep
/// over a weekend night still hears of it, an episode from last week never).
pub const RECENT: i64 = 2 * 86_400;

/// Announcements older than this are forgotten (the episode and the season are long past).
const KEEP: i64 = 120 * 86_400;

/// What to announce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    Started,
    /// Episode N is out.
    Episode(u32),
    /// Manga: chapter N is out (Publishing, `mangaupdates::new_chapter`).
    Chapter(u32),
}

impl What {
    /// The `announced` table's key.
    pub fn key(self) -> String {
        match self {
            What::Started => "started".into(),
            What::Episode(n) => format!("ep{n}"),
            // "m": manga ids are another set than anime ids.
            What::Chapter(n) => format!("mch{n}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Due {
    pub media_id: u32,
    pub what: What,
}

/// A Plan to watch show that started airing since it was last seen not aired yet.
pub fn started(it: &ListItem, seen: Option<&str>) -> bool {
    it.status == Status::PlanToWatch && it.media.air_status == "currently_airing" && seen == Some("not_yet_aired")
}

/// The episode of a Watching show that came out (by the estimate) in the last [`RECENT`]
/// seconds, when it is past the progress. The timetable moves forward with the progress, as
/// on the Airing page.
pub fn new_episode(it: &ListItem, now: i64) -> Option<u32> {
    if it.status != Status::Watching || !matches!(it.media.air_status.as_str(), "currently_airing" | "not_yet_aired") {
        return None;
    }
    let tt = Timetable::of_media(&it.media)?.with_progress(it.progress, now);
    let n = tt.aired(now);
    if n <= it.progress {
        return None;
    }
    let at = tt.time_of(n)?;
    (at <= now && now - at < RECENT).then_some(n)
}

impl Store {
    /// The airing status last looked at, per anime.
    pub fn air_seen(&self, service: &str) -> Result<HashMap<u32, String>> {
        let mut st = self.conn.prepare("SELECT media_id, air_status FROM air_seen WHERE service = ?1")?;
        let rows = st.query_map([service], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Remembers the list's airing statuses (anime no longer on the list go).
    pub fn set_air_seen(&mut self, service: &str, items: &[ListItem]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM air_seen WHERE service = ?1", [service])?;
        for it in items {
            tx.execute("INSERT INTO air_seen (service, media_id, air_status) VALUES (?1, ?2, ?3)", params![service, it.media.id, it.media.air_status])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Marks `what` announced; false when it already was.
    pub fn announce_once(&self, service: &str, media_id: u32, what: What, now: i64) -> Result<bool> {
        let n = self.conn.execute("INSERT OR IGNORE INTO announced (service, media_id, what, at) VALUES (?1, ?2, ?3, ?4)", params![service, media_id, what.key(), now])?;
        Ok(n == 1)
    }
}

/// One look at the saved list: what to announce now, each marked announced. The airing statuses
/// are remembered for the next look. The first look (nothing seen yet) announces no start.
pub fn pass(store: &mut Store, service: &str, now: i64) -> Result<Vec<Due>> {
    let items = store.load_list(service)?;
    let seen = store.air_seen(service)?;
    let mut due = Vec::new();
    for it in &items {
        let id = it.media.id;
        let mut what = Vec::new();
        if started(it, seen.get(&id).map(String::as_str)) {
            what.push(What::Started);
        }
        if let Some(n) = new_episode(it, now) {
            what.push(What::Episode(n));
        }
        for w in what {
            if store.announce_once(service, id, w, now)? {
                due.push(Due { media_id: id, what: w });
            }
        }
    }
    store.set_air_seen(service, &items)?;
    store.conn.execute("DELETE FROM announced WHERE service = ?1 AND at < ?2", params![service, now - KEEP])?;
    Ok(due)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airing::Broadcast;
    use crate::store::tests::item;

    /// Saturday 2026-10-03, slot Saturday 23:30 JST = 14:30 UTC.
    const EP1: i64 = 1_791_037_800;
    const H: i64 = 3600;

    fn airing(id: u32, status: Status, progress: u32, air: &str) -> ListItem {
        let mut it = item(id, &format!("Show {id}"), status, progress, 1);
        it.media.air_status = air.into();
        it.media.start_date = Some("2026-10-03".into());
        it.media.broadcast = Broadcast::parse("saturday", Some("23:30"));
        it
    }

    #[test]
    fn the_estimate_matches_the_airing_page() {
        let tt = Timetable::of_media(&airing(1, Status::Watching, 0, "currently_airing").media).unwrap();
        assert_eq!(tt.time_of(1), Some(EP1));
    }

    #[test]
    fn a_new_episode_once_its_estimated_slot_passes() {
        // At 5 of 12, episode 6 airs five weeks after episode 1.
        let it = airing(1, Status::Watching, 5, "currently_airing");
        let ep6 = EP1 + 5 * crate::airing::WEEK;
        assert_eq!(new_episode(&it, ep6 - 60), None, "not yet");
        assert_eq!(new_episode(&it, ep6), Some(6));
        assert_eq!(new_episode(&it, ep6 + RECENT - 1), Some(6));
        assert_eq!(new_episode(&it, ep6 + RECENT), None, "too long ago: no old news");
        // A week behind: only the newest episode, when it comes out.
        assert_eq!(new_episode(&it, ep6 + crate::airing::WEEK + H), Some(7));
    }

    #[test]
    fn never_for_an_episode_already_watched() {
        let ep6 = EP1 + 5 * crate::airing::WEEK;
        assert_eq!(new_episode(&airing(1, Status::Watching, 6, "currently_airing"), ep6 + H), None);
        // Progress past the estimate: nothing until an episode past the progress is due.
        let ahead = airing(1, Status::Watching, 7, "currently_airing");
        assert_eq!(new_episode(&ahead, ep6 + H), None);
        assert_eq!(new_episode(&ahead, ep6 + crate::airing::WEEK + H), None, "7 is watched");
        assert_eq!(new_episode(&ahead, ep6 + 2 * crate::airing::WEEK + H), Some(8));
    }

    #[test]
    fn only_watching_shows_that_air_get_episodes() {
        let ep6 = EP1 + 5 * crate::airing::WEEK + H;
        for st in [Status::PlanToWatch, Status::Completed, Status::OnHold, Status::Dropped] {
            assert_eq!(new_episode(&airing(1, st, 5, "currently_airing"), ep6), None, "{st:?}");
        }
        assert_eq!(new_episode(&airing(1, Status::Watching, 5, "finished_airing"), ep6), None);
        let mut no_slot = airing(1, Status::Watching, 5, "currently_airing");
        no_slot.media.broadcast = Broadcast::parse("saturday", None);
        assert_eq!(new_episode(&no_slot, ep6), None, "no slot time, no estimate");
        // Past the count: nothing.
        let mut short = airing(1, Status::Watching, 5, "currently_airing");
        short.media.episodes = Some(5);
        assert_eq!(new_episode(&short, ep6), None);
    }

    #[test]
    fn started_airing_is_the_status_change_on_plan_to_watch() {
        let ptw = airing(1, Status::PlanToWatch, 0, "currently_airing");
        assert!(started(&ptw, Some("not_yet_aired")));
        assert!(!started(&ptw, None), "first look: nothing to compare");
        assert!(!started(&ptw, Some("currently_airing")));
        assert!(!started(&airing(1, Status::Watching, 0, "currently_airing"), Some("not_yet_aired")));
        assert!(!started(&airing(1, Status::PlanToWatch, 0, "not_yet_aired"), Some("not_yet_aired")));
    }

    #[test]
    fn each_is_announced_once_and_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!("kotora-airnotes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kotora.db");
        let ep6 = EP1 + 5 * crate::airing::WEEK + H;
        {
            let mut s = Store::open(&path).unwrap();
            s.replace_list("mal", &[airing(1, Status::PlanToWatch, 0, "not_yet_aired"), airing(2, Status::Watching, 5, "currently_airing")], 1).unwrap();
            assert_eq!(pass(&mut s, "mal", ep6).unwrap(), [Due { media_id: 2, what: What::Episode(6) }]);
            assert_eq!(pass(&mut s, "mal", ep6 + 60).unwrap(), [], "once");
            // The sync says it started airing: announced once.
            s.replace_list("mal", &[airing(1, Status::PlanToWatch, 0, "currently_airing"), airing(2, Status::Watching, 5, "currently_airing")], 2).unwrap();
            assert_eq!(pass(&mut s, "mal", ep6 + 120).unwrap(), [Due { media_id: 1, what: What::Started }]);
            assert_eq!(pass(&mut s, "mal", ep6 + 180).unwrap(), []);
        }
        let mut s = Store::open(&path).unwrap();
        assert_eq!(pass(&mut s, "mal", ep6 + 240).unwrap(), [], "not again after a restart");
        // Back to not aired and airing again (MAL flips): still never again.
        s.replace_list("mal", &[airing(1, Status::PlanToWatch, 0, "not_yet_aired")], 3).unwrap();
        pass(&mut s, "mal", ep6 + 300).unwrap();
        s.replace_list("mal", &[airing(1, Status::PlanToWatch, 0, "currently_airing")], 4).unwrap();
        assert_eq!(pass(&mut s, "mal", ep6 + 360).unwrap(), []);
        // Sign out forgets it all.
        s.clear_service("mal").unwrap();
        assert!(s.air_seen("mal").unwrap().is_empty());
        assert!(s.announce_once("mal", 1, What::Started, 0).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_look_announces_no_start() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[airing(1, Status::PlanToWatch, 0, "currently_airing")], 1).unwrap();
        assert_eq!(pass(&mut s, "mal", EP1 + H).unwrap(), []);
        assert_eq!(s.air_seen("mal").unwrap().get(&1).map(String::as_str), Some("currently_airing"));
    }
}
