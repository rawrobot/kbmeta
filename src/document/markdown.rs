use std::path::Path;

/// Source-specific cleaner selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    /// Medium-hosted article content.
    Medium,
    /// Generic markdown source with conservative cleaning.
    Generic,
}

/// Detects source type from explicit URL and markdown hints.
pub fn detect_source_type(input: &str, source_url: Option<&str>) -> SourceType {
    if source_url
        .map(|x| x.to_ascii_lowercase().contains("medium.com"))
        .unwrap_or(false)
    {
        return SourceType::Medium;
    }

    if input.to_ascii_lowercase().contains("medium.com/") {
        return SourceType::Medium;
    }

    SourceType::Generic
}

/// Extracts a likely source URL from markdown text.
pub fn extract_source_url(input: &str) -> Option<String> {
    let mut first_any = None;

    for token in input.split_whitespace() {
        let cleaned = strip_url_wrappers(token);
        if !(cleaned.starts_with("http://") || cleaned.starts_with("https://")) {
            continue;
        }

        if first_any.is_none() {
            first_any = Some(cleaned.to_string());
        }

        if cleaned.to_ascii_lowercase().contains("medium.com/") {
            return Some(cleaned.to_string());
        }
    }

    first_any
}

/// Extracts first markdown H1 title.
pub fn extract_h1(input: &str) -> Option<String> {
    input
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .map(|x| x.to_string())
}

/// Generates an FS-safe filename from title or original filename fallback.
pub fn generate_filename(title: Option<&str>, original_filename: &Path) -> String {
    generate_filename_with_limit(title, original_filename, 32)
}

/// Generates an FS-safe filename from title or original filename fallback with a max stem length.
pub fn generate_filename_with_limit(
    title: Option<&str>,
    original_filename: &Path,
    max_len: usize,
) -> String {
    let fallback = original_filename
        .file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());

    let fallback = truncate_by_suffix(&fallback);

    let base = title
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .unwrap_or(&fallback);

    let mut out = String::new();
    let mut last_sep = false;

    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
            last_sep = false;
            continue;
        }

        if !last_sep {
            out.push('_');
            last_sep = true;
        }
    }

    while out.ends_with('_') || out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('_') || out.starts_with('-') {
        out.remove(0);
    }

    if out.is_empty() {
        out = "document".to_string();
    }

    let safe_max_len = max_len.max(8);
    if out.len() > safe_max_len {
        out.truncate(safe_max_len);
        while out.ends_with('_') || out.ends_with('-') {
            out.pop();
        }
    }

    format!("{}.md", out)
}

fn truncate_by_suffix(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if let Some(idx) = lower.find(" by ") {
        let candidate = input[..idx].trim();
        if !candidate.is_empty() {
            return candidate.to_string();
        }
    }
    input.trim().to_string()
}

fn strip_url_wrappers(input: &str) -> &str {
    input
        .trim_start_matches(['(', '[', '<', '"', '\''])
        .trim_end_matches([')', ']', '>', '"', '\'', ',', '.', ';', ':'])
}

#[cfg(test)]
mod tests {
    use super::{
        SourceType, detect_source_type, extract_h1, extract_source_url, generate_filename,
        generate_filename_with_limit,
    };
    use std::path::Path;

    #[test]
    fn detects_medium_from_source_url() {
        let source = Some("https://medium.com/some-post");
        assert_eq!(detect_source_type("text", source), SourceType::Medium);
    }

    #[test]
    fn extracts_medium_url_first() {
        let input = "ref https://example.com and https://medium.com/abc";
        let got = extract_source_url(input);
        assert_eq!(got.as_deref(), Some("https://medium.com/abc"));
    }

    #[test]
    fn extracts_h1() {
        let got = extract_h1("# Linux Environment Variables\n\nText");
        assert_eq!(got.as_deref(), Some("Linux Environment Variables"));
    }

    #[test]
    fn filename_from_h1() {
        let got = generate_filename(
            Some("Linux Environment Variables"),
            Path::new("article.md"),
        );
        assert_eq!(got, "Linux_Environment_Variables.md");
    }

    #[test]
    fn filename_is_limited_to_32_characters() {
        let got = generate_filename(
            Some("This is a very long technical title that should be truncated"),
            Path::new("article.md"),
        );
        assert_eq!(got, "This_is_a_very_long_technical_ti.md");
    }

    #[test]
    fn filename_truncates_fallback_by_suffix() {
        let got = generate_filename(
            None,
            Path::new("25+ Linux Variables by Dhanush N Apr 2026.md"),
        );
        assert_eq!(got, "25_Linux_Variables.md");
    }

    #[test]
    fn filename_respects_custom_length_limit() {
        let got = generate_filename_with_limit(
            Some("Linux environment variables and shell startup behavior"),
            Path::new("article.md"),
            20,
        );
        assert_eq!(got, "Linux_environment_va.md");
    }
}
