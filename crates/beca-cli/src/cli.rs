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
    /// Run a module action (e.g. `bar:show`, `voice:quick`)
    Mod {
        target: String,

        #[arg(long)]
        local: bool,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run a Lua script from disk or the config scripts directory
    Run {
        script: String,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Manage configuration files and the running daemon
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Start the IPC daemon
    Daemon,

    /// Check whether the daemon is reachable
    Ping,

    /// Stream Hyprland events from the daemon
    Subscribe,

    /// Stop and relaunch Beca processes
    Restart {
        /// Restart daemon, bar, launcher and voice as local module processes
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Reload configuration in the running daemon
    Reload,

    /// Create the default config and theme files
    Init {
        #[arg(long)]
        force: bool,
    },
}

/// Splits `<module>:<action>` into its two parts.
pub fn parse_target(target: &str) -> anyhow::Result<(String, String)> {
    let (module, action) = target.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("invalid target '{target}', expected <module>:<action> (e.g. voice:show)")
    })?;
    if module.is_empty() || action.is_empty() {
        anyhow::bail!("invalid target '{target}', expected <module>:<action>");
    }
    Ok((module.to_string(), action.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_splits_module_and_action() {
        assert_eq!(
            parse_target("voice:show").unwrap(),
            ("voice".to_string(), "show".to_string())
        );
    }

    #[test]
    fn parse_target_rejects_invalid_targets() {
        assert!(parse_target("voice").is_err());
        assert!(parse_target(":show").is_err());
        assert!(parse_target("voice:").is_err());
    }
}
