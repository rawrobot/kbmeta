use crate::command::prelude::{CommandContext, backup_and_write};
use crate::pipeline::{CleanOperation, DefaultCleanOperation};
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Handles `kbmeta clean`.
pub(crate) fn run(
    ctx: &CommandContext,
    input: &Path,
    output: Option<&Path>,
    verbose: bool,
    backup: bool,
) -> Result<()> {
    let input_path = resolve_from_root(ctx.root(), input);
    let original = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;

    let cleaner = DefaultCleanOperation;
    let clean = cleaner.run(&original, &input_path)?;

    let out_dir = output
        .map(|p| resolve_from_root(ctx.root(), p))
        .unwrap_or_else(|| ctx.root().join("knowledge/documents"));
    let out_path = out_dir.join(&clean.filename);

    if verbose {
        println!("input: {}", input_path.display());
        println!("detected source: {}", clean.source_type);
        println!("output file: {}", out_path.display());
        println!("size: {} -> {} bytes", original.len(), clean.markdown.len());
    }

    if ctx.dry_run() {
        println!("dry-run: no files written");
        return Ok(());
    }

    backup_and_write(&out_path, backup, &clean.markdown, &original)?;
    println!("clean markdown: {}", out_path.display());
    Ok(())
}
