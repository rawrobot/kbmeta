use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Top-level CLI arguments for kbmeta.
#[derive(Debug, Parser)]
#[command(name = "kbmeta", version, about = "Knowledge base metadata CLI")]
pub struct Cli {
    /// Workspace root where `.kb` and knowledge documents are resolved.
    #[arg(short = 'r', long, default_value = ".")]
    pub root: PathBuf,

    /// Optional config path; takes precedence over `KBMETA_CONFIG` and default location.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Show planned actions without writing files.
    #[arg(long)]
    pub dry_run: bool,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Phase 1 command surface.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize local kbmeta state.
    Init,
    /// Rebuild SQLite index and generate folder `index.md` files.
    Index {
        /// Fail if any Markdown file has invalid metadata.
        #[arg(long)]
        strict: bool,
        /// Normalize tags before indexing (lowercase, trimmed, spaces to dashes).
        #[arg(long)]
        normalize_tags: bool,
        /// Reindex only these document subfolders (relative to documents_root).
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
        /// Rewrite existing `index.md` files.
        #[arg(long)]
        rewrite: bool,
    },
    /// Rebuild SQLite index from Markdown source files.
    #[command(hide = true)]
    Rebuild {
        /// Fail if any Markdown file has invalid metadata.
        #[arg(long)]
        strict: bool,
        /// Normalize tags before indexing (lowercase, trimmed, spaces to dashes).
        #[arg(long)]
        normalize_tags: bool,
        /// Reindex only these document subfolders (relative to documents_root).
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
    },
    /// Scan documents and report missing/stale metadata.
    Scan {
        /// Skip invalid metadata documents and report warnings instead of failing.
        #[arg(long)]
        skip_invalid: bool,
    },
    /// Suggest metadata for one Markdown document.
    #[command(hide = true)]
    Suggest {
        /// Path to the Markdown file, relative to workspace root or absolute.
        file: PathBuf,
        /// Optional non-interactive action for CI or scripted runs.
        #[arg(long, value_enum)]
        action: Option<SuggestActionArg>,
        /// YAML file used when action is edit.
        #[arg(long)]
        edit_file: Option<PathBuf>,
        /// Editor command used for interactive edit mode.
        #[arg(long)]
        editor: Option<String>,
    },
    /// One-shot pipeline: clean -> context -> suggest -> kb-ready output.
    Cook {
        /// Input markdown file.
        input: PathBuf,
        /// Accept suggestion without prompt (batch mode).
        #[arg(short = 'a', long)]
        accept: bool,
    },
    /// Generate `index.md` files for document folders.
    #[command(hide = true)]
    IndexFolders {
        /// Rewrite existing `index.md` files.
        #[arg(long)]
        rewrite: bool,
    },
    /// Clean a markdown document using source-specific policy.
    Clean {
        /// Input markdown file.
        input: PathBuf,
        /// Output directory for cleaned document.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Show extra processing details.
        #[arg(long)]
        verbose: bool,
        /// Write a backup copy of the input markdown near outputs.
        #[arg(long)]
        backup: bool,
    },
    /// Build compact markdown context for metadata LLM input.
    #[command(hide = true)]
    Context {
        /// Input markdown file.
        input: PathBuf,
        /// Output directory for llm context file.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Show extra processing details.
        #[arg(long)]
        verbose: bool,
        /// Write a backup copy of the input markdown near outputs.
        #[arg(long)]
        backup: bool,
    },
}

/// Review decision for a metadata suggestion.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SuggestActionArg {
    /// Accept suggestion and write metadata.
    Accept,
    /// Edit suggestion before writing metadata.
    Edit,
    /// Reject suggestion and keep document unchanged.
    Reject,
}
