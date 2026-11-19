//! The user's marks on feed releases (`feed_marks`, an additive table: `store.rs`). Per
//! service, series and episodes: *got* (opened; its hash and version, so a v2 shows),
//! *hidden* (this episode), *not this series* (a release title, by `Parsed::key`). Local only:
//! never synced, never exported; signing out keeps them, "Clear feed history" drops them.

use rusqlite::params;

use super::select::Marks;
use crate::store::{Result, Store};

/// How long the history keeps a got mark (the History tab shows the last 90 days).
pub const KEEP: i64 = 90 * 86_400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Got {
        info_hash: Option<String>,
        version: Option<u32>,
        title: String,
    },
    Hidden,
    /// The release titles with this key are not this series.
    NotThis {
        key: String,
    },
}

/// One opened release, for the History tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub media_id: u32,
    pub episodes: (u32, u32),
    pub title: String,
    pub at: i64,
}

impl Store {
    /// Marks `episodes` of a series (for `NotThis`, the episodes are ignored).
    pub fn feed_mark(
        &self,
        service: &str,
        media_id: u32,
        episodes: (u32, u32),
        mark: &Mark,
        at: i64,
    ) -> Result<()> {
        let (state, (a, b), key, hash, version, title) = match mark {
            Mark::Got {
                info_hash,
                version,
                title,
            } => (
                "got",
                episodes,
                "",
                info_hash.as_deref(),
                *version,
                title.as_str(),
            ),
            Mark::Hidden => ("hidden", episodes, "", None, None, ""),
            Mark::NotThis { key } => ("not", (0, 0), key.as_str(), None, None, ""),
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO feed_marks (service, media_id, ep_from, ep_to, state, rkey, info_hash, version, title, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![service, media_id, a, b, state, key, hash, version, title, at],
        )?;
        Ok(())
    }

    /// Takes a mark back ("Undo mark"). `NotThis` by its key.
    pub fn feed_unmark(
        &self,
        service: &str,
        media_id: u32,
        episodes: (u32, u32),
        mark: &Mark,
    ) -> Result<()> {
        let (state, (a, b), key) = match mark {
            Mark::Got { .. } => ("got", episodes, ""),
            Mark::Hidden => ("hidden", episodes, ""),
            Mark::NotThis { key } => ("not", (0, 0), key.as_str()),
        };
        self.conn.execute(
            "DELETE FROM feed_marks WHERE service = ?1 AND media_id = ?2 AND ep_from = ?3 AND ep_to = ?4 AND state = ?5 AND rkey = ?6",
            params![service, media_id, a, b, state, key],
        )?;
        Ok(())
    }

    /// Every mark of one service, as [`super::select::select`] reads them.
    pub fn feed_marks(&self, service: &str) -> Result<Marks> {
        let mut m = Marks::default();
        let mut st = self.conn.prepare("SELECT media_id, ep_from, ep_to, state, rkey, info_hash, version FROM feed_marks WHERE service = ?1")?;
        let rows = st.query_map([service], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<u32>>(6)?,
            ))
        })?;
        for row in rows {
            let (id, a, b, state, key, hash, version) = row?;
            match state.as_str() {
                "got" => {
                    m.got.insert((id, a, b), (hash, version));
                }
                "hidden" => {
                    m.hidden.insert((id, a, b));
                }
                "not" => {
                    m.not_this.insert((id, key));
                }
                _ => {}
            }
        }
        Ok(m)
    }

    /// Opened releases since `since`, newest first.
    pub fn feed_history(&self, service: &str, since: i64) -> Result<Vec<Opened>> {
        let mut st = self.conn.prepare(
            "SELECT media_id, ep_from, ep_to, title, at FROM feed_marks WHERE service = ?1 AND state = 'got' AND at >= ?2 ORDER BY at DESC",
        )?;
        let rows = st.query_map(params![service, since], |r| {
            Ok(Opened {
                media_id: r.get(0)?,
                episodes: (r.get(1)?, r.get(2)?),
                title: r.get(3)?,
                at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Got marks older than [`KEEP`] go (the progress has passed them by then, or the user
    /// moved on). Hidden and not-this marks stay.
    pub fn prune_feed_marks(&self, now: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM feed_marks WHERE state = 'got' AND at < ?1",
            [now - KEEP],
        )?)
    }

    /// "Clear feed history": every mark, every service.
    pub fn clear_feed_marks(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM feed_marks", [])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_round_trip() {
        let s = Store::open_memory().unwrap();
        let got = Mark::Got {
            info_hash: Some("ab".into()),
            version: Some(2),
            title: "[Hoshi] Lanterns of Kirigaoka - 08v2".into(),
        };
        s.feed_mark("mal", 1, (8, 8), &got, 1000).unwrap();
        s.feed_mark("mal", 1, (9, 9), &Mark::Hidden, 1000).unwrap();
        s.feed_mark(
            "mal",
            1,
            (0, 0),
            &Mark::NotThis {
                key: "kirigaoka#1".into(),
            },
            1000,
        )
        .unwrap();
        s.feed_mark("anilist", 7, (1, 1), &Mark::Hidden, 1000)
            .unwrap();
        let m = s.feed_marks("mal").unwrap();
        assert_eq!(m.got.get(&(1, 8, 8)), Some(&(Some("ab".into()), Some(2))));
        assert!(m.hidden.contains(&(1, 9, 9)) && m.not_this.contains(&(1, "kirigaoka#1".into())));
        assert_eq!(m.hidden.len(), 1, "per service");
        assert_eq!(
            s.feed_history("mal", 0).unwrap(),
            [Opened {
                media_id: 1,
                episodes: (8, 8),
                title: "[Hoshi] Lanterns of Kirigaoka - 08v2".into(),
                at: 1000
            }]
        );
        s.feed_unmark("mal", 1, (9, 9), &Mark::Hidden).unwrap();
        assert!(s.feed_marks("mal").unwrap().hidden.is_empty());
        assert_eq!(s.prune_feed_marks(1000 + KEEP + 1).unwrap(), 1);
        assert_eq!(s.clear_feed_marks().unwrap(), 2);
    }
}
