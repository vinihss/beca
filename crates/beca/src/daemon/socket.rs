use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;

#[derive(Default)]
struct DaemonState {
    /// tracked bar surfaces: key "top"/"bottom" -> pid
    bars: HashMap<String, u32>,
    /// resident launcher pid (serves show/hide via control socket)
    launcher_pid: Option<u32>,
    /// resident voice pid (serves show via control socket)
    voice_pid: Option<u32>,
}

fn state() -> &'static Mutex<DaemonState> {
    static S: OnceLock<Mutex<DaemonState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(DaemonState::default()))
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // kill -0 returns success for zombie processes too; treat zombies as not alive.
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // format: pid (comm) state ...
        if let Some(end) = stat.rfind(')') {
            let rest = stat[end + 1..].trim_start();
            if let Some(state) = rest.chars().next() {
                if state == 'Z' || state == 'X' || state == 'x' {
                    return false;
                }
            }
        }
    } else {
        return false;
    }
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Best-effort: clear exclusive zone (reserved space) for a bar position on all monitors.
fn clear_bar_reserved(position: &str) {
    let monitors_out = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok();
    let Some(out) = monitors_out else { return };
    if !out.status.success() {
        return;
    }
    let monitors: Vec<serde_json::Value> =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap_or_default();
    for m in monitors {
        let Some(name) = m.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        // reserved array observed: [left, top, right, bottom]
        let reserved = m.get("reserved").and_then(|v| v.as_array());
        let l = reserved
            .and_then(|a| a.get(0))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let t = reserved
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let r = reserved
            .and_then(|a| a.get(2))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let b = reserved
            .and_then(|a| a.get(3))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let (nt, nb) = match position {
            "top" => (0, b),
            "bottom" => (t, 0),
            _ => (t, b),
        };
        if nt == t && nb == b {
            continue;
        }
        let lua = format!(
            "hl.monitor({{output='{name}', reserved={{left={l},top={nt},right={r},bottom={nb}}}}}); return 'ok'"
        );
        let _ = Command::new("hyprctl").args(["eval", &lua]).status();
    }
}

use super::hyprland::{hyprland_dispatch, hyprland_paths, stream_hyprland_events};

pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("beca.sock")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("beca.sock")
    }
}

pub fn run() -> Result<()> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind daemon socket {}", path.display()))?;
    eprintln!("[beca daemon] listening on {}", path.display());

    // Pre-warm resident launcher and voice in background so first show is instant.
    thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(150));
        match ensure_resident_launcher() {
            Ok(pid) => eprintln!("[beca daemon] resident launcher ready (pid {pid})"),
            Err(e) => eprintln!("[beca daemon] resident launcher warmup failed: {e}"),
        }
        match ensure_resident_voice() {
            Ok(pid) => eprintln!("[beca daemon] resident voice ready (pid {pid})"),
            Err(e) => eprintln!("[beca daemon] resident voice warmup failed: {e}"),
        }
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream) {
                        eprintln!("[beca daemon] client error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[beca daemon] accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_client(stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    // read first request
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let req: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => {
            let resp = json!({"ok": false, "error": format!("invalid json: {e}")});
            writeln!(writer, "{}", resp)?;
            return Ok(());
        }
    };

    let req_type = req.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Long-lived streaming modes
    if req_type == "hyprland.subscribe" {
        let ack = json!({"ok": true, "type": "subscribed"});
        writeln!(writer, "{}", ack)?;
        writer.flush()?;
        return stream_hyprland_events(writer);
    }

    // Normal request-response
    let resp = handle_request(&req);
    writeln!(writer, "{}", resp)?;
    writer.flush()?;

    // Keep connection open for additional requests (simple session)
    line.clear();
    while reader.read_line(&mut line)? > 0 {
        let req: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({"ok": false, "error": format!("invalid json: {e}")});
                writeln!(writer, "{}", resp)?;
                line.clear();
                continue;
            }
        };
        let req_type = req.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if req_type == "hyprland.subscribe" {
            let ack = json!({"ok": true, "type": "subscribed"});
            writeln!(writer, "{}", ack)?;
            writer.flush()?;
            return stream_hyprland_events(writer);
        }
        let resp = handle_request(&req);
        writeln!(writer, "{}", resp)?;
        writer.flush()?;
        line.clear();
    }
    Ok(())
}

