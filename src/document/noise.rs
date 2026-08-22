/// Noise patterns shared between the document cleaner and LLM context builder.

/// Biographical/author headings that should be removed from Medium articles.
pub const BIOGRAPHY_HEADINGS: &[&str] = &[
    "about the author",
    "who am i",
    "more from the author",
    "written by",
    "author bio",
    "meet the author",
    "about the writer",
];

/// Noise line patterns common across source-specific cleaners.
/// Lines matching these substrings (when short enough) are promotional boilerplate.
pub const NOISE_LINE_PATTERNS: &[&str] = &[
    "follow me",
    "sponsor",
    "clap",
    "member-only story",
    "published in",
    "sign up",
    "become a member",
    "get started",
    "write on",
    "medium account",
    "towards data science",
    "level up yours",
    "the startup",
    "better programming",
];

/// Max line length for noise line detection.
/// Lines longer than this are assumed to be substantive content.
pub const NOISE_LINE_MAX_LEN: usize = 140;

/// Returns true if the trimmed line is promotional boilerplate.
pub fn is_noise_line(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    let l = trimmed.to_ascii_lowercase();
    if l.len() > NOISE_LINE_MAX_LEN {
        return false;
    }
    NOISE_LINE_PATTERNS.iter().any(|p| l.contains(p))
}

/// Returns true if the heading text matches a known biography pattern.
pub fn is_biography_heading(heading_text: &str) -> bool {
    let l = heading_text.to_ascii_lowercase();
    BIOGRAPHY_HEADINGS.iter().any(|p| l.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_line_detects_follow_me() {
        assert!(is_noise_line("Follow me for more content"));
    }

    #[test]
    fn noise_line_detects_sponsor() {
        assert!(is_noise_line("Sponsor this story"));
    }

    #[test]
    fn noise_line_rejects_long_lines() {
        let long = "a".repeat(NOISE_LINE_MAX_LEN + 1);
        assert!(!is_noise_line(&format!("follow me {}", long)));
    }

    #[test]
    fn noise_line_rejects_empty() {
        assert!(!is_noise_line(""));
    }

    #[test]
    fn biography_heading_detects_common_patterns() {
        assert!(is_biography_heading("## About the Author"));
        assert!(is_biography_heading("## Who Am I?"));
        assert!(is_biography_heading("## More from the Author"));
        assert!(is_biography_heading("### Written by"));
        assert!(is_biography_heading("## Author Bio"));
    }

    #[test]
    fn biography_heading_rejects_non_bio_headings() {
        assert!(!is_biography_heading("## About the Technology"));
        assert!(!is_biography_heading("## About Memory Management"));
        assert!(!is_biography_heading("## The Author's Guide to Writing"));
        assert!(!is_biography_heading("## About Me隔壁老王"));
    }
}
