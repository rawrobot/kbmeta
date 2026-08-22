mod mock;
mod openai_api;
mod r#trait;

use crate::config::Config;

pub use r#trait::{MetadataProvider, SuggestionRules, apply_rules, derive_rules};
pub use mock::MockMetadataProvider;
pub use openai_api::OpenAiApiProvider;

/// Builds a metadata provider from runtime configuration.
pub fn build_provider(config: &Config) -> anyhow::Result<Box<dyn MetadataProvider>> {
    match config.provider.kind.trim().to_ascii_lowercase().as_str() {
        "mock" => Ok(Box::new(MockMetadataProvider)),
        "openai-api" | "nvidia" => {
            Ok(Box::new(OpenAiApiProvider::new(config.provider.clone())?))
        }
        other => anyhow::bail!(
            "unsupported provider kind '{}'; expected mock|openai-api",
            other
        ),
    }
}
