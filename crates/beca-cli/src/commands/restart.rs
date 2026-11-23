use anyhow::{Result, bail};
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use crate::output;

/// Handles `beca restart [--local]`.
pub fn execute(local: bool) -> Result<()> {
    if local {
        restart_local()
    } else {
        restart_full()
    }
}

/// Restarts the Hyprland autostart-managed daemons and bars.
fn restart_full() -> Result<()> {
    let daemon_pids = beca_process::find_daemon_pids();
    let bar_pids = beca_process::find_bar_pids();
    beca_process::kill_pids(&daemon_pids);
    beca_process::kill_pids(&bar_pids);
    if !daemon_pids.is_empty() || !bar_pids.is_empty() {
        sleep(Duration::from_millis(500));
    }

    let script = PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".config/hypr/scripts/beca-autostart.sh");
    if script.exists() {
        let status = Command::new("bash")
            .arg(&script)
            .status()
            .map_err(|error| anyhow::anyhow!("failed to run autostart: {error}"))?;
        if !status.success() {
            bail!("autostart exited with status {status}");
        }
        return output::print_json(&serde_json::json!({
            "ok": true,
            "restarted": true,
            "pids": daemon_pids,
        }));
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to start daemon: {error}"))?;
    output::print_json(&serde_json::json!({
        "ok": true,
        "restarted": true,
        "pids": daemon_pids,
        "mode": "daemon-only",
    }))
}

/// Restarts every Beca process as a local module supervised by this CLI.
fn restart_local() -> Result<()> {
    let runtime = beca_process::runtime_dir();
    std::fs::create_dir_all(&runtime).ok();

    let local_mods = beca_process::find_local_module_pids();
    let daemons = beca_process::find_daemon_pids();
    beca_process::kill_pids(&local_mods);
    beca_process::kill_pids(&daemons);
    if !local_mods.is_empty() || !daemons.is_empty() {
        sleep(Duration::from_millis(2000));
    }
    beca_process::cleanup_runtime_files(&runtime);

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    beca_process::spawn_logged(&exe, &["daemon"], &runtime.join("daemon.log"))?;
    sleep(Duration::from_millis(2000));
    beca_process::spawn_logged(
        &exe,
        &["mod", "--local", "bar:show"],
        &runtime.join("bar.log"),
    )?;
    sleep(Duration::from_millis(1000));
    beca_process::spawn_logged(
        &exe,
        &["mod", "--local", "launcher:serve"],
        &runtime.join("launcher.log"),
    )?;
    sleep(Duration::from_millis(1000));
    beca_process::spawn_logged(
        &exe,
        &["mod", "--local", "voice:serve"],
        &runtime.join("voice.log"),
    )?;
    sleep(Duration::from_millis(2000));

    let running = beca_process::find_local_module_pids();
    output::print_json(&serde_json::json!({
        "ok": true,
        "restarted": true,
        "mode": "local",
        "killed_local_mods": local_mods,
        "killed_daemons": daemons,
        "running_local_mods": running,
    }))
}
