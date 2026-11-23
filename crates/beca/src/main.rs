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

fn find_beca_bar_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    let self_pid = std::process::id();
    for entry in entries.flatten() {
        let Ok(pid) = entry
            .file_name()
            .to_str()
            .unwrap_or("")
            .parse::<u32>()
        else {
            continue;
        };
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
        if parts.len() >= 4 {
            let exe = parts[0].rsplit('/').next().unwrap_or(&parts[0]);
            if exe == "beca" && parts[1] == "mod" && parts[2] == "--local" {
                // Check if it's a bar process (bar:show-top or bar:show-bottom)
                if parts[3] == "bar:show-top" || parts[3] == "bar:show-bottom" {
                    pids.push(pid);
                }
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}


fn find_beca_pids_matching(pred: impl Fn(&[String]) -> bool) -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    let self_pid = std::process::id();
    for entry in entries.flatten() {
        let Ok(pid) = entry
            .file_name()
            .to_str()
            .unwrap_or("")
            .parse::<u32>()
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let parts: Vec<String> = cmdline
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).to_string())
            .collect();
        if pred(&parts) {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn kill_pids(pids: &[u32]) {
    for pid in pids {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc_uid() })))
        .join("beca")
}

fn libc_uid() -> u32 {
    // fallback without libc dep
    std::env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

fn spawn_logged(exe: &PathBuf, args: &[&str], log: &PathBuf) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| anyhow::anyhow!("open log {}: {e}", log.display()))?;
    let file_err = file
        .try_clone()
        .map_err(|e| anyhow::anyhow!("clone log {}: {e}", log.display()))?;
    Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(file))
        .stderr(std::process::Stdio::from(file_err))
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn {:?} {:?}: {e}", exe, args))?;
    Ok(())
}

fn restart_full() -> Result<()> {
    let daemon_pids = find_beca_daemon_pids();
    let bar_pids = find_beca_bar_pids();
    kill_pids(&daemon_pids);
    kill_pids(&bar_pids);
    if !daemon_pids.is_empty() || !bar_pids.is_empty() {
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
            println!("{}", serde_json::json!({"ok": true, "restarted": true, "pids": daemon_pids}));
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
    println!("{}", serde_json::json!({"ok": true, "restarted": true, "pids": daemon_pids, "mode": "daemon-only"}));
    Ok(())
}

fn restart_local() -> Result<()> {
    let runtime = runtime_dir();
    std::fs::create_dir_all(&runtime).ok();

    let local_mods = find_beca_pids_matching(|parts| {
        parts.len() >= 3
            && parts[0].rsplit('/').next().unwrap_or(&parts[0]) == "beca"
            && parts[1] == "mod"
            && parts[2] == "--local"
    });
    let daemons = find_beca_daemon_pids();
    kill_pids(&local_mods);
    kill_pids(&daemons);
    if !local_mods.is_empty() || !daemons.is_empty() {
        std::thread::sleep(Duration::from_millis(2000));
    }
    if let Ok(entries) = std::fs::read_dir(&runtime) {
        for e in entries.flatten() {
            let p = e.path();
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.ends_with(".sock") || name == "launcher.ctl" || name == "voice.ctl" {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    spawn_logged(&exe, &["daemon"], &runtime.join("daemon.log"))?;
    std::thread::sleep(Duration::from_millis(2000));
    spawn_logged(
        &exe,
        &["mod", "--local", "bar:show"],
        &runtime.join("bar.log"),
    )?;
    std::thread::sleep(Duration::from_millis(1000));
    spawn_logged(
        &exe,
        &["mod", "--local", "launcher:serve"],
        &runtime.join("launcher.log"),
    )?;
    std::thread::sleep(Duration::from_millis(1000));
    spawn_logged(
        &exe,
        &["mod", "--local", "voice:serve"],
        &runtime.join("voice.log"),
    )?;
    std::thread::sleep(Duration::from_millis(2000));

    let running = find_beca_pids_matching(|parts| {
        parts.len() >= 3
            && parts[0].rsplit('/').next().unwrap_or(&parts[0]) == "beca"
            && parts[1] == "mod"
            && parts[2] == "--local"
    });
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "restarted": true,
            "mode": "local",
            "killed_local_mods": local_mods,
            "killed_daemons": daemons,
            "running_local_mods": running,
        })
    );
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
            cli::ConfigAction::Init { force } => {
                let paths = beca_core::config::BecaPaths::resolve();
                let created = beca_core::config::ensure_defaults_with_paths(&paths, force)
                    .map_err(|e| anyhow::anyhow!(e))?;
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "config_dir": paths.config_dir,
                        "created": created,
                        "force": force,
                    })
                );
                Ok(())
            }
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
        cli::Commands::Restart { local } => {
            if local {
                restart_local()
            } else {
                restart_full()
            }
        }
    }
}
