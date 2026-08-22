use anyhow::Result;
use okf::Value;

const KEY_TOPICS: &str = "topics";
const KEY_TECHNOLOGIES: &str = "technologies";
const KEY_LANGUAGES: &str = "languages";

pub fn get_string_list(fm: &okf::Frontmatter, key: &str) -> Vec<String> {
    match fm.get(key) {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| match *v {
                Value::String(ref s) => {
                    let t = s.trim().to_string();
                    if t.is_empty() { None } else { Some(t) }
                }
                _ => v.as_display_string().map(|s| {
                    let t = s.trim().to_string();
                    t
                }),
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Some(v) => v
            .as_display_string()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { vec![] } else { vec![t] }
            })
            .unwrap_or_default(),
        _ => vec![],
    }
}

pub fn set_string_list(fm: &mut okf::Frontmatter, key: &str, values: Vec<String>) {
    let seq: Vec<Value> = values.into_iter().map(Value::String).collect();
    fm.set(key, Value::Sequence(seq));
}

pub fn get_topics(fm: &okf::Frontmatter) -> Vec<String> {
    get_string_list(fm, KEY_TOPICS)
}

pub fn set_topics(fm: &mut okf::Frontmatter, values: Vec<String>) {
    set_string_list(fm, KEY_TOPICS, values);
}

pub fn get_technologies(fm: &okf::Frontmatter) -> Vec<String> {
    get_string_list(fm, KEY_TECHNOLOGIES)
}

pub fn set_technologies(fm: &mut okf::Frontmatter, values: Vec<String>) {
    set_string_list(fm, KEY_TECHNOLOGIES, values);
}

pub fn get_languages(fm: &okf::Frontmatter) -> Vec<String> {
    get_string_list(fm, KEY_LANGUAGES)
}

pub fn set_languages(fm: &mut okf::Frontmatter, values: Vec<String>) {
    set_string_list(fm, KEY_LANGUAGES, values);
}

pub fn get_doc_type(fm: &okf::Frontmatter) -> Option<String> {
    fm.type_().map(|cow| cow.into_owned())
}

pub fn get_title(fm: &okf::Frontmatter) -> Option<String> {
    fm.title().map(|cow| cow.into_owned())
}

pub fn get_description(fm: &okf::Frontmatter) -> Option<String> {
    fm.description().map(|cow| cow.into_owned())
}

pub fn build_frontmatter(
    doc_type: Option<String>,
    title: Option<String>,
    description: Option<String>,
    tags: Vec<String>,
    topics: Vec<String>,
    technologies: Vec<String>,
    languages: Vec<String>,
) -> okf::Frontmatter {
    let mut fm = okf::Frontmatter::new();
    if let Some(t) = doc_type {
        fm.set("type", Value::String(t));
    }
    if let Some(t) = title {
        fm.set("title", Value::String(t));
    }
    if let Some(d) = description {
        fm.set("description", Value::String(d));
    }
    fm.set(
        "tags",
        Value::Sequence(tags.into_iter().map(Value::String).collect()),
    );
    set_topics(&mut fm, topics);
    set_technologies(&mut fm, technologies);
    set_languages(&mut fm, languages);
    fm
}

#[derive(Debug, Clone)]
pub struct ParsedMarkdown {
    pub metadata: Option<okf::Frontmatter>,
}

pub fn missing_required_fields(fm: &okf::Frontmatter) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if fm.type_().map(|v| v.trim().is_empty()).unwrap_or(true) {
        missing.push("type");
    }

    missing
}

pub fn missing_recommended_fields(fm: &okf::Frontmatter) -> Vec<&'static str> {
    let doc = okf::Document::new(fm.clone(), "");
    doc.missing_recommended()
}

pub fn parse_markdown(input: &str) -> Result<ParsedMarkdown> {
    let doc = okf::Document::parse(input).map_err(|e| anyhow::anyhow!("{}", e))?;
    if doc.frontmatter.is_empty() {
        return Ok(ParsedMarkdown { metadata: None });
    }
    Ok(ParsedMarkdown {
        metadata: Some(doc.frontmatter),
    })
}

fn serialize_frontmatter_yaml(fm: &okf::Frontmatter) -> String {
    let mut fm = fm.clone();
    fm.reorder_preferred();
    let doc = okf::Document::new(fm, "");
    let serialized = doc.serialize();
    let mut yaml_lines = Vec::new();
    let mut in_yaml = false;
    for line in serialized.lines() {
        if line.trim() == "---" {
            if in_yaml {
                break;
            }
            in_yaml = true;
            continue;
        }
        if in_yaml {
            yaml_lines.push(line);
        }
    }
    yaml_lines.join("\n")
}

pub fn to_yaml(fm: &okf::Frontmatter) -> Result<String> {
    let mut out = serialize_frontmatter_yaml(fm);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

pub fn upsert_metadata_block(input: &str, fm: &okf::Frontmatter) -> Result<String> {
    let body = find_body_after_metadata(input)?;

    let yaml = serialize_frontmatter_yaml(fm);

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
    out.push_str(&body);

    Ok(out)
}

fn find_body_after_metadata(input: &str) -> Result<String> {
    let doc = okf::Document::parse(input).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(doc.body)
}

pub fn frontmatter_from_json(json: &serde_json::Value) -> Result<okf::Frontmatter> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("provider response is not a JSON object"))?;

    let mut fm = okf::Frontmatter::new();

    if let Some(v) = obj.get("type").and_then(|v| v.as_str()) {
        let t = v.trim().to_string();
        if !t.is_empty() {
            fm.set("type", Value::String(t));
        }
    }
    if let Some(v) = obj.get("title").and_then(|v| v.as_str()) {
        let t = v.trim().to_string();
        if !t.is_empty() {
            fm.set("title", Value::String(t));
        }
    }
    if let Some(v) = obj.get("description").and_then(|v| v.as_str()) {
        let d = v.trim().to_string();
        if !d.is_empty() {
            fm.set("description", Value::String(d));
        }
    }

    let tags = json_string_array(obj, "tags");
    let topics = json_string_array(obj, "topics");
    let technologies = json_string_array(obj, "technologies");
    let languages = json_string_array(obj, "languages");

    fm.set(
        "tags",
        Value::Sequence(tags.into_iter().map(Value::String).collect()),
    );
    set_topics(&mut fm, topics);
    set_technologies(&mut fm, technologies);
    set_languages(&mut fm, languages);

    Ok(fm)
}

