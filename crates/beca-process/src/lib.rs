use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Returns `(pid, arguments)` for every process except this one.
fn running_processes() -> Vec<(u32, Vec<String>)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let self_pid = std::process::id();

    let mut processes = entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            if pid == self_pid {
                return None;
            }
            let bytes = std::fs::read(entry.path().join("cmdline")).ok()?;
            let args = bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect::<Vec<_>>();
            (!args.is_empty()).then_some((pid, args))
        })
        .collect::<Vec<_>>();

    processes.sort_unstable_by_key(|(pid, _)| *pid);
    processes
}

/// Finds processes whose command line matches `predicate`.
fn find_pids_matching(predicate: impl Fn(&[String]) -> bool) -> Vec<u32> {
    running_processes()
        .into_iter()
        .filter_map(|(pid, args)| predicate(&args).then_some(pid))
        .collect()
}

fn is_beca_exe(arg: &str) -> bool {
    arg.rsplit('/').next().unwrap_or(arg) == "beca"
}

/// PIDs of `beca daemon` processes.
pub fn find_daemon_pids() -> Vec<u32> {
    find_pids_matching(|args| args.len() >= 2 && is_beca_exe(&args[0]) && args[1] == "daemon")
}

/// PIDs of the local bar module processes used by `beca restart`.
pub fn find_bar_pids() -> Vec<u32> {
    find_pids_matching(|args| {
        args.len() >= 4
            && is_beca_exe(&args[0])
            && args[1] == "mod"
            && args[2] == "--local"
            && matches!(args[3].as_str(), "bar:show-top" | "bar:show-bottom")
    })
}

/// PIDs of local module processes (`beca mod --local ...`).
pub fn find_local_module_pids() -> Vec<u32> {
    find_pids_matching(|args| {
        args.len() >= 3 && is_beca_exe(&args[0]) && args[1] == "mod" && args[2] == "--local"
    })
}

pub fn kill_pids(pids: &[u32]) {
    for pid in pids {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

/// Checks whether a PID is alive, ignoring zombie processes.
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(end) = stat.rfind(')') else {
        return false;
    };
    if stat[end + 1..]
        .trim_start()
        .chars()
        .next()
        .is_some_and(|state| matches!(state, 'Z' | 'X' | 'x'))
    {
        return false;
    }
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())))
        .join("beca")
}

fn uid() -> u32 {
    std::env::var("UID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000)
}

/// Spawns a process detached from the terminal with its output appended to `log`.
pub fn spawn_logged(exe: &Path, args: &[&str], log: &Path) -> Result<()> {
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|error| anyhow::anyhow!("open log {}: {error}", log.display()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| anyhow::anyhow!("clone log {}: {error}", log.display()))?;

    Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| anyhow::anyhow!("spawn {exe:?} {args:?}: {error}"))?;
    Ok(())
}

/// Removes stale control sockets and named pipes from the runtime directory.
pub fn cleanup_runtime_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_stale = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.ends_with(".sock") || name == "launcher.ctl" || name == "voice.ctl"
            });
        if is_stale {
            let _ = std::fs::remove_file(path);
        }
    }
}
