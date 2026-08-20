use crate::document::metadata_block::MetadataBlock;
use crate::provider::{MetadataProvider, SuggestionRules};
use anyhow::Result;
use std::path::Path;

/// Deterministic fallback provider used for local development and testing.
pub struct MockMetadataProvider;

impl MetadataProvider for MockMetadataProvider {
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<MetadataBlock> {
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

        Ok(MetadataBlock {
            doc_type: Some(
                rules
                    .preferred_doc_type
                    .clone()
                    .unwrap_or_else(|| "Note".to_string()),
            ),
            title: Some(title.clone()),
            description: Some(format!("Suggested metadata for {}.", title)),
            tags,
            topics: vec![rules.canonical_topic("notes")],
            technologies,
            languages,
        })
    }
}
