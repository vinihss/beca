use anyhow::Result;

use crate::cli::{Cli, Commands};

pub mod alert;
pub mod bar;
pub mod config;
pub mod daemon;
pub mod module;
pub mod registry;
pub mod restart;

/// Executes the parsed command line.
pub fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Mod { target, local, args } => module::execute(&target, local, &args),
        Commands::Run { script, args } => beca_core::lua::runner::run_script(&script, &args),
        Commands::Config { action } => config::execute(action),
        Commands::Daemon => daemon::serve(),
        Commands::Ping => daemon::ping(),
        Commands::Subscribe => daemon::subscribe(),
        Commands::Restart { local } => restart::execute(local),
    }
}
