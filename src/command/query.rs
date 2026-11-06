use crate::cli::QueryFormatArg;
use crate::command::open_db;
use crate::query::render::{
    ALL_FIELDS, DEFAULT_TABLE_FIELDS, FIELD_PATH, CsvRenderer, FilesRenderer, JsonRenderer,
    MarkdownRenderer, QueryRenderer, QueryRow, TableRenderer,
};
use crate::query::filters::MetadataFilters;
use anyhow::{Context, Result, bail};
use rusqlite::{ToSql, params_from_iter};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) struct QueryArgs {
    pub(crate) doc_type: Option<String>,
    pub(crate) tags: Option<String>,
    pub(crate) topics: Option<String>,
    pub(crate) technologies: Option<String>,
    pub(crate) languages: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) format: QueryFormatArg,
    pub(crate) fields: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) fail_empty: bool,
    pub(crate) json: bool,
}

pub(crate) fn run(root: &Path, args: &QueryArgs) -> Result<()> {
    let format = if args.json {
        QueryFormatArg::Json
    } else {
        args.format
    };

    let selected_fields = parse_fields(args.fields.as_deref(), format)?;
    if (format == QueryFormatArg::Files || format == QueryFormatArg::Markdown)
        && args.fields.is_some()
    {
        bail!("--fields is only valid with --format csv, --format table, or --format json");
    }

    let (sql, binds) = build_query(args)?;
    let conn = open_db(root)?;
    let docs = fetch_documents(&conn, &sql, &binds, root)?;

    if args.fail_empty && docs.is_empty() {
        bail!("no documents matched filters");
    }

    render_results(&docs, format, &selected_fields)
}

fn build_query(args: &QueryArgs) -> Result<(String, Vec<String>)> {
    let filters = MetadataFilters::parse(
        args.doc_type.as_deref(),
        args.title.as_deref(),
        args.tags.as_deref(),
        args.topics.as_deref(),
        args.technologies.as_deref(),
        args.languages.as_deref(),
    )?;
    let (where_parts, binds) = filters.where_clauses();

    let mut sql = String::from(
        "SELECT i.id, i.path, i.title, i.doc_type, i.indexed_at_utc FROM documents i",
    );
    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }
    sql.push_str(" ORDER BY i.path");
    if let Some(limit) = args.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    Ok((sql, binds))
}

