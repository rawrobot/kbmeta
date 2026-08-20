use crate::document::Document;
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Rebuild-time options controlling how metadata is projected into the index.
#[derive(Debug, Clone, Copy)]
pub struct RebuildOptions {
    /// When true, tags are normalized before writing to index tables.
    pub normalize_tags: bool,
}

/// Summary of a scoped incremental reindex operation.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopedRebuildSummary {
    pub scanned: usize,
    pub inserted: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged_skipped: usize,
}

impl Default for RebuildOptions {
    fn default() -> Self {
        Self {
            normalize_tags: false,
        }
    }
}

fn indexed_at_utc() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ts.to_string()
}

/// Rebuilds the entire derived SQLite index from discovered documents.
///
/// This operation is destructive for index tables and is intended to be idempotent.
pub fn rebuild_index(conn: &mut Connection, docs: &[Document], opts: RebuildOptions) -> Result<()> {
    let tx = conn
        .transaction()
        .with_context(|| "failed to start rebuild transaction")?;

    tx.execute("DELETE FROM document_tags", [])?;
    tx.execute("DELETE FROM document_topics", [])?;
    tx.execute("DELETE FROM document_technologies", [])?;
    tx.execute("DELETE FROM document_languages", [])?;
    tx.execute("DELETE FROM tags", [])?;
    tx.execute("DELETE FROM topics", [])?;
    tx.execute("DELETE FROM technologies", [])?;
    tx.execute("DELETE FROM languages", [])?;
    tx.execute("DELETE FROM documents", [])?;

    for doc in docs {
        let fm = doc.metadata.as_ref();
        tx.execute(
            "INSERT INTO documents(path, title, description, doc_type, content_hash, indexed_at_utc)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                doc.path.to_string_lossy().to_string(),
                fm.and_then(|x| x.title.as_deref()),
                fm.and_then(|x| x.description.as_deref()),
                fm.and_then(|x| x.doc_type.as_deref()),
                doc.content_hash,
                indexed_at_utc(),
            ],
        )
        .with_context(|| format!("failed to insert document {}", doc.path.display()))?;

        let doc_id: i64 = tx.query_row(
            "SELECT id FROM documents WHERE path = ?1",
            [doc.path.to_string_lossy().to_string()],
            |row| row.get(0),
        )?;

        if let Some(fm) = fm {
            let tags = if opts.normalize_tags {
                normalize_values(&fm.tags)
            } else {
                fm.tags.clone()
            };

            insert_many(&tx, doc_id, &tags, "tags", "document_tags", "tag_id")?;
            insert_many(&tx, doc_id, &fm.topics, "topics", "document_topics", "topic_id")?;
            insert_many(
                &tx,
                doc_id,
                &fm.technologies,
                "technologies",
                "document_technologies",
                "technology_id",
            )?;
            insert_many(
                &tx,
                doc_id,
                &fm.languages,
                "languages",
                "document_languages",
                "language_id",
            )?;
        }
    }

    tx.commit().with_context(|| "failed to commit rebuild")?;
    Ok(())
}

/// Rebuilds only selected folder scopes and uses stored `content_hash` to skip unchanged documents.
pub fn rebuild_index_scoped(
    conn: &mut Connection,
    docs: &[Document],
    scope_roots: &[&Path],
    opts: RebuildOptions,
) -> Result<ScopedRebuildSummary> {
    let tx = conn
        .transaction()
        .with_context(|| "failed to start scoped rebuild transaction")?;

    let mut summary = ScopedRebuildSummary {
        scanned: docs.len(),
        ..ScopedRebuildSummary::default()
    };

    let existing_hashes = load_existing_hashes(&tx)?;
    let scoped_existing = load_scoped_existing_doc_ids(&tx, scope_roots)?;
    let mut seen_paths = HashSet::new();

    for doc in docs {
        let path_str = doc.path.to_string_lossy().to_string();
        seen_paths.insert(path_str.clone());

        if existing_hashes
            .get(&path_str)
            .map(|h| h == &doc.content_hash)
            .unwrap_or(false)
        {
            summary.unchanged_skipped += 1;
            continue;
        }

        let existed = existing_hashes.contains_key(&path_str);
        if let Some((doc_id, _)) = scoped_existing.get(&path_str) {
            delete_document_relations(&tx, *doc_id)?;
        }

        let doc_id = insert_document(&tx, doc, opts)?;
        if existed {
            summary.updated += 1;
        } else {
            summary.inserted += 1;
        }

        if let Some(fm) = doc.metadata.as_ref() {
            let tags = if opts.normalize_tags {
                normalize_values(&fm.tags)
            } else {
                fm.tags.clone()
            };
            insert_many(&tx, doc_id, &tags, "tags", "document_tags", "tag_id")?;
            insert_many(&tx, doc_id, &fm.topics, "topics", "document_topics", "topic_id")?;
            insert_many(
                &tx,
                doc_id,
                &fm.technologies,
                "technologies",
                "document_technologies",
                "technology_id",
            )?;
            insert_many(
                &tx,
                doc_id,
                &fm.languages,
                "languages",
                "document_languages",
                "language_id",
            )?;
        }
    }

    for (path, (doc_id, _hash)) in scoped_existing {
        if !seen_paths.contains(&path) {
            delete_document_relations(&tx, doc_id)?;
            summary.removed += 1;
        }
    }

    cleanup_orphan_vocab(&tx)?;

    tx.commit()
        .with_context(|| "failed to commit scoped rebuild")?;
    Ok(summary)
}

