use crate::config::ProviderConfig;
use crate::document::metadata_block::frontmatter_from_json;
use crate::provider::r#trait::{MetadataProvider, SuggestionRules};
use anyhow::{Context, Result};
use log::debug;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Default sampling temperature when user hasn't configured one.
/// Lower = more deterministic; higher = more creative.
const DEFAULT_TEMPERATURE: f32 = 0.2;

/// Default top-p nucleus sampling threshold when user hasn't configured one.
/// Controls diversity via cumulative probability mass.
const DEFAULT_TOP_P: f32 = 0.7;

/// Default maximum response tokens when user hasn't configured one.
const DEFAULT_MAX_TOKENS: u32 = 1024;

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../prompts/default.md");

/// Max chars shown in debug logs for system prompts.
const DEBUG_SYSTEM_PROMPT_LIMIT: usize = 300;

/// Max chars shown in debug logs for user prompts.
const DEBUG_USER_PROMPT_LIMIT: usize = 500;

/// Max chars shown in debug logs for provider response content.
const DEBUG_CONTENT_LIMIT: usize = 500;

/// Max chars shown in error messages for provider response content.
const ERROR_CONTENT_LIMIT: usize = 200;

/// Max chars of document content sent to the provider in the user prompt.
const DOCUMENT_SAMPLE_LIMIT: usize = 12000;

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
            let token = std::env::var(var)
                .with_context(|| format!("failed to read api key from env var {}", var))?;
            let value = HeaderValue::from_str(&format!("Bearer {}", token))
                .with_context(|| "invalid bearer token header value")?;
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
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<okf::Frontmatter> {
        self.suggest_with_system_prompt(path, content, rules, None)
    }

    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> Result<okf::Frontmatter> {
        let system = system_prompt
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| build_system_prompt(rules));
        let user = build_user_prompt(path, content);
        let endpoint = normalize_chat_completions_endpoint(&self.config.endpoint);

        debug!("provider request: model={}, endpoint={}", self.config.model, endpoint);
        debug!("system prompt: {}", truncate(&system, DEBUG_SYSTEM_PROMPT_LIMIT));
        debug!("user prompt (truncated): {}", truncate(&user, DEBUG_USER_PROMPT_LIMIT));

        let req = ChatRequest {
            model: self.config.model.clone(),
            temperature: self.config.temperature.unwrap_or(DEFAULT_TEMPERATURE as f64) as f32,
            top_p: self.config.top_p.unwrap_or(DEFAULT_TOP_P as f64) as f32,
            max_tokens: self.config.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
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

        let resp = self
            .client
            .post(&endpoint)
            .json(&req)
            .send()
            .with_context(|| format!("request to provider endpoint failed: {}", endpoint))?
            .error_for_status()
            .with_context(|| "provider returned non-success status")?;

        let body = resp
            .json::<ChatResponse>()
            .with_context(|| "failed to parse provider response")?;

        debug!("provider response: {} choices", body.choices.len());

        let content = body
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("provider response missing choices[0].message.content"))?;

        debug!("provider content: {}", truncate(&content, DEBUG_CONTENT_LIMIT));

        let json: serde_json::Value = serde_json::from_str(&content).with_context(|| {
            format!(
                "provider returned non-JSON payload: {}",
                truncate(&content, ERROR_CONTENT_LIMIT)
            )
        })?;

        let metadata = frontmatter_from_json(&json).with_context(|| {
            format!(
                "provider returned invalid metadata JSON: {}",
                truncate(&content, ERROR_CONTENT_LIMIT)
            )
        })?;

        debug!("parsed metadata: type={:?}, title={:?}", metadata.type_(), metadata.title());

        Ok(metadata)
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    temperature: f32,
    top_p: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

fn build_system_prompt(rules: &SuggestionRules) -> String {
    let preferred_type = rules
        .preferred_doc_type
        .clone()
        .unwrap_or_else(|| "Note".to_string());

    DEFAULT_SYSTEM_PROMPT.replace("{{preferred_type}}", &preferred_type)
}

fn build_user_prompt(path: &Path, content: &str) -> String {
    let file_name = path
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document.md".to_string());

    let sample = truncate(content, DOCUMENT_SAMPLE_LIMIT);
    format!(
        "Document: {}\\n\\nContent:\\n{}\\n\\nReturn valid JSON only.",
        file_name, sample
    )
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect::<String>() + "..."
}

fn normalize_chat_completions_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        return trimmed.to_string();
    }
    if trimmed.ends_with("/v1") {
        return format!("{}/chat/completions", trimmed);
    }
    format!("{}/v1/chat/completions", trimmed)
}

#[cfg(test)]
mod tests {
    use super::normalize_chat_completions_endpoint;

    #[test]
    fn keeps_full_chat_completions_endpoint() {
        let got = normalize_chat_completions_endpoint("https://integrate.api.nvidia.com/v1/chat/completions");
        assert_eq!(got, "https://integrate.api.nvidia.com/v1/chat/completions");
    }

    #[test]
    fn appends_chat_completions_to_v1_base_url() {
        let got = normalize_chat_completions_endpoint("https://integrate.api.nvidia.com/v1");
        assert_eq!(got, "https://integrate.api.nvidia.com/v1/chat/completions");
    }

    #[test]
    fn appends_v1_chat_completions_to_host_url() {
        let got = normalize_chat_completions_endpoint("https://integrate.api.nvidia.com");
        assert_eq!(got, "https://integrate.api.nvidia.com/v1/chat/completions");
    }
}
