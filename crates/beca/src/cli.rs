use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "beca",
    version,
    about = "beca — Hyprland/Wayland shell UI (Rust + Slint + Lua)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {

    Mod {

        target: String,

        #[arg(long)]
        local: bool,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    Run {

        script: String,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    Daemon,

    Ping,

    Subscribe,

    Restart {
        /// Stop and relaunch daemon + bar/launcher/voice as local module processes
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {

    Reload,

    Init {

        #[arg(long)]
        force: bool,
    },
}

pub fn parse_target(target: &str) -> anyhow::Result<(String, String)> {
    let (m, a) = target.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("invalid target '{target}', expected <module>:<action> (e.g. voice:show)")
    })?;
    if m.is_empty() || a.is_empty() {
        anyhow::bail!("invalid target '{target}', expected <module>:<action>");
    }
    Ok((m.to_string(), a.to_string()))
}
