mod modules;
mod runtime;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "beca",
    version,
    about = "beca — Hyprland/Wayland shell UI (Rust + Slint + Lua)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a module action: `beca mod <module>:<action> [args...]`
    /// Example: `beca mod voice:show` | `beca mod launcher:show`
    Mod {
        /// Target in the form <module>:<action> (e.g. voice:show, launcher:show)
        target: String,
        /// Extra arguments passed to the module action
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Mod { target, args } => {
            let (module, action) = parse_target(&target)?;
            dispatch(&module, &action, &args)
        }
    }
}

fn parse_target(target: &str) -> Result<(String, String)> {
    let (m, a) = target.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("invalid target '{target}', expected <module>:<action> (e.g. voice:show)")
    })?;
    if m.is_empty() || a.is_empty() {
        bail!("invalid target '{target}', expected <module>:<action>");
    }
    Ok((m.to_string(), a.to_string()))
}

fn dispatch(module: &str, action: &str, args: &[String]) -> Result<()> {
    match module {
        "launcher" | "apps" => match action {
            "show" => modules::launcher::show(),
            other => bail!("unknown launcher action: {other} (expected: show)"),
        },
        "voice" => modules::voice::dispatch(action, args),
        other => bail!("unknown module: {other} (available: launcher, voice)"),
    }
}
