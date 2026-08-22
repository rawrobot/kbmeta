use anyhow::Result;
use std::io;

/// Output field key for document path.
pub const FIELD_PATH: &str = "path";
/// Output field key for document title.
pub const FIELD_TITLE: &str = "title";
/// Output field key for document type.
pub const FIELD_TYPE: &str = "type";
/// Output field key for tags.
pub const FIELD_TAGS: &str = "tags";
/// Output field key for topics.
pub const FIELD_TOPICS: &str = "topics";
/// Output field key for technologies.
pub const FIELD_TECHNOLOGIES: &str = "technologies";
/// Output field key for languages.
pub const FIELD_LANGUAGES: &str = "languages";
/// Output field key for indexing timestamp.
pub const FIELD_INDEXED_AT_UTC: &str = "indexed_at_utc";

/// Default field set for human-readable tabular outputs.
pub const DEFAULT_TABLE_FIELDS: &[&str] = &[FIELD_PATH, FIELD_TITLE, FIELD_TYPE];
/// Full allowed field set for `--fields` validation and JSON defaults.
pub const ALL_FIELDS: &[&str] = &[
    FIELD_PATH,
    FIELD_TITLE,
    FIELD_TYPE,
    FIELD_TAGS,
    FIELD_TOPICS,
    FIELD_TECHNOLOGIES,
    FIELD_LANGUAGES,
    FIELD_INDEXED_AT_UTC,
];

/// Normalized query row used by all renderers.
#[derive(Debug, Clone)]
pub struct QueryRow {
    /// Root-relative document path.
    pub path: String,
    /// Optional metadata title.
    pub title: Option<String>,
    /// Optional metadata type.
    pub doc_type: Option<String>,
    /// UTC timestamp when the row was last indexed.
    pub indexed_at_utc: String,
    /// Associated tags.
    pub tags: Vec<String>,
    /// Associated topics.
    pub topics: Vec<String>,
    /// Associated technologies.
    pub technologies: Vec<String>,
    /// Associated languages.
    pub languages: Vec<String>,
}

/// Strategy trait for rendering query rows to stdout.
pub trait QueryRenderer {
    /// Renders rows using the selected ordered field set.
    fn render(&self, rows: &[QueryRow], fields: &[String]) -> Result<()>;
}

/// Renderer for `files` format (path only).
#[derive(Debug, Default, Clone, Copy)]
pub struct FilesRenderer;

impl QueryRenderer for FilesRenderer {
    fn render(&self, rows: &[QueryRow], _fields: &[String]) -> Result<()> {
        for row in rows {
            println!("{}", row.path);
        }
        Ok(())
    }
}

/// Renderer for CSV output.
#[derive(Debug, Default, Clone, Copy)]
pub struct CsvRenderer;

impl QueryRenderer for CsvRenderer {
    fn render(&self, rows: &[QueryRow], fields: &[String]) -> Result<()> {
        let stdout = io::stdout();
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(stdout.lock());

        writer.write_record(fields)?;
        for row in rows {
            let cols = fields
                .iter()
                .map(|f| value_for_field(row, f))
                .collect::<Vec<_>>();
            writer.write_record(cols)?;
        }
        writer.flush()?;
        Ok(())
    }
}

/// Renderer for tab-separated table output.
#[derive(Debug, Default, Clone, Copy)]
pub struct TableRenderer;

impl QueryRenderer for TableRenderer {
    fn render(&self, rows: &[QueryRow], fields: &[String]) -> Result<()> {
        println!("{}", fields.join("\t"));
        for row in rows {
            let cols = fields
                .iter()
                .map(|f| value_for_field(row, f))
                .collect::<Vec<_>>();
            println!("{}", cols.join("\t"));
        }
        Ok(())
    }
}

/// Renderer for markdown links output (one link per line).
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownRenderer;

impl QueryRenderer for MarkdownRenderer {
    fn render(&self, rows: &[QueryRow], _fields: &[String]) -> Result<()> {
        for row in rows {
            let title = row.title.as_deref().unwrap_or("Untitled");
            println!("- [{}](./{})", title, row.path);
        }
        Ok(())
    }
}

/// Renderer for structured JSON output.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonRenderer;

impl QueryRenderer for JsonRenderer {
    fn render(&self, rows: &[QueryRow], fields: &[String]) -> Result<()> {
        let mut out = Vec::new();
        for row in rows {
            let mut obj = serde_json::Map::new();
            for field in fields {
                obj.insert(field.clone(), json_value_for_field(row, field));
            }
            out.push(serde_json::Value::Object(obj));
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        Ok(())
    }
}

fn value_for_field(row: &QueryRow, field: &str) -> String {
    match field {
        FIELD_PATH => row.path.clone(),
        FIELD_TITLE => row.title.clone().unwrap_or_default(),
        FIELD_TYPE => row.doc_type.clone().unwrap_or_default(),
        FIELD_TAGS => row.tags.join(","),
        FIELD_TOPICS => row.topics.join(","),
        FIELD_TECHNOLOGIES => row.technologies.join(","),
        FIELD_LANGUAGES => row.languages.join(","),
        FIELD_INDEXED_AT_UTC => row.indexed_at_utc.clone(),
        _ => String::new(),
    }
}

fn json_value_for_field(row: &QueryRow, field: &str) -> serde_json::Value {
    match field {
        FIELD_PATH => serde_json::Value::String(row.path.clone()),
        FIELD_TITLE => row
            .title
            .clone()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
        FIELD_TYPE => row
            .doc_type
            .clone()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
        FIELD_TAGS => serde_json::Value::Array(
            row.tags
                .iter()
                .map(|v| serde_json::Value::String(v.clone()))
                .collect(),
        ),
        FIELD_TOPICS => serde_json::Value::Array(
            row.topics
                .iter()
                .map(|v| serde_json::Value::String(v.clone()))
                .collect(),
        ),
        FIELD_TECHNOLOGIES => serde_json::Value::Array(
            row.technologies
                .iter()
                .map(|v| serde_json::Value::String(v.clone()))
                .collect(),
        ),
        FIELD_LANGUAGES => serde_json::Value::Array(
            row.languages
                .iter()
                .map(|v| serde_json::Value::String(v.clone()))
                .collect(),
        ),
        FIELD_INDEXED_AT_UTC => serde_json::Value::String(row.indexed_at_utc.clone()),
        _ => serde_json::Value::Null,
    }
}
