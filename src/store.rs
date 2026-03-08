use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::models::{ClipEntry, ContentType};

/// SQLite-backed persistent store for clipboard history.
///
/// Uses a single `clips` table.  All writes are synchronous (WAL mode for
/// reader/writer concurrency).  The daemon holds one connection; the CLI
/// opens its own short-lived connection.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening SQLite database at {}", path.display()))?;

        // Enable WAL for concurrent daemon writes + CLI reads.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Apply schema migrations (idempotent).
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS clips (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                content      TEXT    NOT NULL,
                content_type TEXT    NOT NULL DEFAULT 'plain_text',
                hash         TEXT    NOT NULL UNIQUE,
                source_app   TEXT,
                created_at   TEXT    NOT NULL,
                pinned       INTEGER NOT NULL DEFAULT 0,
                tags         TEXT    NOT NULL DEFAULT '[]',
                label        TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_clips_created_at ON clips(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_clips_hash       ON clips(hash);
            CREATE INDEX IF NOT EXISTS idx_clips_pinned     ON clips(pinned);

            -- Full-text search virtual table (searches content + label + tags)
            CREATE VIRTUAL TABLE IF NOT EXISTS clips_fts USING fts5(
                content,
                label,
                tags,
                content='clips',
                content_rowid='id'
            );

            -- Keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS clips_ai AFTER INSERT ON clips BEGIN
                INSERT INTO clips_fts(rowid, content, label, tags)
                VALUES (new.id, new.content, COALESCE(new.label,''), new.tags);
            END;
            CREATE TRIGGER IF NOT EXISTS clips_ad AFTER DELETE ON clips BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, content, label, tags)
                VALUES ('delete', old.id, old.content, COALESCE(old.label,''), old.tags);
            END;
            CREATE TRIGGER IF NOT EXISTS clips_au AFTER UPDATE ON clips BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, content, label, tags)
                VALUES ('delete', old.id, old.content, COALESCE(old.label,''), old.tags);
                INSERT INTO clips_fts(rowid, content, label, tags)
                VALUES (new.id, new.content, COALESCE(new.label,''), new.tags);
            END;
            "#,
        )?;
        Ok(())
    }

    /// Insert a new clip.  Returns the new rowid.
    /// If the hash already exists, updates `created_at` to bump it to the top
    /// and returns the existing id (deduplication with recency update).
    pub fn insert(&self, entry: &ClipEntry) -> Result<i64> {
        // Check for duplicate hash
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM clips WHERE hash = ?1",
                params![entry.hash],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Bump timestamp so it appears at top of history
            self.conn.execute(
                "UPDATE clips SET created_at = ?1 WHERE id = ?2",
                params![entry.created_at.to_rfc3339(), id],
            )?;
            return Ok(id);
        }

        let tags_json = serde_json::to_string(&entry.tags)?;
        self.conn.execute(
            r#"INSERT INTO clips (content, content_type, hash, source_app, created_at, pinned, tags, label)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            params![
                entry.content,
                entry.content_type.as_str(),
                entry.hash,
                entry.source_app,
                entry.created_at.to_rfc3339(),
                entry.pinned as i64,
                tags_json,
                entry.label,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Retrieve recent clips (newest first).
    pub fn list(&self, limit: usize, search: Option<&str>) -> Result<Vec<ClipEntry>> {
        if let Some(term) = search {
            self.search(term, limit)
        } else {
            let mut stmt = self.conn.prepare(
                r#"SELECT id, content, content_type, hash, source_app, created_at, pinned, tags, label
                   FROM clips
                   ORDER BY pinned DESC, created_at DESC
                   LIMIT ?1"#,
            )?;
            let entries = stmt.query_map(params![limit as i64], row_to_entry)?;
            entries.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    /// Full-text search across content, label, and tags.
    fn search(&self, term: &str, limit: usize) -> Result<Vec<ClipEntry>> {
        let fts_query = format!("\"{}\"", term.replace('"', "\"\""));
        let mut stmt = self.conn.prepare(
            r#"SELECT c.id, c.content, c.content_type, c.hash, c.source_app,
                      c.created_at, c.pinned, c.tags, c.label
               FROM clips c
               JOIN clips_fts f ON f.rowid = c.id
               WHERE clips_fts MATCH ?1
               ORDER BY c.pinned DESC, c.created_at DESC
               LIMIT ?2"#,
        )?;
        let entries = stmt.query_map(params![fts_query, limit as i64], row_to_entry)?;
        entries.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Fetch a single clip by id.
    pub fn get(&self, id: i64) -> Result<Option<ClipEntry>> {
        let result = self.conn.query_row(
            r#"SELECT id, content, content_type, hash, source_app, created_at, pinned, tags, label
               FROM clips WHERE id = ?1"#,
            params![id],
            row_to_entry,
        );
        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Toggle pin status for a clip.
    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<bool> {
        let rows = self
            .conn
            .execute("UPDATE clips SET pinned = ?1 WHERE id = ?2", params![pinned as i64, id])?;
        Ok(rows > 0)
    }

    /// Add a tag to a clip.
    pub fn add_tag(&self, id: i64, tag: &str) -> Result<()> {
        let entry = self.get(id)?.context("clip not found")?;
        let mut tags = entry.tags;
        if !tags.contains(&tag.to_string()) {
            tags.push(tag.to_string());
        }
        let tags_json = serde_json::to_string(&tags)?;
        self.conn
            .execute("UPDATE clips SET tags = ?1 WHERE id = ?2", params![tags_json, id])?;
        Ok(())
    }

    /// Set a human-readable label on a clip.
    pub fn set_label(&self, id: i64, label: Option<&str>) -> Result<()> {
        self.conn
            .execute("UPDATE clips SET label = ?1 WHERE id = ?2", params![label, id])?;
        Ok(())
    }

    /// Delete a clip by id.
    pub fn delete(&self, id: i64) -> Result<bool> {
        let rows = self
            .conn
            .execute("DELETE FROM clips WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    /// Evict oldest non-pinned clips beyond `max_history`.
    pub fn evict_old(&self, max_history: usize) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE pinned = 0",
            [],
            |row| row.get(0),
        )?;

        let to_delete = count - max_history as i64;
        if to_delete <= 0 {
            return Ok(0);
        }

        let rows = self.conn.execute(
            r#"DELETE FROM clips WHERE id IN (
                SELECT id FROM clips WHERE pinned = 0
                ORDER BY created_at ASC
                LIMIT ?1
            )"#,
            params![to_delete],
        )?;

        Ok(rows)
    }

    /// Return total clip count.
    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))?)
    }

    /// Export all clips (optionally filtered).
    pub fn export_all(&self) -> Result<Vec<ClipEntry>> {
        self.list(usize::MAX, None)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipEntry> {
    use chrono::DateTime;

    let id: i64 = row.get(0)?;
    let content: String = row.get(1)?;
    let content_type_str: String = row.get(2)?;
    let hash: String = row.get(3)?;
    let source_app: Option<String> = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let pinned: i64 = row.get(6)?;
    let tags_json: String = row.get(7)?;
    let label: Option<String> = row.get(8)?;

    let content_type = ContentType::from_str(&content_type_str);
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

    Ok(ClipEntry {
        id,
        content,
        content_type,
        hash,
        source_app,
        created_at,
        pinned: pinned != 0,
        tags,
        label,
    })
}
