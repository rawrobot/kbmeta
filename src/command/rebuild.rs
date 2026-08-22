use crate::command::open_db;
use crate::config::Config;
use crate::database::rebuild::RebuildOptions;
use crate::document::Document;
use crate::document::discovery::DiscoverWarning;
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
    eprintln!("warning: 'rebuild' is deprecated; use 'index' instead");
    let cfg = Config::load_with(root, config_override)?;
    let docs_root = resolve_from_root(root, &cfg.documents_root);

    if paths.is_empty() {
        return run_full(root, &docs_root, strict, normalize_tags, dry_run);
    }

    run_scoped(root, &docs_root, paths, strict, normalize_tags, dry_run)
}

fn run_full(
    root: &Path,
    docs_root: &Path,
    strict: bool,
    normalize_tags: bool,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!("dry-run: full rebuild would scan {}", docs_root.display());
        return Ok(());
    }

    let (docs, warnings) = discover_docs(docs_root, strict)?;
    let mut conn = open_db(root)?;
    crate::database::rebuild::rebuild_index(
        &mut conn,
        &docs,
        RebuildOptions { normalize_tags },
    )?;

    print_warnings(&warnings);
    print_tag_mode(normalize_tags);
    println!("Rebuilt index for {} documents", docs.len());
    Ok(())
}

fn run_scoped(
    root: &Path,
    docs_root: &Path,
    paths: &[PathBuf],
    strict: bool,
    normalize_tags: bool,
    dry_run: bool,
) -> Result<()> {
    let scope_paths = resolve_scopes(docs_root, paths);

    if dry_run {
        for scope in &scope_paths {
            println!("dry-run scope: {}", scope.display());
        }
        println!("dry-run: scoped rebuild would use content_hash cache for unchanged docs");
        return Ok(());
    }

    let mut all_docs: BTreeMap<String, Document> = BTreeMap::new();
    let mut all_warnings = Vec::new();

    for scope in &scope_paths {
        let (docs, warnings) = discover_docs(scope, strict)?;
        for doc in docs {
            all_docs.insert(doc.path.to_string_lossy().to_string(), doc);
        }
        all_warnings.extend(warnings);
    }

    let mut conn = open_db(root)?;
    let doc_values: Vec<Document> = all_docs.into_values().collect();
    let scope_refs: Vec<&Path> = scope_paths.iter().map(PathBuf::as_path).collect();
    let summary = crate::database::rebuild::rebuild_index_scoped(
        &mut conn,
        &doc_values,
        &scope_refs,
        RebuildOptions { normalize_tags },
    )?;

    print_warnings(&all_warnings);
    print_tag_mode(normalize_tags);
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

fn discover_docs(
    docs_root: &Path,
    strict: bool,
) -> Result<(Vec<Document>, Vec<DiscoverWarning>)> {
    if strict {
        let docs = crate::document::discovery::discover_documents_strict(docs_root)
            .with_context(|| format!("failed to scan {}", docs_root.display()))?;
        Ok((docs, Vec::new()))
    } else {
        let report = crate::document::discovery::discover_documents_lenient(docs_root)
            .with_context(|| format!("failed to scan {}", docs_root.display()))?;
        Ok((report.documents, report.warnings))
    }
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

fn print_warnings(warnings: &[DiscoverWarning]) {
    for warning in warnings {
        eprintln!(
            "warning: skipped {}: {}",
            warning.path.display(),
            warning.message
        );
    }
}

fn print_tag_mode(normalize_tags: bool) {
    if normalize_tags {
        println!("Tag mode: normalized for index");
    } else {
        println!("Tag mode: preserved as-is in index");
    }
}
