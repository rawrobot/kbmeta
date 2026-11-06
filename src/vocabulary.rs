use anyhow::{Context, Result};
use okf::yaml::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Subdirectory under `.kb` where `init` places the controlled vocabularies.
pub const VOCAB_SUBDIR: &str = "vocab";
/// Source directory for the controlled vocabularies when `.kb/vocab` is absent.
pub const VOCAB_SOURCE_DIR: &str = "doc/knowledge";

/// The sets of terms that are canonicalized against a controlled vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocabKind {
    Tags,
    Topics,
    Technologies,
    Languages,
    DocTypes,
}

impl VocabKind {
    /// YAML file name for this vocabulary kind.
    pub fn filename(self) -> &'static str {
        match self {
            Self::Tags => "tags.yaml",
            Self::Topics => "topics.yaml",
            Self::Technologies => "technologies.yaml",
            Self::Languages => "languages.yaml",
            Self::DocTypes => "doc-types.yaml",
        }
    }

    /// Top-level key inside the YAML file that holds the term list.
    pub fn key(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            Self::Topics => "topics",
            Self::Technologies => "technologies",
            Self::Languages => "languages",
            Self::DocTypes => "doc-types",
        }
    }
}

/// A controlled vocabulary: canonical names with aliases, per metadata set.
///
/// Each set maps a lowercased lookup key (a canonical name or any alias)
/// to the stable canonical string emitted on output. The raw canonical/alias
/// pairs are also retained so the vocabulary can be rendered into a prompt.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    tags: HashMap<String, String>,
    topics: HashMap<String, String>,
    technologies: HashMap<String, String>,
    languages: HashMap<String, String>,
    doc_types: HashMap<String, String>,
    entries: HashMap<VocabKind, Vec<VocabEntry>>,
}

/// One canonical name plus its aliases, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabEntry {
    pub canonical: String,
    pub aliases: Vec<String>,
}

impl Vocabulary {
    /// Loads every present vocabulary file, preferring `.kb/vocab/<name>.yaml`
    /// and falling back to `doc/knowledge/<name>.yaml` when absent or unreadable.
    pub fn load(root: &Path) -> Self {
        let kb_dir = crate::config::Config::kb_dir(root).join(VOCAB_SUBDIR);
        let fallback_dir = crate::pathing::resolve_from_root(root, Path::new(VOCAB_SOURCE_DIR));

        let mut vocab = Vocabulary::default();
        for kind in [
            VocabKind::Tags,
            VocabKind::Topics,
            VocabKind::Technologies,
            VocabKind::Languages,
            VocabKind::DocTypes,
        ] {
            let preferred = kb_dir.join(kind.filename());
            let fallback = fallback_dir.join(kind.filename());
            let path = if preferred.exists() {
                &preferred
            } else {
                &fallback
            };
            if !path.exists() {
                continue;
            }
            match load_one(path, kind) {
                Ok((map, entries)) => vocab.set(kind, map, entries),
                Err(err) => eprintln!(
                    "warning: failed to load vocabulary {}: {}",
                    path.display(),
                    err
                ),
            }
        }
        vocab
    }

    fn set(&mut self, kind: VocabKind, map: HashMap<String, String>, entries: Vec<VocabEntry>) {
        let slot = match kind {
            VocabKind::Tags => &mut self.tags,
            VocabKind::Topics => &mut self.topics,
            VocabKind::Technologies => &mut self.technologies,
            VocabKind::Languages => &mut self.languages,
            VocabKind::DocTypes => &mut self.doc_types,
        };
        *slot = map;
        self.entries.insert(kind, entries);
    }

    fn map(&self, kind: VocabKind) -> &HashMap<String, String> {
        match kind {
            VocabKind::Tags => &self.tags,
            VocabKind::Topics => &self.topics,
            VocabKind::Technologies => &self.technologies,
            VocabKind::Languages => &self.languages,
            VocabKind::DocTypes => &self.doc_types,
        }
    }

    /// Returns the lowercase-key -> canonical map for a kind.
    pub fn canonical_map(&self, kind: VocabKind) -> &HashMap<String, String> {
        self.map(kind)
    }

    /// Returns the canonical form of `value` for the given kind, if known.
    pub fn canonicalize(&self, kind: VocabKind, value: &str) -> Option<String> {
        self.map(kind)
            .get(&value.trim().to_ascii_lowercase())
            .cloned()
    }

