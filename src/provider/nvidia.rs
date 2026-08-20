use crate::config::ProviderConfig;
use crate::document::metadata_block::MetadataBlock;
use crate::provider::{MetadataProvider, SuggestionRules};
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Metadata provider implementation for NVIDIA-hosted OpenAI-compatible endpoints.
pub struct NvidiaMetadataProvider {
    client: Client,
    config: ProviderConfig,
}

impl NvidiaMetadataProvider {
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

impl MetadataProvider for NvidiaMetadataProvider {
    fn suggest(&self, path: &Path, content: &str, rules: &SuggestionRules) -> Result<MetadataBlock> {
        self.suggest_with_system_prompt(path, content, rules, None)
    }

    fn suggest_with_system_prompt(
        &self,
        path: &Path,
        content: &str,
        rules: &SuggestionRules,
        system_prompt: Option<&str>,
    ) -> Result<MetadataBlock> {
        let system = system_prompt
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| build_system_prompt(rules));
        let user = build_user_prompt(path, content);
        let endpoint = normalize_chat_completions_endpoint(&self.config.endpoint);

        let req = ChatRequest {
            model: self.config.model.clone(),
            temperature: 0.2,
            top_p: 0.7,
            max_tokens: 1024,
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

        let content = body
            .choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|x| !x.is_empty())
            .ok_or_else(|| anyhow::anyhow!("provider response missing choices[0].message.content"))?;

        let metadata = serde_json::from_str::<MetadataBlock>(&content).with_context(|| {
            format!(
                "provider returned non-JSON metadata payload: {}",
                truncate(&content, 200)
            )
        })?;

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
    content: String,
}

fn build_system_prompt(rules: &SuggestionRules) -> String {
    let preferred_type = rules
        .preferred_doc_type
        .clone()
        .unwrap_or_else(|| "Note".to_string());

    format!(
        "You extract technical knowledge-base metadata. Return JSON only with keys: type, title, description, tags, topics, technologies, languages.\\nUse concise values. Use arrays of strings for list fields.\\nPrefer document type: {}.",
        preferred_type
    )
}

fn build_user_prompt(path: &Path, content: &str) -> String {
    let file_name = path
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document.md".to_string());

    let sample = truncate(content, 12000);
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
