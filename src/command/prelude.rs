use crate::config::Config;
use crate::database::rebuild::RebuildOptions;
use crate::document::discovery::DiscoverWarning;
use crate::document::metadata_block::{missing_required_fields, to_yaml};
use crate::pathing::resolve_from_root;
use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

// ── CommandContext ────────────────────────────────────────────────────

pub(crate) struct CommandContext {
    root: PathBuf,
    config_override: Option<PathBuf>,
    dry_run: bool,
}

impl CommandContext {
    pub fn new(root: &Path, config_override: Option<&Path>, dry_run: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            config_override: config_override.map(Path::to_path_buf),
            dry_run,
        }
    }

    pub fn config_override(&self) -> Option<&Path> {
        self.config_override.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn load_config(&self) -> Result<Config> {
        Config::load_with(&self.root, self.config_override.as_deref())
    }

    pub fn open_db(&self) -> Result<Connection> {
        crate::command::open_db(&self.root)
    }

    pub fn discover_docs(
        &self,
        cfg: &Config,
        strict: bool,
    ) -> Result<(Vec<crate::document::Document>, Vec<DiscoverWarning>)> {
        let docs_root = resolve_from_root(&self.root, &cfg.documents_root);
        if strict {
            let docs =
                crate::document::discovery::discover_documents_strict(&docs_root)
                    .with_context(|| format!("failed to scan {}", docs_root.display()))?;
            Ok((docs, Vec::new()))
        } else {
            let report =
                crate::document::discovery::discover_documents_lenient(&docs_root)
                    .with_context(|| format!("failed to scan {}", docs_root.display()))?;
            Ok((report.documents, report.warnings))
        }
    }

    pub fn discover_docs_with_warnings(&self, cfg: &Config) -> Result<(Vec<crate::document::Document>, Vec<DiscoverWarning>)> {
        let docs_root = resolve_from_root(&self.root, &cfg.documents_root);
        let report =
            crate::document::discovery::discover_documents_lenient(&docs_root)
                .with_context(|| format!("failed to scan {}", docs_root.display()))?;
        for warning in &report.warnings {
            eprintln!(
                "warning: skipped {}: {}",
                warning.path.display(),
                warning.message
            );
        }
        Ok((report.documents, report.warnings))
    }

    pub fn rebuild_index(
        &self,
        _cfg: &Config,
        docs: &[crate::document::Document],
        options: RebuildOptions,
    ) -> Result<()> {
        let mut conn = self.open_db()?;
        crate::database::rebuild::rebuild_index(&mut conn, docs, options)?;
        Ok(())
    }
}

// ── Shared helpers ────────────────────────────────────────────────────

pub(crate) fn make_backup_filename(input: &Path) -> String {
    let stem = input
        .file_stem()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    format!("{}.backup.md", stem)
}

pub(crate) fn prompt_yn(msg: &str) -> Result<bool> {
    print!("{} [y/N]: ", msg);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let answer = buf.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

pub(crate) fn backup_and_write(
    out_path: &Path,
    backup: bool,
    content: &str,
    original: &str,
) -> Result<()> {
    if out_path.exists() {
        bail!("output collision: {} already exists", out_path.display());
    }

    let out_dir = out_path
        .parent()
        .with_context(|| format!("{} has no parent directory", out_path.display()))?;
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    fs::write(out_path, content)
        .with_context(|| format!("failed to write {}", out_path.display()))?;

    if backup {
        let backup_path = out_dir.join(make_backup_filename(out_path));
        if backup_path.exists() {
            bail!(
                "backup collision: {} already exists",
                backup_path.display()
            );
        }
        fs::write(&backup_path, original)
            .with_context(|| format!("failed to write {}", backup_path.display()))?;
        println!("backup copy: {}", backup_path.display());
    }

    Ok(())
}

pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let tmp = parent.join(format!(
        ".{}.kbmeta.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to replace {} with temp file", path.display()))?;
    Ok(())
}

// ── Metadata action (absorbed from suggest) ──────────────────────────

/// Review action for suggested metadata.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MetadataAction {
    Accept,
    Edit,
    Reject,
}

pub(crate) fn decide_action(action: Option<MetadataAction>) -> Result<MetadataAction> {
    if let Some(action) = action {
        return Ok(action);
    }

    print!("Action [a=accept, e=edit, r=reject]: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let answer = buf.trim().to_ascii_lowercase();

    match answer.as_str() {
        "a" | "accept" => Ok(MetadataAction::Accept),
        "e" | "edit" => Ok(MetadataAction::Edit),
        "r" | "reject" => Ok(MetadataAction::Reject),
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
    fs::write(&tmp, to_yaml(&current)?)
        .with_context(|| format!("failed to write {}", tmp.display()))?;

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

pub(crate) fn print_yaml(metadata: &okf::Frontmatter) -> Result<()> {
    let yaml = to_yaml(metadata)?;
    println!("{}", yaml.trim_end());
    Ok(())
}

fn parse_yaml_as_frontmatter(yaml: &str) -> Result<okf::Frontmatter> {
    let doc = okf::Document::parse(yaml)
        .map_err(|e| anyhow::anyhow!("invalid YAML: {}", e))?;
    if !doc.frontmatter.is_empty() {
        return Ok(doc.frontmatter);
    }
    let val = okf::Value::parse(yaml.trim())
        .map_err(|e| anyhow::anyhow!("invalid YAML: {}", e))?;
    let mapping = val
        .as_mapping()
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

    let mut cmd = ProcessCommand::new(parts[0]);
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
    root.join(format!(
        ".kb/suggest-edit-{}-{}.yaml",
        std::process::id(),
        nanos
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_filename_from_path() {
        let input = Path::new("/tmp/my-doc.md");
        assert_eq!(make_backup_filename(input), "my-doc.backup.md");
    }

    #[test]
    fn backup_filename_fallback() {
        let input = Path::new("/");
        assert_eq!(make_backup_filename(input), "document.backup.md");
    }
}
