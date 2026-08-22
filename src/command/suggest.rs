use crate::command::open_db;
use crate::config::Config;
use crate::database::rebuild::RebuildOptions;
use crate::document::metadata_block::{missing_required_fields, to_yaml, upsert_metadata_block};
use crate::pathing::resolve_from_root;
use crate::pipeline::{DefaultSuggestOperation, SuggestOperation};
use crate::provider::{apply_rules, build_provider, derive_rules};
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Review action for suggested metadata.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SuggestAction {
    Accept,
    Edit,
    Reject,
}

/// Handles `kbmeta suggest FILE`.
pub(crate) fn run(
    root: &Path,
    config_override: Option<&Path>,
    file: &Path,
    action: Option<SuggestAction>,
    edit_file: Option<&Path>,
    editor_override: Option<&str>,
) -> Result<()> {
    eprintln!("warning: 'suggest' is deprecated; use 'cook' for interactive metadata flow");
    let cfg = Config::load_with(root, config_override)?;
    let doc_path = resolve_from_root(root, file);
    let edit_path = edit_file.map(|p| resolve_from_root(root, p));
    let original = fs::read_to_string(&doc_path)
        .with_context(|| format!("failed to read {}", doc_path.display()))?;

    let docs_root = resolve_from_root(root, &cfg.documents_root);
    let report = crate::document::discovery::discover_documents_lenient(&docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;
    for warning in report.warnings {
        eprintln!("warning: skipped {}: {}", warning.path.display(), warning.message);
    }
    let rules = derive_rules(&report.documents, &doc_path);

    // Validate current metadata syntax if present before suggesting replacement.
    let _ = crate::document::metadata_block::parse_markdown(&original)
        .with_context(|| format!("failed to parse {}", doc_path.display()))?;

    let provider = build_provider(&cfg)?;
    let suggest_op = DefaultSuggestOperation;
    let mut suggestion = suggest_op.run(&doc_path, &original, &rules, provider.as_ref(), None)?;

    println!("Suggested metadata for {}:\n", doc_path.display());
    print_yaml(&suggestion)?;

    let decision = decide_action(action)?;
    match decision {
        SuggestAction::Reject => {
            println!("Suggestion rejected. No changes written.");
            return Ok(());
        }
        SuggestAction::Edit => {
            suggestion = apply_edit(
                root,
                suggestion,
                edit_path.as_deref(),
                editor_override,
                cfg.editor.as_deref(),
            )?;
            // Re-apply KB canonicalization after manual edits, including tag slug normalization.
            suggestion = apply_rules(suggestion, &rules);
            let missing = missing_required_fields(&suggestion);
            if !missing.is_empty() {
                bail!(
                    "edited metadata is invalid: missing required fields: {}",
                    missing.join(", ")
                );
            }
        }
        SuggestAction::Accept => {
            let missing = missing_required_fields(&suggestion);
            if !missing.is_empty() {
                bail!(
                    "suggested metadata is invalid: missing required fields: {}",
                    missing.join(", ")
                );
            }
        }
    }

    let updated = upsert_metadata_block(&original, &suggestion)?;
    atomic_write(&doc_path, &updated)?;
    rebuild_after_write(root, &cfg)?;

    println!("Metadata saved to {}", doc_path.display());
    Ok(())
}

pub(crate) fn decide_action(action: Option<SuggestAction>) -> Result<SuggestAction> {
    if let Some(action) = action {
        return Ok(action);
    }

    print!("Action [a=accept, e=edit, r=reject]: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let answer = buf.trim().to_ascii_lowercase();

    match answer.as_str() {
        "a" | "accept" => Ok(SuggestAction::Accept),
        "e" | "edit" => Ok(SuggestAction::Edit),
        "r" | "reject" => Ok(SuggestAction::Reject),
        _ => bail!("invalid action, expected accept|edit|reject"),
    }
}

pub(crate) fn apply_edit(
    root: &Path,
    current: okf::Frontmatter,
    edit_file: Option<&Path>,
    editor_override: Option<&str>,
    config_editor: Option<&str>,
) -> Result<okf::Frontmatter> {
    if let Some(edit_file) = edit_file {
        let edited = fs::read_to_string(edit_file)
            .with_context(|| format!("failed to read {}", edit_file.display()))?;
        let metadata = parse_yaml_as_frontmatter(&edited)
            .with_context(|| format!("invalid YAML in {}", edit_file.display()))?;
        return Ok(metadata);
    }

    if !io::stdin().is_terminal() {
        bail!(
            "interactive edit requires a TTY; use --edit-file for non-interactive runs"
        );
    }

    let editor = resolve_editor(editor_override, config_editor);
    let tmp = temp_edit_path(root);
    fs::write(&tmp, to_yaml(&current)?).with_context(|| format!("failed to write {}", tmp.display()))?;

    loop {
        launch_editor(&editor, &tmp)?;

        let edited = fs::read_to_string(&tmp)
            .with_context(|| format!("failed to read {}", tmp.display()))?;
        let parsed = parse_yaml_as_frontmatter(&edited);

        match parsed {
            Ok(metadata) => {
                let missing = missing_required_fields(&metadata);
                if missing.is_empty() {
                    let _ = fs::remove_file(&tmp);
                    return Ok(metadata);
                }

                eprintln!(
                    "edited metadata is invalid: missing required fields: {}",
                    missing.join(", ")
                );
            }
            Err(err) => {
                eprintln!("edited YAML is invalid: {}", err);
            }
        }

        if !should_reopen_editor()? {
            let _ = fs::remove_file(&tmp);
            bail!("edit aborted after invalid metadata");
        }
    }
}

fn parse_yaml_as_frontmatter(yaml: &str) -> Result<okf::Frontmatter> {
    let doc = okf::Document::parse(yaml)
        .map_err(|e| anyhow::anyhow!("invalid YAML: {}", e))?;
    if !doc.frontmatter.is_empty() {
        return Ok(doc.frontmatter);
    }
    let val = okf::Value::parse(yaml.trim())
        .map_err(|e| anyhow::anyhow!("invalid YAML: {}", e))?;
    let mapping = val.as_mapping()
        .ok_or_else(|| anyhow::anyhow!("edited YAML is not a mapping"))?;
    Ok(okf::Frontmatter::from_mapping(mapping.clone()))
}

fn resolve_editor(editor_override: Option<&str>, config_editor: Option<&str>) -> String {
    if let Some(editor) = editor_override {
        return editor.to_string();
    }
    if let Some(editor) = config_editor {
        return editor.to_string();
    }
    if let Ok(editor) = std::env::var("VISUAL") {
        if !editor.trim().is_empty() {
            return editor;
        }
    }
    if let Ok(editor) = std::env::var("EDITOR") {
        if !editor.trim().is_empty() {
            return editor;
        }
    }
    "vi".to_string()
}

fn launch_editor(editor: &str, file: &Path) -> Result<()> {
    let parts: Vec<&str> = editor.split_whitespace().collect();
    if parts.is_empty() {
        bail!("editor command is empty");
    }

    let mut cmd = Command::new(parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    let status = cmd
        .arg(file)
        .status()
        .with_context(|| format!("failed to launch editor '{}': {}", editor, file.display()))?;

    if !status.success() {
        bail!("editor exited with non-zero status: {}", status);
    }
    Ok(())
}

fn should_reopen_editor() -> Result<bool> {
    print!("Reopen editor? [y/N]: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let answer = buf.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn temp_edit_path(root: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    root.join(format!(".kb/suggest-edit-{}-{}.yaml", std::process::id(), nanos))
}

pub(crate) fn print_yaml(metadata: &okf::Frontmatter) -> Result<()> {
    let yaml = to_yaml(metadata)?;
    println!("{}", yaml.trim_end());
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let tmp = parent.join(format!(".{}.kbmeta.tmp", path.file_name().unwrap_or_default().to_string_lossy()));

    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to replace {} with temp file", path.display()))?;
    Ok(())
}

fn rebuild_after_write(root: &Path, cfg: &Config) -> Result<()> {
    let docs_root = resolve_from_root(root, &cfg.documents_root);
    let docs = crate::document::discovery::discover_documents_strict(&docs_root)
        .with_context(|| format!("failed to scan {}", docs_root.display()))?;

    let mut conn = open_db(root)?;
    crate::database::rebuild::rebuild_index(&mut conn, &docs, RebuildOptions::default())?;
    Ok(())
}
