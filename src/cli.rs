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

    /// Scan documents and report missing/stale metadata.
    Scan {
        /// Skip invalid metadata documents and report warnings instead of failing.
        #[arg(long)]
        skip_invalid: bool,
    },
    /// Query indexed documents with metadata filters.
    Query {
        /// Filter by type (exact match).
        #[arg(short = 't', long)]
        doc_type: Option<String>,
        /// Tag expression, e.g. "+debug,rust,-wip".
        #[arg(short = 'g', long = "tag", allow_hyphen_values = true)]
        tags: Option<String>,
        /// Topic expression, e.g. "+rust,concurrency".
        #[arg(short = 'o', long = "topic", allow_hyphen_values = true)]
        topics: Option<String>,
        /// Technology expression.
        #[arg(short = 'k', long = "tech", allow_hyphen_values = true)]
        technologies: Option<String>,
        /// Language expression.
        #[arg(short = 'l', long = "lang", allow_hyphen_values = true)]
        languages: Option<String>,
        /// Case-insensitive title substring match.
        #[arg(short = 'n', long)]
        title: Option<String>,
        /// Output format.
        #[arg(short = 'f', long, value_enum, default_value_t = QueryFormatArg::Files)]
        format: QueryFormatArg,
        /// Fields to print for csv/table/json (comma-separated).
        #[arg(short = 'F', long)]
        fields: Option<String>,
        /// Limit number of results.
        #[arg(short = 'L', long)]
        limit: Option<usize>,
        /// Return non-zero when no results match.
        #[arg(long)]
        fail_empty: bool,
        /// Shortcut for JSON output (same as --format json).
        #[arg(short = 'j', long)]
        json: bool,
    },

    /// Broad relevance search over indexed metadata, optionally constrained by filters.
    Search {
        /// Search text; terms are matched across title, description, tags, topics,
        /// technologies and languages.
        text: String,
        /// Restrict to documents of this type (exact match).
        #[arg(short = 't', long)]
        doc_type: Option<String>,
        /// Tag expression, e.g. "+debug,rust,-wip".
        #[arg(short = 'g', long = "tag", allow_hyphen_values = true)]
        tags: Option<String>,
        /// Topic expression, e.g. "+rust,concurrency".
        #[arg(short = 'o', long = "topic", allow_hyphen_values = true)]
        topics: Option<String>,
        /// Technology expression.
        #[arg(short = 'k', long = "tech", allow_hyphen_values = true)]
        technologies: Option<String>,
        /// Language expression.
        #[arg(short = 'l', long = "lang", allow_hyphen_values = true)]
        languages: Option<String>,
        /// Restrict to documents whose title contains this text (case-insensitive).
        #[arg(short = 'n', long)]
        title: Option<String>,
        /// Output format.
        #[arg(short = 'f', long, value_enum, default_value_t = SearchFormatArg::Files)]
        format: SearchFormatArg,
        /// Maximum number of results.
        #[arg(short = 'L', long, default_value_t = 10)]
        limit: usize,
        /// Return non-zero when no documents match.
        #[arg(long)]
        fail_empty: bool,
        /// Shortcut for JSON output (same as --format json).
        #[arg(short = 'j', long)]
        json: bool,
    },

    /// One-shot pipeline: clean -> context -> suggest -> kb-ready output.
    Cook {
        /// Input markdown file.
        input: PathBuf,
        /// Accept suggestion without prompt (batch mode).
        #[arg(short = 'a', long)]
        accept: bool,
        /// Overwrite existing output file (only valid with --accept).
        #[arg(short = 'r', long)]
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

}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum QueryFormatArg {
    Files,
    Csv,
    Table,
    Json,
    Markdown,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SearchFormatArg {
    Files,
    Table,
    Json,
    Markdown,
}
