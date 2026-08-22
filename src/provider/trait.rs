use crate::document::Document;
use crate::document::metadata_block::{get_languages, get_technologies, get_topics, set_languages, set_technologies, set_topics};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const MAX_TAGS: usize = 10;
const MAX_TOPICS: usize = 5;

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

/// Provider abstraction for generating metadata suggestions from document content.
pub trait MetadataProvider {
    /// Returns a metadata suggestion for one Markdown document.
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> anyhow::Result<okf::Frontmatter>;

    /// Returns a metadata suggestion with optional system-prompt override.
    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> anyhow::Result<okf::Frontmatter> {
        let _ = system_prompt;
        self.suggest(path, content, rules)
    }
}

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

        if let Some(doc_type) = metadata.type_().map(|cow| {
            let t = cow.trim().to_string();
            t
        }).filter(|x| !x.is_empty()) {
            let key = doc_type.to_ascii_lowercase();
            *type_counts.entry(key).or_insert(0) += 1;
            type_canonical
                .entry(doc_type.to_ascii_lowercase())
                .or_insert_with(|| doc_type);
        }

        for tag in &metadata.tags() {
            index_vocab(&mut rules.tags, tag);
        }
        for topic in &get_topics(metadata) {
            index_vocab(&mut rules.topics, topic);
        }
        for technology in &get_technologies(metadata) {
            index_vocab(&mut rules.technologies, technology);
        }
        for language in &get_languages(metadata) {
            index_vocab(&mut rules.languages, language);
        }
    }

    if let Some((winner, _)) = type_counts.into_iter().max_by_key(|(_, count)| *count) {
        rules.preferred_doc_type = type_canonical.get(&winner).cloned().or(Some(winner));
    }

    rules
}

/// Applies KB-derived canonicalization rules to provider output.
pub fn apply_rules(mut metadata: okf::Frontmatter, rules: &SuggestionRules) -> okf::Frontmatter {
    if metadata.type_()
        .map(|cow| cow.trim().is_empty())
        .unwrap_or(true)
    {
        if let Some(preferred) = &rules.preferred_doc_type {
            metadata.set("type", okf::Value::String(preferred.clone()));
        }
    }

    let tags = limit_values(
        canonicalize_values(&metadata.tags(), |value| rules.canonical_tag(value)),
        MAX_TAGS,
    );
    metadata.set(
        "tags",
        okf::Value::Sequence(tags.into_iter().map(okf::Value::String).collect()),
    );

    let topics = limit_values(
        canonicalize_values(&get_topics(&metadata), |value| rules.canonical_topic(value)),
        MAX_TOPICS,
    );
    set_topics(&mut metadata, topics);

    let technologies = canonicalize_values(&get_technologies(&metadata), |value| {
        rules.canonical_technology(value)
    });
    set_technologies(&mut metadata, technologies);

    let languages = canonicalize_values(&get_languages(&metadata), |value| {
        rules.canonical_language(value)
    });
    set_languages(&mut metadata, languages);

    metadata
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
    use super::{MAX_TAGS, SuggestionRules, apply_rules, derive_rules, normalize_tag};
    use crate::document::Document;
    use crate::document::metadata_block::{build_frontmatter, get_doc_type, get_title, get_description};
    use std::path::PathBuf;

    fn make_doc(doc_type: &str, tags: Vec<&str>) -> Document {
        Document {
            path: PathBuf::from("/tmp/doc.md"),
            metadata: Some(build_frontmatter(
                Some(doc_type.to_string()),
                Some("Title".to_string()),
                Some("Desc".to_string()),
                tags.into_iter().map(String::from).collect(),
                vec![],
                vec![],
                vec![],
            )),
            content_hash: "abc".to_string(),
        }
    }

    #[test]
    fn tag_normalization_removes_spaces() {
        assert_eq!(normalize_tag("Embedded Systems"), "embedded-systems");
        assert_eq!(normalize_tag("GPU_ACCEL"), "gpu-accel");
        assert_eq!(normalize_tag("  rust  "), "rust");
    }

    #[test]
    fn tag_normalization_empty_returns_default() {
        assert_eq!(normalize_tag(""), "tag");
        assert_eq!(normalize_tag("   "), "tag");
        assert_eq!(normalize_tag("!@#"), "tag");
    }

    #[test]
    fn apply_rules_limits_tags_and_topics() {
        let metadata = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![
                "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
                "ten", "eleven", "twelve",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            vec!["a", "b", "c", "d", "e", "f"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            vec![],
            vec![],
        );

        let out = apply_rules(metadata, &SuggestionRules::default());
        assert_eq!(out.tags().len(), MAX_TAGS);
        assert_eq!(super::get_topics(&out).len(), 5);
        assert_eq!(out.tags()[0], "one");
        assert_eq!(out.tags()[9], "ten");
        assert_eq!(super::get_topics(&out)[0], "a");
        assert_eq!(super::get_topics(&out)[4], "e");
    }

    #[test]
    fn apply_rules_fills_empty_doc_type_from_preferred() {
        let metadata = build_frontmatter(
            None,
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let rules = SuggestionRules {
            preferred_doc_type: Some("Tutorial".to_string()),
            ..SuggestionRules::default()
        };
        let out = apply_rules(metadata, &rules);
        assert_eq!(get_doc_type(&out).as_deref(), Some("Tutorial"));
    }

    #[test]
    fn apply_rules_deduplicates_tags_case_insensitive() {
        let metadata = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec!["Rust".to_string(), "rust".to_string(), "RUST".to_string()],
            vec![],
            vec![],
            vec![],
        );
        let out = apply_rules(metadata, &SuggestionRules::default());
        assert_eq!(out.tags().len(), 1);
    }

    #[test]
    fn derive_rules_selects_most_common_doc_type() {
        let docs = vec![
            make_doc("Note", vec![]),
            make_doc("Note", vec![]),
            make_doc("Tutorial", vec![]),
        ];
        let rules = derive_rules(&docs, &PathBuf::from("/tmp/other.md"));
        assert_eq!(rules.preferred_doc_type.as_deref(), Some("Note"));
    }

    #[test]
    fn derive_rules_indexes_vocabulary() {
        let docs = vec![
            make_doc("Note", vec!["rust", "web"]),
            make_doc("Note", vec!["rust", "db"]),
        ];
        let rules = derive_rules(&docs, &PathBuf::from("/tmp/other.md"));
        assert_eq!(rules.canonical_tag("rust"), "rust");
        assert_eq!(rules.canonical_tag("Rust"), "rust");
    }

    #[test]
    fn derive_rules_skips_target_document() {
        let target = PathBuf::from("/tmp/target.md");
        let docs = vec![
            make_doc("Tutorial", vec![]),
            Document {
                path: target.clone(),
                metadata: Some(build_frontmatter(
                    Some("Note".to_string()),
                    Some("T".to_string()),
                    Some("D".to_string()),
                    vec!["unique-tag".to_string()],
                    vec![],
                    vec![],
                    vec![],
                )),
                content_hash: "abc".to_string(),
            },
        ];
        let rules = derive_rules(&docs, &target);
        assert_eq!(rules.preferred_doc_type.as_deref(), Some("Tutorial"));
        assert_eq!(rules.canonical_tag("unique-tag"), "unique-tag");
    }
}
