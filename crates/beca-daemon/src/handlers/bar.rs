use serde_json::{Value, json};
use std::process::Command;

use crate::{
    control,
    state::{is_pid_alive, state},
};

/// Frees the Hyprland reserved area previously used by a bar.
pub fn clear_reserved(position: &str) {
    let Ok(out) = Command::new("hyprctl").args(["monitors", "-j"]).output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let monitors: Vec<Value> =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap_or_default();
    for monitor in monitors {
        let Some(name) = monitor.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let reserved = monitor.get("reserved").and_then(|value| value.as_array());
        let left = reserved
            .and_then(|array| array.first())
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let top = reserved
            .and_then(|array| array.get(1))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let right = reserved
            .and_then(|array| array.get(2))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let bottom = reserved
            .and_then(|array| array.get(3))
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let (new_top, new_bottom) = match position {
            "top" => (0, bottom),
            "bottom" => (top, 0),
            _ => (top, bottom),
        };
        if new_top == top && new_bottom == bottom {
            continue;
        }
        let lua = format!(
            "hl.monitor({{output='{name}', reserved={{left={left},top={new_top},right={right},bottom={new_bottom}}}}}); return 'ok'"
        );
        let _ = Command::new("hyprctl").args(["eval", &lua]).status();
    }
}

/// Snapshot of running bars.
pub fn status() -> Value {
    let mut st = state().lock().unwrap();
    let mut bars = serde_json::Map::new();
    let keys: Vec<String> = st.bars.keys().cloned().collect();
    for key in keys {
        if let Some(pid) = st.bars.get(&key).copied() {
            if is_pid_alive(pid) {
                bars.insert(key, json!({"running": true, "pid": pid}));
            } else {
                st.bars.remove(&key);
                bars.insert(key, json!({"running": false}));
            }
        }
    }
    json!({"ok": true, "bars": bars})
}

/// Resolves the bar position encoded in a module target.
fn position_for_target(target: &str, args: &[String]) -> Option<String> {
    let explicit = match target {
        "bar:show-top" | "bar:hide-top" | "bar:stop-top" => return Some("top".to_string()),
        "bar:show-bottom" | "bar:hide-bottom" | "bar:stop-bottom" => {
            return Some("bottom".to_string());
        }
        "bar:show" | "bar:hide" | "bar:stop" => args.iter().find_map(|arg| {
            if arg == "top" || arg == "bottom" {
                Some(arg.as_str())
            } else {
                None
            }
        }),
        _ => return None,
    };
    let position = explicit
        .or_else(|| args.first().map(String::as_str))
        .unwrap_or("top");
    Some(if position == "bottom" { "bottom" } else { "top" }.to_string())
}

/// Handles `bar:*` module targets.
pub fn handle(target: &str, args: &[String]) -> Option<Value> {
    if target == "bar:status" {
        let mut response = status();
        response["target"] = json!(target);
        return Some(response);
    }

    let position = position_for_target(target, args);
    if target.contains("hide") || target.contains("stop") {
        let position = position?;
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.bars.remove(&position) {
            if is_pid_alive(pid) {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
            drop(st);
            clear_reserved(&position);
            return Some(json!({
                "ok": true, "target": target, "position": position, "stopped": true, "pid": pid
            }));
        }
        drop(st);
        clear_reserved(&position);
        return Some(json!({
            "ok": true, "target": target, "position": position,
            "stopped": false, "note": "not running"
        }));
    }

    if target.contains("show") {
        let position = position?;
        let mut st = state().lock().unwrap();
        if let Some(pid) = st.bars.get(&position).copied() {
            if is_pid_alive(pid) {
                return Some(json!({
                    "ok": true, "target": target, "position": position,
                    "mode": "already-running", "pid": pid
                }));
            }
            st.bars.remove(&position);
        }
        let spawn_target = if position == "bottom" {
            "bar:show-bottom"
        } else {
            "bar:show-top"
        };
        return Some(match control::spawn_local_mod(spawn_target, &[]) {
            Ok(pid) => {
                st.bars.insert(position.clone(), pid);
                json!({
                    "ok": true, "target": target, "position": position,
                    "mode": "spawned", "pid": pid
                })
            }
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        });
    }

    None
}
