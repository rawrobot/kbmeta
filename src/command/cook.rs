use crate::config::Config;
use crate::document::metadata_block::{missing_required_fields, upsert_metadata_block};
use crate::pathing::resolve_from_root;
use crate::pipeline::{
    DefaultCleanOperation, DefaultContextOperation, DefaultSuggestOperation, rebuild_index_after_write,
    run_cook, write_intermediate_artifacts,
};
use crate::provider::apply_rules;
use crate::command::suggest::{SuggestAction, apply_edit, decide_action, print_yaml};
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Handles `kbmeta cook`.
pub(crate) fn run(
    root: &Path,
    config_override: Option<&Path>,
    input: &Path,
    accept: bool,
) -> Result<()> {
    let cfg = Config::load_with(root, config_override)?;
    let input_path = resolve_from_root(root, input);

    if cfg.provider.kind.trim().eq_ignore_ascii_case("mock") {
        eprintln!(
            "warning: provider kind is 'mock'; prompt files are loaded but metadata output is deterministic sample data"
        );
    }

    let clean_op = DefaultCleanOperation;
    let context_op = DefaultContextOperation;
    let suggest_op = DefaultSuggestOperation;

    let mut cooked = run_cook(
        root,
        &cfg,
        &input_path,
        &clean_op,
        &context_op,
        &suggest_op,
    )?;

    if let Some(path) = &cooked.prompt_file_path {
        println!("prompt file: {}", path.display());
    } else if let Some(path) = &cooked.expected_prompt_path {
        println!(
            "prompt file: built-in provider default (missing {} for prompt_name={})",
            path.display(),
            cooked.prompt_name
        );
    } else {
        println!("prompt file: built-in provider default (cook.prompt_name is empty)");
    }

    println!("Suggested metadata for {}:\n", input_path.display());
    print_yaml(&cooked.metadata)?;

    let decision = if accept {
        SuggestAction::Accept
    } else {
        if !std::io::stdin().is_terminal() {
            bail!("cook requires interactive TTY unless --accept is set for batch mode");
        }
        decide_action(None)?
    };

    match decision {
        SuggestAction::Reject => {
            println!("Suggestion rejected. No changes written.");
            return Ok(());
        }
        SuggestAction::Edit => {
            let edited = apply_edit(
                root,
                cooked.metadata.clone(),
                None,
                None,
                cfg.editor.as_deref(),
            )?;
            cooked.metadata = apply_rules(edited, &cooked.rules);
            let missing = missing_required_fields(&cooked.metadata);
            if !missing.is_empty() {
                bail!(
                    "edited metadata is invalid: missing required fields: {}",
                    missing.join(", ")
                );
            }
            cooked.final_markdown = upsert_metadata_block(&cooked.clean.markdown, &cooked.metadata)?;
        }
        SuggestAction::Accept => {}
    }

    if cooked.output_path.exists() {
        if accept {
            bail!(
                "output collision: {} already exists",
                cooked.output_path.display()
            );
        }

        if !confirm_overwrite(&cooked.output_path)? {
            println!("Output kept unchanged: {}", cooked.output_path.display());
            return Ok(());
        }
    }

    let parent = cooked
        .output_path
        .parent()
        .with_context(|| format!("{} has no parent directory", cooked.output_path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    fs::write(&cooked.output_path, &cooked.final_markdown)
        .with_context(|| format!("failed to write {}", cooked.output_path.display()))?;

    if cfg.keep_intermediate {
        let base_name = PathBuf::from(&cooked.clean.filename)
            .file_stem()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".to_string());
        write_intermediate_artifacts(root, &base_name, &cooked)?;
    }

    rebuild_index_after_write(root, &cfg)?;

    println!("kb-ready markdown: {}", cooked.output_path.display());
    Ok(())
}

fn confirm_overwrite(path: &Path) -> Result<bool> {
    print!("{} already exists. Overwrite? [y/N]: ", path.display());
    io::stdout().flush()?;

    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(is_yes(&buf))
}

fn is_yes(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::is_yes;

    #[test]
    fn recognizes_affirmative_answers() {
        assert!(is_yes("y"));
        assert!(is_yes("Y"));
        assert!(is_yes("yes"));
        assert!(is_yes(" Yes "));
    }

    #[test]
    fn rejects_non_affirmative_answers() {
        assert!(!is_yes(""));
        assert!(!is_yes("n"));
        assert!(!is_yes("no"));
        assert!(!is_yes("anything else"));
    }
}
