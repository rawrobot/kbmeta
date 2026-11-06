use crate::document::metadata_block::{
    get_languages, get_technologies, get_topics, set_languages, set_technologies, set_topics,
    NewTerms,
};
use crate::vocabulary::{VocabKind, Vocabulary};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const MAX_TAGS: usize = 10;
const MAX_TOPICS: usize = 5;

/// A provider suggestion: the frontmatter to persist plus any vocabulary
/// candidates the model surfaced in `new_terms` (OKF metadata-API §4).
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Canonicalized frontmatter ready for persistence.
    pub metadata: okf::Frontmatter,
    /// Model-reported vocabulary candidates, already re-checked by the caller.
    pub new_terms: NewTerms,
}

/// Canonical vocabulary and defaults used to normalize provider output.
#[derive(Debug, Clone, Default)]
pub struct SuggestionRules {
    /// Most common document type in the KB.
    pub preferred_doc_type: Option<String>,
    tags: HashMap<String, String>,
    topics: HashMap<String, String>,
    technologies: HashMap<String, String>,
    languages: HashMap<String, String>,
    doc_types: HashMap<String, String>,
    /// Rendered tag/topic/technology vocabulary blocks, sent in the user
    /// message so the model can map aliases onto canonical terms.
    vocab_text: Option<String>,
}

impl SuggestionRules {
    /// Builds rules from a controlled vocabulary.
    ///
    /// Sets keyed with `lowercase` canonicalization are emitted in lowercase
    /// kebab-case. Document types are emitted verbatim (e.g. `HowTo`), since
    /// the metadata schema's `type` enum matches exactly.
    pub fn from_vocabulary(vocab: &Vocabulary, preferred_doc_type: Option<String>) -> Self {
        Self {
            preferred_doc_type,
            tags: vocab_map(vocab, VocabKind::Tags),
            topics: vocab_map(vocab, VocabKind::Topics),
            technologies: vocab_map(vocab, VocabKind::Technologies),
            languages: vocab_map(vocab, VocabKind::Languages),
            doc_types: vocab_map(vocab, VocabKind::DocTypes),
            vocab_text: Some(vocab.render_vocab_blocks()),
        }
    }

    /// The vocab blocks for the user message, if rules came from a vocabulary.
    pub fn vocab_text(&self) -> Option<&str> {
        self.vocab_text.as_deref()
    }

    /// Returns canonical tag value based on KB vocabulary, if known.
    pub fn canonical_tag(&self, value: &str) -> String {
        normalize_kebab(&canonicalize_with_map(value, &self.tags))
    }

    /// Returns canonical topic value based on KB vocabulary, if known.
    pub fn canonical_topic(&self, value: &str) -> String {
        normalize_kebab(&canonicalize_with_map(value, &self.topics))
    }

    /// Returns canonical technology value based on KB vocabulary, if known.
    pub fn canonical_technology(&self, value: &str) -> String {
        normalize_kebab(&canonicalize_with_map(value, &self.technologies))
    }

    /// Returns canonical language value based on KB vocabulary, if known.
    pub fn canonical_language(&self, value: &str) -> String {
        normalize_kebab(&canonicalize_with_map(value, &self.languages))
    }

    /// Returns canonical document type based on KB vocabulary, emitted verbatim
    /// (no kebab normalization), if known.
    pub fn canonical_doc_type(&self, value: &str) -> String {
        canonicalize_with_map(value, &self.doc_types)
    }
}

fn vocab_map(vocab: &Vocabulary, kind: VocabKind) -> HashMap<String, String> {
    vocab.canonical_map(kind).clone()
}

/// Provider abstraction for generating metadata suggestions from document content.
pub trait MetadataProvider {
    /// Returns a metadata suggestion for one Markdown document.
    fn suggest(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
    ) -> anyhow::Result<Suggestion>;

    /// Returns a metadata suggestion with optional system-prompt override.
    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> anyhow::Result<Suggestion> {
        let _ = system_prompt;
        self.suggest(path, content, rules)
    }
}

