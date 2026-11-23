use serde_json::{Value, json};

use crate::{
    control,
    state::{is_pid_alive, state},
};

pub fn status() -> Value {
    json!({"ok": true, "voice": control::voice_status_inner()})
}

/// Transcribes a file in-process instead of through a resident voice server.
pub fn transcribe(args: &[String]) -> Value {
    let target = "voice:transcribe";
    match beca_voice::dispatch("transcribe", args) {
        Ok(_) => json!({"ok": true, "target": target, "mode": "in-process"}),
        Err(error) => json!({"ok": false, "error": error.to_string(), "target": target}),
    }
}

/// Handles `voice:*` module targets.
pub fn handle(target: &str, _args: &[String]) -> Option<Value> {
    let action = target.strip_prefix("voice:")?;
    match action {
        "status" => {
            let mut response = status();
            response["target"] = json!(target);
            Some(response)
        }
        "show" => Some(match control::ensure_resident_voice() {
            Ok(pid) => match control::send_voice_control("show") {
                Ok(_) => json!({"ok": true, "target": target, "mode": "resident", "pid": pid}),
                Err(error) => json!({"ok": false, "error": error, "target": target}),
            },
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        }),
        "assistant" | "show-assistant" => Some(match control::ensure_resident_voice() {
            Ok(pid) => match control::send_voice_control("show-assistant") {
                Ok(_) => json!({"ok": true, "target": target, "mode": "resident", "pid": pid}),
                Err(error) => json!({"ok": false, "error": error, "target": target}),
            },
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        }),
        "hide" => Some(match control::send_voice_control("hide") {
            Ok(_) => json!({"ok": true, "target": target}),
            Err(error) => json!({"ok": false, "error": error, "target": target}),
        }),
        "stop" | "quit" => {
            let result = control::send_voice_control("quit");
            state().lock().unwrap().voice_pid = None;
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
                if let Some(pid) = st.voice_pid {
                    if is_pid_alive(pid) {
                        return Some(json!({
                            "ok": true, "target": target,
                            "mode": "already-running", "pid": pid
                        }));
                    }
                    st.voice_pid = None;
                }
            }
            Some(match control::spawn_local_mod("voice:serve", &[]) {
                Ok(pid) => {
                    state().lock().unwrap().voice_pid = Some(pid);
                    json!({"ok": true, "target": target, "mode": "spawned", "pid": pid})
                }
                Err(error) => json!({"ok": false, "error": error, "target": target}),
            })
        }
        _ => None,
    }
}
