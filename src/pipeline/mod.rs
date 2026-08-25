use crate::config::Config;
use crate::database::rebuild::{RebuildOptions, rebuild_index};
use crate::document::cleaner::{CleanDocument, clean_document};
use crate::document::llm_context::{ContextBuilder, DefaultLlmContextBuilder, LlmContext};
use crate::document::markdown::generate_filename_with_limit;
use crate::document::metadata_block::{missing_required_fields, to_yaml, upsert_metadata_block, get_filename};
use crate::pathing::resolve_from_root;
use crate::provider::{MetadataProvider, SuggestionRules, apply_rules, build_provider, derive_rules};
use anyhow::{Context, Result, bail};
use okf::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Clean operation abstraction used by command handlers and pipelines.
pub trait CleanOperation {
    fn run(&self, input: &str, input_filename: &Path) -> Result<CleanDocument>;
}

/// LLM-context build abstraction used by command handlers and pipelines.
pub trait ContextOperation {
    fn run(&self, clean_markdown: &str) -> Result<LlmContext>;
}

/// Metadata suggestion abstraction used by command handlers and pipelines.
pub trait SuggestOperation {
    fn run(
        &self,
        source_path: &Path,
        llm_context: &str,
        rules: &SuggestionRules,
        provider: &dyn MetadataProvider,
        system_prompt: Option<&str>,
    ) -> Result<okf::Frontmatter>;
}

/// Default cleaner implementation backed by source-aware deterministic cleaning.
pub struct DefaultCleanOperation;

/// Default context implementation backed by deterministic line filtering.
pub struct DefaultContextOperation;

/// Default suggest implementation backed by runtime-configured providers.
pub struct DefaultSuggestOperation;

impl CleanOperation for DefaultCleanOperation {
    fn run(&self, input: &str, input_filename: &Path) -> Result<CleanDocument> {
        clean_document(input, input_filename, None)
    }
}

impl ContextOperation for DefaultContextOperation {
    fn run(&self, clean_markdown: &str) -> Result<LlmContext> {
        let builder = DefaultLlmContextBuilder;
        builder.build(clean_markdown)
    }
}

impl SuggestOperation for DefaultSuggestOperation {
    fn run(
        &self,
        source_path: &Path,
        llm_context: &str,
        rules: &SuggestionRules,
        provider: &dyn MetadataProvider,
        system_prompt: Option<&str>,
    ) -> Result<okf::Frontmatter> {
        let raw = provider
            .suggest_with_system_prompt(source_path, llm_context, rules, system_prompt)
            .with_context(|| "provider failed to suggest metadata")?;
        Ok(apply_rules(raw, rules))
    }
}

