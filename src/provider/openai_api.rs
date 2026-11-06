use crate::config::ProviderConfig;
use crate::provider::common::{
    build_system_prompt, build_user_prompt, normalize_chat_completions_endpoint,
    okf_metadata_json_schema, send_and_extract, truncate, ChatMessage, ChatRequest, ResponseFormat,
    DEBUG_SYSTEM_PROMPT_LIMIT, DEBUG_USER_PROMPT_LIMIT,
};
use crate::provider::error::ProviderError;
use crate::provider::r#trait::{MetadataProvider, Suggestion, SuggestionRules};
use anyhow::{Context, Result};
use log::debug;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::path::Path;
use std::time::Duration;

/// Default sampling temperature when user hasn't configured one.
/// Lower = more deterministic; higher = more creative.
const DEFAULT_TEMPERATURE: f32 = 0.2;

/// Default top-p nucleus sampling threshold when user hasn't configured one.
/// Controls diversity via cumulative probability mass.
const DEFAULT_TOP_P: f32 = 0.7;

/// Default maximum response tokens when user hasn't configured one.
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Metadata provider for OpenAI-compatible chat completions endpoints.
pub struct OpenAiApiProvider {
    client: Client,
    config: ProviderConfig,
}

impl OpenAiApiProvider {
    /// Creates a provider with request timeout and optional bearer auth.
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

impl MetadataProvider for OpenAiApiProvider {
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
            "provider request: model={}, endpoint={}",
            self.config.model, endpoint
        );
        debug!(
            "system prompt: {}",
            truncate(&system, DEBUG_SYSTEM_PROMPT_LIMIT)
        );
        debug!(
            "user prompt (truncated): {}",
            truncate(&user, DEBUG_USER_PROMPT_LIMIT)
        );

        let response_format = if self.config.structured_output {
            ResponseFormat::json_schema(okf_metadata_json_schema())
        } else {
            ResponseFormat::json_object()
        };

        let req = ChatRequest {
            model: self.config.model.clone(),
            temperature: self
                .config
                .temperature
                .unwrap_or(DEFAULT_TEMPERATURE as f64) as f32,
            top_p: self.config.top_p.unwrap_or(DEFAULT_TOP_P as f64) as f32,
            max_tokens: self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            response_format: Some(response_format),
            chat_template_kwargs: None,
            reasoning_budget: None,
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
