use crate::command::prelude::CommandContext;
use crate::config::Config;
use crate::database::rebuild::RebuildOptions;
use crate::document::Document;
use crate::document::discovery::DiscoverWarning;
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Handles `kbmeta index` — SQLite rebuild + folder `index.md` generation.
pub(crate) fn run(ctx: &CommandContext, strict: bool, normalize_tags: bool, paths: &[PathBuf], rewrite: bool) -> Result<()> {
    let cfg = ctx.load_config()?;
    let docs_root = resolve_from_root(ctx.root(), &cfg.documents_root);

    // Phase 1: SQLite index rebuild
    if paths.is_empty() {
        rebuild_full(ctx, &cfg, &docs_root, strict, normalize_tags)?;
    } else {
        rebuild_scoped(ctx, &cfg, &docs_root, paths, strict, normalize_tags)?;
    }

    // Phase 2: folder index.md generation
    generate_folder_indexes(ctx, &docs_root, rewrite)?;

    Ok(())
}

// ── SQLite rebuild ────────────────────────────────────────────────────

fn rebuild_full(
    ctx: &CommandContext,
    cfg: &Config,
    docs_root: &Path,
    strict: bool,
    normalize_tags: bool,
) -> Result<()> {
    if ctx.dry_run() {
        println!("dry-run: full rebuild would scan {}", docs_root.display());
        return Ok(());
    }

    let (docs, warnings) = discover_docs(docs_root, strict)?;
    ctx.rebuild_index(cfg, &docs, RebuildOptions { normalize_tags })?;

    print_warnings(&warnings);
    print_tag_mode(normalize_tags);
    println!("Rebuilt index for {} documents", docs.len());
    Ok(())
}

fn rebuild_scoped(
    ctx: &CommandContext,
    _cfg: &Config,
    docs_root: &Path,
    paths: &[PathBuf],
    strict: bool,
    normalize_tags: bool,
) -> Result<()> {
    let scope_paths = resolve_scopes(docs_root, paths);

    if ctx.dry_run() {
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

    let doc_values: Vec<Document> = all_docs.into_values().collect();
    let scope_refs: Vec<&Path> = scope_paths.iter().map(PathBuf::as_path).collect();
    let mut conn = ctx.open_db()?;
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

// ── Folder index.md generation ────────────────────────────────────────

fn generate_folder_indexes(ctx: &CommandContext, docs_root: &Path, rewrite: bool) -> Result<()> {
    let report = crate::document::discovery::discover_documents_lenient(docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;

    for warning in &report.warnings {
        eprintln!(
            "warning: skipped {}: {}",
            warning.path.display(),
            warning.message
        );
    }

    let mut by_dir: BTreeMap<PathBuf, Vec<DocRow>> = BTreeMap::new();

    for doc in report.documents {
        let Some(file_name) = doc.path.file_name().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if file_name == "index.md" {
            continue;
        }

        let Some(parent) = doc.path.parent() else {
            continue;
        };

        let title = doc
            .metadata
            .as_ref()
            .and_then(|m| m.title().map(|cow| cow.into_owned()))
            .unwrap_or_else(|| file_stem_fallback(&doc.path));
        let doc_type = doc
            .metadata
            .as_ref()
            .and_then(|m| m.type_().map(|cow| cow.into_owned()))
            .unwrap_or_else(|| "Unknown".to_string());
        let description = doc
            .metadata
            .as_ref()
            .and_then(|m| m.description().map(|cow| cow.into_owned()));

        by_dir
            .entry(parent.to_path_buf())
            .or_default()
            .push(DocRow {
                file_name,
                title,
                doc_type,
                description,
            });
    }

    let mut written = 0usize;
    let mut skipped_existing = 0usize;

    for (dir, mut rows) in by_dir {
        rows.sort_by(|a, b| a.title.cmp(&b.title));

        let index_path = dir.join("index.md");
        if index_path.exists() && !rewrite {
            skipped_existing += 1;
            println!("skip existing: {}", index_path.display());
            continue;
        }

        let content = render_index(&rows);

        if ctx.dry_run() {
            println!("dry-run write: {}", index_path.display());
            continue;
        }

        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        fs::write(&index_path, content)
            .with_context(|| format!("failed to write {}", index_path.display()))?;
        written += 1;
        println!("wrote: {}", index_path.display());
    }

    if ctx.dry_run() {
        println!("dry-run complete");
    } else {
        println!("folder indexes written: {}", written);
        if skipped_existing > 0 {
            println!("folder indexes skipped (existing): {}", skipped_existing);
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct DocRow {
    file_name: String,
    title: String,
    doc_type: String,
    description: Option<String>,
}

fn render_index(rows: &[DocRow]) -> String {
    let mut out = String::new();
    out.push_str("# Index\n\n");
    out.push_str("## Documents\n\n");

    for row in rows {
        out.push_str(&format!(
            "- [{}](./{}) ({})",
            escape_md(&row.title),
            row.file_name,
            escape_md(&row.doc_type)
        ));
        if let Some(desc) = row.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            out.push_str(&format!(": {}", escape_md(desc)));
        }
        out.push('\n');
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn escape_md(value: &str) -> String {
    value.replace('\n', " ")
}

fn file_stem_fallback(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string())
}

#[cfg(test)]
mod tests {
    use super::{DocRow, render_index};

    #[test]
    fn renders_index_rows() {
        let rows = vec![DocRow {
            file_name: "a.md".to_string(),
            title: "Doc A".to_string(),
            doc_type: "Reference".to_string(),
            description: Some("desc".to_string()),
        }];

        let out = render_index(&rows);
        assert!(out.contains("# Index"));
        assert!(out.contains("[Doc A](./a.md) (Reference): desc"));
    }
}