fn insert_document(
    tx: &rusqlite::Transaction<'_>,
    doc: &Document,
    _opts: RebuildOptions,
) -> Result<i64> {
    let fm = doc.metadata.as_ref();
    tx.execute(
        "INSERT INTO documents(path, title, description, doc_type, content_hash, indexed_at_utc)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            doc.path.to_string_lossy().to_string(),
            fm.and_then(|x| x.title.as_deref()),
            fm.and_then(|x| x.description.as_deref()),
            fm.and_then(|x| x.doc_type.as_deref()),
            doc.content_hash,
            indexed_at_utc(),
        ],
    )
    .with_context(|| format!("failed to insert document {}", doc.path.display()))?;

    let id = tx.last_insert_rowid();
    Ok(id)
}

fn load_existing_hashes(tx: &rusqlite::Transaction<'_>) -> Result<HashMap<String, String>> {
    let mut stmt = tx.prepare("SELECT path, content_hash FROM documents")?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let hash: String = row.get(1)?;
        Ok((path, hash))
    })?;

    let mut out = HashMap::new();
    for row in rows {
        let (path, hash) = row?;
        out.insert(path, hash);
    }
    Ok(out)
}

fn load_scoped_existing_doc_ids(
    tx: &rusqlite::Transaction<'_>,
    scope_roots: &[&Path],
) -> Result<HashMap<String, (i64, String)>> {
    let mut out = HashMap::new();
    for scope in scope_roots {
        let scope_prefix = scope.to_string_lossy().to_string();
        let like_nested = format!("{}/%", scope_prefix.trim_end_matches('/'));
        let mut stmt = tx.prepare(
            "SELECT id, path, content_hash FROM documents WHERE path = ?1 OR path LIKE ?2",
        )?;
        let rows = stmt.query_map(params![scope_prefix, like_nested], |row| {
            let id: i64 = row.get(0)?;
            let path: String = row.get(1)?;
            let hash: String = row.get(2)?;
            Ok((id, path, hash))
        })?;
        for row in rows {
            let (id, path, hash) = row?;
            out.insert(path, (id, hash));
        }
    }
    Ok(out)
}

fn delete_document_relations(tx: &rusqlite::Transaction<'_>, doc_id: i64) -> Result<()> {
    tx.execute("DELETE FROM document_tags WHERE document_id = ?1", [doc_id])?;
    tx.execute("DELETE FROM document_topics WHERE document_id = ?1", [doc_id])?;
    tx.execute(
        "DELETE FROM document_technologies WHERE document_id = ?1",
        [doc_id],
    )?;
    tx.execute("DELETE FROM document_languages WHERE document_id = ?1", [doc_id])?;
    tx.execute("DELETE FROM documents WHERE id = ?1", [doc_id])?;
    Ok(())
}

fn cleanup_orphan_vocab(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT DISTINCT tag_id FROM document_tags)",
        [],
    )?;
    tx.execute(
        "DELETE FROM topics WHERE id NOT IN (SELECT DISTINCT topic_id FROM document_topics)",
        [],
    )?;
    tx.execute(
        "DELETE FROM technologies WHERE id NOT IN (SELECT DISTINCT technology_id FROM document_technologies)",
        [],
    )?;
    tx.execute(
        "DELETE FROM languages WHERE id NOT IN (SELECT DISTINCT language_id FROM document_languages)",
        [],
    )?;
    Ok(())
}

fn insert_many(
    tx: &rusqlite::Transaction<'_>,
    document_id: i64,
    values: &[String],
    vocab_table: &str,
    link_table: &str,
    link_col: &str,
) -> Result<()> {
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        tx.execute(
            &format!("INSERT OR IGNORE INTO {vocab_table}(name) VALUES (?1)"),
            [trimmed],
        )?;

        let vocab_id: i64 = tx.query_row(
            &format!("SELECT id FROM {vocab_table} WHERE name = ?1"),
            [trimmed],
            |row| row.get(0),
        )?;

        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO {link_table}(document_id, {link_col}) VALUES (?1, ?2)"
            ),
            params![document_id, vocab_id],
        )?;
    }
    Ok(())
}

fn normalize_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_ascii_lowercase().replace([' ', '_'], "-"))
        .filter(|v| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_values;

    #[test]
    fn normalizes_tag_values() {
        let input = vec![
            " Rust ".to_string(),
            "embedded systems".to_string(),
            "GPU_ACCEL".to_string(),
            "".to_string(),
        ];

        let got = normalize_values(&input);
        assert_eq!(got, vec!["rust", "embedded-systems", "gpu-accel"]);
    }
}
