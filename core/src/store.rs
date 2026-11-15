//! The SQLite store: the local cache of the user's list (DESIGN.md §5.3). Schema versions go by
//! `PRAGMA user_version`; each migration runs once, in a transaction.
//!
//! Tokens are never here (they live in the 0600 token file). Only media on the user's list is kept,
//! and signing out deletes it (MAL API licence: keep data only as long as needed).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::service::Details;
use crate::{ListItem, Media, Status};

/// Each entry is one migration; the schema version is the number of entries applied.
pub(crate) const MIGRATIONS: &[&str] = &[
    // 1: the cached list. `pending_op` and the rest come with the queue (stage 3).
    "CREATE TABLE account (
        service TEXT PRIMARY KEY,
        user_name TEXT NOT NULL
     );
     CREATE TABLE media (
        service TEXT NOT NULL,
        id INTEGER NOT NULL,
        title TEXT NOT NULL,
        title_en TEXT,
        title_ja TEXT,
        synonyms TEXT NOT NULL DEFAULT '[]',
        episodes INTEGER,
        format TEXT NOT NULL DEFAULT '',
        air_status TEXT NOT NULL DEFAULT '',
        cover_url TEXT,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, id)
     );
     CREATE TABLE entry (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        status TEXT NOT NULL,
        progress INTEGER NOT NULL,
        score INTEGER,
        rewatching INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (service, media_id),
        FOREIGN KEY (service, media_id) REFERENCES media (service, id)
     );
     CREATE TABLE kv (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
     );",
    // 2: the sync queue (stage 3, `queue.rs`). One row per change waiting for the service (several
    // edits merge into one row while it waits). `status`, `progress`, `score`: the new values, NULL
    // when unchanged (`score` 0 removes the score). `old_*`: the entry before the first edit.
    // `base_updated_at`: the service's `updated_at` the edit started from (conflict check).
    // `r_*`: the service's entry when a conflict was found (`r_status` NULL: removed there).
    // State: queued, sending, conflict, failed, done (kept a day for the Sent list).
    "CREATE TABLE pending_op (
        id INTEGER PRIMARY KEY,
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        status TEXT,
        progress INTEGER,
        score INTEGER,
        old_status TEXT NOT NULL,
        old_progress INTEGER NOT NULL,
        old_score INTEGER NOT NULL,
        base_updated_at INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        edited_at INTEGER NOT NULL,
        edits INTEGER NOT NULL DEFAULT 1,
        state TEXT NOT NULL DEFAULT 'queued',
        attempts INTEGER NOT NULL DEFAULT 0,
        next_try INTEGER NOT NULL DEFAULT 0,
        error TEXT,
        sent_at INTEGER,
        r_status TEXT,
        r_progress INTEGER,
        r_score INTEGER,
        r_updated_at INTEGER
     );
     CREATE INDEX pending_op_state ON pending_op (service, state);",
    // 3: the recognition cache (stage 4, `recognize.rs`). `key`: the parsed title, normalised, and
    // its season (`lanterns of kirigaoka#1`). `source`: `user` (picked in "Wrong anime"), `ignore`
    // (not an anime), `search` (one MAL search; `media_id` 0 when it found nothing). `title`: the
    // found anime's name, so a search hit not on the list needs no second call. List matches are
    // not cached: they follow the list. Search rows older than 30 days are dropped.
    "CREATE TABLE recognition (
        service TEXT NOT NULL,
        key TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        source TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        used_at INTEGER NOT NULL,
        PRIMARY KEY (service, key)
     );",
    // 4: search and add, details (stage 5). `pending_op.added`: the change puts an anime on the
    // list that wasn't there (Discard removes it again; "not on the service" is no conflict).
    // `details`: an anime's information page as JSON (`service::Details`), kept for titles on
    // the list, and for others only a week, at most `DETAILS_KEEP` of them (no hoarding).
    "ALTER TABLE pending_op ADD COLUMN added INTEGER NOT NULL DEFAULT 0;
     CREATE TABLE details (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        body TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, media_id)
     );",
];

/// Details of titles not on the list kept at most (the newest).
pub const DETAILS_KEEP: i64 = 100;
/// How long details of a title not on the list are kept.
pub const DETAILS_MAX_AGE: i64 = 7 * 86_400;

