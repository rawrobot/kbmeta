/// Source-specific markdown cleaners producing clear markdown documents.
pub mod cleaner;
/// Markdown document discovery and loading utilities.
pub mod discovery;
/// YAML metadata block parsing types and functions.
pub mod metadata_block;
/// Markdown helpers for source detection and filename generation.
pub mod markdown;
/// Deterministic builder for reduced metadata-LLM context markdown.
pub mod llm_context;
/// Content hashing helpers used for stale-index detection.
pub mod hashing;

use std::path::PathBuf;

/// Canonical in-memory representation of a scanned Markdown document.
#[derive(Debug, Clone)]
pub struct Document {
    /// Absolute or workspace-relative path to the source Markdown file.
    pub path: PathBuf,
    /// Parsed YAML metadata block, if present and valid.
    pub metadata: Option<metadata_block::MetadataBlock>,
    /// SHA-256 hash of full file content for change detection.
    pub content_hash: String,
}