    /// Renders the tag, topic and technology vocabularies as the spec's §1
    /// "CONTROLLED ... VOCABULARY" blocks for inclusion in the model request.
    pub fn render_vocab_blocks(&self) -> String {
        use std::fmt::Write;

        let mut out = String::new();
        let blocks = [
            ("CONTROLLED TAG VOCABULARY", VocabKind::Tags),
            ("CONTROLLED TOPIC VOCABULARY", VocabKind::Topics),
            ("CONTROLLED TECHNOLOGY VOCABULARY", VocabKind::Technologies),
        ];
        for (heading, kind) in blocks {
            let mut sorted = self.entries.get(&kind).cloned().unwrap_or_default();
            sorted.sort_by(|a, b| a.canonical.cmp(&b.canonical));
            let _ = writeln!(out, "{heading}");
            if sorted.is_empty() {
                let _ = writeln!(out, "(none)");
            }
            for entry in sorted {
                let _ = writeln!(out, "- canonical: {}", entry.canonical);
                if !entry.aliases.is_empty() {
                    let _ = writeln!(out, "  aliases: {}", entry.aliases.join(", "));
                }
            }
            let _ = writeln!(out);
        }
        out
    }
}

/// Splits model-reported new terms into genuinely new (not present in the
/// vocabulary) and already-known candidates that should be rejected. Mirrors
/// the OKF metadata-API §5 re-check step.
pub fn recheck_new_terms(
    vocab: &Vocabulary,
    new_terms: &crate::document::metadata_block::NewTerms,
) -> (
    crate::document::metadata_block::NewTerms,
    crate::document::metadata_block::NewTerms,
) {
    use crate::document::metadata_block::NewTerms;

    let (tags_ok, tags_bad) = split_new_terms(&new_terms.tags, vocab, VocabKind::Tags);
    let (topics_ok, topics_bad) = split_new_terms(&new_terms.topics, vocab, VocabKind::Topics);
    let (tech_ok, tech_bad) =
        split_new_terms(&new_terms.technologies, vocab, VocabKind::Technologies);

    (
        NewTerms {
            tags: tags_ok,
            topics: topics_ok,
            technologies: tech_ok,
        },
        NewTerms {
            tags: tags_bad,
            topics: topics_bad,
            technologies: tech_bad,
        },
    )
}

fn split_new_terms(
    terms: &[String],
    vocab: &Vocabulary,
    kind: VocabKind,
) -> (Vec<String>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for term in terms {
        let t = term.trim();
        if t.is_empty() {
            continue;
        }
        if vocab.canonicalize(kind, t).is_some() {
            rejected.push(t.to_string());
        } else {
            accepted.push(t.to_string());
        }
    }
    (accepted, rejected)
}

