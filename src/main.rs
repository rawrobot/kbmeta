mod cli;
mod command;
mod config;
mod database;
mod document;
mod pathing;
mod pipeline;
mod provider;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    command::execute(&cli.root, cli.config.as_deref(), cli.dry_run, cli.command)
}