fn handle_request(req: &Value) -> Value {
    let t = req.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "ping" => json!({"ok": true, "type": "pong"}),
        "version" => json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}),
        "hyprland.dispatch" => {
            let cmd = req.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.is_empty() {
                return json!({"ok": false, "error": "missing command"});
            }
            match hyprland_dispatch(cmd) {
                Ok((ok, response)) => json!({"ok": ok, "response": response}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "hyprland.socket_paths" => match hyprland_paths() {
            Some((s1, s2)) => json!({"ok": true, "socket": s1, "socket2": s2}),
            None => json!({"ok": false, "error": "hyprland not available"}),
        },
        "mod" => handle_mod_request(req),
        "bar.status" => {
            let mut st = state().lock().unwrap();
            let mut bars = serde_json::Map::new();
            let keys: Vec<String> = st.bars.keys().cloned().collect();
            for k in keys {
                if let Some(pid) = st.bars.get(&k).copied() {
                    if is_pid_alive(pid) {
                        bars.insert(k, json!({"running": true, "pid": pid}));
                    } else {
                        st.bars.remove(&k);
                        bars.insert(k, json!({"running": false}));
                    }
                }
            }
            json!({"ok": true, "bars": bars})
        }
        "launcher.status" => json!({"ok": true, "launcher": launcher_status_inner()}),
        "config.reload" => handle_config_reload(),
        other => json!({"ok": false, "error": format!("unknown request type: {other}")}),
    }
}

fn handle_config_reload() -> Value {
    // Invalidate in-process caches so restarted modules pick up fresh data/UI.
    beca_ui::ui_loader::invalidate_cache();
    beca_apps::reload_index();
    beca_apps::clear_icon_cache();

    // Force reload: restart managed UI modules so they re-read config/theme from disk.
    let mut restarted_bars: Vec<String> = Vec::new();
    let mut bar_errors: Vec<String> = Vec::new();

    // Collect running bar positions.
    let positions: Vec<String> = {
        let mut st = state().lock().unwrap();
        let keys: Vec<String> = st.bars.keys().cloned().collect();
        let mut running = Vec::new();
        for k in keys {
            if let Some(pid) = st.bars.get(&k).copied() {
                if is_pid_alive(pid) {
                    running.push(k.clone());
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                }
                st.bars.remove(&k);
            }
        }
        running
    };
    // Clear exclusive zones (best-effort).
    for pos in ["top", "bottom"] {
        clear_bar_reserved(pos);
    }
    // Give processes a moment to exit and release state.
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Respawn previously running bars.
    for pos in positions {
        let spawn_target = match pos.as_str() {
            "bottom" => "bar:show-bottom",
            _ => "bar:show-top",
        };
        match spawn_local_mod(spawn_target, &[]) {
            Ok(pid) => {
                let mut st = state().lock().unwrap();
                st.bars.insert(pos.clone(), pid);
                restarted_bars.push(pos);
            }
            Err(e) => bar_errors.push(format!("{pos}: {e}")),
        }
    }

    // Restart resident launcher if running, so it re-reads config/theme.
    let mut launcher_restarted = false;
    let launcher_was_running = {
        let mut st = state().lock().unwrap();
        let was = st.launcher_pid.is_some() || launcher_ctl_alive();
        st.launcher_pid = None;
        was
    };
    if launcher_was_running {
        let _ = send_launcher_control("quit");
        let _ = std::fs::remove_file(launcher_ctl_path());
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Re-spawn resident launcher and keep it hidden/ready.
        match ensure_resident_launcher() {
            Ok(_) => {
                launcher_restarted = true;
            }
            Err(_) => {
                // not fatal; next show will start it
            }
        }
    }

    // Restart resident voice if running (picks up config/theme and warm cache).
    let mut voice_restarted = false;
    let voice_was_running = {
        let mut st = state().lock().unwrap();
        let was = st.voice_pid.is_some() || voice_ctl_alive();
        st.voice_pid = None;
        was
    };
    if voice_was_running {
        let _ = send_voice_control("quit");
        let _ = std::fs::remove_file(voice_ctl_path());
        std::thread::sleep(std::time::Duration::from_millis(200));
        match ensure_resident_voice() {
            Ok(_) => {
                voice_restarted = true;
            }
            Err(_) => {
                // not fatal; next show will start it
            }
        }
    }

    json!({
        "ok": bar_errors.is_empty(),
        "reloaded": true,
        "bars_restarted": restarted_bars,
        "bar_errors": bar_errors,
        "launcher_restarted": launcher_restarted,
        "voice_restarted": voice_restarted,
    })
}

fn spawn_local_mod(target: &str, args: &[String]) -> Result<u32, String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("beca"));
    let mut cmd = Command::new(exe);
    cmd.env("BECA_NO_DAEMON", "1");
    cmd.arg("mod").arg("--local").arg(target);
    for a in args {
        cmd.arg(a);
    }
    match cmd.spawn() {
        Ok(child) => Ok(child.id()),
        Err(e) => Err(format!("spawn failed: {e}")),
    }
}

