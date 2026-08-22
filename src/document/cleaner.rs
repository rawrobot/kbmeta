use crate::document::markdown::{SourceType, detect_source_type, extract_h1, extract_source_url, generate_filename};
use crate::document::noise::{is_biography_heading, is_noise_line};
use anyhow::Result;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::path::Path;

/// Clean markdown plus source metadata for downstream processing.
#[derive(Debug, Clone)]
pub struct CleanDocument {
    /// Human-readable title, usually from H1.
    pub title: Option<String>,
    /// Original source URL when available.
    pub source_url: Option<String>,
    /// Source classifier used for policy selection.
    pub source_type: String,
    /// FS-safe filename for output markdown.
    pub filename: String,
    /// Clear markdown content.
    pub markdown: String,
}

/// Deterministic cleaner interface.
pub trait DocumentCleaner {
    /// Cleans source markdown into clear markdown while preserving technical content.
    fn clean(&self, input: &str, input_filename: &Path, source_url: Option<&str>) -> Result<CleanDocument>;
}

/// Medium-specific cleaner using pulldown-cmark AST walking.
pub struct MediumCleaner;

/// Conservative fallback cleaner for non-Medium sources.
pub struct GenericCleaner;

/// Runs source detection and applies source-specific cleaning policy.
pub fn clean_document(input: &str, input_filename: &Path, source_url: Option<&str>) -> Result<CleanDocument> {
    match detect_source_type(input, source_url) {
        SourceType::Medium => MediumCleaner.clean(input, input_filename, source_url),
        SourceType::Generic => GenericCleaner.clean(input, input_filename, source_url),
    }
}

impl DocumentCleaner for MediumCleaner {
    fn clean(&self, input: &str, input_filename: &Path, source_url: Option<&str>) -> Result<CleanDocument> {
        let lines: Vec<&str> = input.lines().collect();

        let code_block_lines = find_code_block_lines(input);
        let html_block_lines = find_html_block_lines(input);
        let bio_line_ranges = find_biography_line_ranges(input);

        let mut out = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            // Skip lines inside biography sections
            if bio_line_ranges.iter().any(|r| r.contains(&line_idx)) {
                continue;
            }

            // Skip noise lines outside code blocks
            if !code_block_lines.contains(&line_idx) && is_noise_line(line.trim()) {
                continue;
            }

            // Filter HTML block noise lines
            if html_block_lines.contains(&line_idx) {
                if let Some(filtered) = filter_html_block_line(line) {
                    out.push(filtered);
                }
                continue;
            }

            out.push(line.to_string());
        }

        let cleaned = normalize_blank_lines(&out.join("\n"));
        let extracted = extract_source_url(input);
        let final_source_url = source_url.or(extracted.as_deref());
        Ok(build_clean_document(
            &cleaned,
            input_filename,
            final_source_url,
            "medium",
        ))
    }
}

impl DocumentCleaner for GenericCleaner {
    fn clean(&self, input: &str, input_filename: &Path, source_url: Option<&str>) -> Result<CleanDocument> {
        let cleaned = input
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        let extracted = extract_source_url(input);
        let final_source_url = source_url.or(extracted.as_deref());

        Ok(build_clean_document(
            &cleaned,
            input_filename,
            final_source_url,
            "generic",
        ))
    }
}

/// Returns the set of line indices (0-based) that are inside code blocks.
fn find_code_block_lines(input: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut in_code = false;

    for (event, range) in Parser::new_ext(input, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code = true;
                for li in offset_to_line_range(input, range) {
                    lines.push(li);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                for li in offset_to_line_range(input, range) {
                    lines.push(li);
                }
            }
            Event::Html(_) if in_code => {
                for li in offset_to_line_range(input, range) {
                    lines.push(li);
                }
            }
            _ => {}
        }
    }

    lines.sort();
    lines.dedup();
    lines
}

