use crate::command::prelude::{CommandContext, prompt_yn};
use crate::config::Config;
use anyhow::Result;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;

/// Handles `kbmeta init`.
pub(crate) fn run(ctx: &CommandContext) -> Result<()> {
    let kb_dir = Config::kb_dir(ctx.root());
    if kb_dir.exists() {
        if !should_rewrite_kb_dir(&kb_dir)? {
            println!("Skipped init. Existing {} was kept.", kb_dir.display());
            return Ok(());
        }
        fs::remove_dir_all(&kb_dir)?;
    }

    Config::write_default_with(ctx.root(), ctx.config_override())?;
    ensure_default_cook_prompt(ctx.root())?;
    let _conn = ctx.open_db()?;

    println!("Initialized kbmeta state in {}", kb_dir.display());
    Ok(())
}

fn should_rewrite_kb_dir(kb_dir: &Path) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        eprintln!(
            "note: {} already exists; non-interactive init keeps existing .kb (no rewrite)",
            kb_dir.display()
        );
        return Ok(false);
    }

    prompt_yn(&format!("{} already exists. Rewrite existing .kb folder?", kb_dir.display()))
}

fn ensure_default_cook_prompt(root: &Path) -> Result<()> {
    let prompt_dir = crate::pathing::resolve_from_root(root, Path::new(".kb/prompts"));
    fs::create_dir_all(&prompt_dir)?;

    let prompt_path = prompt_dir.join("cook-default.md");
    if prompt_path.exists() {
        return Ok(());
    }

    fs::write(&prompt_path, default_cook_prompt())?;
    Ok(())
}

fn default_cook_prompt() -> &'static str {
    "You are preparing metadata for a knowledge-base markdown document.\n\nRules:\n- Return JSON only.\n- Allowed keys only: type, title, description, tags, topics, technologies, languages.\n- Use arrays of strings for tags/topics/technologies/languages.\n- Keep values concise and technical.\n- Normalize tags to lowercase kebab-case (no spaces).\n- Do not include markdown, comments, or explanations.\n- Prefer a specific, stable title suitable for a KB document.\n- Description should be one sentence and factual.\n\nQuality checks:\n- type must be non-empty.\n- title must be non-empty.\n- description must be non-empty.\n- Drop empty items from arrays.\n- Avoid duplicate values (case-insensitive).\n"
}