fn bar_position_for_target(target: &str, args: &[String]) -> Option<String> {
    match target {
        "bar:show-top" | "bar:hide-top" | "bar:stop-top" => Some("top".to_string()),
        "bar:show-bottom" | "bar:hide-bottom" | "bar:stop-bottom" => Some("bottom".to_string()),
        "bar:show" => {
            let pos = args
                .iter()
                .find_map(|a| {
                    if a == "top" || a == "bottom" {
                        Some(a.as_str())
                    } else {
                        None
                    }
                })
                .or_else(|| args.first().map(|s| s.as_str()))
                .unwrap_or("top");
            Some(if pos == "bottom" { "bottom" } else { "top" }.to_string())
        }
        "bar:hide" | "bar:stop" => {
            let pos = args
                .iter()
                .find_map(|a| {
                    if a == "top" || a == "bottom" {
                        Some(a.as_str())
                    } else {
                        None
                    }
                })
                .or_else(|| args.first().map(|s| s.as_str()))
                .unwrap_or("top");
            Some(if pos == "bottom" { "bottom" } else { "top" }.to_string())
        }
        _ => None,
    }
}

fn launcher_ctl_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("launcher.ctl")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("launcher.ctl")
    }
}

fn send_launcher_control(cmd: &str) -> Result<(), String> {
    let path = launcher_ctl_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
    writeln!(stream, "{cmd}").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn launcher_ctl_alive() -> bool {
    let path = launcher_ctl_path();
    if !path.exists() {
        return false;
    }
    match UnixStream::connect(&path) {
        Ok(_) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            false
        }
    }
}

