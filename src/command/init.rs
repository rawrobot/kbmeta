use crate::command::prelude::{prompt_yn, CommandContext};
use crate::config::Config;
use anyhow::{Context, Result};
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
    ensure_vocab(ctx.root())?;
    let _conn = ctx.open_db()?;

    println!("Initialized kbmeta state in {}", kb_dir.display());
    Ok(())
}

/// Copies the controlled vocabularies from `doc/knowledge` into `.kb/vocab`
/// when present. Missing source files are skipped silently.
fn ensure_vocab(root: &Path) -> Result<()> {
    use crate::vocabulary::{VocabKind, VOCAB_SOURCE_DIR, VOCAB_SUBDIR};

    let source_dir = crate::pathing::resolve_from_root(root, Path::new(VOCAB_SOURCE_DIR));
    let kb_vocab_dir = crate::config::Config::kb_dir(root).join(VOCAB_SUBDIR);

    for kind in [
        VocabKind::Tags,
        VocabKind::Topics,
        VocabKind::Technologies,
        VocabKind::Languages,
        VocabKind::DocTypes,
    ] {
        let src = source_dir.join(kind.filename());
        if !src.exists() {
            continue;
        }
        fs::create_dir_all(&kb_vocab_dir)
            .with_context(|| format!("failed to create {}", kb_vocab_dir.display()))?;
        let dst = kb_vocab_dir.join(kind.filename());
        if dst.exists() {
            continue;
        }
        fs::copy(&src, &dst)
            .with_context(|| format!("failed to copy {} to {}", src.display(), dst.display()))?;
    }

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

    prompt_yn(&format!(
        "{} already exists. Rewrite existing .kb folder?",
        kb_dir.display()
    ))
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
    include_str!("../prompts/default.md")
}
