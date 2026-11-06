use crate::query::signed_filter::{
    ParsedSignedFilter, RelationFilterSqlCompiler, RelationSqlConfig, SignedFilterParser,
    allow_tag_chars,
};
use anyhow::Result;

const TAGS: RelationSqlConfig<'static> = relation("document_tags", "tag_id", "tags");
const TOPICS: RelationSqlConfig<'static> = relation("document_topics", "topic_id", "topics");
const TECHNOLOGIES: RelationSqlConfig<'static> =
    relation("document_technologies", "technology_id", "technologies");
const LANGUAGES: RelationSqlConfig<'static> =
    relation("document_languages", "language_id", "languages");

const fn relation(
    link_table: &'static str,
    link_vocab_fk: &'static str,
    vocab_table: &'static str,
) -> RelationSqlConfig<'static> {
    RelationSqlConfig {
        item_alias: DOCUMENT_ALIAS,
        link_table,
        link_item_fk: "document_id",
        link_vocab_fk,
        vocab_table,
        vocab_id_col: "id",
        vocab_value_col: "name",
    }
}

/// Alias the `documents` table must have in SQL using [`MetadataFilters::where_clauses`].
pub const DOCUMENT_ALIAS: &str = "i";

/// Explicit metadata constraints shared by `query` and `search`.
#[derive(Debug, Clone, Default)]
pub struct MetadataFilters {
    pub tags: Option<ParsedSignedFilter>,
    pub topics: Option<ParsedSignedFilter>,
    pub technologies: Option<ParsedSignedFilter>,
    pub languages: Option<ParsedSignedFilter>,
    /// Exact document type match.
    pub doc_type: Option<String>,
    /// Case-insensitive title substring match.
    pub title: Option<String>,
}

impl MetadataFilters {
    /// Parses raw CLI values; blank values are treated as absent.
    pub fn parse(
        doc_type: Option<&str>,
        title: Option<&str>,
        tags: Option<&str>,
        topics: Option<&str>,
        technologies: Option<&str>,
        languages: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            tags: parse_expression(tags)?,
            topics: parse_expression(topics)?,
            technologies: parse_expression(technologies)?,
            languages: parse_expression(languages)?,
            doc_type: non_blank(doc_type),
            title: non_blank(title),
        })
    }

    /// Compiles filters into `AND`-joinable `WHERE` fragments and ordered binds.
    ///
    /// Fragments reference the documents table as [`DOCUMENT_ALIAS`].
    pub fn where_clauses(&self) -> (Vec<String>, Vec<String>) {
        let mut where_parts = Vec::new();
        let mut binds = Vec::new();

        if let Some(doc_type) = &self.doc_type {
            where_parts.push(format!("{DOCUMENT_ALIAS}.doc_type = ?"));
            binds.push(doc_type.clone());
        }

        if let Some(title) = &self.title {
            where_parts.push(format!(
                "LOWER(COALESCE({DOCUMENT_ALIAS}.title, '')) LIKE LOWER(?)"
            ));
            binds.push(format!("%{}%", title));
        }

        for (filter, cfg) in [
            (&self.tags, TAGS),
            (&self.topics, TOPICS),
            (&self.technologies, TECHNOLOGIES),
            (&self.languages, LANGUAGES),
        ] {
            let Some(filter) = filter else { continue };
            let (frag, params) = RelationFilterSqlCompiler.compile(filter, cfg);
            if !frag.is_empty() {
                where_parts.push(frag);
                binds.extend(params);
            }
        }

        (where_parts, binds)
    }
}

fn parse_expression(expr: Option<&str>) -> Result<Option<ParsedSignedFilter>> {
    let Some(raw) = expr.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    SignedFilterParser
        .parse(raw, allow_tag_chars)
        .map(Some)
        .map_err(|e| anyhow::anyhow!("invalid filter expression: {}", e))
}

fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
