mod clean;
mod cook;
mod index;
mod init;
pub(crate) mod prelude;
mod query;
mod scan;

use crate::cli::Command;
use crate::config::Config;
use anyhow::{Context, Result};
use prelude::CommandContext;
use rusqlite::Connection;
use std::path::Path;

/// Dispatches CLI commands to their dedicated module handlers.
pub fn execute(
    root: &Path,
    config_override: Option<&Path>,
    global_dry_run: bool,
    command: Command,
) -> Result<()> {
    let ctx = CommandContext::new(root, config_override, global_dry_run);

    match command {
        Command::Init => init::run(&ctx),
        Command::Index {
            strict,
            normalize_tags,
            paths,
            rewrite,
        } => index::run(&ctx, strict, normalize_tags, &paths, rewrite),
        Command::Scan { skip_invalid } => scan::run(&ctx, skip_invalid),
        Command::Query {
            doc_type,
            tags,
            topics,
            technologies,
            languages,
            title,
            format,
            fields,
            limit,
            fail_empty,
            json,
        } => query::run(
            root,
            &query::QueryArgs {
                doc_type,
                tags,
                topics,
                technologies,
                languages,
                title,
                format,
                fields,
                limit,
                fail_empty,
                json,
            },
        ),
        Command::Cook { input, accept } => cook::run(&ctx, &input, accept),
        Command::Clean {
            input,
            output,
            verbose,
            backup,
        } => clean::run(&ctx, &input, output.as_deref(), verbose, backup),
    }
}

pub(crate) fn open_db(root: &Path) -> Result<Connection> {
    std::fs::create_dir_all(Config::kb_dir(root))
        .with_context(|| "failed to create .kb directory")?;
    let conn = Connection::open(Config::db_path(root))
        .with_context(|| "failed to open SQLite database")?;
    crate::database::schema::ensure_schema(&conn)?;
    Ok(conn)
}
