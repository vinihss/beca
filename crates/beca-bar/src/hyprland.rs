use super::*;

pub fn hyprland_socket1() -> Option<std::path::PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    Some(runtime.join("hypr").join(sig).join(".socket.sock"))
}

pub fn hypr_dispatch_socket(cmd: &str) -> Option<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let path = hyprland_socket1()?;
    let mut stream = UnixStream::connect(path).ok()?;
    stream.write_all(cmd.as_bytes()).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    Some(buf.trim_end().to_string())
}

pub fn hypr_eval(lua: &str) -> Option<String> {

    hypr_dispatch_socket(&format!("eval {lua}"))
}

pub fn set_monitor_reserved(monitor_name: &str, position: &str, height: i64, enable: bool) {

    let mut left = 0i64;
    let mut top = 0i64;
    let mut right = 0i64;
    let mut bottom = 0i64;
    if let Some(out) = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        && let Ok(monitors) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout)
            && let Some(m) = monitors
                .iter()
                .find(|m| m.get("name").and_then(|v| v.as_str()) == Some(monitor_name))
                && let Some(rsv) = m.get("reserved").and_then(|v| v.as_array())
    {
                left = rsv.first().and_then(|v| v.as_i64()).unwrap_or(0);
                top = rsv.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                right = rsv.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                bottom = rsv.get(3).and_then(|v| v.as_i64()).unwrap_or(0);
    }
    if enable {
        match position {
            "top" => top = height,
            "bottom" => bottom = height,
            _ => {}
        }
    } else {
        match position {
            "top" => top = 0,
            "bottom" => bottom = 0,
            _ => {}
        }
    }
    let lua = format!(
        "hl.monitor({{output='{monitor_name}', reserved={{left={left},top={top},right={right},bottom={bottom}}}}}); return 'ok'"
    );
    let _ = hypr_eval(&lua);
}

pub fn clear_reserved_for_position(position: &str) {
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

        let reserved = m.get("reserved").and_then(|v| v.as_array());
        let l = reserved
            .and_then(|a| a.first())
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
        let lua = format!(
            "hl.monitor({{output='{name}', reserved={{left={l},top={nt},right={r},bottom={nb}}}}}); return 'ok'"
        );
        let _ = hypr_eval(&lua);
    }
}

pub fn dock_bar_hyprland(position: &str, height: f32, exclusive: bool) -> Option<String> {
    let pid = std::process::id();
    let h = (height.max(24.0)) as i64;

    let mut addr: Option<String> = None;
    let mut mon_id: i64 = 0;
    for attempt in 0..60 {
        let delay = if attempt < 10 { 20 } else if attempt < 25 { 40 } else { 80 };
        thread::sleep(Duration::from_millis(delay));
        let clients_out = Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
            .ok();
        let Some(out) = clients_out else { continue };
        if !out.status.success() {
            continue;
        }
        let clients: Vec<serde_json::Value> =
            serde_json::from_slice(&out.stdout).unwrap_or_default();
        let client = clients
            .iter()
            .find(|c| c.get("pid").and_then(|v| v.as_u64()) == Some(pid as u64))
            .or_else(|| {
                let matches: Vec<_> = clients
                    .iter()
                    .filter(|c| c.get("title").and_then(|v| v.as_str()) == Some("beca-bar"))
                    .collect();
                if matches.len() == 1 {
                    matches.into_iter().next()
                } else {
                    None
                }
            });
        if let Some(c) = client {
            addr = c.get("address").and_then(|v| v.as_str()).map(|s| s.to_string());
            mon_id = c.get("monitor").and_then(|v| v.as_i64()).unwrap_or(0);
            if addr.is_some() {
                break;
            }
        }
    }
    let addr = addr?;

    let monitors_out = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()?;
    if !monitors_out.status.success() {
        return None;
    }
    let monitors: Vec<serde_json::Value> =
        serde_json::from_slice(&monitors_out.stdout).unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_i64()) == Some(mon_id))
        .or_else(|| {
            monitors
                .iter()
                .find(|m| m.get("focused").and_then(|v| v.as_bool()) == Some(true))
        })
        .or_else(|| monitors.first())?;
    let mon_name = monitor
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mx = monitor.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
    let my = monitor.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
    let mw = monitor
        .get("width")
        .and_then(|v| v.as_i64())
        .unwrap_or(1366);
    let mh = monitor
        .get("height")
        .and_then(|v| v.as_i64())
        .unwrap_or(768);
    let (x, y) = if position == "bottom" {
        (mx, my + mh - h)
    } else {
        (mx, my)
    };

    let prev_active = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("address").and_then(|a| a.as_str()).map(|s| s.to_string()));

    let focus = format!("dispatch hl.dsp.focus({{window = 'address:{addr}'}})");
    let move_cmd = format!("dispatch hl.dsp.window.move({{x={x},y={y}}})");
    let resize = format!("dispatch hl.dsp.window.resize({{x={mw},y={h}}})");

    for i in 0..8 {
        let _ = hypr_dispatch_socket(&focus);
        let _ = hypr_dispatch_socket(&move_cmd);
        let _ = hypr_dispatch_socket(&resize);

        let wait = if i < 3 { 30 } else { 80 };
        thread::sleep(Duration::from_millis(wait));
    }

    let _ = hypr_dispatch_socket(&format!(
        "dispatch movewindowpixel exact {x} {y},address:{addr}"
    ));
    let _ = hypr_dispatch_socket(&format!(
        "dispatch resizewindowpixel exact {mw} {h},address:{addr}"
    ));

    let pinned_now = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Vec<serde_json::Value>>(&o.stdout).ok())
        .and_then(|cs| {
            cs.into_iter()
                .find(|c| c.get("address").and_then(|v| v.as_str()) == Some(&addr))
                .and_then(|c| c.get("pinned").and_then(|v| v.as_bool()))
        })
        .unwrap_or(false);
    if !pinned_now {
        let _ = hypr_dispatch_socket(&focus);
        let _ = hypr_dispatch_socket("dispatch hl.dsp.window.pin()");
    }

    if let Some(prev) = prev_active {
        if prev != addr {
            let _ = hypr_dispatch_socket(&format!("dispatch focuswindow address:{prev}"));
        } else {
            let _ = hypr_dispatch_socket("dispatch cyclenext");
        }
    } else {
        let _ = hypr_dispatch_socket("dispatch cyclenext");
    }

    if exclusive && !mon_name.is_empty() {
        set_monitor_reserved(&mon_name, position, h, true);
    }

    if mon_name.is_empty() {
        None
    } else {
        Some(mon_name)
    }
}

