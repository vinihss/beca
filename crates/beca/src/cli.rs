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
    /// Run a module action: `beca mod <module>:<action> [args...]`
    /// Example: `beca mod voice:show` | `beca mod launcher:show` | `beca mod bar:show-top`
    Mod {
        /// Target in the form <module>:<action> (e.g. voice:show, launcher:show, bar:show-top)
        target: String,
        /// Run locally, do not route via daemon
        #[arg(long)]
        local: bool,
        /// Extra arguments passed to the module action
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run a Lua script with the `beca.*` runtime.
    /// Path can be absolute, relative, or a name resolved from $XDG_CONFIG_HOME/beca/scripts/
    Run {
        /// Script path or name
        script: String,
        /// Arguments passed as `arg` table to the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Manage configuration (e.g. `beca config reload`)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run the beca daemon (IPC control socket, Hyprland bridge foundation).
    Daemon,
    /// Send a ping to the running daemon.
    Ping,
    /// Subscribe to Hyprland events streamed by the daemon (requires `beca daemon`).
    Subscribe,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Force reload: re-read config and restart managed UI modules (bars/launcher).
    Reload,
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
