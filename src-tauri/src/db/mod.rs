//! SQLite index of the local library. Rebuildable from sidecars at any time,
//! so the sidecars stay the source of truth.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::providers::TagCategory;
use crate::storage::Sidecar;

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE posts (
    id             INTEGER PRIMARY KEY,
    provider       TEXT NOT NULL,
    remote_id      TEXT NOT NULL,
    md5            TEXT NOT NULL,
    rating         TEXT,
    width          INTEGER,
    height         INTEGER,
    file_ext       TEXT NOT NULL,
    file_size      INTEGER NOT NULL,
    source         TEXT,
    post_url       TEXT NOT NULL,
    file_url       TEXT,
    created_at_raw TEXT,
    downloaded_at  TEXT NOT NULL,
    local_path     TEXT NOT NULL,
    raw_json       TEXT NOT NULL,
    UNIQUE (provider, remote_id)
);
CREATE INDEX posts_md5 ON posts (md5);
CREATE INDEX posts_downloaded ON posts (downloaded_at);
CREATE TABLE tags (
    id       INTEGER PRIMARY KEY,
    provider TEXT NOT NULL,
    name     TEXT NOT NULL,
    category TEXT NOT NULL,
    UNIQUE (provider, name)
);
CREATE TABLE post_tags (
    post_id INTEGER NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags (id),
    PRIMARY KEY (post_id, tag_id)
);
CREATE INDEX post_tags_by_tag ON post_tags (tag_id, post_id);
";

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Settings(format!("library database: {e}"))
}

