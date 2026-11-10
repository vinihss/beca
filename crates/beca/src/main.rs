mod daemon;
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
    /// Run a Lua script with the `beca.*` runtime.
    /// Path can be absolute, relative, or a name resolved from $XDG_CONFIG_HOME/beca/scripts/
    Run {
        /// Script path or name
        script: String,
        /// Arguments passed as `arg` table to the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the beca daemon (IPC control socket, Hyprland bridge foundation).
    Daemon,
    /// Send a ping to the running daemon.
    Ping,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Mod { target, args } => {
            let (module, action) = parse_target(&target)?;
            dispatch(&module, &action, &args)
        }
        Commands::Run { script, args } => run_script(&script, &args),
        Commands::Daemon => daemon::run(),
        Commands::Ping => {
            let resp = daemon::send_request(&serde_json::json!({"type":"ping"}))?;
            println!("{}", serde_json::to_string(&resp)?);
            if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                Ok(())
            } else {
                bail!("daemon ping failed")
            }
        }
    }
}

fn run_script(script: &str, args: &[String]) -> Result<()> {
    use anyhow::Context;
    use std::path::Path;

    let path = Path::new(script);
    let code = if path.is_file() {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read script: {}", path.display()))?
    } else {
        // try XDG scripts dir by name
        let paths = beca_core::BecaPaths::resolve();
        let name = if script.ends_with(".lua") || script.ends_with(".luau") {
            script.to_string()
        } else {
            format!("{script}.lua")
        };
        let disk = paths.scripts_dir.join(&name);
        std::fs::read_to_string(&disk).with_context(|| {
            format!(
                "script not found: '{}' (tried file and {})",
                script,
                disk.display()
            )
        })?
    };

    let lua = mlua::Lua::new();
    runtime::install_beca_base(&lua).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // arg table (1-based, like Lua)
    let arg_tbl = lua
        .create_table()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    arg_tbl
        .set(0, script)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    for (i, a) in args.iter().enumerate() {
        arg_tbl
            .set(i + 1, a.as_str())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    lua.globals()
        .set("arg", arg_tbl)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // set module context for reload (best-effort name)
    if let Ok(beca) = lua.globals().get::<mlua::Table>("beca") {
        if let Ok(rt) = beca.get::<mlua::Table>("runtime") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(script)
                .to_string();
            let _ = rt.set("_module", name);
            let _ = rt.set("_embedded", code.as_str());
            let _ = rt.set("_script_path", script);
        }
    }

    lua.load(&code)
        .set_name(script)
        .exec()
        .map_err(|e| anyhow::anyhow!("script failed: {e}"))?;
    runtime::call_hook(&lua, "on_load");
    Ok(())
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
