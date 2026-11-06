use crate::config::ProviderConfig;
use crate::provider::common::{
    build_system_prompt, build_user_prompt, normalize_chat_completions_endpoint, send_and_extract,
    ChatMessage, ChatRequest, ChatTemplateKwargs, ResponseFormat,
};
use crate::provider::error::ProviderError;
use crate::provider::r#trait::{MetadataProvider, Suggestion, SuggestionRules};
use anyhow::{Context, Result};
use log::debug;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::path::Path;
use std::time::Duration;

/// NVIDIA default sampling temperature.
const DEFAULT_TEMPERATURE: f32 = 1.0;

/// NVIDIA default top-p nucleus sampling threshold.
const DEFAULT_TOP_P: f32 = 0.95;

/// NVIDIA default maximum response tokens.
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Metadata provider for NVIDIA NIM chat-completions endpoints.
///
/// NVIDIA reasoning models (`nvidia/*` with `nemotron-*` suffixes) accept
/// top-level `chat_template_kwargs.enable_thinking` and a `reasoning_budget`
/// that can exceed `max_tokens`. This provider sends those fields (the SDK's
/// `extra_body` merges into the request root, so they are top-level, not
/// nested) while reusing the same JSON-mode chat-completions flow used by
/// [`crate::provider::OpenAiApiProvider`].
pub struct NvidiaProvider {
    client: Client,
    config: ProviderConfig,
}

impl NvidiaProvider {
    /// Creates an NVIDIA provider with request timeout and optional bearer
    /// auth. The endpoint may be a bare host, a `/v1` base, or a full
    /// `/chat/completions` URL.
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(var) = &config.api_key_env {
            let token = match std::env::var(var) {
                Ok(t) if !t.trim().is_empty() => t,
                Err(_) => return Err(ProviderError::MissingApiKey(var.clone()).into()),
                _ => return Err(ProviderError::MissingApiKey(var.clone()).into()),
            };
            let value = HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|_| ProviderError::InvalidAuth)?;
            headers.insert(AUTHORIZATION, value);
        }

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .with_context(|| "failed to build HTTP client")?;

        Ok(Self { client, config })
    }
}

/// NVIDIA reasoning-capable model markers (substring match, lowercased).
fn is_reasoning_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    [
        "nemotron",
        "thinking",
        "reasoning",
        "super",
        "ultra",
        "lightning",
    ]
    .iter()
    .any(|marker| m.contains(marker))
}

/// Turns NVIDIA reasoning config into top-level request fields, gated on the
/// model actually being reasoning-capable.
///
/// A plain instruct model (e.g. `gpt-oss`, `llama-3.*-instruct`) rejects
/// `chat_template_kwargs`/`reasoning_budget` with HTTP 400, so those are only
/// sent when the model supports reasoning. If the user configured them for a
/// plain model, warn instead of silently ignoring.
fn reasoning_params(
    model: &str,
    enable_thinking: Option<bool>,
    reasoning_budget: Option<u32>,
) -> (Option<ChatTemplateKwargs>, Option<u32>) {
    if !is_reasoning_model(model) {
        if enable_thinking.is_some() || reasoning_budget.is_some() {
            eprintln!(
                "warning: model '{}' is not a reasoning model; \
                 enable_thinking/reasoning_budget ignored",
                model
            );
        }
        return (None, None);
    }

    let kwargs = enable_thinking.map(|enable_thinking| ChatTemplateKwargs { enable_thinking });
    (kwargs, reasoning_budget)
}

impl MetadataProvider for NvidiaProvider {
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<Suggestion> {
        self.suggest_with_system_prompt(path, content, rules, None)
    }

    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> Result<Suggestion> {
        let system = system_prompt
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| build_system_prompt(rules));
        let user = build_user_prompt(path, content, rules);
        let endpoint = normalize_chat_completions_endpoint(&self.config.endpoint);
        if endpoint.trim().is_empty() {
            return Err(ProviderError::InvalidEndpoint(self.config.endpoint.clone()).into());
        }

        debug!(
            "nvidia provider request: model={}, endpoint={}",
            self.config.model, endpoint
        );
        let (chat_template_kwargs, reasoning_budget) = reasoning_params(
            &self.config.model,
            self.config.enable_thinking,
            self.config.reasoning_budget,
        );
        debug!(
            "nvidia reasoning: enable_thinking={:?}, reasoning_budget={:?}",
            chat_template_kwargs.as_ref().map(|k| k.enable_thinking),
            reasoning_budget
        );

        if self.config.structured_output {
            eprintln!(
                "warning: NVIDIA NIM does not support JSON Schema response_format; \
                 falling back to json_object"
            );
        }

        let req = ChatRequest {
            model: self.config.model.clone(),
            temperature: self
                .config
                .temperature
                .unwrap_or(DEFAULT_TEMPERATURE as f64) as f32,
            top_p: self.config.top_p.unwrap_or(DEFAULT_TOP_P as f64) as f32,
            max_tokens: self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            response_format: Some(ResponseFormat::json_object()),
            chat_template_kwargs,
            reasoning_budget,
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user,
                },
            ],
        };

        send_and_extract(&self.client, &endpoint, &req)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_reasoning_model, reasoning_params, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE,
        DEFAULT_TOP_P,
    };

    #[test]
    fn detects_reasoning_models() {
        assert!(is_reasoning_model("nvidia/nemotron-3-ultra-550b-a55b"));
        assert!(is_reasoning_model("nvidia/nemotron-3-super-120b-a12b"));
        assert!(is_reasoning_model("nvidia/nemotron-3.5-lightning-30b-a3b"));
        assert!(!is_reasoning_model("openai/gpt-oss-120b"));
        assert!(!is_reasoning_model("meta/llama-3.1-70b-instruct"));
    }

    #[test]
    fn reasoning_model_sends_top_level_fields() {
        let (kwargs, budget) =
            reasoning_params("nvidia/nemotron-3-ultra-550b-a55b", Some(true), Some(16384));
        assert_eq!(kwargs.expect("kwargs").enable_thinking, true);
        assert_eq!(budget, Some(16384));
    }

    #[test]
    fn plain_model_ignores_reasoning_fields() {
        let (kwargs, budget) = reasoning_params("openai/gpt-oss-120b", Some(true), Some(16384));
        assert!(kwargs.is_none());
        assert!(budget.is_none());
    }

    #[test]
    fn reasoning_model_without_opts_sends_none() {
        let (kwargs, budget) = reasoning_params("nvidia/nemotron-3-ultra-550b-a55b", None, None);
        assert!(kwargs.is_none());
        assert!(budget.is_none());
    }

    #[test]
    fn defaults_match_nvidia_example() {
        assert_eq!(DEFAULT_TEMPERATURE, 1.0);
        assert_eq!(DEFAULT_TOP_P, 0.95);
        assert!(DEFAULT_MAX_TOKENS > 1024);
    }
}