/// Returns the set of line indices that are inside HTML blocks.
fn find_html_block_lines(input: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut in_html = false;

    for (event, range) in Parser::new_ext(input, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => {
                in_html = true;
            }
            Event::End(TagEnd::HtmlBlock) => {
                in_html = false;
            }
            Event::Html(_) if in_html => {
                for li in offset_to_line_range(input, range) {
                    lines.push(li);
                }
            }
            _ => {}
        }
    }

    lines.sort();
    lines.dedup();
    lines
}

/// Returns line ranges for biography sections (heading to next same-level heading).
fn find_biography_line_ranges(input: &str) -> Vec<Range<usize>> {
    let events: Vec<Event<'_>> = Parser::new_ext(input, Options::all()).collect();
    let mut ranges = Vec::new();

    for (i, event) in events.iter().enumerate() {
        if let Event::Start(Tag::Heading { level, .. }) = event {
            let heading_text = extract_heading_text(&events, i);
            if is_biography_heading(&heading_text) {
                let start_level = *level;
                let mut end_event_idx = events.len();
                for (j, e) in events.iter().enumerate().skip(i + 1) {
                    if let Event::Start(Tag::Heading { level: next_level, .. }) = e {
                        if *next_level <= start_level {
                            end_event_idx = j;
                            break;
                        }
                    }
                }

                let start_offset = event_byte_offset(input, i);
                let end_offset = if end_event_idx < events.len() {
                    event_byte_offset(input, end_event_idx)
                } else {
                    input.len()
                };

                let start_line = offset_to_line(input, start_offset);
                let end_line = offset_to_line(input, end_offset);

                ranges.push(start_line..end_line);
            }
        }
    }

    ranges
}

/// Converts a byte offset to a 0-based line index.
fn offset_to_line(input: &str, offset: usize) -> usize {
    input[..offset].lines().count()
}

/// Converts a byte range to 0-based line indices.
fn offset_to_line_range(input: &str, range: std::ops::Range<usize>) -> Vec<usize> {
    let start_line = offset_to_line(input, range.start);
    let end_line = offset_to_line(input, range.end);
    (start_line..end_line).collect()
}

/// Gets the byte offset of the Nth event by re-parsing.
fn event_byte_offset(input: &str, target_index: usize) -> usize {
    for (i, (_event, range)) in Parser::new_ext(input, Options::all()).into_offset_iter().enumerate() {
        if i == target_index {
            return range.start;
        }
    }
    input.len()
}

/// Extracts text content from a heading event sequence.
fn extract_heading_text(events: &[Event<'_>], heading_index: usize) -> String {
    let mut text = String::new();
    for event in events.iter().skip(heading_index + 1) {
        match event {
            Event::Text(t) => text.push_str(t),
            Event::Code(c) => text.push_str(c),
            Event::End(TagEnd::Heading(_)) => break,
            _ => {}
        }
    }
    text
}

/// Filters a single line of an HTML block, returning None to skip noise.
fn filter_html_block_line(html: &str) -> Option<String> {
    let trimmed = html.trim();
    let lower = trimmed.to_ascii_lowercase();

    if trimmed.is_empty() {
        return None;
    }

    if lower.starts_with("<figure")
        || lower.starts_with("</figure")
        || lower.starts_with("<figcaption")
        || lower.starts_with("</figcaption")
        || lower.starts_with("<aside")
        || lower.starts_with("</aside")
        || lower.starts_with("<div class=\"audio\"")
        || lower.starts_with("<div class=\"speechify\"")
        || lower.starts_with("<span class=\"progressiveMedia")
        || lower.starts_with("<div class=\"progressiveMedia")
        || lower.starts_with("<div data-testid=\"speechify-highlight")
        || lower.starts_with("<div class=\"prebid")
        || lower.starts_with("<div id=\"prebid")
    {
        return None;
    }

    Some(html.to_string())
}

/// Collapses 3+ consecutive blank lines to 2 (one blank line between blocks).
fn normalize_blank_lines(input: &str) -> String {
    let mut out = Vec::new();
    let mut blank_count = 0usize;

    for line in input.lines() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                out.push(String::new());
            }
            continue;
        }
        blank_count = 0;
        out.push(line.to_string());
    }

    let mut joined = out.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

