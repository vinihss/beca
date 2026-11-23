use serde_json::{Value, json};

use crate::{
    control,
    state::{is_pid_alive, state},
};

pub fn status() -> Value {
    json!({"ok": true, "launcher": control::launcher_status_inner()})
}

/// Handles `launcher:*` and `apps:*` module targets.
pub fn handle(target: &str, _args: &[String]) -> Option<Value> {
    let action = target.strip_prefix("launcher:")?;
    match action {
        "status" => Some(status_with_target(target)),
        "show" => Some(match control::ensure_resident_launcher() {
            Ok(pid) => match control::send_launcher_control("show") {
                Ok(_) => json!({"ok": true, "target": target, "mode": "resident", "pid": pid}),
                Err(error) => json!({"ok": false, "error": error, "target": target}),
            },
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        }),
        "hide" => Some(match control::send_launcher_control("hide") {
            Ok(_) => json!({"ok": true, "target": target}),
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        }),
        "stop" | "quit" => {
            let result = control::send_launcher_control("quit");
            state().lock().unwrap().launcher_pid = None;
            Some(match result {
                Ok(_) => json!({"ok": true, "target": target, "stopped": true}),
                Err(error) => json!({
                    "ok": true, "target": target, "stopped": false, "note": error
                }),
            })
        }
        "serve" => {
            {
                let mut st = state().lock().unwrap();
                if let Some(pid) = st.launcher_pid {
                    if is_pid_alive(pid) {
                        return Some(json!({
                            "ok": true, "target": target,
                            "mode": "already-running", "pid": pid
                        }));
                    }
                    st.launcher_pid = None;
                }
            }
            Some(match control::spawn_local_mod("launcher:serve", &[]) {
                Ok(pid) => {
                    state().lock().unwrap().launcher_pid = Some(pid);
                    json!({"ok": true, "target": target, "mode": "spawned", "pid": pid})
                }
                Err(error) => json!({"ok": false, "error": error, "target": target}),
            })
        }
        _ => None,
    }
}

fn status_with_target(target: &str) -> Value {
    let mut response = status();
    response["target"] = json!(target);
    response
}