fn json_string_array(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Vec<String> {
    match obj.get(key) {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(serde_json::Value::String(s)) => {
            let t = s.trim().to_string();
            if t.is_empty() { vec![] } else { vec![t] }
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_metadata_block() {
        let md = "---\ntype: Reference\ntitle: Linux UIO\ndescription: Userspace interface.\ntags:\n  - linux\ntopics:\n  - operating-systems\ntechnologies:\n  - Linux\nlanguages:\n  - rust\n---\n\n# Linux UIO\n";

        let parsed = parse_markdown(md).expect("metadata block should parse");
        let fm = parsed.metadata.expect("metadata block should be present");

        assert_eq!(get_doc_type(&fm).as_deref(), Some("Reference"));
        assert_eq!(get_title(&fm).as_deref(), Some("Linux UIO"));
        assert_eq!(fm.tags(), vec!["linux"]);
        assert_eq!(get_topics(&fm), vec!["operating-systems"]);
        assert_eq!(get_technologies(&fm), vec!["Linux"]);
        assert_eq!(get_languages(&fm), vec!["rust"]);
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
        let md = "---\ntitle: Broken\ntags:\n  - rust\n";
        let err = parse_markdown(md).expect_err("unterminated header should fail");
        assert!(err.to_string().contains("Unterminated"));
    }

    #[test]
    fn reports_missing_required_fields() {
        let md = "---\ntags:\n  - rust\n---\n";
        let parsed = parse_markdown(md).expect("document should parse");
        let fm = parsed.metadata.expect("metadata should be present");
        let missing = missing_required_fields(&fm);
        assert_eq!(missing, vec!["type"]);
    }

    #[test]
    fn required_fields_pass_when_populated() {
        let md = "---\ntype: Note\ntitle: Complete\ndescription: Non-empty\n---\n";
        let parsed = parse_markdown(md).expect("document should parse");
        let fm = parsed.metadata.expect("metadata should be present");
        let missing = missing_required_fields(&fm);
        assert!(missing.is_empty());
    }

    #[test]
    fn upserts_metadata_block_for_document_without_metadata() {
        let input = "# Title\n\nBody\n";
        let fm = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec!["rust".to_string()],
            vec![],
            vec![],
            vec![],
        );

        let out = upsert_metadata_block(input, &fm).expect("upsert should succeed");
        assert!(out.starts_with("---\n"));
        assert!(out.contains("title: Title"));
        assert!(out.contains("# Title"));
    }

    #[test]
    fn upserts_metadata_block_for_document_with_existing_metadata() {
        let input = "---\ntitle: Old\n---\n\n# Body\n";
        let fm = build_frontmatter(
            Some("Note".to_string()),
            Some("New".to_string()),
            Some("Desc".to_string()),
            vec![],
            vec![],
            vec![],
            vec![],
        );

        let out = upsert_metadata_block(input, &fm).expect("upsert should succeed");
        assert!(!out.contains("title: Old"));
        assert!(out.contains("title: New"));
        assert!(out.contains("# Body"));
    }

    #[test]
    fn frontmatter_from_json_parses_valid_payload() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"Note","title":"Test","description":"Desc","tags":["rust"],"topics":["lang"],"technologies":["Rust"],"languages":["rust"]}"#,
        )
        .unwrap();
        let fm = frontmatter_from_json(&json).expect("should parse");
        assert_eq!(get_doc_type(&fm).as_deref(), Some("Note"));
        assert_eq!(get_title(&fm).as_deref(), Some("Test"));
        assert_eq!(get_description(&fm).as_deref(), Some("Desc"));
        assert_eq!(fm.tags(), vec!["rust"]);
        assert_eq!(get_topics(&fm), vec!["lang"]);
        assert_eq!(get_technologies(&fm), vec!["Rust"]);
        assert_eq!(get_languages(&fm), vec!["rust"]);
    }

    #[test]
    fn frontmatter_from_json_handles_null_values() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":null,"title":"Test","description":null,"tags":null}"#,
        )
        .unwrap();
        let fm = frontmatter_from_json(&json).expect("should parse nulls");
        assert!(get_doc_type(&fm).is_none());
        assert_eq!(get_title(&fm).as_deref(), Some("Test"));
        assert!(get_description(&fm).is_none());
        assert!(fm.tags().is_empty());
    }

    #[test]
    fn build_frontmatter_roundtrips_through_yaml() {
        let fm = build_frontmatter(
            Some("Note".to_string()),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            vec!["rust".to_string()],
            vec!["lang".to_string()],
            vec!["Tokio".to_string()],
            vec!["rust".to_string()],
        );
        let yaml = to_yaml(&fm).expect("to_yaml should succeed");
        assert!(yaml.contains("type: Note"));
        assert!(yaml.contains("title: Title"));
        assert!(yaml.contains("tags:"));
        assert!(yaml.contains("- rust"));
    }
}
