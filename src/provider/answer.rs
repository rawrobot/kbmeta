//! Tolerant parsing of provider answer content into a JSON object.
//!
//! Reasoning models routinely emit thinking prose around the metadata payload
//! (or wrap it in a markdown fence). Instead of one rigid parser, every
//! extraction strategy implements [`AnswerParser`] and an [`AnswerPipeline`]
//! runs them in order, returning the first strategy that yields a value.

use serde_json::Value;

/// Extracts a JSON object from raw provider answer content.
///
/// Implementations must be total: returning `None` simply means "not my
/// strategy", so the pipeline can fall through to the next one.
pub(crate) trait AnswerParser {
    fn parse(&self, content: &str) -> Option<Value>;
}

/// Runs strategies in order; first success wins.
pub(crate) struct AnswerPipeline {
    strategies: Vec<Box<dyn AnswerParser>>,
}

impl AnswerPipeline {
    fn default_strategies() -> Self {
        Self {
            strategies: vec![
                Box::new(CleanJson),
                Box::new(FencedJson),
                Box::new(BracedJson::default()),
            ],
        }
    }
}

impl AnswerParser for AnswerPipeline {
    fn parse(&self, content: &str) -> Option<Value> {
        self.strategies
            .iter()
            .find_map(|strategy| strategy.parse(content))
    }
}

/// Convenience: runs the default strategy chain over raw answer content.
pub(crate) fn parse_metadata_json(content: &str) -> Option<Value> {
    AnswerPipeline::default_strategies().parse(content)
}

/// Strategy 1: the whole trimmed content is already a JSON value.
#[derive(Default)]
pub(crate) struct CleanJson;

impl AnswerParser for CleanJson {
    fn parse(&self, content: &str) -> Option<Value> {
        serde_json::from_str::<Value>(content.trim()).ok()
    }
}

/// Strategy 2: the body of the first fenced code block (` ``` `, optionally
/// tagged `json`).
pub(crate) struct FencedJson;

impl AnswerParser for FencedJson {
    fn parse(&self, content: &str) -> Option<Value> {
        serde_json::from_str::<Value>(fenced_block(content)?).ok()
    }
}

/// Strategy 3: scan every `{` for a balanced object and keep the best one.
///
/// "Best" = the candidate carrying the most OKF required fields; ties go to
/// the longer payload. This is the safety net for reasoning output: thinking
/// prose often contains parseable-but-skeletal fragments (e.g. `{"step": 1}`
/// or a stray `{}`), while the real metadata payload carries the full field
/// set.
pub(crate) struct BracedJson {
    required_fields: Vec<&'static str>,
}

impl Default for BracedJson {
    fn default() -> Self {
        Self {
            required_fields: vec![
                "type",
                "title",
                "description",
                "tags",
                "topics",
                "technologies",
                "languages",
                "filename",
                "new_terms",
            ],
        }
    }
}

impl AnswerParser for BracedJson {
    fn parse(&self, content: &str) -> Option<Value> {
        let mut best: Option<(usize, usize, Value)> = None; // (field_score, len, value)
        for candidate in brace_candidates(content) {
            let Ok(value) = serde_json::from_str::<Value>(candidate) else {
                continue;
            };
            let score = value
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .filter(|key| {
                            self.required_fields.iter().any(|field| *field == key.as_str())
                        })
                        .count()
                })
                .unwrap_or(0);
            let len = candidate.len();
            let better = match &best {
                None => true,
                Some((best_score, best_len, _)) => {
                    score > *best_score || (score == *best_score && len > *best_len)
                }
            };
            if better {
                best = Some((score, len, value));
            }
        }
        best.map(|(_, _, value)| value)
    }
}

/// Returns the innards of the first fenced code block (` ``` `), with an
/// optional `json` language tag, or `None` when the content has no fence.
fn fenced_block(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    let start = trimmed.find("```")?;
    let after_open = &trimmed[start + 3..];
    let body = after_open
        .strip_prefix("json")
        .unwrap_or(after_open)
        .trim_start();
    let end = body.find("```")?;
    Some(body[..end].trim())
}

