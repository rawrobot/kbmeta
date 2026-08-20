mod mock;
mod nvidia;

use crate::config::Config;
use crate::document::Document;
use crate::document::metadata_block::MetadataBlock;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub use mock::MockMetadataProvider;
pub use nvidia::NvidiaMetadataProvider;

/// Canonical vocabulary and defaults learned from existing KB metadata.
#[derive(Debug, Clone, Default)]
pub struct SuggestionRules {
    /// Most common document type in the KB.
    pub preferred_doc_type: Option<String>,
    tags: HashMap<String, String>,
    topics: HashMap<String, String>,
    technologies: HashMap<String, String>,
    languages: HashMap<String, String>,
}

impl SuggestionRules {
    /// Returns canonical tag value based on KB vocabulary, if known.
    pub fn canonical_tag(&self, value: &str) -> String {
        normalize_tag(&canonicalize_with_map(value, &self.tags))
    }

    /// Returns canonical topic value based on KB vocabulary, if known.
    pub fn canonical_topic(&self, value: &str) -> String {
        canonicalize_with_map(value, &self.topics)
    }

    /// Returns canonical technology value based on KB vocabulary, if known.
    pub fn canonical_technology(&self, value: &str) -> String {
        canonicalize_with_map(value, &self.technologies)
    }

    /// Returns canonical language value based on KB vocabulary, if known.
    pub fn canonical_language(&self, value: &str) -> String {
        canonicalize_with_map(value, &self.languages)
    }
}

const MAX_TAGS: usize = 10;
const MAX_TOPICS: usize = 5;

/// Builds suggestion rules from existing KB documents.
pub fn derive_rules(documents: &[Document], target_path: &Path) -> SuggestionRules {
    let mut rules = SuggestionRules::default();
    let mut type_counts: HashMap<String, usize> = HashMap::new();
    let mut type_canonical: HashMap<String, String> = HashMap::new();

    for doc in documents {
        if doc.path == target_path {
            continue;
        }

        let Some(metadata) = &doc.metadata else {
            continue;
        };

        if let Some(doc_type) = metadata.doc_type.as_ref().map(|x| x.trim()).filter(|x| !x.is_empty()) {
            let key = doc_type.to_ascii_lowercase();
            *type_counts.entry(key).or_insert(0) += 1;
            type_canonical
                .entry(doc_type.to_ascii_lowercase())
                .or_insert_with(|| doc_type.to_string());
        }

        for tag in &metadata.tags {
            index_vocab(&mut rules.tags, tag);
        }
        for topic in &metadata.topics {
            index_vocab(&mut rules.topics, topic);
        }
        for technology in &metadata.technologies {
            index_vocab(&mut rules.technologies, technology);
        }
        for language in &metadata.languages {
            index_vocab(&mut rules.languages, language);
        }
    }

    if let Some((winner, _)) = type_counts.into_iter().max_by_key(|(_, count)| *count) {
        rules.preferred_doc_type = type_canonical.get(&winner).cloned().or(Some(winner));
    }

    rules
}

fn index_vocab(map: &mut HashMap<String, String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    map.entry(trimmed.to_ascii_lowercase())
        .or_insert_with(|| trimmed.to_string());
}

fn canonicalize_with_map(value: &str, map: &HashMap<String, String>) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    map.get(&trimmed.to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| trimmed.to_string())
}

/// Provider abstraction for generating metadata suggestions from document content.
pub trait MetadataProvider {
    /// Returns a metadata suggestion for one Markdown document.
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<MetadataBlock>;

    /// Returns a metadata suggestion with optional system-prompt override.
    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> Result<MetadataBlock> {
        let _ = system_prompt;
        self.suggest(path, content, rules)
    }
}

/// Builds a metadata provider from runtime configuration.
pub fn build_provider(config: &Config) -> Result<Box<dyn MetadataProvider>> {
    match config.provider.kind.trim().to_ascii_lowercase().as_str() {
        "mock" => Ok(Box::new(MockMetadataProvider)),
        "nvidia" => Ok(Box::new(NvidiaMetadataProvider::new(config.provider.clone())?)),
        other => bail!("unsupported provider kind '{}'; expected mock|nvidia", other),
    }
}

/// Applies KB-derived canonicalization rules to provider output.
pub fn apply_rules(mut metadata: MetadataBlock, rules: &SuggestionRules) -> MetadataBlock {
    if metadata
        .doc_type
        .as_ref()
        .map(|x| x.trim().is_empty())
        .unwrap_or(true)
    {
        metadata.doc_type = rules.preferred_doc_type.clone();
    }

    metadata.tags = limit_values(
        canonicalize_values(&metadata.tags, |value| rules.canonical_tag(value)),
        MAX_TAGS,
    );
    metadata.topics = limit_values(
        canonicalize_values(&metadata.topics, |value| rules.canonical_topic(value)),
        MAX_TOPICS,
    );
    metadata.technologies = canonicalize_values(&metadata.technologies, |value| {
        rules.canonical_technology(value)
    });
    metadata.languages = canonicalize_values(&metadata.languages, |value| {
        rules.canonical_language(value)
    });

    metadata
}

fn limit_values(values: Vec<String>, max: usize) -> Vec<String> {
    if values.len() <= max {
        return values;
    }
    values.into_iter().take(max).collect()
}

fn canonicalize_values(values: &[String], canonicalize: impl Fn(&str) -> String) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for value in values {
        let normalized = canonicalize(value);
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }

    out
}

fn normalize_tag(value: &str) -> String {
    let mut out = String::new();
    let mut sep = false;

    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            sep = false;
            continue;
        }

        if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !sep && !out.is_empty() {
                out.push('-');
                sep = true;
            }
            continue;
        }

        if !sep && !out.is_empty() {
            out.push('-');
            sep = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }

    if out.is_empty() {
        "tag".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{SuggestionRules, apply_rules, normalize_tag};
    use crate::document::metadata_block::MetadataBlock;

    #[test]
    fn tag_normalization_removes_spaces() {
        assert_eq!(normalize_tag("Embedded Systems"), "embedded-systems");
        assert_eq!(normalize_tag("GPU_ACCEL"), "gpu-accel");
        assert_eq!(normalize_tag("  rust  "), "rust");
    }

    #[test]
    fn apply_rules_limits_tags_and_topics() {
        let metadata = MetadataBlock {
            doc_type: Some("Note".to_string()),
            title: Some("Title".to_string()),
            description: Some("Desc".to_string()),
            tags: vec![
                "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
                "ten", "eleven", "twelve",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            topics: vec!["a", "b", "c", "d", "e", "f"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            technologies: vec![],
            languages: vec![],
        };

        let out = apply_rules(metadata, &SuggestionRules::default());
        assert_eq!(out.tags.len(), 10);
        assert_eq!(out.topics.len(), 5);
        assert_eq!(out.tags[0], "one");
        assert_eq!(out.tags[9], "ten");
        assert_eq!(out.topics[0], "a");
        assert_eq!(out.topics[4], "e");
    }
}
