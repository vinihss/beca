mod cli;
mod commands;
mod ipc;
mod output;
mod process;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    commands::execute(cli::Cli::parse())
}
