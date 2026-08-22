use crate::config::Config;
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Handles `kbmeta index-folders`.
pub(crate) fn run(
    root: &Path,
    config_override: Option<&Path>,
    rewrite: bool,
    dry_run: bool,
) -> Result<()> {
    eprintln!("warning: 'index-folders' is deprecated; use 'index' instead");
    let cfg = Config::load_with(root, config_override)?;
    let docs_root = resolve_from_root(root, &cfg.documents_root);

    let report = crate::document::discovery::discover_documents_lenient(&docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;

    for warning in report.warnings {
        eprintln!("warning: skipped {}: {}", warning.path.display(), warning.message);
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

        by_dir.entry(parent.to_path_buf()).or_default().push(DocRow {
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

        if dry_run {
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

    if dry_run {
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
