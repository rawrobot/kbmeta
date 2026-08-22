use crate::document::metadata_block::parse_markdown;
use crate::document::hashing::sha256_hex;
use crate::document::Document;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Warning emitted when a document is skipped in lenient discovery mode.
#[derive(Debug, Clone)]
pub struct DiscoverWarning {
    /// Source file path that was skipped.
    pub path: PathBuf,
    /// Human-readable metadata parse/read error message.
    pub message: String,
}

/// Output of lenient discovery that keeps valid documents and skipped-file warnings.
#[derive(Debug, Clone)]
pub struct DiscoverReport {
    /// Successfully discovered Markdown documents.
    pub documents: Vec<Document>,
    /// Warnings for documents that were skipped.
    pub warnings: Vec<DiscoverWarning>,
}

/// Recursively discovers Markdown documents under `root`.
///
    /// In strict mode, the first metadata parse/read error fails discovery.
pub fn discover_documents_strict(root: &Path) -> Result<Vec<Document>> {
    let report = discover_impl(root, DiscoverMode::Strict)?;
    Ok(report.documents)
}

/// Recursively discovers Markdown documents under `root`.
///
/// In lenient mode, metadata parse/read errors are converted into warnings and files are skipped.
pub fn discover_documents_lenient(root: &Path) -> Result<DiscoverReport> {
    discover_impl(root, DiscoverMode::Lenient)
}

#[derive(Debug, Clone, Copy)]
enum DiscoverMode {
    Strict,
    Lenient,
}

fn discover_impl(root: &Path, mode: DiscoverMode) -> Result<DiscoverReport> {
    let mut docs = Vec::new();
    let mut warnings = Vec::new();

    if !root.exists() {
        return Ok(DiscoverReport {
            documents: docs,
            warnings,
        });
    }

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".kb")
    {
        let entry = entry.with_context(|| "failed to walk document tree")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        if path.file_name().and_then(|x| x.to_str()) == Some("index.md") {
            continue;
        }

        match load_document(path) {
            Ok(doc) => docs.push(doc),
            Err(err) => match mode {
                DiscoverMode::Strict => {
                    return Err(err);
                }
                DiscoverMode::Lenient => {
                    warnings.push(DiscoverWarning {
                        path: path.to_path_buf(),
                        message: format_error_chain(&err),
                    });
                }
            },
        }
    }

    docs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(DiscoverReport {
        documents: docs,
        warnings,
    })
}

fn load_document(path: &Path) -> Result<Document> {
    let content = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = parse_markdown(&content).with_context(|| format!("failed to parse {}", path.display()))?;

    Ok(Document {
        path: path.to_path_buf(),
        metadata: parsed.metadata,
        content_hash: sha256_hex(&content),
    })
}

fn format_error_chain(err: &anyhow::Error) -> String {
    err.chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(": ")
}

#[cfg(test)]
mod tests {
    use super::{discover_documents_lenient, discover_documents_strict};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn discovers_only_markdown_and_sorts_paths() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();

        fs::create_dir_all(root.join("a")).expect("create dir a");
        fs::create_dir_all(root.join("b")).expect("create dir b");

        fs::write(root.join("b/doc2.md"), "# B").expect("write doc2");
        fs::write(root.join("a/doc1.md"), "# A").expect("write doc1");
        fs::write(root.join("a/readme.txt"), "ignore").expect("write txt");

        let docs = discover_documents_strict(root).expect("discovery should succeed");
        assert_eq!(docs.len(), 2);
        assert!(docs[0].path.ends_with("a/doc1.md"));
        assert!(docs[1].path.ends_with("b/doc2.md"));
    }

    #[test]
    fn strict_mode_returns_error_for_invalid_metadata_block() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();

        let invalid = "---\ntitle: bad\ntags: [rust\n---\n";
        fs::write(root.join("broken.md"), invalid).expect("write invalid markdown");

        let err = discover_documents_strict(root).expect_err("invalid metadata should fail");
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn lenient_mode_skips_invalid_metadata_block_with_warning() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();

        fs::write(root.join("ok.md"), "# ok").expect("write valid markdown");
        let invalid = "---\ntitle: bad\ntags: [rust\n---\n";
        fs::write(root.join("broken.md"), invalid).expect("write invalid markdown");

        let report = discover_documents_lenient(root).expect("lenient discovery should succeed");
        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].path.ends_with("broken.md"));
        assert!(report.warnings[0].message.contains("failed to parse"));
    }
}