/// Applies canonicalization rules to provider output, using the controlled
/// vocabulary in `rules`. Set-valued fields are lowered to lowercase kebab-case;
/// the document type is emitted verbatim through the doc-type vocabulary.
pub fn apply_rules(mut metadata: okf::Frontmatter, rules: &SuggestionRules) -> okf::Frontmatter {
    match metadata.type_().map(|cow| cow.trim().to_string()) {
        Some(t) if !t.is_empty() => {
            metadata.set("type", okf::Value::String(rules.canonical_doc_type(&t)));
        }
        _ => {
            if let Some(preferred) = &rules.preferred_doc_type {
                metadata.set("type", okf::Value::String(preferred.clone()));
            }
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

fn normalize_kebab(value: &str) -> String {
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
    use super::{apply_rules, normalize_kebab, SuggestionRules, MAX_TAGS};
    use crate::document::metadata_block::{build_frontmatter, get_doc_type};
    use crate::vocabulary::{VocabKind, Vocabulary};
    use std::fs;

    fn vocab_with_tags(kind: VocabKind, canonical: &str, aliases: &[&str]) -> Vocabulary {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("doc/knowledge");
        fs::create_dir_all(&sub).unwrap();
        let aliases_yaml = aliases
            .iter()
            .map(|a| format!("      - {a}"))
            .collect::<Vec<_>>()
            .join("\n");
        let body = format!(
            "{}:\n  - canonical: {canonical}\n    aliases:\n{}\n",
            kind.key(),
            aliases_yaml
        );
        fs::write(sub.join(kind.filename()), body).unwrap();
        Vocabulary::load(dir.path())
    }

    #[test]
    fn tag_normalization_removes_spaces() {
        assert_eq!(normalize_kebab("Embedded Systems"), "embedded-systems");
        assert_eq!(normalize_kebab("GPU_ACCEL"), "gpu-accel");
        assert_eq!(normalize_kebab("  rust  "), "rust");
    }

    #[test]
    fn tag_normalization_empty_returns_default() {
        assert_eq!(normalize_kebab(""), "tag");
        assert_eq!(normalize_kebab("   "), "tag");
        assert_eq!(normalize_kebab("!@#"), "tag");
    }

    #[test]
    fn from_vocabulary_maps_alias_to_canonical_and_normalizes() {
        let vocab = vocab_with_tags(VocabKind::Tags, "version-control", &["git", "GitHub"]);
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        assert_eq!(rules.canonical_tag("git"), "version-control");
        assert_eq!(rules.canonical_tag("GitHub"), "version-control");
        assert_eq!(rules.canonical_tag("unknown"), "unknown");
    }

    #[test]
    fn from_vocabulary_normalizes_technologies_to_kebab() {
        let vocab = vocab_with_tags(VocabKind::Technologies, "Git", &["git"]);
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        assert_eq!(rules.canonical_technology("git"), "git");
    }

    #[test]
    fn doc_type_kept_verbatim_and_alias_mapped() {
        let vocab = vocab_with_tags(VocabKind::DocTypes, "HowTo", &["tutorial", "how-to"]);
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        assert_eq!(rules.canonical_doc_type("HowTo"), "HowTo");
        assert_eq!(rules.canonical_doc_type("tutorial"), "HowTo");
        assert_eq!(rules.canonical_doc_type("unknown-type"), "unknown-type");
    }

    #[test]
    fn apply_rules_maps_type_alias_to_canonical_verbatim() {
        let vocab = vocab_with_tags(VocabKind::DocTypes, "HowTo", &["tutorial"]);
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        let metadata = build_frontmatter(
            Some("tutorial".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let out = apply_rules(metadata, &rules);
        assert_eq!(get_doc_type(&out).as_deref(), Some("HowTo"));
    }

    #[test]
    fn apply_rules_limits_tags_and_topics() {
        let metadata = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![
                "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
                "eleven", "twelve",
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
    fn apply_rules_maps_aliases_to_lowercased_canonical() {
        let vocab = vocab_with_tags(VocabKind::Technologies, "Git", &["git"]);
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        let metadata = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec!["git".to_string()],
            vec![],
        );
        let out = apply_rules(metadata, &rules);
        assert_eq!(super::get_technologies(&out), vec!["git"]);
    }
}