pub fn category_str(c: TagCategory) -> &'static str {
    match c {
        TagCategory::General => "general",
        TagCategory::Artist => "artist",
        TagCategory::Copyright => "copyright",
        TagCategory::Character => "character",
        TagCategory::Meta => "meta",
        TagCategory::Deprecated => "deprecated",
        TagCategory::Unknown => "unknown",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalEntry {
    pub provider: String,
    pub remote_id: String,
    pub md5: String,
    /// Relative to the library root.
    pub local_path: String,
    pub downloaded_at: String,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path).map_err(db_err)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")
            .map_err(db_err)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(db_err)?;
        if version == 0 {
            conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"))
                .map_err(db_err)?;
        } else if version > SCHEMA_VERSION {
            return Err(AppError::Settings(
                "library database was created by a newer VIBDL version".into(),
            ));
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn row_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<LocalEntry> {
        Ok(LocalEntry {
            provider: r.get(0)?,
            remote_id: r.get(1)?,
            md5: r.get(2)?,
            local_path: r.get(3)?,
            downloaded_at: r.get(4)?,
        })
    }

    /// Already downloaded? Matches the same post, or the same file (md5)
    /// from any board.
    pub fn find(&self, provider: &str, remote_id: &str, md5: Option<&str>) -> AppResult<Option<LocalEntry>> {
        let conn = self.conn.lock().expect("db lock poisoned");
        let sql = "SELECT provider, remote_id, md5, local_path, downloaded_at FROM posts
                   WHERE (provider = ?1 AND remote_id = ?2) OR md5 = ?3 LIMIT 1";
        conn.query_row(sql, params![provider, remote_id, md5.unwrap_or("")], Self::row_entry)
            .optional()
            .map_err(db_err)
    }

    /// Insert or replace one post and its tags, in a single transaction.
    pub fn upsert(&self, s: &Sidecar, local_path: &str) -> AppResult<()> {
        let mut conn = self.conn.lock().expect("db lock poisoned");
        let tx = conn.transaction().map_err(db_err)?;
        let raw = serde_json::to_string(&s.raw).unwrap_or_else(|_| "{}".into());
        tx.execute(
            "INSERT INTO posts (provider, remote_id, md5, rating, width, height, file_ext, file_size,
                                source, post_url, file_url, created_at_raw, downloaded_at, local_path, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (provider, remote_id) DO UPDATE SET
                md5 = excluded.md5, rating = excluded.rating, width = excluded.width,
                height = excluded.height, file_ext = excluded.file_ext, file_size = excluded.file_size,
                source = excluded.source, post_url = excluded.post_url, file_url = excluded.file_url,
                created_at_raw = excluded.created_at_raw, downloaded_at = excluded.downloaded_at,
                local_path = excluded.local_path, raw_json = excluded.raw_json",
            params![
                s.provider, s.remote_id, s.md5, s.rating, s.width, s.height, s.file_ext,
                s.file_size as i64, s.source, s.post_url, s.file_url, s.created_at,
                s.downloaded_at, local_path, raw
            ],
        )
        .map_err(db_err)?;
        let post_id: i64 = tx
            .query_row(
                "SELECT id FROM posts WHERE provider = ?1 AND remote_id = ?2",
                params![s.provider, s.remote_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        tx.execute("DELETE FROM post_tags WHERE post_id = ?1", params![post_id]).map_err(db_err)?;
        for (name, cat) in s.tags.iter() {
            // Never downgrade a known category to "unknown".
            tx.execute(
                "INSERT INTO tags (provider, name, category) VALUES (?1, ?2, ?3)
                 ON CONFLICT (provider, name) DO UPDATE SET category = excluded.category
                 WHERE excluded.category != 'unknown'",
                params![s.provider, name, category_str(cat)],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT OR IGNORE INTO post_tags (post_id, tag_id)
                 SELECT ?1, id FROM tags WHERE provider = ?2 AND name = ?3",
                params![post_id, s.provider, name],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    /// Run read-only queries (used by `library`).
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> AppResult<T> {
        let conn = self.conn.lock().expect("db lock poisoned");
        f(&conn).map_err(db_err)
    }

    pub fn post_count(&self) -> AppResult<u64> {
        let conn = self.conn.lock().expect("db lock poisoned");
        conn.query_row("SELECT COUNT(*) FROM posts", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
            .map_err(db_err)
    }

    #[cfg(test)]
    pub fn tags_of(&self, provider: &str, remote_id: &str) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare(
                "SELECT t.name, t.category FROM tags t JOIN post_tags pt ON pt.tag_id = t.id
                 JOIN posts p ON p.id = pt.post_id WHERE p.provider = ?1 AND p.remote_id = ?2 ORDER BY t.name",
            )
            .unwrap();
        st.query_map(params![provider, remote_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::metadata::CategorizedTags;

    pub fn sidecar(provider: &str, id: &str, md5: &str) -> Sidecar {
        Sidecar {
            vibdl_sidecar: 1,
            provider: provider.into(),
            remote_id: id.into(),
            post_url: format!("https://example/{id}"),
            md5: md5.into(),
            rating: Some("general".into()),
            width: Some(100),
            height: Some(50),
            file_ext: "png".into(),
            file_size: 1234,
            source: None,
            file_url: Some("https://cdn/x.png".into()),
            created_at: None,
            downloaded_at: "2026-11-14T00:00:00Z".into(),
            tags: CategorizedTags {
                artist: vec!["example_artist".into()],
                general: vec!["long_hair".into()],
                unknown: vec!["mystery".into()],
                ..Default::default()
            },
            raw: serde_json::json!({"id": id}),
        }
    }

    const MD5: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn upsert_find_and_tags() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sidecar("danbooru", "1", MD5), "danbooru/01/x.png").unwrap();
        assert_eq!(db.post_count().unwrap(), 1);

        // Same post, and same file from another board, are both duplicates.
        assert!(db.find("danbooru", "1", None).unwrap().is_some());
        let other = db.find("gelbooru", "999", Some(MD5)).unwrap().unwrap();
        assert_eq!(other.provider, "danbooru");
        assert!(db.find("danbooru", "2", Some("ffffffffffffffffffffffffffffffff")).unwrap().is_none());

        assert_eq!(
            db.tags_of("danbooru", "1"),
            vec![
                ("example_artist".into(), "artist".into()),
                ("long_hair".into(), "general".into()),
                ("mystery".into(), "unknown".into())
            ]
        );
    }

    #[test]
    fn reupsert_replaces_and_never_downgrades_categories() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sidecar("danbooru", "1", MD5), "a").unwrap();
        let mut s = sidecar("danbooru", "1", MD5);
        s.tags = CategorizedTags { unknown: vec!["example_artist".into()], ..Default::default() };
        db.upsert(&s, "a").unwrap();
        assert_eq!(db.post_count().unwrap(), 1);
        assert_eq!(db.tags_of("danbooru", "1"), vec![("example_artist".into(), "artist".into())]);
    }

    #[test]
    fn file_db_reopens_with_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".vibdl").join("library.sqlite");
        Db::open(&path).unwrap().upsert(&sidecar("danbooru", "1", MD5), "a").unwrap();
        assert_eq!(Db::open(&path).unwrap().post_count().unwrap(), 1);
    }
}
