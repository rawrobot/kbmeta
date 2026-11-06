use crate::provider::error::Retry;
use std::path::PathBuf;
use thiserror::Error;

/// Dedicated error type for the cook pipeline subsystem.
///
/// Every variant carries its own retry classification so the CLI can branch
/// on it without pattern-matching message strings.
#[derive(Debug, Error)]
pub enum CookError {
    /// The provider suggested metadata that is missing required fields.
    /// Permanent: re-running the provider is unlikely to help, and the input
    /// or prompt is the problem.
    #[error("suggested metadata is invalid: missing required fields: {missing}")]
    InvalidSuggestedMetadata { missing: String },

    /// The computed output path already exists and batch mode must not
    /// overwrite it. Permanent: requires a human decision (`--rewrite`).
    #[error("output collision: {path} already exists (use --rewrite to overwrite)")]
    OutputCollision { path: PathBuf },

    /// The metadata block could not be merged into the body.
    #[error("failed to merge metadata into the body: {0}")]
    UpsertMetadata(#[source] anyhow::Error),
}

impl CookError {
    /// The retry classification for this failure.
    pub fn retry(&self) -> Retry {
        match self {
            CookError::InvalidSuggestedMetadata { .. }
            | CookError::OutputCollision { .. }
            | CookError::UpsertMetadata(_) => Retry::Permanent,
        }
    }
}
