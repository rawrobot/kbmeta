use std::path::{Path, PathBuf};

/// Resolves a path against workspace root when it is relative.
pub fn resolve_from_root(root: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        root.join(value)
    }
}