fn build_clean_document(
    body: &str,
    input_filename: &Path,
    source_url: Option<&str>,
    source_type: &str,
) -> CleanDocument {
    let title = extract_h1(body);
    let filename = generate_filename(title.as_deref(), input_filename);

    let mut markdown = String::new();
    markdown.push_str("---\n");
    if let Some(url) = source_url {
        markdown.push_str(&format!("source_url: {}\n", url));
    }
    markdown.push_str(&format!("source_type: {}\n", source_type));
    markdown.push_str("---\n\n");
    markdown.push_str(body.trim_start());
    if !markdown.ends_with('\n') {
        markdown.push('\n');
    }

    CleanDocument {
        title,
        source_url: source_url.map(|x| x.to_string()),
        source_type: source_type.to_string(),
        filename,
        markdown,
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentCleaner, GenericCleaner, MediumCleaner, clean_document};
    use std::path::Path;

    #[test]
    fn medium_policy_removes_bio_and_follow_noise() {
        let input = "# Linux Environment Variables\n\n## PATH\nThe PATH variable.\n\n## Who Am I?\nHi, I'm author.\nFollow me on Medium\n\n## HOME\nThe HOME variable.";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/post"))
            .expect("clean should succeed");

        assert!(out.markdown.contains("## PATH"));
        assert!(out.markdown.contains("## HOME"));
        assert!(!out.markdown.contains("Who Am I"));
        assert!(!out.markdown.contains("Follow me"));
        assert_eq!(out.source_type, "medium");
    }

    #[test]
    fn generic_policy_preserves_uncertain_sections() {
        let input = "# Linux Environment Variables\n\n## Who Am I?\nHi, I'm author.\nFollow me on Medium\n";
        let out = GenericCleaner
            .clean(input, Path::new("article.md"), None)
            .expect("clean should succeed");

        assert!(out.markdown.contains("Who Am I"));
        assert!(out.markdown.contains("Follow me"));
        assert_eq!(out.source_type, "generic");
    }

    #[test]
    fn policy_switches_by_source_detection() {
        let input = "# Title\n\n## Who Am I?\nAuthor bio\n";
        let out = clean_document(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert_eq!(out.source_type, "medium");
    }

    #[test]
    fn medium_policy_keeps_code_blocks() {
        let input = "# Title\n\n```bash\n# Follow me\necho hi\n```\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("# Follow me"));
    }

    #[test]
    fn medium_policy_removes_about_the_author() {
        let input = "# Title\n\nSome content.\n\n## About the Author\nI am a developer.\nMy blog is here.\n\n## Other Section\nMore content.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("Some content."));
        assert!(out.markdown.contains("## Other Section"));
        assert!(!out.markdown.contains("About the Author"));
        assert!(!out.markdown.contains("I am a developer."));
    }

    #[test]
    fn medium_policy_removes_noise_lines_outside_code() {
        let input = "# Title\n\nSome content.\n\nSponsor this story\n\nMore content.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("Some content."));
        assert!(out.markdown.contains("More content."));
        assert!(!out.markdown.contains("Sponsor this story"));
    }

    #[test]
    fn medium_policy_removes_figure_html() {
        let input = "# Title\n\n<figure><img src=\"avatar.jpg\"></figure>\n\nContent.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("Content."));
        assert!(!out.markdown.contains("avatar.jpg"));
    }

    #[test]
    fn medium_policy_removes_aside_html() {
        let input = "# Title\n\n<aside>\nFollow me!\n</aside>\n\nContent.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("Content."));
        assert!(!out.markdown.contains("Follow me"));
    }

    #[test]
    fn medium_policy_preserves_headings() {
        let input = "# Title\n\n## Section One\n\nContent.\n\n## Section Two\n\nMore.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("# Title"));
        assert!(out.markdown.contains("## Section One"));
        assert!(out.markdown.contains("## Section Two"));
    }

    #[test]
    fn medium_policy_handles_empty_input() {
        let out = MediumCleaner
            .clean("", Path::new("empty.md"), None)
            .expect("clean should succeed");
        assert!(out.markdown.contains("source_type: medium"));
    }

    #[test]
    fn medium_bio_removal_with_subheadings() {
        let input = "# Title\n\n## PATH\nThe PATH variable.\n\n## Who Am I?\n### My Background\nI started coding...\n### My Projects\nI built things...\n\n## HOME\nThe HOME variable.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("## PATH"));
        assert!(out.markdown.contains("## HOME"));
        assert!(!out.markdown.contains("Who Am I"));
        assert!(!out.markdown.contains("My Background"));
        assert!(!out.markdown.contains("My Projects"));
    }

    #[test]
    fn medium_policy_preserves_tables() {
        let input = "# Title\n\n| Col A | Col B |\n|-------|-------|\n| 1     | 2     |\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("| Col A | Col B |"));
        assert!(out.markdown.contains("| 1"));
    }

    #[test]
    fn medium_policy_preserves_blockquotes() {
        let input = "# Title\n\n> This is important.\n> Remember this.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("> This is important."));
    }

    #[test]
    fn medium_policy_preserves_lists() {
        let input = "# Title\n\n- Item one\n- Item two\n- Item three\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("Item one"));
        assert!(out.markdown.contains("Item two"));
        assert!(out.markdown.contains("Item three"));
    }

    #[test]
    fn medium_policy_preserves_inline_code() {
        let input = "# Title\n\nUse `cargo build` to build.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("`cargo build`"));
    }

    #[test]
    fn medium_policy_preserves_bold_and_italic() {
        let input = "# Title\n\n**bold** and *italic* text.\n";
        let out = MediumCleaner
            .clean(input, Path::new("article.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("**bold**"));
        assert!(out.markdown.contains("*italic*"));
    }

    #[test]
    fn medium_policy_handles_realistic_medium_export() {
        let input = r#"# Linux Environment Variables

## PATH

The `PATH` variable tells the shell where to find executables.

```bash
echo $PATH
```

## HOME

The HOME variable points to the user's home directory.

<figure><img src="https://miro.medium.com/v2/resize:fit:1400/1*abc.png"></figure>

*Image: directory structure*

## Who Am I?

Hi, I'm a Linux enthusiast. I write about system administration.

Follow me on Medium for more articles!

Sponsor this story if you found it useful.

## LD_PRELOAD

This variable loads a shared library before others.

> **Warning:** Be careful with LD_PRELOAD in production.

| Variable | Purpose |
|----------|---------|
| PATH | Executable search path |
| HOME | Home directory |

## More from the Author

Check out my other articles on Kubernetes and Docker.
"#;
        let out = MediumCleaner
            .clean(input, Path::new("linux-env-vars.md"), Some("https://medium.com/@author/linux-env-vars-abc123?source=rss----def-456"))
            .expect("clean should succeed");

        // Technical content preserved
        assert!(out.markdown.contains("## PATH"));
        assert!(out.markdown.contains("## HOME"));
        assert!(out.markdown.contains("## LD_PRELOAD"));
        assert!(out.markdown.contains("```bash"));
        assert!(out.markdown.contains("> **Warning:**"));

        // Tables preserved
        assert!(out.markdown.contains("| Variable | Purpose |"));

        // Biography sections removed
        assert!(!out.markdown.contains("Who Am I"));
        assert!(!out.markdown.contains("I'm a Linux enthusiast"));
        assert!(!out.markdown.contains("More from the Author"));
        assert!(!out.markdown.contains("Check out my other articles"));

        // Noise lines removed
        assert!(!out.markdown.contains("Follow me on Medium"));
        assert!(!out.markdown.contains("Sponsor this story"));

        // Figure HTML removed (avatar/decorative)
        assert!(!out.markdown.contains("<figure"));
        assert!(!out.markdown.contains("miro.medium.com"));

        // Metadata correct
        assert_eq!(out.source_type, "medium");
        assert!(out.source_url.unwrap().contains("medium.com"));
    }

    #[test]
    fn medium_policy_handles_nested_code_block_with_noise() {
        let input = "# Title\n\n```python\n# Follow me on Twitter\nimport os\nprint('hello')\n```\n\nSome text.\n";
        let out = MediumCleaner
            .clean(input, Path::new("code.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        // Noise inside code block preserved
        assert!(out.markdown.contains("# Follow me on Twitter"));
        assert!(out.markdown.contains("import os"));
        // Text outside code block preserved
        assert!(out.markdown.contains("Some text."));
    }

    #[test]
    fn medium_policy_handles_multiple_bio_sections() {
        let input = "# Title\n\nContent.\n\n## About the Author\nBio text here.\n\n## HOME\nMore content.\n\n## Written by\nSomeone else.\n\n## LD_PRELOAD\nFinal content.\n";
        let out = MediumCleaner
            .clean(input, Path::new("multi.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");

        assert!(out.markdown.contains("Content."));
        assert!(out.markdown.contains("## HOME"));
        assert!(out.markdown.contains("## LD_PRELOAD"));
        assert!(!out.markdown.contains("About the Author"));
        assert!(!out.markdown.contains("Bio text here"));
        assert!(!out.markdown.contains("Written by"));
        assert!(!out.markdown.contains("Someone else"));
    }

    #[test]
    fn medium_policy_handles_heading_level_boundary() {
        // Biography H2 with H3 sub-headings should be removed until next H2
        let input = "# Title\n\n## Section A\nContent A.\n\n## Who Am I?\n### Sub 1\nText 1.\n### Sub 2\nText 2.\n\n## Section B\nContent B.\n";
        let out = MediumCleaner
            .clean(input, Path::new("level.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");

        assert!(out.markdown.contains("## Section A"));
        assert!(out.markdown.contains("Content A."));
        assert!(out.markdown.contains("## Section B"));
        assert!(out.markdown.contains("Content B."));
        assert!(!out.markdown.contains("Who Am I"));
        assert!(!out.markdown.contains("Sub 1"));
        assert!(!out.markdown.contains("Sub 2"));
    }

    #[test]
    fn medium_policy_handles_html_aside_block() {
        let input = "# Title\n\nContent.\n\n<aside class=\"speechify-ignore\">\n<p>Sponsored content</p>\n</aside>\n\nMore content.\n";
        let out = MediumCleaner
            .clean(input, Path::new("aside.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");

        assert!(out.markdown.contains("Content."));
        assert!(out.markdown.contains("More content."));
        assert!(!out.markdown.contains("speechify-ignore"));
        assert!(!out.markdown.contains("Sponsored content"));
    }

    #[test]
    fn medium_policy_preserves_blockquote_in_content() {
        let input = "# Title\n\n## Section\n\n> Important note.\n> Remember this.\n\nText after quote.\n";
        let out = MediumCleaner
            .clean(input, Path::new("quote.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");
        assert!(out.markdown.contains("> Important note."));
        assert!(out.markdown.contains("> Remember this."));
        assert!(out.markdown.contains("Text after quote."));
    }

    #[test]
    fn medium_policy_handles_empty_bio_section() {
        // Bio heading with no content before next heading
        let input = "# Title\n\nContent.\n\n## About the Author\n\n## HOME\nMore.\n";
        let out = MediumCleaner
            .clean(input, Path::new("empty-bio.md"), Some("https://medium.com/x"))
            .expect("clean should succeed");

        assert!(out.markdown.contains("Content."));
        assert!(out.markdown.contains("## HOME"));
        assert!(out.markdown.contains("More."));
        assert!(!out.markdown.contains("About the Author"));
    }
}
