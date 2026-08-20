use std::path::{Path, PathBuf};

use anyhow::Result;

/// Handles `kbmeta index`.
pub(crate) fn run(
    root: &Path,
    config_override: Option<&Path>,
    strict: bool,
    normalize_tags: bool,
    paths: &[PathBuf],
    rewrite: bool,
    dry_run: bool,
) -> Result<()> {
    crate::command::rebuild::run(root, config_override, strict, normalize_tags, paths, dry_run)?;
    crate::command::index_folders::run(root, config_override, rewrite, dry_run)?;
    Ok(())
}
