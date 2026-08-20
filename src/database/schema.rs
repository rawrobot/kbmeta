use anyhow::{Context, Result};
use rusqlite::Connection;

/// Ensures the SQLite schema required by Phase 1 exists.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS documents (
          id INTEGER PRIMARY KEY,
          path TEXT UNIQUE NOT NULL,
          title TEXT,
          description TEXT,
          doc_type TEXT,
          content_hash TEXT NOT NULL,
          indexed_at_utc TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tags (
          id INTEGER PRIMARY KEY,
          name TEXT UNIQUE NOT NULL
        );

        CREATE TABLE IF NOT EXISTS topics (
          id INTEGER PRIMARY KEY,
          name TEXT UNIQUE NOT NULL
        );

        CREATE TABLE IF NOT EXISTS technologies (
          id INTEGER PRIMARY KEY,
          name TEXT UNIQUE NOT NULL
        );

        CREATE TABLE IF NOT EXISTS languages (
          id INTEGER PRIMARY KEY,
          name TEXT UNIQUE NOT NULL
        );

        CREATE TABLE IF NOT EXISTS document_tags (
          document_id INTEGER NOT NULL,
          tag_id INTEGER NOT NULL,
          PRIMARY KEY (document_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS document_topics (
          document_id INTEGER NOT NULL,
          topic_id INTEGER NOT NULL,
          PRIMARY KEY (document_id, topic_id)
        );

        CREATE TABLE IF NOT EXISTS document_technologies (
          document_id INTEGER NOT NULL,
          technology_id INTEGER NOT NULL,
          PRIMARY KEY (document_id, technology_id)
        );

        CREATE TABLE IF NOT EXISTS document_languages (
          document_id INTEGER NOT NULL,
          language_id INTEGER NOT NULL,
          PRIMARY KEY (document_id, language_id)
        );
        ",
    )
    .with_context(|| "failed to initialize SQLite schema")?;

    Ok(())
}
