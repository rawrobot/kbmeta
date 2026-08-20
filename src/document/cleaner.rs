use crate::document::markdown::{SourceType, detect_source_type, extract_h1, extract_source_url, generate_filename};
use anyhow::Result;
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

/// Medium-specific cleaner implementation.
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
        let mut out = Vec::new();
        let lines: Vec<&str> = input.lines().collect();
        let mut i = 0usize;
        let mut in_code = false;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            if trimmed.starts_with("```") {
                in_code = !in_code;
                out.push(line.to_string());
                i += 1;
                continue;
            }

            if !in_code {
                if let Some(level) = heading_level(trimmed) {
                    if is_biography_heading(trimmed) {
                        i += 1;
                        while i < lines.len() {
                            let t = lines[i].trim();
                            if t.starts_with("```") {
                                break;
                            }
                            if let Some(next_level) = heading_level(t) {
                                if next_level <= level {
                                    break;
                                }
                            }
                            i += 1;
                        }
                        continue;
                    }
                }

                if is_medium_noise_line(trimmed) {
                    i += 1;
                    continue;
                }
            }

            out.push(line.to_string());
            i += 1;
        }

        let cleaned = out.join("\n");
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

fn heading_level(trimmed: &str) -> Option<usize> {
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 {
        return None;
    }
    if trimmed.chars().nth(hashes) != Some(' ') {
        return None;
    }
    Some(hashes)
}

fn is_biography_heading(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("who am i") || l.contains("about the author") || l.contains("about me")
}

fn is_medium_noise_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }

    let l = line.to_ascii_lowercase();
    let is_short = l.len() <= 120;

    (is_short && l.contains("follow me"))
        || (is_short && l.contains("sponsor"))
        || (is_short && l.contains("clap"))
        || (is_short && l.contains("member-only story"))
        || (is_short && l.contains("published in"))
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
}
