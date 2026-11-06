use crate::document::metadata_block::{frontmatter_from_json, new_terms_from_json};
use crate::provider::error::ProviderError;
use crate::provider::r#trait::{Suggestion, SuggestionRules};
use anyhow::{Context, Result};
use log::{debug, error};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../prompts/default.md");

/// Max chars shown in debug logs for system prompts.
pub(crate) const DEBUG_SYSTEM_PROMPT_LIMIT: usize = 300;

/// Max chars shown in debug logs for user prompts.
pub(crate) const DEBUG_USER_PROMPT_LIMIT: usize = 500;

/// Max chars shown in error messages for provider response content.
const ERROR_CONTENT_LIMIT: usize = 200;

/// Max chars of document content sent to the provider in the user prompt.
const DOCUMENT_SAMPLE_LIMIT: usize = 12000;

/// Max chars of an error response body logged from the provider.
const ERROR_BODY_LIMIT: usize = 500;

#[derive(Debug, Serialize)]
pub(crate) struct ChatRequest {
    pub(crate) model: String,
    pub(crate) temperature: f32,
    pub(crate) top_p: f32,
    pub(crate) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_format: Option<ResponseFormat>,
    // NVIDIA reasoning kwargs, sent as top-level fields (the SDK's
    // `extra_body` merges into the request root, not a nested key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) chat_template_kwargs: Option<ChatTemplateKwargs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_budget: Option<u32>,
    pub(crate) messages: Vec<ChatMessage>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatTemplateKwargs {
    pub(crate) enable_thinking: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseFormat {
    #[serde(rename = "type")]
    pub(crate) format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) json_schema: Option<serde_json::Value>,
}

impl ResponseFormat {
    /// Plain JSON-object mode, supported by every OpenAI-compatible endpoint.
    pub(crate) fn json_object() -> Self {
        Self {
            format_type: "json_object".to_string(),
            json_schema: None,
        }
    }

    /// Structured Outputs / JSON Schema mode with `strict: true`.
    pub(crate) fn json_schema(schema: serde_json::Value) -> Self {
        Self {
            format_type: "json_schema".to_string(),
            json_schema: Some(serde_json::json!({
                "strict": true,
                "name": "okf_metadata",
                "schema": schema,
            })),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatResponse {
    pub(crate) choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoice {
    pub(crate) message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoiceMessage {
    pub(crate) content: Option<String>,
}

/// Builds the default system prompt with the KB-derived preferred document
/// type substituted.
pub(crate) fn build_system_prompt(rules: &SuggestionRules) -> String {
    let preferred_type = rules
        .preferred_doc_type
        .clone()
        .unwrap_or_else(|| "Note".to_string());
    DEFAULT_SYSTEM_PROMPT.replace("{{preferred_type}}", &preferred_type)
}

/// Builds the user prompt from a document path and its content sample, per the
/// OKF metadata-API spec: controlled vocabulary blocks (§1) followed by the
/// MARKDOWN DOCUMENT whose content must be treated as data.
pub(crate) fn build_user_prompt(path: &Path, content: &str, rules: &SuggestionRules) -> String {
    let file_name = path
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document.md".to_string());

    let sample = truncate(content, DOCUMENT_SAMPLE_LIMIT);
    match rules.vocab_text() {
        Some(vocab) if !vocab.trim().is_empty() => format!(
            "{}\n\nMARKDOWN DOCUMENT ({})\n\n{}\n",
            vocab.trim_end(),
            file_name,
            sample
        ),
        _ => format!(
            "Document: {}\n\nContent:\n{}\n\nReturn valid JSON only.",
            file_name, sample
        ),
    }
}

/// Truncates input to `max` chars, appending "..." when cut.
pub(crate) fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect::<String>() + "..."
}

/// Normalizes a chat-completions base URL or full endpoint.
pub(crate) fn normalize_chat_completions_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        return trimmed.to_string();
    }
    if trimmed.ends_with("/v1") {
        return format!("{}/chat/completions", trimmed);
    }
    format!("{}/v1/chat/completions", trimmed)
}

/// Builds the OKF metadata JSON Schema (spec §3) for Structured Outputs.
pub(crate) fn okf_metadata_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "type": {
                "type": "string",
                "enum": ["Note", "Design", "Reference", "HowTo", "Cheatsheet"]
            },
            "title": { "type": "string" },
            "description": { "type": "string" },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "maxItems": 5
            },
            "topics": {
                "type": "array",
                "items": { "type": "string" },
                "maxItems": 3
            },
            "technologies": {
                "type": "array",
                "items": { "type": "string" },
                "maxItems": 6
            },
            "languages": {
                "type": "array",
                "items": { "type": "string" },
                "maxItems": 3
            },
            "filename": {
                "type": "string",
                "maxLength": 50
            },
            "new_terms": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "topics": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "technologies": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["tags", "topics", "technologies"]
            }
        },
        "required": [
            "type",
            "title",
            "description",
            "tags",
            "topics",
            "technologies",
            "languages",
            "filename",
            "new_terms"
        ]
    })
}

/// Parses provider JSON content into a metadata suggestion (frontmatter plus
/// any `new_terms` candidates).
pub(crate) fn extract_metadata(content: &str) -> Result<Suggestion> {
    crate::llm_answers::dump(content);
    debug!(
        "provider content: {}",
        truncate(content, ERROR_CONTENT_LIMIT)
    );

    let json: serde_json::Value = super::answer::parse_metadata_json(content).ok_or_else(|| {
        ProviderError::InvalidMetadata(anyhow::anyhow!(
            "provider returned non-JSON payload: {}",
            truncate(content, ERROR_CONTENT_LIMIT)
        ))
    })?;

    let metadata = frontmatter_from_json(&json).map_err(|e| {
        ProviderError::InvalidMetadata(anyhow::anyhow!(
            "provider returned invalid metadata JSON: {} ({e})",
            truncate(content, ERROR_CONTENT_LIMIT)
        ))
    })?;

    debug!(
        "parsed metadata: type={:?}, title={:?}",
        metadata.type_(),
        metadata.title()
    );

    Ok(Suggestion {
        metadata,
        new_terms: new_terms_from_json(&json),
    })
}