/// Returns the slice from the `{` at byte index `start` to its matching `}`
/// using a balanced scan that respects string literals and escapes.
fn balanced_object_at(text: &str, start: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(&text[start..=i]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Every `{`-anchored balanced object slice in `text`, in source order.
fn brace_candidates(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices('{')
        .filter_map(|(index, _)| balanced_object_at(text, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_accepts_clean_json() {
        let out = parse_metadata_json(r#"{"type":"Note"}"#).expect("clean json");
        assert_eq!(out["type"], "Note");
    }

    #[test]
    fn chain_ignores_leading_prose() {
        let out = parse_metadata_json("Here is the result:\n{\"type\":\"Note\",\"tags\":[\"rust\"]}")
            .expect("json after prose");
        assert_eq!(out["type"], "Note");
    }

    #[test]
    fn chain_ignores_trailing_prose_with_braces() {
        let out = parse_metadata_json("{\"type\":\"Note\"}\nDone, and the schema is {strict} anyway.")
            .expect("json before prose with braces");
        assert_eq!(out["type"], "Note");
    }

    #[test]
    fn chain_preserves_braces_inside_strings() {
        let out = parse_metadata_json(
            "{\"title\":\"use }\",\"description\":\"say {hi} then }\"}\nfinito }",
        )
        .expect("balanced scan");
        assert_eq!(out["title"], "use }");
        assert_eq!(out["description"], "say {hi} then }");
    }

    #[test]
    fn chain_extracts_json_fenced_block() {
        let out = parse_metadata_json("```json\n{\"type\":\"Reference\"}\n```")
            .expect("fenced json");
        assert_eq!(out["type"], "Reference");
    }

    #[test]
    fn chain_extracts_plain_fenced_block() {
        let out = parse_metadata_json("Here you go:\n```\n{\"type\":\"HowTo\"}\n```\nbye")
            .expect("plain fenced block");
        assert_eq!(out["type"], "HowTo");
    }

    #[test]
    fn chain_rejects_noise_without_json() {
        assert!(parse_metadata_json("no json here whatsoever").is_none());
        assert!(parse_metadata_json("").is_none());
    }

    #[test]
    fn picks_complete_object_after_thinking_prose() {
        // Reasoning-model output: a parseable-but-skeletal fragment in the
        // thinking prose, then the real metadata payload.
        let content = concat!(
            "Here's a thinking process:\n",
            "1. **Analyze:** the doc mentions rust. {\"step\": 1}\n",
            "2. Draft a note {\"note_id\": 7} for later.\n",
            r#"{"type":"Note","title":"T","description":"D","tags":["rust"],"topics":[],"technologies":[],"languages":[],"filename":"t","new_terms":{"tags":[],"topics":[],"technologies":[]}}"#,
        );
        let out = parse_metadata_json(content).expect("real payload wins");
        assert_eq!(out["type"], "Note");
        assert_eq!(out["title"], "T");
    }

    #[test]
    fn tolerates_skippable_brace_fragments_before_json() {
        let content = "Steps: {} then {} ok\n{\"type\":\"Note\"}";
        let out = parse_metadata_json(content).expect("json after {} noise");
        assert_eq!(out["type"], "Note");
    }

    #[test]
    fn braced_prefers_full_payload_over_first_hit() {
        let strategy = BracedJson::default();
        let content = concat!(
            r#"{"step":"analyze"}"#,
            "\n",
            r#"{"type":"Reference","title":"T","description":"D","tags":[],"topics":[],"technologies":[],"languages":[],"filename":"t","new_terms":{"tags":[],"topics":[],"technologies":[]}}"#,
        );
        let out = strategy.parse(content).expect("full payload wins");
        assert_eq!(out["type"], "Reference");
        assert!(out.get("step").is_none());
    }
}