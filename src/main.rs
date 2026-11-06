mod cli;
mod command;
mod config;
mod database;
mod document;
mod error;
mod llm_answers;
mod logging;
mod pathing;
mod pipeline;
mod provider;
mod query;
mod vocabulary;

use clap::Parser;
use cli::Cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    logging::init(cli.root.as_path());
    llm_answers::init(cli.root.as_path());
    match command::execute(&cli.root, cli.config.as_deref(), cli.dry_run, cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("Error: {err:#}");
            ExitCode::from(error::exit_code(&err) as u8)
        }
    }
}
