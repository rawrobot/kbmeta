use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// OKF-style YAML metadata fields supported in Phase 1.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetadataBlock {
    /// Document classification stored under YAML key `type`.
    #[serde(rename = "type")]
    pub doc_type: Option<String>,
    /// Human-readable title.
    pub title: Option<String>,
    /// Short document description.
    pub description: Option<String>,
    /// Freeform keyword labels.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Canonical topic labels.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Technology names mentioned by the document.
    #[serde(default)]
    pub technologies: Vec<String>,
    /// Human/programming languages related to the document.
    #[serde(default)]
    pub languages: Vec<String>,
}

/// Result of parsing Markdown content for metadata.
#[derive(Debug, Clone)]
pub struct ParsedMarkdown {
    /// Parsed metadata block, present only when a valid YAML header exists.
    pub metadata: Option<MetadataBlock>,
}

/// Returns names of required metadata fields that are missing or blank.
pub fn missing_required_fields(metadata: &MetadataBlock) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if is_missing(&metadata.doc_type) {
        missing.push("type");
    }
    if is_missing(&metadata.title) {
        missing.push("title");
    }
    if is_missing(&metadata.description) {
        missing.push("description");
    }

    missing
}

fn is_missing(value: &Option<String>) -> bool {
    value
        .as_ref()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
}

/// Parses Markdown and extracts a leading YAML metadata block.
///
/// If no metadata block is present, returns `metadata: None` without error.
pub fn parse_markdown(input: &str) -> Result<ParsedMarkdown> {
    let mut lines = input.lines();
    let Some(first) = lines.next() else {
        return Ok(ParsedMarkdown { metadata: None });
    };

    if first.trim() != "---" {
        return Ok(ParsedMarkdown { metadata: None });
    }

    let mut yaml_lines = Vec::new();
    let mut found_closing_delimiter = false;

    for line in lines {
        if line.trim() == "---" {
            found_closing_delimiter = true;
            break;
        }
        yaml_lines.push(line);
    }

    if !found_closing_delimiter {
        bail!(
            "unterminated YAML metadata block starting at line 1: missing closing '---' delimiter"
        );
    }

    let yaml = yaml_lines.join("\n");
    let fm = match serde_yaml::from_str::<MetadataBlock>(&yaml) {
        Ok(fm) => fm,
        Err(err) => {
            if let Some(loc) = err.location() {
                // YAML parsing starts after the first delimiter line, so shift by +1.
                let doc_line = loc.line() + 1;
                bail!(
                    "invalid YAML metadata block at line {}, column {}: {}",
                    doc_line,
                    loc.column(),
                    err
                );
            }
            bail!("invalid YAML metadata block: {}", err);
        }
    };

    Ok(ParsedMarkdown { metadata: Some(fm) })
}

/// Serializes metadata to a YAML string suitable for human review.
pub fn to_yaml(metadata: &MetadataBlock) -> Result<String> {
    Ok(serde_yaml::to_string(metadata)?)
}

/// Replaces or inserts a leading metadata block in Markdown content.
pub fn upsert_metadata_block(input: &str, metadata: &MetadataBlock) -> Result<String> {
    let body = match find_metadata_block_end(input)? {
        Some(end) => &input[end..],
        None => input,
    };

    let mut yaml = serde_yaml::to_string(metadata)?;
    if let Some(stripped) = yaml.strip_prefix("---\n") {
        yaml = stripped.to_string();
    }

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&yaml);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---\n");

    if !body.is_empty() && !body.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(body);

    Ok(out)
}

fn find_metadata_block_end(input: &str) -> Result<Option<usize>> {
    let mut offset = 0usize;
    let mut chunks = input.split_inclusive('\n');
    let Some(first) = chunks.next() else {
        return Ok(None);
    };

    if first.trim() != "---" {
        return Ok(None);
    }
    offset += first.len();

    for chunk in chunks {
        offset += chunk.len();
        if chunk.trim() == "---" {
            return Ok(Some(offset));
        }
    }

    bail!(
        "unterminated YAML metadata block starting at line 1: missing closing '---' delimiter"
    )
}

#[cfg(test)]
mod tests {
    use super::{MetadataBlock, missing_required_fields, parse_markdown, upsert_metadata_block};

    #[test]
    fn parses_valid_metadata_block() {
        let md = r#"---
type: Reference
title: Linux UIO
description: Userspace interface.
tags:
  - linux
topics:
  - operating-systems
technologies:
  - Linux
languages:
  - rust
---

# Linux UIO
"#;

        let parsed = parse_markdown(md).expect("metadata block should parse");
        let fm = parsed
            .metadata
            .expect("metadata block should be present for valid YAML header");

        assert_eq!(fm.doc_type.as_deref(), Some("Reference"));
        assert_eq!(fm.title.as_deref(), Some("Linux UIO"));
        assert_eq!(fm.tags, vec!["linux"]);
        assert_eq!(fm.topics, vec!["operating-systems"]);
        assert_eq!(fm.technologies, vec!["Linux"]);
        assert_eq!(fm.languages, vec!["rust"]);
    }

