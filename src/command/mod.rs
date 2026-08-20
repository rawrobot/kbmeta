mod clean;
mod cook;
mod context;
mod index;
mod index_folders;
mod init;
mod rebuild;
mod scan;
mod suggest;

use crate::cli::{Command, SuggestActionArg};
use crate::config::Config;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

/// Dispatches CLI commands to their dedicated module handlers.
pub fn execute(
    root: &Path,
    config_override: Option<&Path>,
    global_dry_run: bool,
    command: Command,
) -> Result<()> {
    let config_override = config_override.map(Path::to_path_buf);

    match command {
        Command::Init => init::run(root, config_override.as_deref()),
        Command::Index {
            strict,
            normalize_tags,
            paths,
            rewrite,
        } => index::run(
            root,
            config_override.as_deref(),
            strict,
            normalize_tags,
            &paths,
            rewrite,
            global_dry_run,
        ),
        Command::Rebuild {
            strict,
            normalize_tags,
            paths,
        } => {
            eprintln!("warning: 'rebuild' is deprecated; use 'index' instead");
            rebuild::run(
                root,
                config_override.as_deref(),
                strict,
                normalize_tags,
                &paths,
                global_dry_run,
            )
        }
        Command::Scan { skip_invalid } => scan::run(root, config_override.as_deref(), skip_invalid),
        Command::Suggest {
            file,
            action,
            edit_file,
            editor,
        } => {
            eprintln!("warning: 'suggest' is deprecated; use 'cook' for interactive metadata flow");
            suggest::run(
                root,
                config_override.as_deref(),
                &file,
                action.map(map_suggest_action),
                edit_file.as_deref(),
                editor.as_deref(),
            )
        }
        Command::Cook { input, accept } => {
            cook::run(root, config_override.as_deref(), &input, accept)
        }
        Command::IndexFolders { rewrite } => {
            eprintln!("warning: 'index-folders' is deprecated; use 'index' instead");
            index_folders::run(root, config_override.as_deref(), rewrite, global_dry_run)
        }
        Command::Clean {
            input,
            output,
            verbose,
            backup,
        } => clean::run(
            root,
            &input,
            output.as_deref(),
            global_dry_run,
            verbose,
            backup,
        ),
        Command::Context {
            input,
            output,
            verbose,
            backup,
        } => {
            eprintln!("warning: 'context' is deprecated; use 'cook' instead");
            context::run(
                root,
                &input,
                output.as_deref(),
                global_dry_run,
                verbose,
                backup,
            )
        }
    }
}

fn map_suggest_action(action: SuggestActionArg) -> suggest::SuggestAction {
    match action {
        SuggestActionArg::Accept => suggest::SuggestAction::Accept,
        SuggestActionArg::Edit => suggest::SuggestAction::Edit,
        SuggestActionArg::Reject => suggest::SuggestAction::Reject,
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
