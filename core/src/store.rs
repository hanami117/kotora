//! The SQLite store: the local cache of the user's list. Schema versions go by
//! `PRAGMA user_version`; each migration runs once, in a transaction.
//!
//! Tokens are never here (they live in the 0600 token file). Only media on the user's list is kept,
//! and signing out deletes it (MAL API licence: keep data only as long as needed).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::airing::{Broadcast, NextAiring, SEASON_MAX_AGE, SEASONS_KEPT, Season, SeasonAnime};
use crate::service::Details;
use crate::{Kind, ListItem, Media, Scope, Status};

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
    // 5: rewatching. `entry.times_rewatched`: MAL `num_times_rewatched`. In `pending_op`, like the
    // other fields: `rewatching` and `times_rewatched` NULL when unchanged, `old_*` before the
    // first edit, `r_*` the service's in a conflict.
    "ALTER TABLE entry ADD COLUMN times_rewatched INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN rewatching INTEGER;
     ALTER TABLE pending_op ADD COLUMN times_rewatched INTEGER;
     ALTER TABLE pending_op ADD COLUMN old_rewatching INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN old_times_rewatched INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN r_rewatching INTEGER;
     ALTER TABLE pending_op ADD COLUMN r_times_rewatched INTEGER;",
    // 6: Airing. `media.start_date` and `media.broadcast` (`saturday 23:30`, Japan time) for the
    // timetable of airing entries. `season`: a season's anime list as JSON (`airing::SeasonAnime`),
    // fetched at most every `SEASON_FRESH` seconds; only the seasons the page shows are kept, and
    // sign out deletes them.
    "ALTER TABLE media ADD COLUMN start_date TEXT;
     ALTER TABLE media ADD COLUMN broadcast TEXT;
     CREATE TABLE season (
        service TEXT NOT NULL,
        year INTEGER NOT NULL,
        season TEXT NOT NULL,
        body TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, year, season)
     );",
    // 7: library folders (`library.rs`). One row per episode file found in a folder of Settings →
    // Library: its size and time (a rescan parses only new or changed files) and what its name
    // says. Not service data: matching to the list happens in memory, so signing out keeps it.
    "CREATE TABLE library_file (
        path TEXT PRIMARY KEY,
        folder TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime INTEGER NOT NULL,
        title TEXT NOT NULL,
        episode INTEGER,
        season INTEGER,
        grp TEXT
     );
     CREATE INDEX library_file_folder ON library_file (folder);",
    // 8: airing notifications (`airnotes.rs`). `air_seen`: the airing status last looked at, per
    // anime on the list (not_yet_aired, then currently_airing, is "Started airing").
    // `announced`: what was already announced (`started`, `ep7`), so nothing ever repeats.
    // Service data: sign out deletes both.
    "CREATE TABLE air_seen (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        air_status TEXT NOT NULL,
        PRIMARY KEY (service, media_id)
     );
     CREATE TABLE announced (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        what TEXT NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (service, media_id, what)
     );",
    // 9: the season picker. A saved season list is now the whole list (the top shows by members),
    // so the Airing page can show any season from it; before, the previous season kept only its
    // shows still airing. The old copies are partial: forget them (they are only a cache).
    "DELETE FROM season;",
    // 10: manga (0.2). The list's kind is part of every key that holds a MAL id, because anime
    // and manga ids overlap: `media`, `entry`, `details`, `recognition` (service + kind + id) and
    // `pending_op.kind`. SQLite can't change a primary key, so those four tables are rebuilt: the
    // old one is renamed, the new one created, the rows copied over as `anime`, the old one
    // dropped. `entry` and `media` go together (the foreign key). New fields: `media.volumes`
    // (the count), `media.authors` (JSON names), `entry.volumes` (read), and in `pending_op`
    // `volumes` / `old_volumes` / `r_volumes` like the other fields. The whole step runs in one
    // transaction with the version bump, and `Store::open` keeps a copy of the file first.
    // `air_seen`, `announced`, `season` stay anime-only.
    "ALTER TABLE entry RENAME TO entry_v9;
     ALTER TABLE media RENAME TO media_v9;
     CREATE TABLE media (
        service TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT 'anime',
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
        start_date TEXT,
        broadcast TEXT,
        volumes INTEGER,
        authors TEXT NOT NULL DEFAULT '[]',
        PRIMARY KEY (service, kind, id)
     );
     INSERT INTO media (service, kind, id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at,
        start_date, broadcast)
       SELECT service, 'anime', id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at,
        start_date, broadcast FROM media_v9;
     CREATE TABLE entry (
        service TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT 'anime',
        media_id INTEGER NOT NULL,
        status TEXT NOT NULL,
        progress INTEGER NOT NULL,
        score INTEGER,
        rewatching INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        times_rewatched INTEGER NOT NULL DEFAULT 0,
        volumes INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (service, kind, media_id),
        FOREIGN KEY (service, kind, media_id) REFERENCES media (service, kind, id)
     );
     INSERT INTO entry (service, kind, media_id, status, progress, score, rewatching, updated_at, times_rewatched)
       SELECT service, 'anime', media_id, status, progress, score, rewatching, updated_at, times_rewatched FROM entry_v9;
     DROP TABLE entry_v9;
     DROP TABLE media_v9;
     ALTER TABLE details RENAME TO details_v9;
     CREATE TABLE details (
        service TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT 'anime',
        media_id INTEGER NOT NULL,
        body TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, kind, media_id)
     );
     INSERT INTO details (service, kind, media_id, body, fetched_at) SELECT service, 'anime', media_id, body, fetched_at FROM details_v9;
     DROP TABLE details_v9;
     ALTER TABLE recognition RENAME TO recognition_v9;
     CREATE TABLE recognition (
        service TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT 'anime',
        key TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        source TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        used_at INTEGER NOT NULL,
        PRIMARY KEY (service, kind, key)
     );
     INSERT INTO recognition (service, kind, key, media_id, source, title, used_at)
       SELECT service, 'anime', key, media_id, source, title, used_at FROM recognition_v9;
     DROP TABLE recognition_v9;
     ALTER TABLE pending_op ADD COLUMN kind TEXT NOT NULL DEFAULT 'anime';
     ALTER TABLE pending_op ADD COLUMN volumes INTEGER;
     ALTER TABLE pending_op ADD COLUMN old_volumes INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN r_volumes INTEGER;
     CREATE INDEX pending_op_entry ON pending_op (service, kind, media_id);",
    // 11: AniList (0.2). `media.next_airing`: the next episode and its exact time from AniList's
    // `nextAiringEpisode` (`13 1767196800`: episode, unix seconds), which the Airing timetable
    // prefers to its own estimate. `media.id_mal`: an AniList media's MyAnimeList id (for
    // mirroring later). MyAnimeList rows leave both empty.
    "ALTER TABLE media ADD COLUMN next_airing TEXT;
     ALTER TABLE media ADD COLUMN id_mal INTEGER;",
    // 12 (0.2.1): scores on the 100-point scale (`score.rs`), so an AniList 73 stays 73 instead of
    // a rounded 7. Every score column held 1-10 (0 or NULL: none): times 10. Saved details pages
    // (`details.body`, JSON) keep the user's entry with its score too. `pending_op.removed`: the
    // change takes the entry off the list ("Remove from list"); `old_*` hold the entry as it was,
    // for Discard (Undo) and the conflict card. The mirror's saved website edits (kv
    // `mirror:edits`, JSON, written by 0.2.1 builds before this step) hold scores too.
    "UPDATE entry SET score = score * 10 WHERE score > 0;
     UPDATE pending_op SET score = score * 10 WHERE score > 0;
     UPDATE pending_op SET old_score = old_score * 10 WHERE old_score > 0;
     UPDATE pending_op SET r_score = r_score * 10 WHERE r_score > 0;
     UPDATE details SET body = json_set(body, '$.mine.score', json_extract(body, '$.mine.score') * 10)
       WHERE json_valid(body) AND json_extract(body, '$.mine.score') > 0;
     UPDATE kv SET value = (
       SELECT json_group_array(json(json_set(e.value,
         '$.main.score', CASE WHEN json_extract(e.value, '$.main.score') > 0 THEN json_extract(e.value, '$.main.score') * 10 END,
         '$.mirror.score', CASE WHEN json_extract(e.value, '$.mirror.score') > 0 THEN json_extract(e.value, '$.mirror.score') * 10 END)))
       FROM (SELECT value FROM json_each(kv.value) ORDER BY key) e)
       WHERE key = 'mirror:edits' AND json_valid(value) AND json_type(value) = 'array';
     ALTER TABLE pending_op ADD COLUMN removed INTEGER NOT NULL DEFAULT 0;",
];