#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// Opens (and creates, and migrates) the database file. The folder is created 0700.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            crate::paths::create_private_dir(dir).map_err(|e| StoreError(format!("can't create {}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        // Another thread of ours may write while the UI loads: wait a little instead of failing.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        Store::init(conn)
    }

    pub fn open_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    #[cfg(test)]
    pub(crate) fn from_connection(conn: Connection) -> Result<Store> {
        Store::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > MIGRATIONS.len() {
            return Err(StoreError(format!("the database is from a newer Kotora (schema {version})")));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        Ok(Store { conn })
    }

    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ----------------------------------------------------------------------------- account

    pub fn set_account(&self, service: &str, user_name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO account (service, user_name) VALUES (?1, ?2)
             ON CONFLICT (service) DO UPDATE SET user_name = excluded.user_name",
            params![service, user_name],
        )?;
        Ok(())
    }

    pub fn account(&self, service: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT user_name FROM account WHERE service = ?1", [service], |r| r.get(0)).optional()?)
    }

    /// Sign out: forget the account and its whole saved list.
    pub fn clear_service(&mut self, service: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        for sql in [
            "DELETE FROM pending_op WHERE service = ?1",
            "DELETE FROM entry WHERE service = ?1",
            "DELETE FROM media WHERE service = ?1",
            "DELETE FROM account WHERE service = ?1",
            "DELETE FROM kv WHERE key LIKE ?1 || ':%'",
            "DELETE FROM recognition WHERE service = ?1",
            "DELETE FROM details WHERE service = ?1",
        ] {
            tx.execute(sql, [service])?;
        }
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------------ recognition

    /// The cached answer for a parsed title: (media id, source, title).
    pub fn recognition(&self, service: &str, key: &str) -> Result<Option<(u32, String, String)>> {
        Ok(self
            .conn
            .query_row("SELECT media_id, source, title FROM recognition WHERE service = ?1 AND key = ?2", params![service, key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
    }

    /// Saves an answer (and drops search answers older than 30 days).
    pub fn set_recognition(&self, service: &str, key: &str, media_id: u32, source: &str, title: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO recognition (service, key, media_id, source, title, used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (service, key) DO UPDATE SET media_id = ?3, source = ?4, title = ?5, used_at = ?6",
            params![service, key, media_id, source, title, now],
        )?;
        self.conn.execute("DELETE FROM recognition WHERE source = 'search' AND used_at < ?1", [now - 30 * 86_400])?;
        Ok(())
    }

    pub fn forget_recognition(&self, service: &str, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM recognition WHERE service = ?1 AND key = ?2", params![service, key])?;
        Ok(())
    }

    // ------------------------------------------------------------------------------- list

    /// Replaces the saved list of `service` with the fetched one, in one transaction. Entries and
    /// media that are no longer on the list go away (no hoarding). Records the sync time.
    /// Changes still in the queue are laid over the fetched entries: they win until sent.
    pub fn replace_list(&mut self, service: &str, items: &[ListItem], now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM entry WHERE service = ?1", [service])?;
        {
            let mut media = tx.prepare(
                "INSERT INTO media (service, id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT (service, id) DO UPDATE SET title = excluded.title, title_en = excluded.title_en,
                   title_ja = excluded.title_ja, synonyms = excluded.synonyms, episodes = excluded.episodes,
                   format = excluded.format, air_status = excluded.air_status, cover_url = excluded.cover_url,
                   fetched_at = excluded.fetched_at",
            )?;
            let mut entry = tx.prepare(
                "INSERT OR REPLACE INTO entry (service, media_id, status, progress, score, rewatching, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for it in items {
                let m = &it.media;
                media.execute(params![
                    service,
                    m.id,
                    m.title,
                    m.title_en,
                    m.title_ja,
                    serde_json::to_string(&m.synonyms).unwrap_or_else(|_| "[]".into()),
                    m.episodes,
                    m.format,
                    m.air_status,
                    m.cover_url,
                    now
                ])?;
                entry.execute(params![service, m.id, it.status.mal_name(), it.progress, it.score, it.rewatching, it.updated_at])?;
            }
        }
        crate::queue::lay_over(&tx, service)?;
        // Keep the media of a change that still waits (its title shows in the queue).
        tx.execute(
            "DELETE FROM media WHERE service = ?1 AND id NOT IN (SELECT media_id FROM entry WHERE service = ?1)
               AND id NOT IN (SELECT media_id FROM pending_op WHERE service = ?1 AND state != 'done')",
            [service],
        )?;
        tx.execute("INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![format!("{service}:synced_at"), now.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// The saved list, newest update first.
    pub fn load_list(&self, service: &str) -> Result<Vec<ListItem>> {
        let mut st = self.conn.prepare(&format!("{ITEM_SELECT} WHERE e.service = ?1 ORDER BY e.updated_at DESC, m.id"))?;
        let rows = st.query_map([service], item_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// One saved entry.
    pub fn load_item(&self, service: &str, media_id: u32) -> Result<Option<ListItem>> {
        Ok(self.conn.query_row(&format!("{ITEM_SELECT} WHERE e.service = ?1 AND e.media_id = ?2"), params![service, media_id], item_row).optional()?)
    }


    /// Updates the saved media row of an anime on the list (titles, count, cover) from fresher
    /// data, e.g. its details page. Nothing happens for an anime not on the list.
    pub fn refresh_media(&self, service: &str, m: &Media, now: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE media SET title = ?3, title_en = ?4, title_ja = ?5, synonyms = ?6, episodes = ?7, format = ?8,
               air_status = ?9, cover_url = COALESCE(?10, cover_url), fetched_at = ?11
             WHERE service = ?1 AND id = ?2 AND id IN (SELECT media_id FROM entry WHERE service = ?1)",
            params![
                service,
                m.id,
                m.title,
                m.title_en,
                m.title_ja,
                serde_json::to_string(&m.synonyms).unwrap_or_else(|_| "[]".into()),
                m.episodes,
                m.format,
                m.air_status,
                m.cover_url,
                now
            ],
        )?;
        Ok(n > 0)
    }

    // ---------------------------------------------------------------------------- details

    /// The saved information page of an anime, and when it was fetched.
    pub fn details(&self, service: &str, media_id: u32) -> Result<Option<(Details, i64)>> {
        let row: Option<(String, i64)> = self
            .conn
            .query_row("SELECT body, fetched_at FROM details WHERE service = ?1 AND media_id = ?2", params![service, media_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.and_then(|(body, at)| serde_json::from_str(&body).ok().map(|d| (d, at))))
    }

    /// Saves an information page, then forgets old ones of titles not on the list: older than
    /// a week, or past the newest [`DETAILS_KEEP`].
    pub fn save_details(&self, service: &str, d: &Details, now: i64) -> Result<()> {
        let body = serde_json::to_string(d).map_err(|e| StoreError(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO details (service, media_id, body, fetched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (service, media_id) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
            params![service, d.id, body, now],
        )?;
        const OFF_LIST: &str = "service = ?1 AND media_id NOT IN (SELECT media_id FROM entry WHERE service = ?1)";
        self.conn.execute(&format!("DELETE FROM details WHERE {OFF_LIST} AND fetched_at < ?2"), params![service, now - DETAILS_MAX_AGE])?;
        self.conn.execute(
            &format!(
                "DELETE FROM details WHERE {OFF_LIST} AND media_id NOT IN
                   (SELECT media_id FROM details WHERE {OFF_LIST} ORDER BY fetched_at DESC, media_id LIMIT ?2)"
            ),
            params![service, DETAILS_KEEP],
        )?;
        Ok(())
    }

    // --------------------------------------------------------------------------------- kv

    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    /// Unix seconds of the last full sync of `service`.
    pub fn synced_at(&self, service: &str) -> Result<Option<i64>> {
        Ok(self.kv_get(&format!("{service}:synced_at"))?.and_then(|v| v.parse().ok()))
    }
}

const ITEM_SELECT: &str = "SELECT m.id, m.title, m.title_en, m.title_ja, m.synonyms, m.episodes, m.format, m.air_status, m.cover_url,
        e.status, e.progress, e.score, e.rewatching, e.updated_at
 FROM entry e JOIN media m ON m.service = e.service AND m.id = e.media_id";

fn item_row(r: &rusqlite::Row) -> rusqlite::Result<ListItem> {
    let synonyms: String = r.get(4)?;
    let status: String = r.get(9)?;
    Ok(ListItem {
        media: Media {
            id: r.get(0)?,
            title: r.get(1)?,
            title_en: r.get(2)?,
            title_ja: r.get(3)?,
            synonyms: serde_json::from_str(&synonyms).unwrap_or_default(),
            episodes: r.get(5)?,
            format: r.get(6)?,
            air_status: r.get(7)?,
            cover_url: r.get(8)?,
        },
        status: Status::from_mal(&status).unwrap_or(Status::PlanToWatch),
        progress: r.get(10)?,
        score: r.get(11)?,
        rewatching: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn item(id: u32, title: &str, status: Status, progress: u32, updated_at: i64) -> ListItem {
        ListItem {
            media: Media {
                id,
                title: title.into(),
                title_en: Some(format!("{title} (en)")),
                title_ja: None,
                synonyms: vec!["Alias One".into()],
                episodes: Some(12),
                format: "tv".into(),
                air_status: if id.is_multiple_of(2) { "currently_airing".into() } else { "finished_airing".into() },
                cover_url: Some(format!("https://example.invalid/{id}.jpg")),
            },
            status,
            progress,
            score: (progress > 3).then_some(8),
            rewatching: false,
            updated_at,
        }
    }

    #[test]
    fn migrates_once_and_reopens() {
        let dir = std::env::temp_dir().join(format!("kotora-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sub/kotora.db");
        {
            let mut s = Store::open(&path).unwrap();
            assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
            s.replace_list("mal", &[item(1, "One", Status::Watching, 2, 100)], 5).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        assert_eq!(s.load_list("mal").unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_a_newer_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        assert!(Store::init(conn).is_err());
    }

    #[test]
    fn round_trips_the_list_newest_first() {
        let mut s = Store::open_memory().unwrap();
        let items = vec![item(1, "Old", Status::Completed, 12, 100), item(2, "New", Status::Watching, 5, 300), item(3, "Mid", Status::Dropped, 1, 200)];
        s.replace_list("mal", &items, 1000).unwrap();
        let got = s.load_list("mal").unwrap();
        assert_eq!(got.iter().map(|i| i.media.id).collect::<Vec<_>>(), vec![2, 3, 1]);
        assert_eq!(got[0], items[1]);
        assert_eq!(s.synced_at("mal").unwrap(), Some(1000));
        assert!(s.load_list("anilist").unwrap().is_empty());
    }

    #[test]
    fn replacing_drops_removed_entries_and_their_media() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[item(1, "A", Status::Watching, 1, 1), item(2, "B", Status::Watching, 1, 2)], 1).unwrap();
        s.replace_list("mal", &[item(2, "B2", Status::Completed, 12, 3)], 2).unwrap();
        let got = s.load_list("mal").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].media.title, "B2");
        let media: i64 = s.conn.query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0)).unwrap();
        assert_eq!(media, 1);
    }

    #[test]
    fn failed_replace_keeps_the_old_list() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[item(1, "A", Status::Watching, 1, 1)], 1).unwrap();
        // Two items with a corrupt value can't be built through the types, so break the schema instead.
        s.conn.execute_batch("DROP TABLE kv").unwrap();
        assert!(s.replace_list("mal", &[item(9, "Z", Status::Watching, 1, 1)], 2).is_err());
        let got = s.load_list("mal").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].media.id, 1);
    }

    #[test]
    fn account_and_sign_out() {
        let mut s = Store::open_memory().unwrap();
        assert_eq!(s.account("mal").unwrap(), None);
        s.set_account("mal", "demo_user").unwrap();
        s.set_account("mal", "demo_user2").unwrap();
        assert_eq!(s.account("mal").unwrap().as_deref(), Some("demo_user2"));
        s.replace_list("mal", &[item(1, "A", Status::Watching, 1, 1)], 1).unwrap();
        s.kv_set("other", "keep").unwrap();
        s.clear_service("mal").unwrap();
        assert_eq!(s.account("mal").unwrap(), None);
        assert!(s.load_list("mal").unwrap().is_empty());
        assert_eq!(s.synced_at("mal").unwrap(), None);
        assert_eq!(s.kv_get("other").unwrap().as_deref(), Some("keep"));
    }

    fn details(id: u32) -> crate::service::Details {
        crate::service::Details { id, title: format!("Show {id}"), synopsis: "made up".into(), episodes: Some(12), ..Default::default() }
    }

    #[test]
    fn details_are_kept_for_the_list_and_briefly_for_the_rest() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[item(1, "On the list", Status::Watching, 1, 1)], 1).unwrap();
        s.save_details("mal", &details(1), 100).unwrap();
        s.save_details("mal", &details(2), 100).unwrap();
        let (d, at) = s.details("mal", 2).unwrap().unwrap();
        assert_eq!((d.title.as_str(), d.episodes, at), ("Show 2", Some(12), 100));
        // A week later, another save drops the one not on the list.
        s.save_details("mal", &details(3), 100 + DETAILS_MAX_AGE + 1).unwrap();
        assert!(s.details("mal", 2).unwrap().is_none());
        assert!(s.details("mal", 1).unwrap().is_some(), "on the list: kept");
        // At most DETAILS_KEEP off the list, the newest.
        for id in 10..(10 + DETAILS_KEEP as u32 + 5) {
            s.save_details("mal", &details(id), 1_000_000 + i64::from(id)).unwrap();
        }
        let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM details WHERE media_id != 1", [], |r| r.get(0)).unwrap();
        assert_eq!(n, DETAILS_KEEP);
        assert!(s.details("mal", 10).unwrap().is_none() && s.details("mal", 10 + DETAILS_KEEP as u32 + 4).unwrap().is_some());
        // Sign out forgets them.
        s.clear_service("mal").unwrap();
        assert!(s.details("mal", 1).unwrap().is_none());
    }

    #[test]
    fn refresh_media_touches_only_the_list() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[item(1, "Old title", Status::Watching, 1, 1)], 1).unwrap();
        let mut m = details(1).media();
        m.episodes = Some(24);
        assert!(s.refresh_media("mal", &m, 5).unwrap());
        assert_eq!(s.load_item("mal", 1).unwrap().unwrap().media.episodes, Some(24));
        assert!(!s.refresh_media("mal", &details(9).media(), 5).unwrap(), "not on the list: nothing saved");
    }
}
