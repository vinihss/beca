use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;

use crate::state::{is_pid_alive, state};

fn runtime_path(name: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join(name)
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join(name)
    }
}

/// Spawns `beca mod --local <target>` with the daemon bypass flag set.
pub fn spawn_local_mod(target: &str, args: &[String]) -> Result<u32, String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    let mut cmd = Command::new(exe);
    cmd.env("BECA_NO_DAEMON", "1");
    cmd.arg("mod").arg("--local").arg(target);
    cmd.args(args);
    cmd.spawn()
        .map(|child| child.id())
        .map_err(|error| format!("spawn failed: {error}"))
}

pub fn launcher_ctl_path() -> PathBuf {
    runtime_path("launcher.ctl")
}

pub fn voice_ctl_path() -> PathBuf {
    runtime_path("voice.ctl")
}

fn send_control(path: &PathBuf, cmd: &str) -> Result<(), String> {
    let mut stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    writeln!(stream, "{cmd}").map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

pub fn send_launcher_control(cmd: &str) -> Result<(), String> {
    send_control(&launcher_ctl_path(), cmd)
}

pub fn send_voice_control(cmd: &str) -> Result<(), String> {
    send_control(&voice_ctl_path(), cmd)
}

fn ctl_alive(path: &PathBuf) -> bool {
    if !path.exists() {
        return false;
    }
    match UnixStream::connect(path) {
        Ok(_) => true,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            false
        }
    }
}

pub fn launcher_ctl_alive() -> bool {
    ctl_alive(&launcher_ctl_path())
}

pub fn voice_ctl_alive() -> bool {
    ctl_alive(&voice_ctl_path())
}

pub fn launcher_status_inner() -> Value {
    let mut st = state().lock().unwrap();
    if let Some(pid) = st.launcher_pid {
        if is_pid_alive(pid) && launcher_ctl_path().exists() {
            return json!({"running": true, "pid": pid, "resident": true});
        }
        st.launcher_pid = None;
    }
    drop(st);
    if launcher_ctl_alive() {
        return json!({"running": true, "resident": true, "pid": null});
    }
    json!({"running": false, "resident": false})
}

pub fn voice_status_inner() -> Value {
    let mut st = state().lock().unwrap();
    if let Some(pid) = st.voice_pid {
        if is_pid_alive(pid) && voice_ctl_path().exists() {
            return json!({"running": true, "pid": pid, "resident": true});
        }
        st.voice_pid = None;
    }
    drop(st);
    if voice_ctl_alive() {
        return json!({"running": true, "resident": true, "pid": null});
    }
    json!({"running": false, "resident": false})
}

/// Returns the resident launcher pid, starting it when needed.
pub fn ensure_resident_launcher() -> Result<u32, String> {
    {
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.launcher_pid {
            if is_pid_alive(pid) && launcher_ctl_alive() {
                return Ok(pid);
            }
            st.launcher_pid = None;
        }
    }

    if launcher_ctl_alive() {
        return Ok(0);
    }
    let _ = std::fs::remove_file(launcher_ctl_path());
    let pid = spawn_local_mod("launcher:serve", &[])?;

    let ctl = launcher_ctl_path();
    for _ in 0..100 {
        if ctl.exists() && send_launcher_control("show").is_ok() {
            // The resident server expects an initial command before it is ready.
        }
        if ctl.exists() && UnixStream::connect(&ctl).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if UnixStream::connect(&ctl).is_err() {
        return Err("resident launcher started but control socket not ready".to_string());
    }
    state().lock().unwrap().launcher_pid = Some(pid);
    Ok(pid)
}

/// Returns the resident voice pid, starting it when needed.
pub fn ensure_resident_voice() -> Result<u32, String> {
    {
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.voice_pid {
            if is_pid_alive(pid) && voice_ctl_alive() {
                return Ok(pid);
            }
            st.voice_pid = None;
        }
    }

    if voice_ctl_alive() {
        return Ok(0);
    }
    let _ = std::fs::remove_file(voice_ctl_path());
    let pid = spawn_local_mod("voice:serve", &[])?;

    let ctl = voice_ctl_path();
    for _ in 0..100 {
        if ctl.exists() && UnixStream::connect(&ctl).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if UnixStream::connect(&ctl).is_err() {
        return Err("resident voice started but control socket not ready".to_string());
    }
    state().lock().unwrap().voice_pid = Some(pid);
    Ok(pid)
}