fn launcher_status_inner() -> serde_json::Value {
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

fn ensure_resident_launcher() -> Result<u32, String> {
    {
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.launcher_pid {
            if is_pid_alive(pid) && launcher_ctl_alive() {
                return Ok(pid);
            }
            st.launcher_pid = None;
        }
    }
    // maybe running but untracked
    if launcher_ctl_alive() {
        return Ok(0);
    }
    let _ = std::fs::remove_file(launcher_ctl_path());
    let pid = spawn_local_mod("launcher:serve", &[])?;
    // wait for control socket
    let ctl = launcher_ctl_path();
    for _ in 0..100 {
        if ctl.exists() && send_launcher_control("show").is_ok() {
            // it responded to show probe? no, don't send show here - just check connect
        }
        if ctl.exists() {
            if UnixStream::connect(&ctl).is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // final readiness check
    if UnixStream::connect(&ctl).is_err() {
        return Err("resident launcher started but control socket not ready".to_string());
    }
    let mut st = state().lock().unwrap();
    st.launcher_pid = Some(pid);
    Ok(pid)
}

fn voice_ctl_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("voice.ctl")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("voice.ctl")
    }
}

fn send_voice_control(cmd: &str) -> Result<(), String> {
    let path = voice_ctl_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
    writeln!(stream, "{cmd}").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn voice_ctl_alive() -> bool {
    let path = voice_ctl_path();
    if !path.exists() {
        return false;
    }
    match UnixStream::connect(&path) {
        Ok(_) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            false
        }
    }
}

fn voice_status_inner() -> serde_json::Value {
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

fn ensure_resident_voice() -> Result<u32, String> {
    {
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.voice_pid {
            if is_pid_alive(pid) && voice_ctl_alive() {
                return Ok(pid);
            }
            st.voice_pid = None;
        }
    }
    // maybe running but untracked
    if voice_ctl_alive() {
        return Ok(0);
    }
    let _ = std::fs::remove_file(voice_ctl_path());
    let pid = spawn_local_mod("voice:serve", &[])?;
    // wait for control socket
    let ctl = voice_ctl_path();
    for _ in 0..100 {
        if ctl.exists() {
            if UnixStream::connect(&ctl).is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if UnixStream::connect(&ctl).is_err() {
        return Err("resident voice started but control socket not ready".to_string());
    }
    let mut st = state().lock().unwrap();
    st.voice_pid = Some(pid);
    Ok(pid)
}

fn handle_mod_request(req: &Value) -> Value {
    let target = match req.get("target").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return json!({"ok": false, "error": "missing target"}),
    };
    let args: Vec<String> = req
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // voice:transcribe is safe to run in-process in daemon (no UI)
    if target == "voice:transcribe" {
        return match crate::comandos::voice::dispatch("transcribe", &args) {
            Ok(_) => json!({"ok": true, "target": target, "mode": "in-process"}),
            Err(e) => json!({"ok": false, "error": e.to_string(), "target": target}),
        };
    }

    // Voice lifecycle (resident, instant show / warm process)
    if target.starts_with("voice:") {
        match target.as_str() {
            "voice:status" => {
                return json!({"ok": true, "target": target, "voice": voice_status_inner()});
            }
            "voice:show" => {
                return match ensure_resident_voice() {
                    Ok(pid) => match send_voice_control("show") {
                        Ok(_) => json!({"ok": true, "target": target, "mode": "resident", "pid": pid}),
                        Err(e) => json!({"ok": false, "error": e, "target": target}),
                    },
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            "voice:hide" => {
                return match send_voice_control("hide") {
                    Ok(_) => json!({"ok": true, "target": target}),
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            "voice:stop" | "voice:quit" => {
                let res = send_voice_control("quit");
                let mut st = state().lock().unwrap();
                st.voice_pid = None;
                return match res {
                    Ok(_) => json!({"ok": true, "target": target, "stopped": true}),
                    Err(e) => json!({"ok": true, "target": target, "stopped": false, "note": e}),
                };
            }
            "voice:serve" => {
                let mut st = state().lock().unwrap();
                if let Some(pid) = st.voice_pid {
                    if is_pid_alive(pid) {
                        return json!({"ok": true, "target": target, "mode": "already-running", "pid": pid});
                    }
                    st.voice_pid = None;
                }
                drop(st);
                return match spawn_local_mod("voice:serve", &[]) {
                    Ok(pid) => {
                        let mut st = state().lock().unwrap();
                        st.voice_pid = Some(pid);
                        json!({"ok": true, "target": target, "mode": "spawned", "pid": pid})
                    }
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            _ => {} // fall through to spawn (transcribe handled above, other actions not resident)
        }
    }

    // Launcher lifecycle (resident, instant show)
    if target.starts_with("launcher:") {
        match target.as_str() {
            "launcher:status" => {
                return json!({"ok": true, "target": target, "launcher": launcher_status_inner()});
            }
            "launcher:show" => {
                return match ensure_resident_launcher() {
                    Ok(pid) => match send_launcher_control("show") {
                        Ok(_) => json!({"ok": true, "target": target, "mode": "resident", "pid": pid}),
                        Err(e) => json!({"ok": false, "error": e, "target": target}),
                    },
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            "launcher:hide" => {
                return match send_launcher_control("hide") {
                    Ok(_) => json!({"ok": true, "target": target}),
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            "launcher:stop" | "launcher:quit" => {
                let res = send_launcher_control("quit");
                let mut st = state().lock().unwrap();
                st.launcher_pid = None;
                return match res {
                    Ok(_) => json!({"ok": true, "target": target, "stopped": true}),
                    Err(e) => json!({"ok": true, "target": target, "stopped": false, "note": e}),
                };
            }
            "launcher:serve" => {
                // explicit serve spawn (tracked)
                let mut st = state().lock().unwrap();
                if let Some(pid) = st.launcher_pid {
                    if is_pid_alive(pid) {
                        return json!({"ok": true, "target": target, "mode": "already-running", "pid": pid});
                    }
                    st.launcher_pid = None;
                }
                drop(st);
                return match spawn_local_mod("launcher:serve", &[]) {
                    Ok(pid) => {
                        let mut st = state().lock().unwrap();
                        st.launcher_pid = Some(pid);
                        json!({"ok": true, "target": target, "mode": "spawned", "pid": pid})
                    }
                    Err(e) => json!({"ok": false, "error": e, "target": target}),
                };
            }
            _ => {} // fall through to generic spawn
        }
    }

    // Bar lifecycle management (single instance per position)
    if target.starts_with("bar:") {
        if target == "bar:status" {
            // report tracked bars
            let mut st = state().lock().unwrap();
            let mut bars = serde_json::Map::new();
            let keys: Vec<String> = st.bars.keys().cloned().collect();
            for k in keys {
                if let Some(pid) = st.bars.get(&k).copied() {
                    if is_pid_alive(pid) {
                        bars.insert(k, json!({"running": true, "pid": pid}));
                    } else {
                        st.bars.remove(&k);
                        bars.insert(k, json!({"running": false}));
                    }
                }
            }
            return json!({"ok": true, "target": target, "bars": bars});
        }

        if target.contains("hide") || target.contains("stop") {
            if let Some(pos) = bar_position_for_target(&target, &args) {
                let mut st = state().lock().unwrap();
                if let Some(pid) = st.bars.remove(&pos) {
                    if is_pid_alive(pid) {
                        let _ = Command::new("kill").arg(pid.to_string()).status();
                    }
                    drop(st);
                    // best-effort: clear exclusive zone (reserved space) for this bar position
                    clear_bar_reserved(&pos);
                    return json!({"ok": true, "target": target, "position": pos, "stopped": true, "pid": pid});
                } else {
                    drop(st);
                    // ensure reserved is cleared even if we lost tracking
                    clear_bar_reserved(&pos);
                    return json!({"ok": true, "target": target, "position": pos, "stopped": false, "note": "not running"});
                }
            }
        }

        if target.contains("show") {
            if let Some(pos) = bar_position_for_target(&target, &args) {
                let mut st = state().lock().unwrap();
                if let Some(pid) = st.bars.get(&pos).copied() {
                    if is_pid_alive(pid) {
                        return json!({"ok": true, "target": target, "position": pos, "mode": "already-running", "pid": pid});
                    } else {
                        st.bars.remove(&pos);
                    }
                }
                // normalize spawn target to explicit position action
                let spawn_target = match pos.as_str() {
                    "bottom" => "bar:show-bottom",
                    _ => "bar:show-top",
                };
                match spawn_local_mod(spawn_target, &[]) {
                    Ok(pid) => {
                        st.bars.insert(pos.clone(), pid);
                        return json!({"ok": true, "target": target, "position": pos, "mode": "spawned", "pid": pid});
                    }
                    Err(e) => return json!({"ok": false, "error": e, "target": target}),
                }
            }
        }
    }

    // For UI/interactive modules, spawn a local beca process (daemon routes lifecycle)
    match spawn_local_mod(&target, &args) {
        Ok(pid) => json!({"ok": true, "target": target, "mode": "spawned", "pid": pid}),
        Err(e) => json!({"ok": false, "error": e, "target": target}),
    }
}

/// Send a request to a running daemon and return the response.
pub fn send_request(req: &Value) -> Result<Value> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to daemon at {}", path.display()))?;
    writeln!(stream, "{}", req)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let resp: Value = serde_json::from_str(line.trim()).context("invalid daemon response")?;
    Ok(resp)
}

/// Try to send a mod request to daemon; returns Some(response) if daemon handled/routed it.
pub fn try_route_mod(target: &str, args: &[String]) -> Option<Value> {
    let path = socket_path();
    if !path.exists() {
        return None;
    }
    let req = json!({"type": "mod", "target": target, "args": args});
    send_request(&req).ok()
}
