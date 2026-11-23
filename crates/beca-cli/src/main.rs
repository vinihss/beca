mod cli;
mod commands;
mod output;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    commands::execute(cli::Cli::parse())
}