/// Full pipeline result used by the `cook` command.
#[derive(Debug, Clone)]
pub struct CookArtifacts {
    pub clean: CleanDocument,
    pub context: LlmContext,
    pub rules: SuggestionRules,
    pub metadata: okf::Frontmatter,
    pub final_markdown: String,
    pub output_path: PathBuf,
    pub prompt_file_path: Option<PathBuf>,
    pub prompt_name: String,
    pub expected_prompt_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct LoadedCookPrompt {
    prompt_name: String,
    expected_prompt_path: Option<PathBuf>,
    system_prompt: Option<String>,
    prompt_file_path: Option<PathBuf>,
}

/// Orchestrates clean -> context -> suggest -> merge for one input file.
pub fn run_cook(
    root: &Path,
    config: &Config,
    input_path: &Path,
    clean_op: &dyn CleanOperation,
    context_op: &dyn ContextOperation,
    suggest_op: &dyn SuggestOperation,
) -> Result<CookArtifacts> {
    let original = fs::read_to_string(input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;

    let clean = clean_op.run(&original, input_path)?;
    let context = context_op.run(&clean.markdown)?;

    let docs_root = resolve_from_root(root, &config.documents_root);
    let report = crate::document::discovery::discover_documents_lenient(&docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;

    for warning in report.warnings {
        eprintln!("warning: skipped {}: {}", warning.path.display(), warning.message);
    }

    let output_dir = resolve_from_root(root, &config.cook.output_dir);
    let fallback_stem = generate_filename_with_limit(
        clean.title.as_deref(),
        input_path,
        config.cook.max_filename_len,
    );
    let temp_output_path = output_dir.join(&fallback_stem);
    let rules = derive_rules(&report.documents, &temp_output_path);
    let provider = build_provider(config)?;
    let loaded_prompt = load_cook_system_prompt(root, config)?;
    let mut metadata = suggest_op.run(
        input_path,
        &context.markdown,
        &rules,
        provider.as_ref(),
        loaded_prompt.system_prompt.as_deref(),
    )?;

    ensure_generated(&mut metadata);

    let missing = missing_required_fields(&metadata);
    if !missing.is_empty() {
        bail!(
            "suggested metadata is invalid: missing required fields: {}",
            missing.join(", ")
        );
    }

    let output_stem = extract_filename(&metadata, config.cook.max_filename_len)
        .unwrap_or(fallback_stem);
    let output_path = output_dir.join(&output_stem);

    let body = ensure_body_heading(&clean.markdown, &clean.title);
    let final_markdown = upsert_metadata_block(&body, &metadata)?;

    Ok(CookArtifacts {
        clean,
        context,
        rules,
        metadata,
        final_markdown,
        output_path,
        prompt_file_path: loaded_prompt.prompt_file_path,
        prompt_name: loaded_prompt.prompt_name,
        expected_prompt_path: loaded_prompt.expected_prompt_path,
    })
}

fn ensure_generated(metadata: &mut okf::Frontmatter) {
    if metadata.get("generated").is_some() {
        return;
    }
    let mut by = okf::yaml::Mapping::new();
    by.insert("by".to_string(), Value::String("kbmeta/cook".to_string()));
    by.insert("at".to_string(), Value::String(utc_now_iso8601()));
    metadata.set("generated", Value::Mapping(by));
}

fn ensure_body_heading(body: &str, title: &Option<String>) -> String {
    let trimmed = body.trim_start();
    if trimmed.starts_with("# ") {
        return body.to_string();
    }
    let heading = title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("Untitled");
    format!("# {heading}\n\n{body}")
}

fn utc_now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let days = (secs / 86400) as i64;
    let time_of_day = (secs % 86400) as u32;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since epoch (algorithm from Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn extract_filename(metadata: &okf::Frontmatter, max_len: usize) -> Option<String> {
    let raw = get_filename(metadata)?;
    let safe_max = max_len.max(8);

    let mut out = String::new();
    let mut last_sep = false;
    for ch in raw.chars() {
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
        return None;
    }
    if out.len() > safe_max {
        out.truncate(safe_max);
        while out.ends_with('_') || out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(format!("{}.md", out))
}

/// Writes optional intermediate artifacts for debugging and review workflows.
pub fn write_intermediate_artifacts(root: &Path, base_name: &str, cooked: &CookArtifacts) -> Result<Vec<PathBuf>> {
    let out_dir = resolve_from_root(root, Path::new(".kb/intermediate"));
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;

    let clean_path = out_dir.join(format!("{}.clean.md", base_name));
    let context_path = out_dir.join(format!("{}.context.md", base_name));
    let suggest_path = out_dir.join(format!("{}.suggested.yaml", base_name));

    fs::write(&clean_path, &cooked.clean.markdown)
        .with_context(|| format!("failed to write {}", clean_path.display()))?;
    fs::write(&context_path, &cooked.context.markdown)
        .with_context(|| format!("failed to write {}", context_path.display()))?;
    fs::write(&suggest_path, to_yaml(&cooked.metadata)?)
        .with_context(|| format!("failed to write {}", suggest_path.display()))?;

    Ok(vec![clean_path, context_path, suggest_path])
}

/// Rebuilds the index after a write to keep scan/rebuild state consistent.
pub fn rebuild_index_after_write(root: &Path, config: &Config) -> Result<()> {
    let docs_root = resolve_from_root(root, &config.documents_root);
    let docs = crate::document::discovery::discover_documents_strict(&docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;

    let mut conn = crate::command::open_db(root)?;
    rebuild_index(&mut conn, &docs, RebuildOptions::default())?;
    Ok(())
}

fn load_cook_system_prompt(root: &Path, config: &Config) -> Result<LoadedCookPrompt> {
    let name = config.cook.prompt_name.trim();
    if name.is_empty() {
        return Ok(LoadedCookPrompt {
            prompt_name: String::new(),
            expected_prompt_path: None,
            system_prompt: None,
            prompt_file_path: None,
        });
    }

    let prompt_path = resolve_from_root(root, Path::new(".kb/prompts"))
        .join(format!("{}.md", name));

    if !prompt_path.exists() {
        return Ok(LoadedCookPrompt {
            prompt_name: name.to_string(),
            expected_prompt_path: Some(prompt_path),
            system_prompt: None,
            prompt_file_path: None,
        });
    }

    let content = fs::read_to_string(&prompt_path)
        .with_context(|| format!("failed to read {}", prompt_path.display()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(LoadedCookPrompt {
            prompt_name: name.to_string(),
            expected_prompt_path: Some(prompt_path),
            system_prompt: None,
            prompt_file_path: None,
        });
    }

    Ok(LoadedCookPrompt {
        prompt_name: name.to_string(),
        expected_prompt_path: Some(prompt_path.clone()),
        system_prompt: Some(trimmed.to_string()),
        prompt_file_path: Some(prompt_path),
    })
}

#[cfg(test)]
mod tests {
    use super::load_cook_system_prompt;
    use crate::config::Config;

    #[test]
    fn loads_named_cook_prompt_when_present() {
        let root = tempfile::tempdir().expect("temp dir should be created");
        let prompt_dir = root.path().join(".kb/prompts");
        std::fs::create_dir_all(&prompt_dir).expect("prompt dir should be created");
        std::fs::write(prompt_dir.join("cook-default.md"), "hello prompt")
            .expect("prompt file should be written");

        let cfg = Config::default();
        let got = load_cook_system_prompt(root.path(), &cfg)
            .expect("prompt load should succeed");
        assert_eq!(got.prompt_name, "cook-default");
        assert_eq!(got.system_prompt.as_deref(), Some("hello prompt"));
        assert_eq!(
            got.prompt_file_path,
            Some(prompt_dir.join("cook-default.md"))
        );
        assert_eq!(
            got.expected_prompt_path,
            Some(prompt_dir.join("cook-default.md"))
        );
    }

    #[test]
    fn returns_none_when_prompt_is_missing() {
        let root = tempfile::tempdir().expect("temp dir should be created");
        let cfg = Config::default();
        let got = load_cook_system_prompt(root.path(), &cfg)
            .expect("prompt load should succeed");
        assert_eq!(got.prompt_name, "cook-default");
        assert!(got.system_prompt.is_none());
        assert!(got.prompt_file_path.is_none());
        assert!(got.expected_prompt_path.is_some());
    }
}
