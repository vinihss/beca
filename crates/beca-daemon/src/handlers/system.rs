use serde_json::{Value, json};
use std::process::Command;

use crate::{
    control,
    handlers::bar,
    hyprland::{hyprland_dispatch, hyprland_paths},
    state::{is_pid_alive, state},
};

pub fn ping() -> Value {
    json!({"ok": true, "type": "pong"})
}

pub fn version() -> Value {
    json!({"ok": true, "version": env!("CARGO_PKG_VERSION")})
}

pub fn hyprland_dispatch_request(command: &str) -> Value {
    if command.is_empty() {
        return json!({"ok": false, "error": "missing command"});
    }
    match hyprland_dispatch(command) {
        Ok((ok, response)) => json!({"ok": ok, "response": response}),
        Err(error) => json!({"ok": false, "error": error}),
    }
}

pub fn hyprland_socket_paths() -> Value {
    match hyprland_paths() {
        Some((socket, socket2)) => json!({"ok": true, "socket": socket, "socket2": socket2}),
        None => json!({"ok": false, "error": "hyprland not available"}),
    }
}

/// Invalidates UI/app caches and restarts supervised modules.
pub fn config_reload() -> Value {
    beca_ui::ui_loader::invalidate_cache();
    beca_apps::reload_index();
    beca_apps::clear_icon_cache();

    let mut restarted_bars: Vec<String> = Vec::new();
    let mut bar_errors: Vec<String> = Vec::new();

    let positions: Vec<String> = {
        let mut st = state().lock().unwrap();
        let keys: Vec<String> = st.bars.keys().cloned().collect();
        let mut running = Vec::new();
        for key in keys {
            if let Some(pid) = st.bars.get(&key).copied() {
                if is_pid_alive(pid) {
                    running.push(key.clone());
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                }
                st.bars.remove(&key);
            }
        }
        running
    };

    for position in ["top", "bottom"] {
        bar::clear_reserved(position);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    for position in positions {
        let target = if position == "bottom" {
            "bar:show-bottom"
        } else {
            "bar:show-top"
        };
        match control::spawn_local_mod(target, &[]) {
            Ok(pid) => {
                state().lock().unwrap().bars.insert(position.clone(), pid);
                restarted_bars.push(position);
            }
            Err(error) => bar_errors.push(format!("{position}: {error}")),
        }
    }

    let mut launcher_restarted = false;
    let launcher_was_running = {
        let mut st = state().lock().unwrap();
        let was = st.launcher_pid.is_some() || control::launcher_ctl_alive();
        st.launcher_pid = None;
        was
    };
    if launcher_was_running {
        let _ = control::send_launcher_control("quit");
        let _ = std::fs::remove_file(control::launcher_ctl_path());
        std::thread::sleep(std::time::Duration::from_millis(200));
        if control::ensure_resident_launcher().is_ok() {
            launcher_restarted = true;
        }
    }

    let mut voice_restarted = false;
    let voice_was_running = {
        let mut st = state().lock().unwrap();
        let was = st.voice_pid.is_some() || control::voice_ctl_alive();
        st.voice_pid = None;
        was
    };
    if voice_was_running {
        let _ = control::send_voice_control("quit");
        let _ = std::fs::remove_file(control::voice_ctl_path());
        std::thread::sleep(std::time::Duration::from_millis(200));
        if control::ensure_resident_voice().is_ok() {
            voice_restarted = true;
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