/// Tables added without a schema step, so that an older Kotora (which ignores them) can still
/// open the database: they only hold Kotora's own records, never something an older version
/// must understand. Created at open when missing.
///
/// `activity` (0.4.5, `activity.rs`): one row per edit made in Kotora that raised an entry's
/// progress (`p_*`, episodes or chapters) or volumes (`v_*`), with its time, for the
/// Statistics page. Not service data: signing out keeps it. kv `activity:since`: when the log
/// started, so the page can say what it doesn't know.
///
/// `feed_marks` (0.6, `feeds/marks.rs`): the user's marks on feed releases, per series and
/// episodes: got (opened), hidden, not this series. Local only: never synced or exported.
const ADDITIVE: &str = "CREATE TABLE IF NOT EXISTS activity (
        id INTEGER PRIMARY KEY,
        service TEXT NOT NULL,
        kind TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        at INTEGER NOT NULL,
        p_from INTEGER NOT NULL,
        p_to INTEGER NOT NULL,
        v_from INTEGER NOT NULL DEFAULT 0,
        v_to INTEGER NOT NULL DEFAULT 0
     );
     CREATE INDEX IF NOT EXISTS activity_list ON activity (service, kind, at);
     INSERT OR IGNORE INTO kv (key, value) VALUES ('activity:since', CAST(strftime('%s', 'now') AS TEXT));
     CREATE TABLE IF NOT EXISTS feed_marks (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        ep_from INTEGER NOT NULL,
        ep_to INTEGER NOT NULL,
        state TEXT NOT NULL,
        rkey TEXT NOT NULL DEFAULT '',
        info_hash TEXT,
        version INTEGER,
        title TEXT NOT NULL DEFAULT '',
        at INTEGER NOT NULL,
        PRIMARY KEY (service, media_id, ep_from, ep_to, state, rkey)
     );";

/// The schema of Kotora 0.1.x (before manga): a database at this version is copied aside
/// before it is upgraded.
pub const SCHEMA_0_1: usize = 9;

/// The copy `Store::open` keeps of a database file before upgrading it from schema `version`:
/// `kotora.db.schema-9.bak` beside `kotora.db`.
pub fn backup_path(path: &Path, version: usize) -> std::path::PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".schema-{version}.bak"));
    path.with_file_name(name)
}

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

