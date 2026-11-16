-- A made-up Kotora 0.1.1 database (schema 9), the input of the manga migration test
-- (store.rs `the_0_1_1_database_upgrades`). The schema below is Kotora 0.1.1's
-- migrations 1-9 verbatim (from the v0.1.1 tag); the rows are invented. Rebuild with:
--   rm -f kotora-0.1.1.db && sqlite3 kotora-0.1.1.db < kotora-0.1.1.sql
PRAGMA foreign_keys = ON;
BEGIN;
-- 0.1.1 migration 1
CREATE TABLE account (
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
     );

-- 0.1.1 migration 2
CREATE TABLE pending_op (
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
     CREATE INDEX pending_op_state ON pending_op (service, state);

-- 0.1.1 migration 3
CREATE TABLE recognition (
        service TEXT NOT NULL,
        key TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        source TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        used_at INTEGER NOT NULL,
        PRIMARY KEY (service, key)
     );

-- 0.1.1 migration 4
ALTER TABLE pending_op ADD COLUMN added INTEGER NOT NULL DEFAULT 0;
     CREATE TABLE details (
        service TEXT NOT NULL,
        media_id INTEGER NOT NULL,
        body TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, media_id)
     );

-- 0.1.1 migration 5
ALTER TABLE entry ADD COLUMN times_rewatched INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN rewatching INTEGER;
     ALTER TABLE pending_op ADD COLUMN times_rewatched INTEGER;
     ALTER TABLE pending_op ADD COLUMN old_rewatching INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN old_times_rewatched INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE pending_op ADD COLUMN r_rewatching INTEGER;
     ALTER TABLE pending_op ADD COLUMN r_times_rewatched INTEGER;

-- 0.1.1 migration 6
ALTER TABLE media ADD COLUMN start_date TEXT;
     ALTER TABLE media ADD COLUMN broadcast TEXT;
     CREATE TABLE season (
        service TEXT NOT NULL,
        year INTEGER NOT NULL,
        season TEXT NOT NULL,
        body TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (service, year, season)
     );

-- 0.1.1 migration 7
CREATE TABLE library_file (
        path TEXT PRIMARY KEY,
        folder TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime INTEGER NOT NULL,
        title TEXT NOT NULL,
        episode INTEGER,
        season INTEGER,
        grp TEXT
     );
     CREATE INDEX library_file_folder ON library_file (folder);

-- 0.1.1 migration 8
CREATE TABLE air_seen (
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
     );

-- 0.1.1 migration 9
DELETE FROM season;

-- Rows (made up).
INSERT INTO account (service, user_name) VALUES ('mal', 'demo_user');
INSERT INTO media (service, id, title, title_en, title_ja, synonyms, episodes, format, air_status, cover_url, fetched_at, start_date, broadcast) VALUES
  ('mal', 1001, 'Lanterns of Kirigaoka', 'Lanterns of Kirigaoka (English)', NULL, '["Kirigaoka no Tomoshibi"]', 12, 'tv', 'currently_airing', 'https://cdn.example.invalid/1001m.jpg', 1790000000, '2026-10-03', 'saturday 23:30'),
  ('mal', 1002, 'Harbor Lights After School', NULL, NULL, '[]', 24, 'tv', 'finished_airing', NULL, 1790000000, '2025-04-05', NULL),
  ('mal', 2001, 'Same Id As A Manga', NULL, NULL, '[]', 1, 'movie', 'finished_airing', NULL, 1790000000, NULL, NULL),
  ('mal', 1004, 'Queued Add, Not Yet On MAL', NULL, NULL, '[]', 13, 'tv', 'not_yet_aired', NULL, 1790000000, NULL, NULL);
INSERT INTO entry (service, media_id, status, progress, score, rewatching, updated_at, times_rewatched) VALUES
  ('mal', 1001, 'watching', 5, 8, 0, 1790000100, 0),
  ('mal', 1002, 'completed', 24, 9, 1, 1790000200, 2),
  ('mal', 2001, 'plan_to_watch', 0, NULL, 0, 1790000300, 0),
  ('mal', 1004, 'plan_to_watch', 0, NULL, 0, 1790000400, 0);
-- A waiting edit (progress 5 -> 6), a conflict (score), an add not yet sent, a sent one.
INSERT INTO pending_op (id, service, media_id, status, progress, score, old_status, old_progress, old_score, base_updated_at, created_at, edited_at, edits, state, attempts, next_try, error, sent_at, r_status, r_progress, r_score, r_updated_at, added, rewatching, times_rewatched, old_rewatching, old_times_rewatched, r_rewatching, r_times_rewatched) VALUES
  (1, 'mal', 1001, NULL, 6, NULL, 'watching', 5, 8, 1790000100, 1790000500, 1790000500, 1, 'queued', 0, 0, NULL, NULL, NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, 0, NULL, NULL),
  (2, 'mal', 1002, NULL, NULL, 10, 'completed', 24, 9, 1790000200, 1790000600, 1790000600, 1, 'conflict', 0, 0, NULL, NULL, 'completed', 24, 7, 1790000650, 0, NULL, NULL, 1, 2, 1, 2),
  (3, 'mal', 1004, 'plan_to_watch', NULL, NULL, 'plan_to_watch', 0, 0, 0, 1790000700, 1790000700, 1, 'queued', 0, 0, NULL, NULL, NULL, NULL, NULL, NULL, 1, NULL, NULL, 0, 0, NULL, NULL),
  (4, 'mal', 2001, 'plan_to_watch', NULL, NULL, 'watching', 0, 0, 1790000250, 1790000260, 1790000260, 1, 'done', 1, 0, NULL, 1790000300, NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, 0, NULL, NULL);
INSERT INTO recognition (service, key, media_id, source, title, used_at) VALUES
  ('mal', 'lanterns of kirigaoka#1', 1001, 'user', 'Lanterns of Kirigaoka', 1790000800),
  ('mal', 'some extras', 0, 'ignore', '', 1790000800),
  ('mal', 'harbor lights#1', 1002, 'search', 'Harbor Lights After School', 1790000800);
INSERT INTO details (service, media_id, body, fetched_at) VALUES
  ('mal', 1001, '{"id":1001,"title":"Lanterns of Kirigaoka","episodes":12,"synopsis":"made up","mine":{"status":"watching","progress":5,"score":8,"rewatching":false,"times_rewatched":0,"start_date":null,"finish_date":null}}', 1790000900),
  ('mal', 3005, '{"id":3005,"title":"Looked At Once","synopsis":"made up"}', 1790000900);
INSERT INTO kv (key, value) VALUES
  ('mal:synced_at', '1790000000'),
  ('mal:picture', 'https://cdn.example.invalid/u.jpg'),
  ('mal:picture_at', '1790000000'),
  ('mal:profile', '{"joined":"2019-03-02","stats":{"completed":3}}');
INSERT INTO season (service, year, season, body, fetched_at) VALUES ('mal', 2026, 'fall', '[]', 1790000000);
INSERT INTO library_file (path, folder, size, mtime, title, episode, season, grp) VALUES
  ('/media/anime/Lanterns of Kirigaoka - 05.mkv', '/media/anime', 734003200, 1790000000, 'Lanterns of Kirigaoka', 5, NULL, 'Group');
INSERT INTO air_seen (service, media_id, air_status) VALUES ('mal', 1001, 'currently_airing');
INSERT INTO announced (service, media_id, what, at) VALUES ('mal', 1001, 'ep5', 1790000000);
PRAGMA user_version = 9;
COMMIT;