/// Parses one vocabulary file into a lowercase-key -> canonical map plus the
/// raw canonical/alias entries in file order.
fn load_one(path: &Path, kind: VocabKind) -> Result<(HashMap<String, String>, Vec<VocabEntry>)> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value =
        Value::parse(&text).with_context(|| format!("invalid YAML in {}", path.display()))?;

    let mapping = value
        .as_mapping()
        .with_context(|| format!("{} must be a mapping", path.display()))?;

    let seq = mapping
        .get(kind.key())
        .and_then(|v| v.as_sequence())
        .with_context(|| {
            format!(
                "{} must contain a '{}' sequence",
                path.display(),
                kind.key()
            )
        })?;

    let mut out = HashMap::new();
    let mut entries: Vec<VocabEntry> = Vec::new();
    for entry in seq {
        let em = entry
            .as_mapping()
            .with_context(|| format!("{} entry must be a mapping", path.display()))?;

        let canonical = em
            .get("canonical")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .with_context(|| {
                format!(
                    "{} entry is missing a non-empty 'canonical'",
                    path.display()
                )
            })?;

        out.insert(canonical.to_ascii_lowercase(), canonical.to_string());

        let mut aliases = Vec::new();
        if let Some(alias_seq) = em.get("aliases").and_then(|v| v.as_sequence()) {
            for alias in alias_seq {
                if let Some(alias) = alias.as_str() {
                    let alias = alias.trim();
                    if !alias.is_empty() {
                        out.insert(alias.to_ascii_lowercase(), canonical.to_string());
                        aliases.push(alias.to_string());
                    }
                }
            }
        }
        entries.push(VocabEntry {
            canonical: canonical.to_string(),
            aliases,
        });
    }

    Ok((out, entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Writes vocabulary into `.kb/vocab/<name>.yaml` (the preferred location).
    fn write_vocab(root: &std::path::Path, kind: VocabKind, body: &str) {
        let dir = root.join(".kb").join(VOCAB_SUBDIR);
        fs::create_dir_all(&dir).expect("create .kb/vocab dir");
        fs::write(dir.join(kind.filename()), body).expect("write vocab");
    }

    #[test]
    fn loads_canonical_and_aliases() {
        let root = tempfile::tempdir().unwrap();
        write_vocab(
            root.path(),
            VocabKind::Tags,
            "tags:\n  - canonical: version-control\n    aliases:\n      - git\n      - github\n",
        );

        let vocab = Vocabulary::load(root.path());
        assert_eq!(
            vocab.canonicalize(VocabKind::Tags, "git"),
            Some("version-control".to_string())
        );
        assert_eq!(
            vocab.canonicalize(VocabKind::Tags, "GitHub"),
            Some("version-control".to_string())
        );
        assert_eq!(
            vocab.canonicalize(VocabKind::Tags, "version-control"),
            Some("version-control".to_string())
        );
        assert_eq!(vocab.canonicalize(VocabKind::Tags, "unknown"), None);
    }

    #[test]
    fn preserves_canonical_casing_in_technologies() {
        let root = tempfile::tempdir().unwrap();
        write_vocab(
            root.path(),
            VocabKind::Technologies,
            "technologies:\n  - canonical: Git\n    aliases:\n      - git\n",
        );

        let vocab = Vocabulary::load(root.path());
        assert_eq!(
            vocab.canonicalize(VocabKind::Technologies, "git"),
            Some("Git".to_string())
        );
    }

    #[test]
    fn falls_back_to_doc_knowledge_dir() {
        let root = tempfile::tempdir().unwrap();
        let fallback = root.path().join("doc/knowledge");
        fs::create_dir_all(&fallback).expect("create fallback dir");
        fs::write(
            fallback.join("topics.yaml"),
            "topics:\n  - canonical: linux\n    aliases: []\n",
        )
        .expect("write fallback");

        let vocab = Vocabulary::load(root.path());
        assert_eq!(
            vocab.canonicalize(VocabKind::Topics, "linux"),
            Some("linux".to_string())
        );
    }

    #[test]
    fn empty_when_no_vocab_present() {
        let root = tempfile::tempdir().unwrap();
        let vocab = Vocabulary::load(root.path());
        assert_eq!(vocab.canonicalize(VocabKind::Tags, "git"), None);
    }

    #[test]
    fn render_vocab_blocks_lists_canonicals_and_aliases() {
        let root = tempfile::tempdir().unwrap();
        write_vocab(
            root.path(),
            VocabKind::Tags,
            "tags:\n  - canonical: beta\n    aliases: []\n  - canonical: version-control\n    aliases:\n      - git\n      - github\n",
        );
        write_vocab(
            root.path(),
            VocabKind::Topics,
            "topics:\n  - canonical: tooling\n    aliases: []\n",
        );

        let vocab = Vocabulary::load(root.path());
        let rendered = vocab.render_vocab_blocks();

        assert!(rendered.contains("CONTROLLED TAG VOCABULARY"));
        assert!(rendered.contains("- canonical: version-control"));
        assert!(rendered.contains("aliases: git, github"));
        assert!(rendered.contains("- canonical: beta"));
        assert!(rendered.contains("CONTROLLED TOPIC VOCABULARY"));
        assert!(rendered.contains("- canonical: tooling"));
        assert!(rendered.contains("CONTROLLED TECHNOLOGY VOCABULARY"));
    }

    #[test]
    fn doc_types_loaded_as_kind() {
        let root = tempfile::tempdir().unwrap();
        write_vocab(
            root.path(),
            VocabKind::DocTypes,
            "doc-types:\n  - canonical: HowTo\n    aliases:\n      - tutorial\n",
        );

        let vocab = Vocabulary::load(root.path());
        assert_eq!(
            vocab.canonicalize(VocabKind::DocTypes, "tutorial"),
            Some("HowTo".to_string())
        );
        assert_eq!(
            vocab.canonicalize(VocabKind::DocTypes, "HowTo"),
            Some("HowTo".to_string())
        );
    }

    #[test]
    fn recheck_new_terms_splits_known_and_genuinely_new() {
        use crate::document::metadata_block::NewTerms;

        let root = tempfile::tempdir().unwrap();
        write_vocab(
            root.path(),
            VocabKind::Tags,
            "tags:\n  - canonical: version-control\n    aliases:\n      - git\n      - github\n",
        );
        write_vocab(
            root.path(),
            VocabKind::Topics,
            "topics:\n  - canonical: tooling\n    aliases: []\n",
        );
        write_vocab(
            root.path(),
            VocabKind::Technologies,
            "technologies:\n  - canonical: Rust\n    aliases:\n      - rust\n",
        );

        let vocab = Vocabulary::load(root.path());
        let report = NewTerms {
            tags: vec![
                "git".to_string(),  // alias of existing -> rejected
                "dbus".to_string(), // genuinely new -> accepted
            ],
            topics: vec!["tooling".to_string(), "gui".to_string()],
            technologies: vec!["rust".to_string(), "niri".to_string()],
        };

        let (accepted, rejected) = recheck_new_terms(&vocab, &report);
        assert_eq!(accepted.tags, vec!["dbus"]);
        assert_eq!(rejected.tags, vec!["git"]);
        assert_eq!(accepted.topics, vec!["gui"]);
        assert_eq!(rejected.topics, vec!["tooling"]);
        assert_eq!(accepted.technologies, vec!["niri"]);
        assert_eq!(rejected.technologies, vec!["rust"]);
    }
}
