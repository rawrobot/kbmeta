use crate::command::open_db;
use crate::config::Config;
use crate::document::metadata_block::missing_required_fields;
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result, bail};
use rusqlite::OptionalExtension;
use std::path::Path;

const CODE_MISSING_REQUIRED_FIELDS: &str = "KBMETA_MISSING_REQUIRED_FIELDS";
const CODE_DISCOVERY_WARNING: &str = "KBMETA_DISCOVERY_WARNING";

/// Handles `kbmeta scan`.
pub(crate) fn run(root: &Path, config_override: Option<&Path>, skip_invalid: bool) -> Result<()> {
    let cfg = Config::load_with(root, config_override)?;
    let docs_root = resolve_from_root(root, &cfg.documents_root);
    let (docs, warnings) = if skip_invalid {
        let report = crate::document::discovery::discover_documents_lenient(&docs_root)
            .with_context(|| {
                format!(
                    "failed to scan {} [code={}]",
                    docs_root.display(),
                    CODE_DISCOVERY_WARNING
                )
            })?;
        (report.documents, report.warnings)
    } else {
        let docs = crate::document::discovery::discover_documents_strict(&docs_root)
            .with_context(|| {
                format!(
                    "failed to scan {} [code={}]",
                    docs_root.display(),
                    CODE_DISCOVERY_WARNING
                )
            })?;
        (docs, Vec::new())
    };

    let conn = open_db(root)?;
    let mut stale = 0usize;
    let mut missing = 0usize;
    let mut hard_errors = Vec::new();
    let mut validation_warnings = Vec::new();
    let mut scanned = 0usize;

    for doc in docs {
        if doc.metadata.is_none() {
            missing += 1;
            scanned += 1;
        } else if let Some(metadata) = &doc.metadata {
            let required_missing = missing_required_fields(metadata);
            if !required_missing.is_empty() {
                let message = format!(
                    "code={} metadata block near line 1 is missing required fields: {}",
                    CODE_MISSING_REQUIRED_FIELDS,
                    required_missing.join(", ")
                );

                if skip_invalid {
                    validation_warnings.push((doc.path.to_string_lossy().to_string(), message));
                    continue;
                }

                hard_errors.push(format!("{}: {}", doc.path.display(), message));
                continue;
            }

            scanned += 1;
        }

        let stored: Option<String> = conn
            .query_row(
                "SELECT content_hash FROM documents WHERE path = ?1",
                [doc.path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(hash) = stored {
            if hash != doc.content_hash {
                stale += 1;
            }
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

    for warning in warnings {
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

    println!("Scanned {} documents", scanned);
    println!("Missing metadata block: {}", missing);
    println!("Stale index entries: {}", stale);
    println!("Run 'kbmeta rebuild' to refresh SQLite index");
    Ok(())
}
