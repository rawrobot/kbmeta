use crate::pipeline::{CleanOperation, DefaultCleanOperation};
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// Handles `kbmeta clean`.
pub(crate) fn run(
    root: &Path,
    input: &Path,
    output: Option<&Path>,
    dry_run: bool,
    verbose: bool,
    backup: bool,
) -> Result<()> {
    let input_path = resolve_from_root(root, input);
    let original = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;

    let cleaner = DefaultCleanOperation;
    let clean = cleaner.run(&original, &input_path)?;

    let out_dir = output
        .map(|p| resolve_from_root(root, p))
        .unwrap_or_else(|| root.join("knowledge/documents"));
    let out_path = out_dir.join(&clean.filename);
    let backup_path = out_dir.join(make_backup_filename(&input_path));

    if verbose {
        println!("input: {}", input_path.display());
        println!("detected source: {}", clean.source_type);
        println!("output file: {}", out_path.display());
        println!("size: {} -> {} bytes", original.len(), clean.markdown.len());
    }

    if dry_run {
        println!("dry-run: no files written");
        return Ok(());
    }

    if out_path.exists() {
        bail!("output collision: {} already exists", out_path.display());
    }
    if backup && backup_path.exists() {
        bail!("backup collision: {} already exists", backup_path.display());
    }

    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    fs::write(&out_path, clean.markdown)
        .with_context(|| format!("failed to write {}", out_path.display()))?;

    if backup {
        fs::write(&backup_path, original)
            .with_context(|| format!("failed to write {}", backup_path.display()))?;
        println!("backup copy: {}", backup_path.display());
    }

    println!("clean markdown: {}", out_path.display());
    Ok(())
}

fn make_backup_filename(input: &Path) -> String {
    let stem = input
        .file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    format!("{}.backup.md", stem)
}
