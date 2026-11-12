mod cli;
mod comandos;
mod daemon;

use anyhow::{Result, bail};
use clap::Parser;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn find_beca_daemon_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    let self_pid = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(pid) = name.parse::<u32>() else { continue };
        if pid == self_pid {
            continue;
        }
        let cmdline = match std::fs::read(entry.path().join("cmdline")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let parts: Vec<String> = cmdline
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).to_string())
            .collect();
        if parts.len() >= 2 {
            let exe = parts[0].rsplit('/').next().unwrap_or(&parts[0]);
            if exe == "beca" && parts[1] == "daemon" {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn restart_full() -> Result<()> {
    let pids = find_beca_daemon_pids();
    for pid in &pids {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    if !pids.is_empty() {
        std::thread::sleep(Duration::from_millis(500));
    }
    let script = PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".config/hypr/scripts/beca-autostart.sh");
    if script.exists() {
        let status = Command::new("bash")
            .arg(&script)
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run autostart: {e}"))?;
        if status.success() {
            println!("{}", serde_json::json!({"ok": true, "restarted": true, "pids": pids}));
            return Ok(());
        }
        bail!("autostart exited with status {status}");
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start daemon: {e}"))?;
    println!("{}", serde_json::json!({"ok": true, "restarted": true, "pids": pids, "mode": "daemon-only"}));
    Ok(())
}

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Mod {
            target,
            local,
            args,
        } => {
            if target == "list" {
                for (name, desc) in comandos::list_modules() {
                    println!("{name}\t{desc}");
                }
                return Ok(());
            }
            let (module, action) = cli::parse_target(&target)?;
            if action == "info" {
                if let Some(info) = comandos::module_info(&module) {
                    println!("{info}");
                    return Ok(());
                }
                bail!("unknown module: {module} (available: bar, launcher, voice)");
            }

            let no_daemon = local || std::env::var_os("BECA_NO_DAEMON").is_some();
            if !no_daemon && target != "voice:transcribe" {
                if let Some(resp) = daemon::try_route_mod(&target, &args) {
                    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {

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
        cli::Commands::Restart => restart_full(),
    }
}