fn fetch_documents(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[String],
    root: &Path,
) -> Result<Vec<QueryRow>> {
    let bind_refs: Vec<&dyn ToSql> = binds.iter().map(|v| v as &dyn ToSql).collect();
    let mut stmt = conn.prepare(sql).with_context(|| "failed to prepare query")?;
    let rows = stmt
        .query_map(params_from_iter(bind_refs), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .with_context(|| "failed to execute query")?;

    let mut docs = Vec::new();
    for row in rows {
        let (id, path, title, doc_type, indexed_at_utc) = row?;
        docs.push(QueryRow {
            path: to_root_relative(root, &path),
            title,
            doc_type,
            indexed_at_utc,
            tags: load_vocab(conn, id, "document_tags", "tag_id", "tags")?,
            topics: load_vocab(conn, id, "document_topics", "topic_id", "topics")?,
            technologies: load_vocab(
                conn,
                id,
                "document_technologies",
                "technology_id",
                "technologies",
            )?,
            languages: load_vocab(conn, id, "document_languages", "language_id", "languages")?,
        });
    }
    Ok(docs)
}

fn render_results(docs: &[QueryRow], format: QueryFormatArg, fields: &[String]) -> Result<()> {
    let renderer: Box<dyn QueryRenderer> = match format {
        QueryFormatArg::Files => Box::new(FilesRenderer),
        QueryFormatArg::Csv => Box::new(CsvRenderer),
        QueryFormatArg::Table => Box::new(TableRenderer),
        QueryFormatArg::Json => Box::new(JsonRenderer),
        QueryFormatArg::Markdown => Box::new(MarkdownRenderer),
    };
    renderer.render(docs, fields)
}

fn parse_fields(fields: Option<&str>, format: QueryFormatArg) -> Result<Vec<String>> {
    let raw = match (fields, format) {
        (None, QueryFormatArg::Csv) => {
            return Ok(DEFAULT_TABLE_FIELDS
                .iter()
                .map(|f| (*f).to_string())
                .collect())
        }
        (None, QueryFormatArg::Table) => {
            return Ok(DEFAULT_TABLE_FIELDS
                .iter()
                .map(|f| (*f).to_string())
                .collect())
        }
        (None, QueryFormatArg::Json) => {
            return Ok(ALL_FIELDS.iter().map(|f| (*f).to_string()).collect())
        }
        (None, QueryFormatArg::Files) => return Ok(vec![FIELD_PATH.to_string()]),
        (None, QueryFormatArg::Markdown) => return Ok(vec![FIELD_PATH.to_string()]),
        (Some(v), _) => v,
    };

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for part in raw.split(',') {
        let field = part.trim();
        if field.is_empty() {
            continue;
        }
        if !ALL_FIELDS.contains(&field) {
            bail!(
                "unknown field '{}'; allowed: {}",
                field,
                ALL_FIELDS.join(",")
            );
        }
        if seen.insert(field.to_string()) {
            out.push(field.to_string());
        }
    }
    if out.is_empty() {
        bail!("--fields did not contain any valid entries");
    }
    Ok(out)
}

fn load_vocab(
    conn: &rusqlite::Connection,
    doc_id: i64,
    link_table: &str,
    link_vocab_fk: &str,
    vocab_table: &str,
) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT v.name FROM {vocab_table} v JOIN {link_table} l ON l.{link_vocab_fk} = v.id WHERE l.document_id = ?1 ORDER BY v.name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([doc_id], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn to_root_relative(root: &Path, path: &str) -> String {
    let p = PathBuf::from(path);
    match p.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{QueryArgs, build_query, parse_fields};
    use crate::cli::QueryFormatArg;

    fn default_args() -> QueryArgs {
        QueryArgs {
            doc_type: None,
            tags: None,
            topics: None,
            technologies: None,
            languages: None,
            title: None,
            format: QueryFormatArg::Table,
            fields: None,
            limit: None,
            fail_empty: false,
            json: false,
        }
    }

    #[test]
    fn table_defaults_to_core_fields() {
        let fields = parse_fields(None, QueryFormatArg::Table).expect("fields should parse");
        assert_eq!(fields, vec!["path", "title", "type"]);
    }

    #[test]
    fn csv_defaults_to_core_fields() {
        let fields = parse_fields(None, QueryFormatArg::Csv).expect("fields should parse");
        assert_eq!(fields, vec!["path", "title", "type"]);
    }

    #[test]
    fn markdown_defaults_to_path() {
        let fields = parse_fields(None, QueryFormatArg::Markdown).expect("fields should parse");
        assert_eq!(fields, vec!["path"]);
    }

    #[test]
    fn json_defaults_to_all_fields() {
        let fields = parse_fields(None, QueryFormatArg::Json).expect("fields should parse");
        assert!(fields.contains(&"path".to_string()));
        assert!(fields.contains(&"tags".to_string()));
    }

    #[test]
    fn build_query_no_filters() {
        let args = default_args();
        let (sql, binds) = build_query(&args).unwrap();
        assert!(sql.contains("FROM documents i"));
        assert!(!sql.contains("WHERE"));
        assert!(binds.is_empty());
    }

    #[test]
    fn build_query_with_doc_type() {
        let mut args = default_args();
        args.doc_type = Some("Note".to_string());
        let (sql, binds) = build_query(&args).unwrap();
        assert!(sql.contains("WHERE i.doc_type = ?"));
        assert_eq!(binds, vec!["Note".to_string()]);
    }

    #[test]
    fn build_query_with_title() {
        let mut args = default_args();
        args.title = Some("rust".to_string());
        let (sql, binds) = build_query(&args).unwrap();
        assert!(sql.contains("LIKE LOWER(?)"));
        assert_eq!(binds, vec!["%rust%".to_string()]);
    }

    #[test]
    fn build_query_with_limit() {
        let mut args = default_args();
        args.limit = Some(10);
        let (sql, _) = build_query(&args).unwrap();
        assert!(sql.contains("LIMIT 10"));
    }

    #[test]
    fn build_query_with_multiple_filters() {
        let mut args = default_args();
        args.doc_type = Some("Note".to_string());
        args.tags = Some("rust".to_string());
        args.limit = Some(5);
        let (sql, binds) = build_query(&args).unwrap();
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("AND"));
        assert!(sql.contains("LIMIT 5"));
        assert!(binds.iter().any(|b| b == "Note"));
    }

    #[test]
    fn build_query_empty_doc_type_ignored() {
        let mut args = default_args();
        args.doc_type = Some("  ".to_string());
        let (sql, binds) = build_query(&args).unwrap();
        assert!(!sql.contains("WHERE"));
        assert!(binds.is_empty());
    }
}
