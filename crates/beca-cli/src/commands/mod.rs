use anyhow::Result;

use crate::cli::{Cli, Commands};

pub mod bar;
pub mod config;
pub mod daemon;
pub mod launcher;
pub mod module;
pub mod registry;
pub mod restart;
pub mod run;
pub mod voice;

/// Executes the parsed command line.
pub fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Mod { target, local, args } => module::execute(&target, local, &args),
        Commands::Run { script, args } => run::run_script(&script, &args),
        Commands::Config { action } => config::execute(action),
        Commands::Daemon => daemon::serve(),
        Commands::Ping => daemon::ping(),
        Commands::Subscribe => daemon::subscribe(),
        Commands::Restart { local } => restart::execute(local),
    }
}
