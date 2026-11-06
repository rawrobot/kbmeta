use crate::document::metadata_block::{build_frontmatter, NewTerms};
use crate::provider::r#trait::{MetadataProvider, Suggestion, SuggestionRules};
use anyhow::Result;
use okf::Value;
use std::path::Path;

/// Deterministic fallback provider used for local development and testing.
pub struct MockMetadataProvider;

impl MetadataProvider for MockMetadataProvider {
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<Suggestion> {
        let title = content
            .lines()
            .find_map(|line| line.strip_prefix("# ").map(str::trim).map(str::to_string))
            .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Untitled".to_string());

        let lowered = content.to_ascii_lowercase();
        let mut tags = vec![rules.canonical_tag("draft")];
        if lowered.contains("rust") {
            tags.push(rules.canonical_tag("rust"));
        }
        if lowered.contains("tokio") {
            tags.push(rules.canonical_tag("tokio"));
        }

        let mut technologies = Vec::new();
        if lowered.contains("tokio") {
            technologies.push(rules.canonical_technology("Tokio"));
        }

        let mut languages = Vec::new();
        if lowered.contains("rust") {
            languages.push(rules.canonical_language("rust"));
        }

        let mut fm = build_frontmatter(
            Some(
                rules
                    .preferred_doc_type
                    .clone()
                    .unwrap_or_else(|| "Note".to_string()),
            ),
            Some(title.clone()),
            Some(format!("Suggested metadata for {}.", title)),
            tags,
            vec![rules.canonical_topic("notes")],
            technologies,
            languages,
        );

        let filename = title
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let filename = filename.trim_matches(|c| c == '_' || c == '-').to_string();
        if !filename.is_empty() {
            fm.set("filename", Value::String(filename));
        }

        // Deterministic new-term candidates: report a genuinely new tag when
        // the document mentions "dbus" but no vocab entry covers it, plus an
        // already-known term so the pipeline re-check has something to reject.
        let mut new_terms = NewTerms::default();
        let lowered = content.to_ascii_lowercase();
        if lowered.contains("dbus") {
            new_terms.tags.push("dbus".to_string());
        }
        new_terms.tags.push("draft".to_string());

        Ok(Suggestion {
            metadata: fm,
            new_terms,
        })
    }
}
