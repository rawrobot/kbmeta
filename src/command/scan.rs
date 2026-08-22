use crate::command::open_db;
use crate::config::Config;
use crate::document::Document;
use crate::document::discovery::DiscoverWarning;
use crate::document::metadata_block::{missing_recommended_fields, missing_required_fields};
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result, bail};
use rusqlite::OptionalExtension;
use std::path::Path;

const CODE_MISSING_REQUIRED_FIELDS: &str = "KBMETA_MISSING_REQUIRED_FIELDS";
const CODE_DISCOVERY_WARNING: &str = "KBMETA_DISCOVERY_WARNING";

#[derive(Debug)]
enum DocValidation {
    Valid,
    MissingMetadata,
    Warnings(Vec<(String, String)>),
    InvalidMetadata { warnings: Vec<(String, String)> },
    HardError(String),
}

fn validate_doc_metadata(doc: &Document, skip_invalid: bool) -> DocValidation {
    let Some(metadata) = &doc.metadata else {
        return DocValidation::MissingMetadata;
    };

    let required_missing = missing_required_fields(metadata);
    if !required_missing.is_empty() {
        let message = format!(
            "code={} metadata block near line 1 is missing required fields: {}",
            CODE_MISSING_REQUIRED_FIELDS,
            required_missing.join(", ")
        );
        if skip_invalid {
            return DocValidation::InvalidMetadata {
                warnings: vec![(doc.path.to_string_lossy().to_string(), message)],
            };
        } else {
            return DocValidation::HardError(format!("{}: {}", doc.path.display(), message));
        }
    }

    let recommended_missing = missing_recommended_fields(metadata);
    if !recommended_missing.is_empty() {
        let message = format!(
            "code={} metadata block near line 1 is missing recommended fields: {}",
            CODE_MISSING_REQUIRED_FIELDS,
            recommended_missing.join(", ")
        );
        return DocValidation::Warnings(vec![
            (doc.path.to_string_lossy().to_string(), message),
        ]);
    }

    DocValidation::Valid
}

fn check_staleness(conn: &rusqlite::Connection, doc: &Document) -> Result<bool> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM documents WHERE path = ?1",
            [doc.path.to_string_lossy().to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored.is_some_and(|hash| hash != doc.content_hash))
}

/// Handles `kbmeta scan`.
pub(crate) fn run(root: &Path, config_override: Option<&Path>, skip_invalid: bool) -> Result<()> {
    let cfg = Config::load_with(root, config_override)?;
    let docs_root = resolve_from_root(root, &cfg.documents_root);
    let (docs, warnings) = discover(&docs_root, skip_invalid)?;

    let conn = open_db(root)?;
    let mut stale = 0usize;
    let mut missing = 0usize;
    let mut hard_errors = Vec::new();
    let mut validation_warnings = Vec::new();
    let mut scanned = 0usize;

    for doc in &docs {
        match validate_doc_metadata(doc, skip_invalid) {
            DocValidation::Valid => {}
            DocValidation::MissingMetadata => {
                missing += 1;
            }
            DocValidation::Warnings(mut vws) => {
                validation_warnings.append(&mut vws);
            }
            DocValidation::InvalidMetadata {
                warnings: mut vws,
            } => {
                validation_warnings.append(&mut vws);
                continue;
            }
            DocValidation::HardError(msg) => {
                hard_errors.push(msg);
                continue;
            }
        }
        scanned += 1;
        if check_staleness(&conn, doc)? {
            stale += 1;
        }
    }

    if !hard_errors.is_empty() {
        bail!(
            "metadata validation failed for {} document(s) [code={}]:\n- {}",
            hard_errors.len(),
            CODE_MISSING_REQUIRED_FIELDS,
            hard_errors.join("\n- ")
        );
    }

    print_warnings(&warnings, &validation_warnings);
    println!("Scanned {} documents", scanned);
    println!("Missing metadata block: {}", missing);
    println!("Stale index entries: {}", stale);
    println!("Run 'kbmeta index' to refresh SQLite index");
    Ok(())
}

fn discover(
    docs_root: &Path,
    skip_invalid: bool,
) -> Result<(Vec<Document>, Vec<DiscoverWarning>)> {
    if skip_invalid {
        let report = crate::document::discovery::discover_documents_lenient(docs_root)
            .with_context(|| {
                format!(
                    "failed to scan {} [code={}]",
                    docs_root.display(),
                    CODE_DISCOVERY_WARNING
                )
            })?;
        Ok((report.documents, report.warnings))
    } else {
        let docs = crate::document::discovery::discover_documents_strict(docs_root)
            .with_context(|| {
                format!(
                    "failed to scan {} [code={}]",
                    docs_root.display(),
                    CODE_DISCOVERY_WARNING
                )
            })?;
        Ok((docs, Vec::new()))
    }
}

fn print_warnings(discovery_warnings: &[DiscoverWarning], validation_warnings: &[(String, String)]) {
    for warning in discovery_warnings {
        eprintln!(
            "warning [code={}]: skipped {}: {}",
            CODE_DISCOVERY_WARNING,
            warning.path.display(),
            warning.message
        );
    }
    for (path, message) in validation_warnings {
        eprintln!(
            "warning [code={}]: skipped {}: {}",
            CODE_MISSING_REQUIRED_FIELDS,
            path,
            message
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{DocValidation, validate_doc_metadata};
    use crate::document::Document;
    use crate::document::metadata_block::{build_frontmatter, get_doc_type};
    use std::path::PathBuf;

    fn make_doc(metadata: Option<okf::Frontmatter>) -> Document {
        Document {
            path: PathBuf::from("/tmp/test.md"),
            metadata,
            content_hash: "abc123".to_string(),
        }
    }

    fn complete_metadata() -> okf::Frontmatter {
        let mut fm = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec![],
            vec![],
        );
        fm.set("generated", okf::Value::Mapping({
            let mut m = okf::yaml::Mapping::new();
            m.insert("by", okf::Value::String("test".to_string()));
            m
        }));
        fm
    }

    #[test]
    fn valid_metadata_returns_valid() {
        let doc = make_doc(Some(complete_metadata()));
        assert!(matches!(validate_doc_metadata(&doc, false), DocValidation::Valid));
    }

    #[test]
    fn missing_metadata_returns_missing() {
        let doc = make_doc(None);
        assert!(matches!(validate_doc_metadata(&doc, false), DocValidation::MissingMetadata));
    }

    #[test]
    fn missing_doc_type_returns_hard_error_when_strict() {
        let mut meta = complete_metadata();
        meta.set("type", okf::Value::Null);
        let doc = make_doc(Some(meta));
        match validate_doc_metadata(&doc, false) {
            DocValidation::HardError(msg) => assert!(msg.contains("missing required fields")),
            other => panic!("expected HardError, got {:?}", other),
        }
    }

    #[test]
    fn missing_doc_type_returns_warning_when_lenient() {
        let mut meta = complete_metadata();
        meta.set("type", okf::Value::Null);
        let doc = make_doc(Some(meta));
        match validate_doc_metadata(&doc, true) {
            DocValidation::InvalidMetadata { warnings } => {
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].1.contains("missing required fields"));
            }
            other => panic!("expected InvalidMetadata, got {:?}", other),
        }
    }

    #[test]
    fn multiple_missing_fields_reported() {
        let meta = build_frontmatter(
            None,
            None,
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let doc = make_doc(Some(meta));
        match validate_doc_metadata(&doc, false) {
            DocValidation::HardError(msg) => {
                assert!(msg.contains("type"));
                assert!(!msg.contains("description"));
            }
            other => panic!("expected HardError, got {:?}", other),
        }
    }
}