    #[test]
    fn returns_none_when_no_metadata_block() {
        let md = "# Document\nNo YAML header here.";

        let parsed = parse_markdown(md).expect("parser should not fail without header");
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn returns_none_for_empty_document() {
        let parsed = parse_markdown("").expect("empty input should parse");
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn fails_for_unterminated_metadata_block() {
        let md = r#"---
title: Broken
tags:
  - rust
"#;

        let err = parse_markdown(md).expect_err("unterminated header should fail");
        assert!(err.to_string().contains("unterminated YAML metadata block"));
        assert!(err.to_string().contains("line 1"));
    }

    #[test]
    fn fails_for_invalid_yaml_metadata_block() {
        let md = r#"---
title: Broken
tags: [rust
---
"#;

        let err = parse_markdown(md).expect_err("invalid YAML should fail");
        assert!(err.to_string().contains("invalid YAML metadata block"));
        assert!(err.to_string().contains("line"));
        assert!(err.to_string().contains("column"));
    }

    #[test]
    fn supports_crlf_newlines() {
        let md = "---\r\ntitle: CRLF\r\ntags:\r\n  - rust\r\n---\r\n\r\n# Body\r\n";

        let parsed = parse_markdown(md).expect("CRLF should parse");
        let fm = parsed.metadata.expect("metadata block should be present");
        assert_eq!(fm.title.as_deref(), Some("CRLF"));
        assert_eq!(fm.tags, vec!["rust"]);
    }

    #[test]
    fn allows_body_separators_without_metadata_block() {
        let md = r#"# Notes

Section A
---
Section B
---
Section C
"#;

        let parsed = parse_markdown(md).expect("body separators should not fail parsing");
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn allows_body_separators_after_valid_metadata_block() {
        let md = r#"---
title: Doc with breaks
tags:
  - rust
---

Part 1
---
Part 2
"#;

        let parsed = parse_markdown(md).expect("document should parse");
        let fm = parsed.metadata.expect("metadata block should be present");
        assert_eq!(fm.title.as_deref(), Some("Doc with breaks"));
        assert_eq!(fm.tags, vec!["rust"]);
    }

    #[test]
    fn reports_missing_required_fields() {
        let md = r#"---
title: "   "
tags:
  - rust
---
"#;

        let parsed = parse_markdown(md).expect("document should parse");
        let metadata = parsed.metadata.expect("metadata should be present");
        let missing = missing_required_fields(&metadata);

        assert_eq!(missing, vec!["type", "title", "description"]);
    }

    #[test]
    fn required_fields_pass_when_populated() {
        let md = r#"---
type: Note
title: Complete
description: Non-empty
---
"#;

        let parsed = parse_markdown(md).expect("document should parse");
        let metadata = parsed.metadata.expect("metadata should be present");
        let missing = missing_required_fields(&metadata);

        assert!(missing.is_empty());
    }

    #[test]
    fn upserts_metadata_block_for_document_without_metadata() {
        let input = "# Title\n\nBody\n";
        let metadata = MetadataBlock {
            doc_type: Some("Note".to_string()),
            title: Some("Title".to_string()),
            description: Some("Desc".to_string()),
            tags: vec!["rust".to_string()],
            topics: vec![],
            technologies: vec![],
            languages: vec![],
        };

        let out = upsert_metadata_block(input, &metadata).expect("upsert should succeed");
        assert!(out.starts_with("---\n"));
        assert!(out.contains("title: Title"));
        assert!(out.contains("# Title"));
    }

    #[test]
    fn upserts_metadata_block_for_document_with_existing_metadata() {
        let input = "---\ntitle: Old\n---\n\n# Body\n";
        let metadata = MetadataBlock {
            doc_type: Some("Note".to_string()),
            title: Some("New".to_string()),
            description: Some("Desc".to_string()),
            tags: vec![],
            topics: vec![],
            technologies: vec![],
            languages: vec![],
        };

        let out = upsert_metadata_block(input, &metadata).expect("upsert should succeed");
        assert!(!out.contains("title: Old"));
        assert!(out.contains("title: New"));
        assert!(out.contains("# Body"));
    }

    #[test]
    fn preserves_numbered_headings_without_escaping() {
        let input = "# Title\n\n## 1. bla\n\nText\n";
        let metadata = MetadataBlock {
            doc_type: Some("Note".to_string()),
            title: Some("Title".to_string()),
            description: Some("Desc".to_string()),
            tags: vec![],
            topics: vec![],
            technologies: vec![],
            languages: vec![],
        };

        let out = upsert_metadata_block(input, &metadata).expect("upsert should succeed");
        assert!(out.contains("## 1. bla"));
        assert!(!out.contains("## 1\\. bla"));
    }
}
