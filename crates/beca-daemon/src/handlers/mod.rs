pub mod bar;
pub mod launcher;
pub mod system;
pub mod voice;

use serde_json::{Value, json};

use crate::{control, protocol::Request};

/// Dispatches a typed request to its handler.
pub fn handle_request(req: Request) -> Value {
    match req {
        Request::Ping => system::ping(),
        Request::Version => system::version(),
        Request::HyprlandDispatch { command } => system::hyprland_dispatch_request(&command),
        Request::HyprlandSocketPaths => system::hyprland_socket_paths(),
        Request::HyprlandSubscribe => json!({"ok": true, "type": "subscribed"}),
        Request::Mod { target, args } => handle_mod_request(&target, &args),
        Request::BarStatus => bar::status(),
        Request::LauncherStatus => launcher::status(),
        Request::ConfigReload => system::config_reload(),
    }
}

/// Routes a module request to a resident module or spawns it locally.
pub fn handle_mod_request(target: &str, args: &[String]) -> Value {
    if target == "voice:transcribe" {
        return voice::transcribe(args);
    }
    if let Some(response) = voice::handle(target, args) {
        return response;
    }
    if let Some(response) = launcher::handle(target, args) {
        return response;
    }
    if let Some(response) = bar::handle(target, args) {
        return response;
    }

    match control::spawn_local_mod(target, args) {
        Ok(pid) => json!({"ok": true, "target": target, "mode": "spawned", "pid": pid}),
        Err(error) => json!({"ok": false, "error": error, "target": target}),
    }
}