/// Sends a prepared chat-completions request and extracts a suggestion.
pub(crate) fn send_and_extract(
    client: &Client,
    endpoint: &str,
    req: &ChatRequest,
) -> Result<Suggestion> {
    let send_result = client
        .post(endpoint)
        .json(req)
        .send()
        .map_err(|e| {
            error!(
                "provider request to {} failed: {e} (timeout={}, connect={})",
                endpoint,
                e.is_timeout(),
                e.is_connect()
            );
            ProviderError::from_transport(e)
        })
        .and_then(|resp| {
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            let body = resp.text().unwrap_or_default();
            error!(
                "provider returned HTTP {} for {}: {}",
                status.as_u16(),
                endpoint,
                truncate(&body, ERROR_BODY_LIMIT)
            );
            Err(ProviderError::from_status(Some(status), body))
        });

    let resp = send_result
        .with_context(|| format!("request to provider endpoint failed: {}", endpoint))?;

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

    extract_metadata(&content)
}

#[cfg(test)]
mod tests {
    use super::{build_user_prompt, normalize_chat_completions_endpoint, okf_metadata_json_schema};

    #[test]
    fn keeps_full_chat_completions_endpoint() {
        let got = normalize_chat_completions_endpoint(
            "https://integrate.api.nvidia.com/v1/chat/completions",
        );
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

    #[test]
    fn build_user_prompt_embeds_controlled_vocab_blocks() {
        use crate::provider::r#trait::SuggestionRules;
        use crate::vocabulary::VocabKind;
        use crate::vocabulary::Vocabulary;
        use std::fs;
        use std::path::Path;

        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join(".kb").join("vocab");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("tags.yaml"),
            "tags:\n  - canonical: version-control\n    aliases:\n      - git\n",
        )
        .unwrap();
        fs::write(
            sub.join("topics.yaml"),
            "topics:\n  - canonical: tooling\n    aliases:\n      - build-tools\n",
        )
        .unwrap();
        fs::write(
            sub.join("technologies.yaml"),
            "technologies:\n  - canonical: rust\n    aliases:\n      - rust-lang\n",
        )
        .unwrap();
        fs::write(
            sub.join("doc-types.yaml"),
            "doc-types:\n  - canonical: Note\n    aliases: []\n",
        )
        .unwrap();
        fs::write(
            sub.join("languages.yaml"),
            "languages:\n  - canonical: English\n    aliases: []\n",
        )
        .unwrap();

        let vocab = Vocabulary::load(dir.path());
        let rules = SuggestionRules::from_vocabulary(&vocab, None);
        let out = build_user_prompt(Path::new("docs/foo.md"), "# Hello\ncontent here", &rules);

        assert!(out.contains("CONTROLLED TAG VOCABULARY"));
        assert!(out.contains("CONTROLLED TOPIC VOCABULARY"));
        assert!(out.contains("CONTROLLED TECHNOLOGY VOCABULARY"));
        assert!(out.contains("version-control"));
        assert!(out.contains("aliases: git")); // aliases help the model map terms
        assert!(out.contains("MARKDOWN DOCUMENT (foo.md)"));
        assert!(out.contains("# Hello"));
        assert!(out.contains("content here"));
    }

    #[test]
    fn build_user_prompt_falls_back_without_vocab() {
        use crate::provider::r#trait::SuggestionRules;
        use std::path::Path;

        let rules = SuggestionRules::default();
        let out = build_user_prompt(Path::new("docs/foo.md"), "plain content", &rules);
        assert!(out.contains("Document: foo.md"));
        assert!(out.contains("plain content"));
        assert!(out.contains("Return valid JSON only"));
    }

    #[test]
    fn extract_metadata_tolerates_wrapped_json() {
        use super::extract_metadata;
        let suggestion = extract_metadata(
            "Sure, here is the metadata:\n```json\n\
             {\"type\":\"Note\",\"title\":\"T\",\"description\":\"D\",\
             \"tags\":[\"rust\"],\"topics\":[],\"technologies\":[],\
             \"languages\":[],\"filename\":\"t\",\
             \"new_terms\":{\"tags\":[],\"topics\":[],\"technologies\":[]}}\n\
             ```\nHope that helps!",
        )
        .expect("wrapped json should extract");
        assert_eq!(suggestion.metadata.type_().as_deref(), Some("Note"));
    }

    #[test]
    fn json_schema_matches_okf_spec() {
        let schema = okf_metadata_json_schema();
        let obj = schema.as_object().unwrap();
        assert_eq!(obj["additionalProperties"], serde_json::Value::Bool(false));
        assert_eq!(
            obj["properties"]["type"]["enum"],
            serde_json::json!(["Note", "Design", "Reference", "HowTo", "Cheatsheet"])
        );
        assert_eq!(obj["properties"]["tags"]["maxItems"], 5);
        assert_eq!(obj["properties"]["topics"]["maxItems"], 3);
        assert_eq!(obj["properties"]["technologies"]["maxItems"], 6);
        assert_eq!(obj["properties"]["languages"]["maxItems"], 3);
        assert_eq!(obj["properties"]["filename"]["maxLength"], 50);
        let required = obj["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("new_terms")));
        assert!(required.contains(&serde_json::json!("filename")));
    }
}
