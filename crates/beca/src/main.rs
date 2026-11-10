mod cli;
mod comandos;
mod daemon;

use anyhow::{Result, bail};
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Mod {
            target,
            local,
            args,
        } => {
            // Route via daemon when available (except voice:transcribe which must preserve pipe/stdout contract)
            let no_daemon = local || std::env::var_os("BECA_NO_DAEMON").is_some();
            if !no_daemon && target != "voice:transcribe" {
                if let Some(resp) = daemon::try_route_mod(&target, &args) {
                    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        // print daemon response for observability (spawn/status/lifecycle)
                        println!("{}", serde_json::to_string(&resp)?);
                        return Ok(());
                    } else {
                        let err = resp
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("daemon routing failed");
                        bail!("{err}");
                    }
                }
            }
            let (module, action) = cli::parse_target(&target)?;
            comandos::dispatch(&module, &action, &args)
        }
        cli::Commands::Run { script, args } => comandos::runner::run_script(&script, &args),
        cli::Commands::Config { action } => match action {
            cli::ConfigAction::Reload => {
                let resp = daemon::send_request(&serde_json::json!({"type":"config.reload"}))?;
                println!("{}", serde_json::to_string(&resp)?);
                if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    Ok(())
                } else {
                    bail!(
                        "{}",
                        resp.get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("config reload failed")
                    )
                }
            }
        },
        cli::Commands::Daemon => daemon::run(),
        cli::Commands::Ping => {
            let resp = daemon::send_request(&serde_json::json!({"type":"ping"}))?;
            println!("{}", serde_json::to_string(&resp)?);
            if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                Ok(())
            } else {
                bail!("daemon ping failed")
            }
        }
        cli::Commands::Subscribe => {
            use std::io::{BufRead, BufReader, Write};
            use std::os::unix::net::UnixStream;
            let path = daemon::socket_path();
            let mut stream = UnixStream::connect(&path).map_err(|e| {
                anyhow::anyhow!("failed to connect to daemon at {}: {e}", path.display())
            })?;
            writeln!(
                stream,
                "{}",
                serde_json::json!({"type":"hyprland.subscribe"})
            )?;
            stream.flush()?;
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let line = line?;
                println!("{line}");
            }
            Ok(())
        }
    }
}
