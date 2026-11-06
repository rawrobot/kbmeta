mod answer;
mod common;
pub mod error;
mod mock;
mod nvidia;
mod openai_api;
mod r#trait;

use crate::config::Config;

pub use mock::MockMetadataProvider;
pub use nvidia::NvidiaProvider;
pub use openai_api::OpenAiApiProvider;
pub use r#trait::{apply_rules, MetadataProvider, Suggestion, SuggestionRules};

/// Builds a metadata provider from runtime configuration.
pub fn build_provider(config: &Config) -> anyhow::Result<Box<dyn MetadataProvider>> {
    let kind = config.provider.kind.trim().to_ascii_lowercase();
    match kind.as_str() {
        "mock" => {
            warn_on_unused_nvidia_opts(&config.provider, "mock");
            Ok(Box::new(MockMetadataProvider))
        }
        "openai-api" => {
            warn_on_unused_nvidia_opts(&config.provider, "openai-api");
            Ok(Box::new(OpenAiApiProvider::new(config.provider.clone())?))
        }
        "nvidia" => Ok(Box::new(NvidiaProvider::new(config.provider.clone())?)),
        other => anyhow::bail!(
            "unsupported provider kind '{}'; expected mock|openai-api|nvidia",
            other
        ),
    }
}

/// Warns (to stderr, always visible) when provider-specific options are set
/// on a provider kind that ignores them, so silent misconfiguration is visible.
fn warn_on_unused_nvidia_opts(config: &crate::config::ProviderConfig, kind: &str) {
    if config.enable_thinking.is_some() || config.reasoning_budget.is_some() {
        eprintln!(
            "warning: provider kind '{}' ignores enable_thinking/reasoning_budget \
             (NVIDIA-only options); set kind = \"nvidia\" to use them",
            kind
        );
    }
}
