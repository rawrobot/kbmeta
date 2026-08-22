use crate::document::noise::is_noise_line;
use anyhow::Result;

/// Compact markdown intended as metadata-LLM input.
#[derive(Debug, Clone)]
pub struct LlmContext {
    /// Reduced markdown content.
    pub markdown: String,
}

/// Deterministic context builder interface.
pub trait ContextBuilder {
    /// Builds reduced markdown context from clear markdown input.
    fn build(&self, clear_markdown: &str) -> Result<LlmContext>;
}

/// Conservative line-based context builder for v1.
pub struct DefaultLlmContextBuilder;

impl ContextBuilder for DefaultLlmContextBuilder {
    fn build(&self, clear_markdown: &str) -> Result<LlmContext> {
        let mut out = Vec::new();
        let mut in_code = false;
        let mut code_buf: Vec<String> = Vec::new();
        let mut section_text_lines = 0usize;

        for line in clear_markdown.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("```") {
                if !in_code {
                    in_code = true;
                    code_buf.clear();
                    code_buf.push(line.to_string());
                } else {
                    code_buf.push(line.to_string());
                    in_code = false;
                    append_code_block(&mut out, &code_buf);
                    code_buf.clear();
                }
                continue;
            }

            if in_code {
                code_buf.push(line.to_string());
                continue;
            }

            if is_noise_line(trimmed) {
                continue;
            }

            if is_heading(trimmed) {
                section_text_lines = 0;
                out.push(line.to_string());
                continue;
            }

            if is_structural_line(trimmed) {
                out.push(line.to_string());
                continue;
            }

            if trimmed.is_empty() {
                out.push(String::new());
                continue;
            }

            if section_text_lines < 18 || is_technical_line(trimmed) {
                out.push(line.to_string());
                section_text_lines += 1;
            }
        }

        // Keep output tidy and deterministic.
        let markdown = normalize_blank_lines(&out.join("\n"));
        Ok(LlmContext { markdown })
    }
}

fn append_code_block(out: &mut Vec<String>, code: &[String]) {
    // Include opening and closing fence in count; truncate only large blocks.
    if code.len() <= 42 {
        out.extend(code.iter().cloned());
        return;
    }

    for line in code.iter().take(21) {
        out.push(line.clone());
    }
    out.push("# ... trimmed for llm context ...".to_string());
    for line in code.iter().skip(code.len().saturating_sub(11)) {
        out.push(line.clone());
    }
}

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

fn is_heading(line: &str) -> bool {
    let count = line.chars().take_while(|&c| c == '#').count();
    count > 0 && line.chars().nth(count) == Some(' ')
}

fn is_structural_line(line: &str) -> bool {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return true;
    }
    if line.starts_with(">") || line.starts_with("|") {
        return true;
    }

    let mut digits = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        if ch == '.' && digits > 0 {
            return line.chars().nth(digits + 1) == Some(' ');
        }
        break;
    }

    false
}

fn is_technical_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let keys = [
        "linux", "kernel", "rust", "tokio", "grpc", "http", "https", "api", "uio", "ebpf",
        "opencv", "fpga", "cuda", "docker", "kubernetes", "shell", "path", "ld_preload",
        "ld_library_path", "security", "protocol", "sqlite",
    ];

    keys.iter().any(|k| l.contains(k))
}

#[cfg(test)]
mod tests {
    use super::{ContextBuilder, DefaultLlmContextBuilder};

    #[test]
    fn preserves_title_and_headings() {
        let input = "# Linux Environment Variables\n\n## PATH\nLine\n\n## HOME\nLine\n";
        let out = DefaultLlmContextBuilder
            .build(input)
            .expect("context build should succeed");

        assert!(out.markdown.contains("# Linux Environment Variables"));
        assert!(out.markdown.contains("## PATH"));
        assert!(out.markdown.contains("## HOME"));
    }

    #[test]
    fn removes_boilerplate_noise_lines() {
        let input = "# Title\n\nFollow me for more\n\nImportant technical section\n";
        let out = DefaultLlmContextBuilder
            .build(input)
            .expect("context build should succeed");

        assert!(!out.markdown.contains("Follow me"));
        assert!(out.markdown.contains("Important technical section"));
    }

    #[test]
    fn truncates_long_code_blocks() {
        let mut input = String::from("# Title\n\n```rust\n");
        for i in 0..80 {
            input.push_str(&format!("println!(\"{}\");\n", i));
        }
        input.push_str("```\n");

        let out = DefaultLlmContextBuilder
            .build(&input)
            .expect("context build should succeed");

        assert!(out.markdown.contains("trimmed for llm context"));
        assert!(out.markdown.contains("```rust"));
        assert!(out.markdown.contains("```"));
    }

    #[test]
    fn keeps_technical_lines_after_section_budget() {
        let mut input = String::from("# Title\n\n");
        for _ in 0..25 {
            input.push_str("general explanation line\n");
        }
        input.push_str("linux kernel behavior note\n");

        let out = DefaultLlmContextBuilder
            .build(&input)
            .expect("context build should succeed");

        assert!(out.markdown.contains("linux kernel behavior note"));
    }
}
