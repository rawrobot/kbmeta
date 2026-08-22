use crate::document::markdown::{extract_h1, generate_filename};
use crate::pathing::resolve_from_root;
use crate::pipeline::{ContextOperation, DefaultContextOperation};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// Handles `kbmeta context`.
pub(crate) fn run(
    root: &Path,
    input: &Path,
    output: Option<&Path>,
    dry_run: bool,
    verbose: bool,
    backup: bool,
) -> Result<()> {
    eprintln!("warning: 'context' is deprecated; use 'cook' instead");
    let input_path = resolve_from_root(root, input);
    let original = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;

    let builder = DefaultContextOperation;
    let context = builder.run(&original)?;

    let out_dir = output
        .map(|p| resolve_from_root(root, p))
        .unwrap_or_else(|| input_path.parent().unwrap_or(root).to_path_buf());
    let out_path = out_dir.join(make_llm_filename(&input_path, &original));
    let backup_path = out_dir.join(make_backup_filename(&input_path));

    if verbose {
        println!("input: {}", input_path.display());
        println!("llm context: {}", out_path.display());
        println!("size: {} -> {} bytes", original.len(), context.markdown.len());
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
    fs::write(&out_path, context.markdown)
        .with_context(|| format!("failed to write {}", out_path.display()))?;

    if backup {
        fs::write(&backup_path, original)
            .with_context(|| format!("failed to write {}", backup_path.display()))?;
        println!("backup copy: {}", backup_path.display());
    }

    println!("llm context: {}", out_path.display());
    Ok(())
}

fn make_llm_filename(input: &Path, markdown: &str) -> String {
    let title = extract_h1(markdown);
    let clean_md = generate_filename(title.as_deref(), input);
    let stem = Path::new(&clean_md)
        .file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    format!("{}.llm.md", stem)
}

fn make_backup_filename(input: &Path) -> String {
    let stem = input
        .file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    format!("{}.backup.md", stem)
}

#[cfg(test)]
mod tests {
    use super::make_llm_filename;
    use std::path::Path;

    #[test]
    fn context_filename_is_normalized_from_h1() {
        let input = Path::new("25+ Linux Environment Variables Every Hacker Knows.md");
        let md = "# Linux Environment Variables\n\nText";
        let got = make_llm_filename(input, md);
        assert_eq!(got, "Linux_Environment_Variables.llm.md");
    }
}