/// Held while a database file is backed up and migrated (see [`Store::open`]).
static UPGRADE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn user_version(conn: &Connection) -> Result<usize> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize)
}

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// Opens (and creates, and migrates) the database file. The folder is created 0700.
    ///
    /// Several threads open their own `Store` at start (the UI, sign-in, the queue, …). The
    /// backup and the migrations of one file run one opener at a time in this process
    /// ([`UPGRADE_LOCK`]), and each migration step takes the write lock first and checks the
    /// version again inside it, so a step another connection already ran is skipped.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            crate::paths::create_private_dir(dir).map_err(|e| StoreError(format!("can't create {}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path)?;
        // Another thread of ours may write while the UI loads: wait a little instead of failing.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if user_version(&conn)? >= MIGRATIONS.len() {
            // Up to date (or newer: `init` says so). No lock needed.
            return Store::init(conn);
        }
        let _upgrading = UPGRADE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Store::back_up_before_upgrade(&conn, path)?;
        Store::init(conn)
    }

    /// Before an existing database is upgraded, a whole copy goes beside it
    /// ([`backup_path`]); without one nothing is migrated. A copy already there (from an earlier
    /// try) is kept as it is: it is the older one. `VACUUM INTO` reads a consistent snapshot,
    /// WAL included. It writes to a temporary name first and the copy only takes its real name
    /// when none is there yet, so a half-written copy never looks like a backup.
    fn back_up_before_upgrade(conn: &Connection, path: &Path) -> Result<()> {
        let version = user_version(conn)?;
        if version == 0 || version >= MIGRATIONS.len() {
            return Ok(());
        }
        let copy = backup_path(path, version);
        if copy.exists() {
            return Ok(());
        }
        let fail = |e: String| StoreError(format!("can't keep a copy of the database before upgrading it ({e}); nothing was changed"));
        let mut tmp = copy.clone().into_os_string();
        tmp.push(format!(".part-{}", std::process::id()));
        let tmp = std::path::PathBuf::from(tmp);
        let _ = std::fs::remove_file(&tmp);
        let target = tmp.to_str().ok_or_else(|| StoreError(format!("can't name a backup of {}", path.display())))?;
        conn.execute("VACUUM INTO ?1", [target]).map_err(|e| fail(e.to_string()))?;
        // Another process may have made the copy meanwhile: the first one stays.
        let placed = if copy.exists() { Ok(()) } else { std::fs::hard_link(&tmp, &copy).or_else(|e| if copy.exists() { Ok(()) } else { Err(e) }) };
        let _ = std::fs::remove_file(&tmp);
        placed.map_err(|e| fail(e.to_string()))
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
        // One step at a time, each under the write lock (IMMEDIATE), the version read again
        // inside it: another connection to the same file may have run the step already.
        loop {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let version = user_version(&tx)?;
            if version > MIGRATIONS.len() {
                return Err(StoreError(format!("it was saved by a newer Kotora (database schema {version}, this one knows {}). Update Kotora to open it; nothing was changed", MIGRATIONS.len())));
            }
            if version == MIGRATIONS.len() {
                tx.commit()?;
                break;
            }
            tx.execute_batch(MIGRATIONS[version])?;
            tx.pragma_update(None, "user_version", (version + 1) as u32)?;
            tx.commit()?;
        }
        // Only a write when something is missing (most opens just read the schema).
        let have: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('activity', 'feed_marks')", [], |r| r.get(0))?;
        if have < 2 {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(ADDITIVE)?;
            tx.commit()?;
        }
        Ok(Store { conn })
    }

    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize)
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
            "DELETE FROM season WHERE service = ?1",
            "DELETE FROM air_seen WHERE service = ?1",
            "DELETE FROM announced WHERE service = ?1",
            // Either side of the mirror signed out: the mirror is off.
            "DELETE FROM kv WHERE key LIKE 'mirror:%' AND EXISTS (SELECT 1 FROM kv l WHERE l.key = 'mirror:link' AND (l.value LIKE ?1 || '>%' OR l.value LIKE '%>' || ?1))",
        ] {
            tx.execute(sql, [service])?;
        }
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------------ recognition

    /// The cached answer for a parsed title: (media id, source, title).
    pub fn recognition(&self, service: &str, key: &str) -> Result<Option<(u32, String, String)>> {
        self.recognition_of(service, Kind::Anime, key)
    }

    /// [`Store::recognition`] for a list of `kind` (manga reading uses `Kind::Manga`).
    /// The keys the user picked `media_id` for ("Wrong manga…"), of a list of `kind`.
    pub fn picked_keys(&self, service: &str, kind: Kind, media_id: u32) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT key FROM recognition WHERE service = ?1 AND kind = ?2 AND source = 'user' AND media_id = ?3")?;
        let rows = st.query_map(params![service, kind.name(), media_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn recognition_of(&self, service: &str, kind: Kind, key: &str) -> Result<Option<(u32, String, String)>> {
        Ok(self
            .conn
            .query_row("SELECT media_id, source, title FROM recognition WHERE service = ?1 AND kind = ?2 AND key = ?3", params![service, kind.name(), key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
    }

    /// Saves an answer (and drops search answers older than 30 days).
    pub fn set_recognition(&self, service: &str, key: &str, media_id: u32, source: &str, title: &str, now: i64) -> Result<()> {
        self.set_recognition_of(service, Kind::Anime, key, media_id, source, title, now)
    }

    /// [`Store::set_recognition`] for a list of `kind`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_recognition_of(&self, service: &str, kind: Kind, key: &str, media_id: u32, source: &str, title: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO recognition (service, kind, key, media_id, source, title, used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (service, kind, key) DO UPDATE SET media_id = ?4, source = ?5, title = ?6, used_at = ?7",
            params![service, kind.name(), key, media_id, source, title, now],
        )?;
        self.conn.execute("DELETE FROM recognition WHERE source = 'search' AND used_at < ?1", [now - 30 * 86_400])?;
        Ok(())
    }

    pub fn forget_recognition(&self, service: &str, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM recognition WHERE service = ?1 AND kind = 'anime' AND key = ?2", params![service, key])?;
        Ok(())
    }

    // ------------------------------------------------------------------------------- list

    /// Replaces the saved list of `scope` (a service name: its anime list) with the fetched one,
    /// in one transaction. Entries and media that are no longer on the list go away (no
    /// hoarding); the other kind's list is not touched. Records the sync time. Changes still in
    /// the queue are laid over the fetched entries: they win until sent.
    pub fn replace_list<'a>(&mut self, scope: impl Into<Scope<'a>>, items: &[ListItem], now: i64) -> Result<()> {
        let Scope { service, kind } = scope.into();
        let k = kind.name();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM entry WHERE service = ?1 AND kind = ?2", [service, k])?;
        {
            let mut media = tx.prepare(
                "INSERT INTO media (service, id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at,
                   start_date, broadcast, kind, volumes, authors, next_airing, id_mal)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                 ON CONFLICT (service, kind, id) DO UPDATE SET title = excluded.title, title_en = excluded.title_en,
                   title_ja = excluded.title_ja, synonyms = excluded.synonyms, episodes = excluded.episodes,
                   format = excluded.format, air_status = excluded.air_status, cover_url = excluded.cover_url,
                   fetched_at = excluded.fetched_at, start_date = excluded.start_date, broadcast = excluded.broadcast,
                   volumes = excluded.volumes, authors = excluded.authors, next_airing = excluded.next_airing, id_mal = excluded.id_mal",
            )?;
            let mut entry = tx.prepare(
                "INSERT OR REPLACE INTO entry (service, media_id, status, progress, score, rewatching, times_rewatched, updated_at, kind, volumes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                    now,
                    m.start_date,
                    m.broadcast.map(crate::airing::Broadcast::to_text),
                    k,
                    m.volumes,
                    serde_json::to_string(&m.authors).unwrap_or_else(|_| "[]".into()),
                    m.next_airing.map(NextAiring::to_text),
                    m.id_mal
                ])?;
                entry.execute(params![service, m.id, it.status.mal_name(), it.progress, it.score, it.rewatching, it.times_rewatched, it.updated_at, k, it.volumes])?;
            }
        }
        crate::queue::lay_over(&tx, Scope::new(service, kind))?;
        // Keep the media of a change that still waits (its title shows in the queue).
        tx.execute(
            "DELETE FROM media WHERE service = ?1 AND kind = ?2 AND id NOT IN (SELECT media_id FROM entry WHERE service = ?1 AND kind = ?2)
               AND id NOT IN (SELECT media_id FROM pending_op WHERE service = ?1 AND kind = ?2 AND state != 'done')",
            [service, k],
        )?;
        tx.execute("INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![synced_key(service, kind), now.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// The saved list of `scope` (a service name: its anime list), newest update first.
    pub fn load_list<'a>(&self, scope: impl Into<Scope<'a>>) -> Result<Vec<ListItem>> {
        let Scope { service, kind } = scope.into();
        let mut st = self.conn.prepare(&format!("{ITEM_SELECT} WHERE e.service = ?1 AND e.kind = ?2 ORDER BY e.updated_at DESC, m.id"))?;
        let rows = st.query_map([service, kind.name()], item_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// One saved entry.
    pub fn load_item<'a>(&self, scope: impl Into<Scope<'a>>, media_id: u32) -> Result<Option<ListItem>> {
        let Scope { service, kind } = scope.into();
        Ok(self
            .conn
            .query_row(&format!("{ITEM_SELECT} WHERE e.service = ?1 AND e.kind = ?2 AND e.media_id = ?3"), params![service, kind.name(), media_id], item_row)
            .optional()?)
    }

    /// Whether Manga mode was turned on (opened once). Off by default: then a sync never asks
    /// for the manga list, so people with no manga pay no extra requests. A setting of this
    /// install, not of the account: sign out keeps it.
    pub fn manga_enabled(&self) -> Result<bool> {
        Ok(self.kv_get(MANGA_ENABLED)?.as_deref() == Some("1"))
    }

    pub fn set_manga_enabled(&self, on: bool) -> Result<()> {
        self.kv_set(MANGA_ENABLED, if on { "1" } else { "0" })
    }


    /// Updates the saved media row of an anime (or a manga: `m.kind`) on the list (titles,
    /// counts, cover) from fresher data, e.g. its details page. Nothing happens for one not on
    /// the list.
    pub fn refresh_media(&self, service: &str, m: &Media, now: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE media SET title = ?3, title_en = ?4, title_ja = ?5, synonyms = ?6, episodes = ?7, format = ?8,
               air_status = ?9, cover_url = COALESCE(?10, cover_url), fetched_at = ?11,
               start_date = COALESCE(?12, start_date), broadcast = COALESCE(?13, broadcast),
               volumes = ?15, authors = CASE WHEN ?16 = '[]' THEN authors ELSE ?16 END,
               next_airing = COALESCE(?17, next_airing), id_mal = COALESCE(?18, id_mal)
             WHERE service = ?1 AND kind = ?14 AND id = ?2 AND id IN (SELECT media_id FROM entry WHERE service = ?1 AND kind = ?14)",
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
                now,
                m.start_date,
                m.broadcast.map(crate::airing::Broadcast::to_text),
                m.kind.name(),
                m.volumes,
                serde_json::to_string(&m.authors).unwrap_or_else(|_| "[]".into()),
                m.next_airing.map(NextAiring::to_text),
                m.id_mal
            ],
        )?;
        Ok(n > 0)
    }

    /// The saved media of a list with a MyAnimeList id: (the service's id, the MyAnimeList id).
    /// The account mirror's links ("Open on AniList" from a MyAnimeList entry).
    pub fn mal_id_pairs<'a>(&self, scope: impl Into<Scope<'a>>) -> Result<Vec<(u32, u32)>> {
        let Scope { service, kind } = scope.into();
        let mut st = self.conn.prepare("SELECT id, id_mal FROM media WHERE service = ?1 AND kind = ?2 AND id_mal IS NOT NULL AND id_mal > 0 ORDER BY id")?;
        let rows = st.query_map(params![service, kind.name()], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The anime on `service`'s list whose saved media is about only a title: no format, and no
    /// cover or no episode count.
    /// Now Playing's "Add as Watching" knows little more than the title, and a change still
    /// waiting to be sent isn't in the fetched list, so a sync doesn't fill it in either.
    pub fn bare_media(&self, service: &str) -> Result<Vec<u32>> {
        let mut st = self.conn.prepare(
            "SELECT e.media_id FROM entry e JOIN media m ON m.service = e.service AND m.kind = e.kind AND m.id = e.media_id
             WHERE e.service = ?1 AND e.kind = 'anime' AND COALESCE(m.format, '') = '' AND (m.cover_url IS NULL OR m.episodes IS NULL)
             ORDER BY e.media_id",
        )?;
        let ids = st.query_map([service], |r| r.get::<_, u32>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    // ---------------------------------------------------------------------------- details

    /// The saved information page of an anime (or a manga, in a manga scope), and when it was
    /// fetched.
    pub fn details<'a>(&self, scope: impl Into<Scope<'a>>, media_id: u32) -> Result<Option<(Details, i64)>> {
        let Scope { service, kind } = scope.into();
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT body, fetched_at FROM details WHERE service = ?1 AND kind = ?2 AND media_id = ?3",
                params![service, kind.name(), media_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(body, at)| serde_json::from_str(&body).ok().map(|d| (d, at))))
    }

    /// Saves an information page (its kind is `d.kind`), then forgets old ones of titles not on
    /// the list: older than a week, or past the newest [`DETAILS_KEEP`] (both kinds together).
    pub fn save_details(&self, service: &str, d: &Details, now: i64) -> Result<()> {
        let body = serde_json::to_string(d).map_err(|e| StoreError(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO details (service, kind, media_id, body, fetched_at) VALUES (?1, ?5, ?2, ?3, ?4)
             ON CONFLICT (service, kind, media_id) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
            params![service, d.id, body, now, d.kind.name()],
        )?;
        const OFF_LIST: &str =
            "service = ?1 AND NOT EXISTS (SELECT 1 FROM entry e WHERE e.service = details.service AND e.kind = details.kind AND e.media_id = details.media_id)";
        self.conn.execute(&format!("DELETE FROM details WHERE {OFF_LIST} AND fetched_at < ?2"), params![service, now - DETAILS_MAX_AGE])?;
        self.conn.execute(
            &format!(
                "DELETE FROM details WHERE {OFF_LIST} AND rowid NOT IN
                   (SELECT rowid FROM details WHERE {OFF_LIST} ORDER BY fetched_at DESC, media_id LIMIT ?2)"
            ),
            params![service, DETAILS_KEEP],
        )?;
        Ok(())
    }

    // ----------------------------------------------------------------------------- seasons

    /// A saved season list and when it was fetched.
    pub fn season(&self, service: &str, season: Season) -> Result<Option<(Vec<SeasonAnime>, i64)>> {
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT body, fetched_at FROM season WHERE service = ?1 AND year = ?2 AND season = ?3",
                params![service, season.year, season.name()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(body, at)| serde_json::from_str(&body).ok().map(|v| (v, at))))
    }

    /// Saves a season list, then forgets what the page has no use for (no hoarding): lists older
    /// than [`SEASON_MAX_AGE`], and all but the newest [`SEASONS_KEPT`] lists. `season` and the
    /// seasons in `keep` (the current and the previous one) are never dropped by the count.
    pub fn save_season(&self, service: &str, season: Season, list: &[SeasonAnime], now: i64, keep: &[Season]) -> Result<()> {
        let body = serde_json::to_string(list).map_err(|e| StoreError(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO season (service, year, season, body, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (service, year, season) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
            params![service, season.year, season.name(), body, now],
        )?;
        self.conn.execute("DELETE FROM season WHERE service = ?1 AND fetched_at < ?2", params![service, now - SEASON_MAX_AGE])?;
        let protected: Vec<String> = keep.iter().chain([&season]).map(|s| s.key()).collect();
        let mut rows = self.conn.prepare("SELECT year, season FROM season WHERE service = ?1 ORDER BY fetched_at DESC, year DESC")?;
        let all: Vec<(u16, String)> = rows.query_map([service], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        let mut room = SEASONS_KEPT.saturating_sub(protected.len());
        for (y, s) in all {
            if protected.contains(&format!("{y}/{s}")) {
                continue;
            }
            if room > 0 {
                room -= 1;
            } else {
                self.conn.execute("DELETE FROM season WHERE service = ?1 AND year = ?2 AND season = ?3", params![service, y, s])?;
            }
        }
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

    pub fn kv_del(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        Ok(())
    }

    /// The user's profile picture URL (`None`: none set or not asked yet) and when it was last
    /// asked for. Kept in `kv`, so sign out deletes it with the rest.
    pub fn picture(&self, service: &str) -> Result<(Option<String>, Option<i64>)> {
        let url = self.kv_get(&format!("{service}:picture"))?.filter(|u| !u.is_empty());
        let at = self.kv_get(&format!("{service}:picture_at"))?.and_then(|v| v.parse().ok());
        Ok((url, at))
    }

    pub fn set_picture(&self, service: &str, url: Option<&str>, now: i64) -> Result<()> {
        self.kv_set(&format!("{service}:picture"), url.unwrap_or(""))?;
        self.kv_set(&format!("{service}:picture_at"), &now.to_string())
    }

    /// The Profile page's facts from the last answer (`None`: never asked).
    pub fn profile(&self, service: &str) -> Result<Option<crate::service::Profile>> {
        Ok(self.kv_get(&format!("{service}:profile"))?.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub fn set_profile(&self, service: &str, p: &crate::service::Profile) -> Result<()> {
        self.kv_set(&format!("{service}:profile"), &serde_json::to_string(p).unwrap_or_default())
    }

    /// Unix seconds of the last full sync of `scope` (a service name: its anime list).
    pub fn synced_at<'a>(&self, scope: impl Into<Scope<'a>>) -> Result<Option<i64>> {
        let Scope { service, kind } = scope.into();
        Ok(self.kv_get(&synced_key(service, kind))?.and_then(|v| v.parse().ok()))
    }
}

/// The `kv` key of Manga mode (see [`Store::manga_enabled`]). Not under a service prefix, so
/// sign out keeps it.
const MANGA_ENABLED: &str = "manga:enabled";

/// `mal:synced_at` (anime, as before manga), `mal:manga:synced_at`.
fn synced_key(service: &str, kind: Kind) -> String {
    match kind {
        Kind::Anime => format!("{service}:synced_at"),
        Kind::Manga => format!("{service}:manga:synced_at"),
    }
}

const ITEM_SELECT: &str = "SELECT m.id, m.title, m.title_en, m.title_ja, m.synonyms, m.episodes, m.format, m.air_status, m.cover_url,
        e.status, e.progress, e.score, e.rewatching, e.updated_at, e.times_rewatched, m.start_date, m.broadcast,
        m.kind, m.volumes, m.authors, e.volumes, m.next_airing, m.id_mal
 FROM entry e JOIN media m ON m.service = e.service AND m.kind = e.kind AND m.id = e.media_id";

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
            start_date: r.get(15)?,
            broadcast: r.get::<_, Option<String>>(16)?.as_deref().and_then(Broadcast::from_text),
            kind: Kind::from_name(&r.get::<_, String>(17)?),
            volumes: r.get(18)?,
            authors: serde_json::from_str(&r.get::<_, String>(19)?).unwrap_or_default(),
            next_airing: r.get::<_, Option<String>>(21)?.as_deref().and_then(NextAiring::from_text),
            id_mal: r.get(22)?,
        },
        status: Status::from_mal(&status).unwrap_or(Status::PlanToWatch),
        progress: r.get(10)?,
        score: r.get(11)?,
        rewatching: r.get(12)?,
        times_rewatched: r.get(14)?,
        updated_at: r.get(13)?,
        volumes: r.get(20)?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::score::ten;
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
                start_date: None,
                broadcast: None,
                authors: Vec::new(),
                kind: Default::default(),
                volumes: None,
                next_airing: None,
                id_mal: None,
            },
            status,
            progress,
            score: ten(8).filter(|_| progress > 3),
            rewatching: false,
            times_rewatched: 0,
            updated_at,
            volumes: 0,
        }
    }

    /// A database from before rewatching (schema 4), with a list and a waiting change, opens
    /// with both kept: no rewatch, never rewatched.
    #[test]
    fn schema_4_upgrades_with_its_list_and_queue() {
        let dir = std::env::temp_dir().join(format!("kotora-db4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kotora.db");
        {
            let conn = Connection::open(&path).unwrap();
            for (i, m) in MIGRATIONS[..4].iter().enumerate() {
                conn.execute_batch(m).unwrap();
                conn.pragma_update(None, "user_version", (i + 1) as u32).unwrap();
            }
            conn.execute_batch(
                "INSERT INTO media (service, id, title, fetched_at) VALUES ('mal', 1, 'Show 1', 1);
                 INSERT INTO entry (service, media_id, status, progress, score, rewatching, updated_at) VALUES ('mal', 1, 'completed', 12, 8, 0, 5);
                 INSERT INTO pending_op (service, media_id, score, old_status, old_progress, old_score, base_updated_at, created_at, edited_at)
                   VALUES ('mal', 1, 9, 'completed', 12, 8, 5, 6, 6);",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        let it = s.load_item("mal", 1).unwrap().unwrap();
        assert_eq!((it.status, it.progress, it.rewatching, it.times_rewatched), (Status::Completed, 12, false, 0));
        let q = s.queue("mal", 10).unwrap();
        assert_eq!((q[0].change.score, q[0].change.rewatching, q[0].old.times_rewatched), (Some(ten(9)), None, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The 0.1.1 fixture (`tests/fixtures/kotora-0.1.1.db`, made-up rows) in a fresh scratch
    /// folder; the folder goes when the guard drops.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn with_0_1_1(name: &str) -> (Scratch, std::path::PathBuf) {
            let dir = std::env::temp_dir().join(format!("kotora-mig-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("kotora.db");
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kotora-0.1.1.db"), &path).unwrap();
            (Scratch(dir), path)
        }

        fn with_0_1_3(name: &str) -> (Scratch, std::path::PathBuf) {
            let dir = std::env::temp_dir().join(format!("kotora-mig-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("kotora.db");
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kotora-0.1.3.db"), &path).unwrap();
            (Scratch(dir), path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Every table's and index's SQL, by name (whitespace folded).
    fn schema_of(c: &Connection) -> Vec<(String, String)> {
        let mut st = c.prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name").unwrap();
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap().map(|r| r.unwrap()).map(|(n, s)| (n, s.split_whitespace().collect::<Vec<_>>().join(" "))).collect()
    }

    fn counts(c: &Connection) -> Vec<i64> {
        ["account", "media", "entry", "pending_op", "recognition", "details", "kv", "season", "library_file", "air_seen", "announced"]
            .iter()
            // The activity log's start (schema 13) is new in every upgrade.
            .map(|t| c.query_row(&format!("SELECT COUNT(*) FROM {t} WHERE {}", if *t == "kv" { "key != 'activity:since'" } else { "1" }), [], |r| r.get(0)).unwrap())
            .collect()
    }

    /// The fixture really is a 0.1.1 database: schema 9, and the same tables and indexes as the
    /// first nine migrations make (the ones 0.1.1 shipped; they never change).
    #[test]
    fn the_fixture_has_the_0_1_1_schema() {
        let (_g, path) = Scratch::with_0_1_1("schema");
        let fixture = Connection::open(&path).unwrap();
        assert_eq!(fixture.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap() as usize, SCHEMA_0_1);
        let fresh = Connection::open_in_memory().unwrap();
        for m in &MIGRATIONS[..SCHEMA_0_1] {
            fresh.execute_batch(m).unwrap();
        }
        assert_eq!(schema_of(&fixture), schema_of(&fresh));
        assert_eq!(counts(&fixture), [1, 4, 4, 4, 3, 2, 4, 1, 1, 1, 1]);
    }

    /// The manga migration on a 0.1.1 database: a copy of the file is kept first, every row
    /// stays and becomes `anime`, the queue (waiting, conflict, add, sent) keeps working, and an
    /// anime and a manga with the same MAL id live side by side afterwards. Opening again
    /// changes nothing.
    #[test]
    fn the_0_1_1_database_upgrades() {
        let (_g, path) = Scratch::with_0_1_1("upgrade");
        let before = counts(&Connection::open(&path).unwrap());
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        // The backup: the untouched 0.1.1 database.
        let bak = backup_path(&path, SCHEMA_0_1);
        assert_eq!(bak.file_name().unwrap(), "kotora.db.schema-9.bak");
        {
            let b = Connection::open(&bak).unwrap();
            assert_eq!(b.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap() as usize, SCHEMA_0_1);
            assert_eq!(counts(&b), before);
            let fixture = Connection::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kotora-0.1.1.db")).unwrap();
            assert_eq!(schema_of(&b), schema_of(&fixture), "the copy is the old schema");
        }
        // Every row kept, all anime.
        assert_eq!(counts(&s.conn), before);
        let fk: Option<String> = s.conn.query_row("PRAGMA foreign_key_check", [], |r| r.get(0)).optional().unwrap();
        assert_eq!(fk, None, "no broken foreign key");
        for t in ["media", "entry", "pending_op", "recognition", "details"] {
            let other: i64 = s.conn.query_row(&format!("SELECT COUNT(*) FROM {t} WHERE kind != 'anime'"), [], |r| r.get(0)).unwrap();
            assert_eq!(other, 0, "{t}");
        }
        let list = s.load_list("mal").unwrap();
        assert_eq!(list.len(), 4);
        assert!(list.iter().all(|i| i.media.kind == Kind::Anime && i.volumes == 0 && i.media.volumes.is_none()));
        let lanterns = s.load_item("mal", 1001).unwrap().unwrap();
        assert_eq!((lanterns.progress, lanterns.score, lanterns.media.start_date.as_deref()), (5, ten(8), Some("2026-10-03")));
        assert_eq!(lanterns.media.broadcast, Broadcast::from_text("saturday 23:30"));
        let harbor = s.load_item("mal", 1002).unwrap().unwrap();
        assert_eq!((harbor.rewatching, harbor.times_rewatched), (true, 2));
        assert_eq!(s.synced_at("mal").unwrap(), Some(1_790_000_000));
        assert_eq!(s.account("mal").unwrap().as_deref(), Some("demo_user"));
        assert_eq!(s.recognition("mal", "lanterns of kirigaoka#1").unwrap(), Some((1001, "user".into(), "Lanterns of Kirigaoka".into())));
        let (d, _) = s.details("mal", 1001).unwrap().unwrap();
        assert_eq!((d.kind, d.mine.unwrap().progress), (Kind::Anime, 5));
        assert!(s.details("mal", 3005).unwrap().is_some());
        assert!(!s.manga_enabled().unwrap(), "Manga mode starts off");
        // The queue: same changes, same states, the conflict's service values kept.
        let q = s.queue("mal", 1_790_000_700).unwrap();
        assert_eq!(q.iter().map(|o| (o.id, o.state, o.kind)).collect::<Vec<_>>(), [
            (1, crate::queue::OpState::Queued, Kind::Anime),
            (2, crate::queue::OpState::Conflict, Kind::Anime),
            (3, crate::queue::OpState::Queued, Kind::Anime),
            (4, crate::queue::OpState::Done, Kind::Anime)
        ]);
        assert_eq!(q[0].change.progress, Some(6));
        let r = q[1].remote.unwrap();
        assert_eq!((r.values.score, r.values.times_rewatched, r.values.volumes, r.updated_at), (ten(7), 2, 0, 1_790_000_650));
        assert!(q[2].added);
        assert_eq!(s.open_count("mal").unwrap(), 3);
        // A sync still lays the waiting changes over the fetched list (the add included).
        let fetched: Vec<ListItem> = list.iter().filter(|i| i.media.id != 1004).cloned().collect();
        s.replace_list("mal", &fetched, 1_790_001_000).unwrap();
        assert_eq!(s.load_item("mal", 1001).unwrap().unwrap().progress, 6, "the waiting edit wins until sent");
        assert!(s.load_item("mal", 1004).unwrap().is_some(), "the waiting add stays on the list");
        // Edits still queue and merge.
        s.enqueue("mal", 1001, crate::Change { progress: Some(7), ..Default::default() }, 1_790_001_100).unwrap();
        assert_eq!(s.queue("mal", 1_790_001_100).unwrap()[0].change.progress, Some(7));
        // MAL id 2001 is an anime here; a manga 2001 now fits beside it.
        let mut manga = tests::item(2001, "Paper Lanterns", Status::Watching, 21, 50);
        manga.media.kind = Kind::Manga;
        manga.volumes = 3;
        s.replace_list(("mal", Kind::Manga), &[manga], 1_790_002_000).unwrap();
        assert_eq!(s.load_item("mal", 2001).unwrap().unwrap().media.title, "Same Id As A Manga");
        assert_eq!(s.load_item(("mal", Kind::Manga), 2001).unwrap().unwrap().volumes, 3);
        assert_eq!(s.load_list("mal").unwrap().len(), 4, "the manga sync left the anime list alone");
        drop(s);
        // Opening again: nothing runs, the copy is not touched.
        let bak_before = std::fs::read(&bak).unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        assert_eq!(std::fs::read(&bak).unwrap(), bak_before);
        assert_eq!(s.load_list("mal").unwrap().len(), 4);
        assert_eq!(s.load_list(("mal", Kind::Manga)).unwrap().len(), 1);
    }

    /// The bug 0.2 hit live: the UI and the sign-in thread both open a schema-9 file at once.
    /// Both must open it; the upgrade runs once and one copy is kept.
    #[test]
    fn two_threads_open_an_old_database_at_once() {
        for round in 0..8 {
            let (_g, path) = Scratch::with_0_1_3(&format!("race-{round}"));
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = (0..2)
                .map(|_| {
                    let (b, p) = (barrier.clone(), path.clone());
                    std::thread::spawn(move || {
                        b.wait();
                        Store::open(&p).map(|s| s.schema_version().unwrap())
                    })
                })
                .collect();
            for t in threads {
                assert_eq!(t.join().unwrap().unwrap(), MIGRATIONS.len());
            }
            let dir = path.parent().unwrap();
            let baks: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.contains(".bak")).collect();
            assert_eq!(baks, ["kotora.db.schema-9.bak"]);
            let b = Connection::open(backup_path(&path, SCHEMA_0_1)).unwrap();
            assert_eq!(user_version(&b).unwrap(), SCHEMA_0_1);
        }
    }

    /// The same without the in-process lock (two processes, say): each step re-reads the version
    /// under the write lock, so a step already run is skipped instead of failing.
    #[test]
    fn two_connections_migrate_the_same_file_once() {
        for round in 0..8 {
            let (_g, path) = Scratch::with_0_1_3(&format!("race-conn-{round}"));
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = (0..2)
                .map(|_| {
                    let (b, p) = (barrier.clone(), path.clone());
                    std::thread::spawn(move || {
                        let c = Connection::open(&p).unwrap();
                        c.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
                        b.wait();
                        Store::from_connection(c).map(|s| s.schema_version().unwrap())
                    })
                })
                .collect();
            for t in threads {
                assert_eq!(t.join().unwrap().unwrap(), MIGRATIONS.len());
            }
        }
    }

    /// A migration that fails changes nothing: the version and every row stay, and the next
    /// start tries again (and keeps the first copy).
    #[test]
    fn a_failed_upgrade_changes_nothing_and_runs_again() {
        let (_g, path) = Scratch::with_0_1_1("failed");
        {
            // Something in the way of the rebuild (a table with the name it renames to).
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE media_v9 (x INTEGER);").unwrap();
        }
        let before = counts(&Connection::open(&path).unwrap());
        assert!(Store::open(&path).is_err());
        {
            let c = Connection::open(&path).unwrap();
            assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap() as usize, SCHEMA_0_1);
            assert_eq!(counts(&c), before);
            let kind: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('media') WHERE name = 'kind'", [], |r| r.get(0)).unwrap();
            assert_eq!(kind, 0, "the half-done rebuild was rolled back");
            c.execute_batch("DROP TABLE media_v9;").unwrap();
        }
        let bak = backup_path(&path, SCHEMA_0_1);
        assert!(bak.exists(), "the copy was made before the try");
        let first = std::fs::read(&bak).unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        assert_eq!(s.load_list("mal").unwrap().len(), 4);
        assert_eq!(std::fs::read(&bak).unwrap(), first, "the first copy is kept");
    }

    /// The activity log (0.4.5) comes without a schema step, empty, with the day it started:
    /// a 0.4.4 database stays schema 12 (0.4.4 can still open it) and nothing else changes.
    #[test]
    fn the_activity_log_comes_without_a_schema_step() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, m) in MIGRATIONS[..12].iter().enumerate() {
            conn.execute_batch(m).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as u32).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO media (service, id, title, fetched_at) VALUES ('mal', 1, 'A', 0);
             INSERT INTO entry (service, media_id, status, progress, score, updated_at) VALUES ('mal', 1, 'watching', 3, 70, 0);
             INSERT INTO kv (key, value) VALUES ('mal:synced_at', '5');",
        )
        .unwrap();
        let before = counts(&conn);
        let s = Store::from_connection(conn).unwrap();
        assert_eq!(s.schema_version().unwrap(), 12);
        assert_eq!(counts(&s.conn), before);
        assert!(s.activity("mal").unwrap().is_empty());
        let since = s.activity_since().unwrap();
        assert!(since.is_some_and(|t| t > 1_700_000_000));
        // Opening again keeps the start.
        let s = Store::from_connection(s.conn).unwrap();
        assert_eq!(s.activity_since().unwrap(), since);
    }

    /// Schema 11 (0.2.0) to 12: every saved score goes from 1-10 to the 100-point scale (the
    /// list, the queue's new, old and service values, and the saved details' entry); no score
    /// stays none and 0 stays "removes".
    #[test]
    fn schema_12_moves_scores_to_100_points() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, m) in MIGRATIONS[..11].iter().enumerate() {
            conn.execute_batch(m).unwrap();
            conn.pragma_update(None, "user_version", (i + 1) as u32).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO media (service, id, title, fetched_at) VALUES ('anilist', 1, 'A', 0), ('anilist', 2, 'B', 0);
             INSERT INTO entry (service, media_id, status, progress, score, updated_at) VALUES ('anilist', 1, 'watching', 3, 7, 0), ('anilist', 2, 'watching', 1, NULL, 0);
             INSERT INTO pending_op (service, media_id, score, old_score, base_updated_at, created_at, edited_at, old_status, old_progress, r_score)
               VALUES ('anilist', 1, 9, 7, 0, 0, 0, 'watching', 3, 8), ('anilist', 2, 0, 0, 0, 0, 0, 'watching', 1, NULL);
             INSERT INTO details (service, media_id, body, fetched_at) VALUES
               ('anilist', 1, '{\"id\":1,\"title\":\"A\",\"mine\":{\"status\":\"watching\",\"progress\":3,\"score\":7,\"rewatching\":false,\"start_date\":null,\"finish_date\":null}}', 0),
               ('anilist', 2, '{\"id\":2,\"title\":\"B\",\"mine\":null}', 0),
               ('anilist', 3, 'not json', 0);
             INSERT INTO kv (key, value) VALUES ('mirror:edits',
               '[{\"mal_id\":5,\"title\":\"A\",\"main_id\":5,\"main\":{\"status\":\"watching\",\"progress\":3,\"score\":7,\"rewatching\":false,\"times\":0},\"mirror_id\":105,\"mirror\":{\"status\":\"watching\",\"progress\":4,\"score\":null,\"rewatching\":false,\"times\":0},\"at\":9},
                 {\"mal_id\":6,\"title\":\"B\",\"main_id\":6,\"main\":{\"status\":\"completed\",\"progress\":12,\"score\":9,\"rewatching\":false,\"times\":1},\"mirror_id\":null,\"mirror\":null,\"at\":8}]');",
        )
        .unwrap();
        let s = Store::from_connection(conn).unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
        let e: Vec<Option<i64>> = s.conn.prepare("SELECT score FROM entry ORDER BY media_id").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(e, [Some(70), None]);
        let q = s.queue("anilist", 0).unwrap();
        assert_eq!((q[0].change.score, q[0].old.score), (Some(ten(9)), ten(7)));
        assert_eq!(q[1].change.score, Some(None), "0 still removes the score");
        let r: Option<i64> = s.conn.query_row("SELECT r_score FROM pending_op WHERE media_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(r, Some(80));
        assert_eq!(s.details("anilist", 1).unwrap().unwrap().0.mine.unwrap().score, ten(7));
        assert!(s.details("anilist", 2).unwrap().unwrap().0.mine.is_none());
        // The mirror's website edits: same order, scores x10, no score and no side kept.
        let edits = s.web_edits().unwrap();
        assert_eq!(edits.iter().map(|e| e.mal_id).collect::<Vec<_>>(), [5, 6]);
        assert_eq!((edits[0].main.as_ref().unwrap().score, edits[0].mirror.as_ref().unwrap().score), (ten(7), None));
        assert_eq!((edits[1].main.as_ref().unwrap().score, edits[1].mirror.is_none()), (ten(9), true));
    }

    /// A database written by Kotora 0.1.3 itself (`tests/fixtures/kotora-0.1.3.db`: made up by
    /// 0.1.3's own `Store` calls, WAL mode as installed, schema 9) upgrades to 0.2 (schema 11):
    /// the copy first, every row kept, the AniList columns empty on MyAnimeList rows, and an
    /// AniList list beside the MyAnimeList one afterwards.
    #[test]
    fn the_0_1_3_database_upgrades_to_0_2() {
        let dir = std::env::temp_dir().join(format!("kotora-mig-013-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        let path = dir.join("kotora.db");
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kotora-0.1.3.db");
        std::fs::copy(fixture, &path).unwrap();
        let before = counts(&Connection::open(&path).unwrap());
        assert_eq!(before, [1, 6, 6, 3, 2, 1, 5, 1, 2, 0, 1]);
        {
            // Its schema is the one migrations 1 to 9 make: 0.1.3 shipped those unchanged.
            let c = Connection::open(&path).unwrap();
            assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap() as usize, SCHEMA_0_1);
            let fresh = Connection::open_in_memory().unwrap();
            for m in &MIGRATIONS[..SCHEMA_0_1] {
                fresh.execute_batch(m).unwrap();
            }
            assert_eq!(schema_of(&c), schema_of(&fresh));
        }
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), 12);
        assert_eq!(MIGRATIONS.len(), 12, "0.2.1 is schema 12; a new migration needs its own upgrade test");
        let bak = backup_path(&path, SCHEMA_0_1);
        {
            let b = Connection::open(&bak).unwrap();
            assert_eq!(b.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap() as usize, SCHEMA_0_1);
            assert_eq!(counts(&b), before, "the copy is the whole 0.1.3 database");
            assert_eq!(b.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
        }
        assert_eq!(counts(&s.conn), before);
        let fk: Option<String> = s.conn.query_row("PRAGMA foreign_key_check", [], |r| r.get(0)).optional().unwrap();
        assert_eq!(fk, None);
        let empty: i64 = s.conn.query_row("SELECT COUNT(*) FROM media WHERE next_airing IS NULL AND id_mal IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(empty, 6, "MyAnimeList rows leave the AniList columns empty");
        // The list, the queue and the account as 0.1.3 left them.
        let list = s.load_list("mal").unwrap();
        assert_eq!(list.len(), 6);
        assert!(list.iter().all(|i| i.media.kind == Kind::Anime && i.media.next_airing.is_none() && i.media.id_mal.is_none()));
        let lanterns = s.load_item("mal", 5001).unwrap().unwrap();
        assert_eq!((lanterns.progress, lanterns.score), (6, ten(8)), "the waiting edit lies over the list");
        assert_eq!(lanterns.media.broadcast, Broadcast::from_text("saturday 23:30"));
        let harbor = s.load_item("mal", 5002).unwrap().unwrap();
        assert_eq!((harbor.rewatching, harbor.times_rewatched), (true, 2));
        assert_eq!(s.account("mal").unwrap().as_deref(), Some("demo_user"));
        assert_eq!(s.synced_at("mal").unwrap(), Some(1_790_000_000));
        assert!(s.profile("mal").unwrap().is_some());
        assert_eq!(s.recognition("mal", "lanterns of kirigaoka#1").unwrap(), Some((5001, "user".into(), "Lanterns of Kirigaoka".into())));
        let (d, _) = s.details("mal", 5001).unwrap().unwrap();
        assert!(d.mine.is_none_or(|m| m.score.is_none_or(|sc| sc.points() % 10 == 0 && sc.points() >= 10)), "saved scores are on the 100 scale");
        let q = s.queue("mal", 1_790_000_100).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!((q[0].change.progress, q[1].change.score, q[2].added), (Some(6), Some(ten(10)), true));
        assert_eq!(s.open_count("mal").unwrap(), 3);
        // AniList beside it: its own list and account; MyAnimeList's untouched.
        assert!(s.load_list("anilist").unwrap().is_empty() && s.account("anilist").unwrap().is_none());
        let mut al = tests::item(5001, "AniList Lanterns", Status::Watching, 2, 1_790_001_000);
        al.media.id_mal = Some(5001);
        al.media.next_airing = Some(NextAiring { episode: 3, at: 1_790_100_000 });
        s.replace_list("anilist", &[al], 1_790_001_000).unwrap();
        s.set_account("anilist", "demo_user").unwrap();
        let back = s.load_item("anilist", 5001).unwrap().unwrap();
        assert_eq!((back.media.title.as_str(), back.media.id_mal, back.media.next_airing.map(|n| n.episode)), ("AniList Lanterns", Some(5001), Some(3)));
        assert_eq!(s.load_list("mal").unwrap().len(), 6);
        assert_eq!(s.load_item("mal", 5001).unwrap().unwrap().media.title, "Lanterns of Kirigaoka");
        drop(s);
        // Opening again: nothing runs, the copy stays as it was.
        let bak_before = std::fs::read(&bak).unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), 12);
        assert_eq!(std::fs::read(&bak).unwrap(), bak_before);
        let baks: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.contains(".bak")).collect();
        assert_eq!(baks, ["kotora.db.schema-9.bak"]);
    }

    /// A database from a newer Kotora (what 0.1.x sees after 0.2 upgraded it) is refused with
    /// a plain error, and the file is not changed: no migration, no copy.
    #[test]
    fn a_database_from_a_newer_kotora_is_refused_untouched() {
        let dir = std::env::temp_dir().join(format!("kotora-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        let path = dir.join("kotora.db");
        drop(Store::open(&path).unwrap());
        let future = MIGRATIONS.len() + 1;
        Connection::open(&path).unwrap().pragma_update(None, "user_version", future as u32).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let err = Store::open(&path).err().expect("a newer schema is refused");
        assert!(err.0.starts_with(&format!("it was saved by a newer Kotora (database schema {future}, this one knows {})", MIGRATIONS.len())), "{}", err.0);
        assert!(err.0.contains("Update Kotora"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "not changed");
        assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains(".bak")));
    }

    /// A new database (and one already current) gets no copy.
    #[test]
    fn a_new_database_needs_no_copy() {
        let dir = std::env::temp_dir().join(format!("kotora-nobak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kotora.db");
        Store::open(&path).unwrap();
        Store::open(&path).unwrap();
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.contains(".bak")).collect();
        assert!(files.is_empty(), "{files:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn manga(id: u32, title: &str, chapters: u32, volumes: u32, updated_at: i64) -> ListItem {
        let mut it = item(id, title, Status::Watching, chapters, updated_at);
        it.media.kind = Kind::Manga;
        it.media.format = "manga".into();
        it.media.air_status = "currently_publishing".into();
        it.media.episodes = None;
        it.media.volumes = Some(14);
        it.media.authors = vec!["Aoi Hoshimiya".into()];
        it.volumes = volumes;
        it
    }

    #[test]
    fn anime_and_manga_lists_are_kept_apart() {
        let mut s = Store::open_memory().unwrap();
        s.replace_list("mal", &[item(7, "Anime Seven", Status::Watching, 3, 10)], 100).unwrap();
        s.replace_list(("mal", Kind::Manga), &[manga(7, "Manga Seven", 21, 3, 20), manga(8, "Manga Eight", 2, 0, 30)], 200).unwrap();
        assert_eq!(s.load_list("mal").unwrap().iter().map(|i| i.media.title.as_str()).collect::<Vec<_>>(), ["Anime Seven"]);
        let m = s.load_list(("mal", Kind::Manga)).unwrap();
        assert_eq!(m.iter().map(|i| i.media.id).collect::<Vec<_>>(), [8, 7], "newest first");
        assert_eq!(m[1], manga(7, "Manga Seven", 21, 3, 20), "every manga field round-trips");
        assert_eq!((s.synced_at("mal").unwrap(), s.synced_at(("mal", Kind::Manga)).unwrap()), (Some(100), Some(200)));
        // A manga sync drops only manga gone from the list, and their media.
        s.replace_list(("mal", Kind::Manga), &[manga(8, "Manga Eight", 3, 0, 40)], 300).unwrap();
        assert!(s.load_item(("mal", Kind::Manga), 7).unwrap().is_none());
        assert!(s.load_item("mal", 7).unwrap().is_some(), "the anime with the same id stays");
        let media: i64 = s.conn.query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0)).unwrap();
        assert_eq!(media, 2);
        // Fresher data for a manga touches the manga row only.
        let mut fresh = manga(8, "Manga Eight (new)", 3, 0, 40).media;
        fresh.volumes = Some(15);
        assert!(s.refresh_media("mal", &fresh, 400).unwrap());
        assert_eq!(s.load_item(("mal", Kind::Manga), 8).unwrap().unwrap().media.volumes, Some(15));
        let mut anime8 = fresh.clone();
        anime8.kind = Kind::Anime;
        assert!(!s.refresh_media("mal", &anime8, 400).unwrap(), "no anime 8 on the list");
        // Details of the same id, one per kind.
        let mut dm = details(7);
        dm.kind = Kind::Manga;
        dm.title = "Manga Seven".into();
        s.save_details("mal", &details(7), 500).unwrap();
        s.save_details("mal", &dm, 500).unwrap();
        assert_eq!(s.details("mal", 7).unwrap().unwrap().0.title, "Show 7");
        assert_eq!(s.details(("mal", Kind::Manga), 7).unwrap().unwrap().0.title, "Manga Seven");
        // Sign out forgets both lists; Manga mode is a setting of this install and stays.
        s.set_manga_enabled(true).unwrap();
        s.clear_service("mal").unwrap();
        assert!(s.load_list(("mal", Kind::Manga)).unwrap().is_empty() && s.load_list("mal").unwrap().is_empty());
        assert_eq!(s.synced_at(("mal", Kind::Manga)).unwrap(), None);
        assert!(s.manga_enabled().unwrap());
        s.set_manga_enabled(false).unwrap();
        assert!(!s.manga_enabled().unwrap());
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
    fn seasons_are_cached_and_the_newest_kept() {
        let mut st = Store::open_memory().unwrap();
        let fall = Season { year: 2026, quarter: 3 };
        let a = SeasonAnime { id: 9201, title: "Kirigaoka no Lantern".into(), broadcast: Some(Broadcast { day: 5, minute: Some(1410) }), members: Some(5), ..Default::default() };
        assert_eq!(st.season("mal", fall).unwrap(), None);
        // Browsing back one season at a time: each is kept until the count is over.
        let day = 86_400;
        for (i, back) in (2..=8).enumerate() {
            st.save_season("mal", fall.shift(-back), &[], 10 * day + i as i64, &[fall, fall.previous()]).unwrap();
        }
        let kept: Vec<i32> = (0..=8).filter(|&b| st.season("mal", fall.shift(-b)).unwrap().is_some()).collect();
        assert_eq!(kept, [5, 6, 7, 8], "the 4 newest of the browsed ones (6 less the current and previous)");
        // The current and the previous season stay whatever was browsed since.
        st.save_season("mal", fall.previous(), &[], 11 * day, &[fall]).unwrap();
        st.save_season("mal", fall, std::slice::from_ref(&a), 11 * day + 1, &[fall.previous()]).unwrap();
        assert_eq!(st.season("mal", fall).unwrap(), Some((vec![a], 11 * day + 1)));
        assert!(st.season("mal", fall.previous()).unwrap().is_some());
        let n = |st: &Store| (0..=8).filter(|&b| st.season("mal", fall.shift(-b)).unwrap().is_some()).count();
        assert_eq!(n(&st), SEASONS_KEPT, "at most {SEASONS_KEPT} lists");
        // One more browsed season pushes out the one fetched longest ago (back 5), not a newer one.
        st.save_season("mal", fall.shift(-2), &[], 12 * day, &[fall, fall.previous()]).unwrap();
        assert_eq!(n(&st), SEASONS_KEPT);
        assert!(st.season("mal", fall.shift(-5)).unwrap().is_none() && st.season("mal", fall.shift(-2)).unwrap().is_some());
        assert!(st.season("mal", fall.shift(-8)).unwrap().is_some() && st.season("mal", fall).unwrap().is_some());
        // A list nobody asked for in a month is forgotten at the next save.
        st.save_season("mal", fall, &[], 11 * day + 2 + SEASON_MAX_AGE, &[fall.previous()]).unwrap();
        assert!(st.season("mal", fall.shift(-8)).unwrap().is_none() && st.season("mal", fall).unwrap().is_some());
        st.clear_service("mal").unwrap();
        assert_eq!(st.season("mal", fall).unwrap(), None, "sign out forgets the seasons");
    }

    #[test]
    fn the_list_keeps_the_start_date_and_the_slot() {
        let mut st = Store::open_memory().unwrap();
        let mut it = item(2, "Kirigaoka no Lantern", Status::Watching, 6, 100);
        it.media.start_date = Some("2026-10-03".into());
        it.media.broadcast = Some(Broadcast { day: 5, minute: Some(1410) });
        st.replace_list("mal", std::slice::from_ref(&it), 1000).unwrap();
        assert_eq!(st.load_item("mal", 2).unwrap().unwrap().media, it.media);
        // A details refresh (no slot known) keeps the saved one.
        let mut m = it.media.clone();
        m.broadcast = None;
        st.refresh_media("mal", &m, 2000).unwrap();
        assert_eq!(st.load_item("mal", 2).unwrap().unwrap().media.broadcast, it.media.broadcast);
    }

    #[test]
    fn the_list_keeps_the_exact_next_airing_per_service() {
        let mut st = Store::open_memory().unwrap();
        let mut it = item(2, "Kirigaoka no Lantern", Status::Watching, 6, 100);
        it.media.next_airing = Some(NextAiring { episode: 8, at: 1_767_196_800 });
        it.media.id_mal = Some(59001);
        st.replace_list("anilist", std::slice::from_ref(&it), 1000).unwrap();
        assert_eq!(st.load_item("anilist", 2).unwrap().unwrap().media.next_airing, it.media.next_airing);
        assert_eq!(st.load_item("mal", 2).unwrap(), None, "AniList rows are not MyAnimeList's");
        // A refresh without one keeps it; a newer one replaces it.
        let mut m = it.media.clone();
        m.next_airing = None;
        st.refresh_media("anilist", &m, 2000).unwrap();
        assert_eq!(st.load_item("anilist", 2).unwrap().unwrap().media.next_airing, it.media.next_airing);
        m.next_airing = Some(NextAiring { episode: 9, at: 1_767_801_600 });
        st.refresh_media("anilist", &m, 3000).unwrap();
        assert_eq!(st.load_item("anilist", 2).unwrap().unwrap().media.next_airing.map(|n| n.episode), Some(9));
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
        assert_eq!(s.picture("mal").unwrap(), (None, None));
        s.set_picture("mal", Some("https://cdn.example.invalid/u.jpg"), 7).unwrap();
        assert_eq!(s.picture("mal").unwrap(), (Some("https://cdn.example.invalid/u.jpg".into()), Some(7)));
        s.set_picture("mal", None, 8).unwrap();
        assert_eq!(s.picture("mal").unwrap(), (None, Some(8)), "none set: asked, nothing to show");
        s.set_picture("mal", Some("https://cdn.example.invalid/u.jpg"), 9).unwrap();
        assert_eq!(s.profile("mal").unwrap(), None);
        let p = crate::service::Profile { joined: Some("2019-03-02".into()), stats: Some(crate::service::AnimeStats { completed: 3, days: 1.5, ..Default::default() }), ..Default::default() };
        s.set_profile("mal", &p).unwrap();
        assert_eq!(s.profile("mal").unwrap(), Some(p), "the profile keeps through a restart");
        s.clear_service("mal").unwrap();
        assert_eq!(s.profile("mal").unwrap(), None, "sign out deletes the profile");
        assert_eq!(s.picture("mal").unwrap(), (None, None), "sign out deletes the picture");
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
