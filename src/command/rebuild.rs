use crate::command::open_db;
use crate::config::Config;
use crate::database::rebuild::RebuildOptions;
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Handles `kbmeta rebuild`.
pub(crate) fn run(
    root: &Path,
    config_override: Option<&Path>,
    strict: bool,
    normalize_tags: bool,
    paths: &[PathBuf],
    dry_run: bool,
) -> Result<()> {
    let cfg = Config::load_with(root, config_override)?;
    let docs_root = resolve_from_root(root, &cfg.documents_root);

    if paths.is_empty() {
        if dry_run {
            println!("dry-run: full rebuild would scan {}", docs_root.display());
            return Ok(());
        }

        let (docs, warnings) = if strict {
            let docs = crate::document::discovery::discover_documents_strict(&docs_root)
                .with_context(|| format!("failed to scan {}", docs_root.display()))?;
            (docs, Vec::new())
        } else {
            let report = crate::document::discovery::discover_documents_lenient(&docs_root)
                .with_context(|| format!("failed to scan {}", docs_root.display()))?;
            (report.documents, report.warnings)
        };

        let mut conn = open_db(root)?;
        crate::database::rebuild::rebuild_index(
            &mut conn,
            &docs,
            RebuildOptions { normalize_tags },
        )?;

        for warning in warnings {
            eprintln!(
                "warning: skipped {}: {}",
                warning.path.display(),
                warning.message
            );
        }
        if normalize_tags {
            println!("Tag mode: normalized for index");
        } else {
            println!("Tag mode: preserved as-is in index");
        }
        println!("Rebuilt index for {} documents", docs.len());
        return Ok(());
    }

    let scope_paths = resolve_scopes(&docs_root, paths);
    if dry_run {
        for scope in &scope_paths {
            println!("dry-run scope: {}", scope.display());
        }
        println!("dry-run: scoped rebuild would use content_hash cache for unchanged docs");
        return Ok(());
    }

    let mut all_docs: BTreeMap<String, crate::document::Document> = BTreeMap::new();
    let mut all_warnings = Vec::new();

    for scope in &scope_paths {
        let (docs, warnings) = if strict {
            let docs = crate::document::discovery::discover_documents_strict(scope)
                .with_context(|| format!("failed to scan {}", scope.display()))?;
            (docs, Vec::new())
        } else {
            let report = crate::document::discovery::discover_documents_lenient(scope)
                .with_context(|| format!("failed to scan {}", scope.display()))?;
            (report.documents, report.warnings)
        };

        for doc in docs {
            all_docs.insert(doc.path.to_string_lossy().to_string(), doc);
        }
        all_warnings.extend(warnings);
    }

    let mut conn = open_db(root)?;
    let doc_values: Vec<crate::document::Document> = all_docs.into_values().collect();
    let scope_refs: Vec<&Path> = scope_paths.iter().map(PathBuf::as_path).collect();
    let summary = crate::database::rebuild::rebuild_index_scoped(
        &mut conn,
        &doc_values,
        &scope_refs,
        RebuildOptions { normalize_tags },
    )?;

    for warning in all_warnings {
        eprintln!(
            "warning: skipped {}: {}",
            warning.path.display(),
            warning.message
        );
    }
    if normalize_tags {
        println!("Tag mode: normalized for index");
    } else {
        println!("Tag mode: preserved as-is in index");
    }
    println!(
        "Scoped rebuild summary: scanned={}, inserted={}, updated={}, removed={}, skipped_unchanged={}",
        summary.scanned,
        summary.inserted,
        summary.updated,
        summary.removed,
        summary.unchanged_skipped
    );
    Ok(())
}

fn resolve_scopes(docs_root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                docs_root.join(p)
            }
        })
        .collect()
}
