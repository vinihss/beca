use anyhow::Result;
use slint_interpreter::{ComponentHandle, Value};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use beca_ui::ui_loader;

fn converse_spawn_or_signal(slot: &std::sync::Mutex<Option<std::process::Child>>) -> bool {
    let mut slot = match slot.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut alive = false;
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(None) => alive = true,
            Ok(Some(_)) => {
                *slot = None;
            }
            Err(_) => alive = true,
        }
    }
    if alive {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(dir)
                .join("beca")
                .join("converse.ctl");
            if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
                use std::io::Write;
                let _ = s.write_all(b"quit\n");
            } else {
                eprintln!("[beca-bar] converse ctl unreachable: {}", path.display());
            }
        }
        return false;
    }

    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .env("BECA_NO_DAEMON", "1")
            .arg("mod")
            .arg("--local")
            .arg("voice:converse")
            .spawn();
        match child {
            Ok(c) => {
                *slot = Some(c);
                return true;
            }
            Err(e) => eprintln!("[beca-bar] failed to spawn voice:converse: {e}"),
        }
    }
    false
}

fn format_clock(format: &str) -> String {

    let out = Command::new("date").arg(format!("+{}", format)).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "--:--".to_string(),
    }
}

#[derive(Clone, Debug)]
struct VolumeInfo {
    text: String,
    muted: bool,
}

fn read_volume_wpctl(target: &str) -> Option<VolumeInfo> {
    let out = Command::new("wpctl")
        .args(["get-volume", target])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);

    let muted = s.contains("[MUTED]");
    let vol = s
        .split_whitespace()
        .find_map(|part| part.parse::<f32>().ok())
        .unwrap_or(0.0);
    let pct = (vol * 100.0).round().clamp(0.0, 999.0) as i32;
    Some(VolumeInfo {
        text: format!("{pct}%"),
        muted,
    })
}

fn read_volume_pactl(target: &str) -> Option<VolumeInfo> {
    let (cmd_target, mute_flag) = if target == "@DEFAULT_AUDIO_SINK@" {
        ("@DEFAULT_SINK@", "get-sink-mute")
    } else {
        ("@DEFAULT_SOURCE@", "get-source-mute")
    };
    let vol_out = Command::new("pactl")
        .args(["get-sink-volume", cmd_target])
        .output()
        .ok();
    let vol_out = if target == "@DEFAULT_AUDIO_SINK@" {
        vol_out
    } else {
        Command::new("pactl")
            .args(["get-source-volume", cmd_target])
            .output()
            .ok()
    };
    let vol_out = vol_out?;
    if !vol_out.status.success() {
        return None;
    }
    let vs = String::from_utf8_lossy(&vol_out.stdout);
    let pct = vs
        .split('%')
        .next()
        .and_then(|head| head.split_whitespace().last())
        .and_then(|n| n.parse::<i32>().ok())
        .unwrap_or(0);
    let mute_out = Command::new("pactl")
        .args([mute_flag, cmd_target])
        .output()
        .ok();
    let muted = mute_out
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("yes"))
        .unwrap_or(false);
    Some(VolumeInfo {
        text: format!("{pct}%"),
        muted,
    })
}

fn read_volume(target: &str) -> VolumeInfo {
    read_volume_wpctl(target)
        .or_else(|| read_volume_pactl(target))
        .unwrap_or(VolumeInfo {
            text: "--%".to_string(),
            muted: false,
        })
}

fn adjust_volume_wpctl(target: &str, up: bool) -> bool {
    let step = if up { "5%+" } else { "5%-" };
    Command::new("wpctl")
        .args(["set-volume", "-l", "1.5", target, step])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn adjust_volume_pactl(target: &str, up: bool) -> bool {
    let (cmd, t) = if target == "@DEFAULT_AUDIO_SINK@" {
        ("set-sink-volume", "@DEFAULT_SINK@")
    } else {
        ("set-source-volume", "@DEFAULT_SOURCE@")
    };
    let step = if up { "+5%" } else { "-5%" };
    Command::new("pactl")
        .args([cmd, t, step])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn adjust_volume(target: &str, up: bool) {
    if !adjust_volume_wpctl(target, up) {
        let _ = adjust_volume_pactl(target, up);
    }
}

pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    match action {
        "show-top" => show("top"),
        "show-bottom" => show("bottom"),
        "show" => {
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
            let pos = match pos {
                "bottom" => "bottom",
                _ => "top",
            };
            show(pos)
        }
        "status" => {

            let resp = crate::daemon::send_request(&serde_json::json!({"type":"bar.status"}));
            match resp {
                Ok(v) => {
                    println!(
                        "{}",
                        serde_json::to_string(&v).unwrap_or_else(|_| "{}".into())
                    );
                    Ok(())
                }
                Err(e) => {
                    println!(
                        "{}",
                        serde_json::json!({"ok": false, "error": e.to_string()})
                    );
                    Ok(())
                }
            }
        }
        "hide" | "hide-top" | "hide-bottom" | "stop" | "stop-top" | "stop-bottom" => {

            let target = format!("bar:{action}");
            if let Some(resp) = crate::daemon::try_route_mod(&target, args) {
                println!(
                    "{}",
                    serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into())
                );
                if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "{}",
                        resp.get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("bar lifecycle action failed")
                    )
                }
            } else {
                anyhow::bail!(
                    "bar {action} requires daemon-managed lifecycle (start `beca daemon` or run via `beca mod bar:{action}`)"
                )
            }
        }
        other => anyhow::bail!(
            "unknown bar action: {other} (expected: show, show-top, show-bottom, hide, hide-top, hide-bottom, status)"
        ),
    }
}

fn hyprland_socket1() -> Option<std::path::PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    Some(runtime.join("hypr").join(sig).join(".socket.sock"))
}

fn hypr_dispatch_socket(cmd: &str) -> Option<String> {
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

fn hypr_eval(lua: &str) -> Option<String> {

    hypr_dispatch_socket(&format!("eval {lua}"))
}

fn set_monitor_reserved(monitor_name: &str, position: &str, height: i64, enable: bool) {

    let mut left = 0i64;
    let mut top = 0i64;
    let mut right = 0i64;
    let mut bottom = 0i64;
    if let Some(out) = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
    {
        if let Ok(monitors) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
            if let Some(m) = monitors
                .iter()
                .find(|m| m.get("name").and_then(|v| v.as_str()) == Some(monitor_name))
            {
                if let Some(rsv) = m.get("reserved").and_then(|v| v.as_array()) {
                    left = rsv.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                    top = rsv.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                    right = rsv.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                    bottom = rsv.get(3).and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }
        }
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
        let lua = format!(
            "hl.monitor({{output='{name}', reserved={{left={l},top={nt},right={r},bottom={nb}}}}}); return 'ok'"
        );
        let _ = hypr_eval(&lua);
    }
}

fn dock_bar_hyprland(position: &str, height: f32, exclusive: bool) -> Option<String> {
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

pub fn show(position: &str) -> Result<()> {
    let resolved = beca_core::config::load();
    let bar_cfg = resolved.config.modules.bar_config_for(position);
    let theme = &resolved.theme;
    if !bar_cfg.enabled {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let force_window = std::env::var_os("BECA_BAR_FORCE_WINDOW").is_some();
        if !force_window {
            let pos = match position {
                "bottom" => beca_layer_shell::BarPosition::Bottom,
                _ => beca_layer_shell::BarPosition::Top,
            };
            match beca_layer_shell::run_bar(pos, &bar_cfg, theme) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    eprintln!("[beca-bar] layer-shell unavailable, falling back to window mode: {e}");

                    if e.contains("dispatch:")
                        || e.contains("roundtrip:")
                        || e.contains("flush:")
                        || e.contains("Backend error")
                        || e.contains("Connection reset")
                    {
                        if let Ok(exe) = std::env::current_exe() {
                            let status = Command::new(exe)
                                .env("BECA_NO_DAEMON", "1")
                                .env("BECA_BAR_FORCE_WINDOW", "1")
                                .args(["mod", "--local", &format!("bar:show-{}", position)])
                                .status();
                            match status {
                                Ok(s) if s.success() => return Ok(()),
                                Ok(s) => anyhow::bail!(
                                    "bar window fallback process exited with status {s}"
                                ),
                                Err(err) => anyhow::bail!(
                                    "failed to spawn bar window fallback: {err}"
                                ),
                            }
                        }
                    }
                }
            }
        }
    }

    let theme = resolved.theme.voice_theme();

    let app = ui_loader::create("BarWindow").map_err(|e| anyhow::anyhow!(e))?;
    ui_loader::apply_theme_global(&app, &theme);

    let (left_l, center_l, right_l) = bar_cfg.resolved_layout();
    let find_section = |name: &str| -> Option<String> {
        if left_l.iter().any(|w| w == name) {
            Some("left".into())
        } else if right_l.iter().any(|w| w == name) {
            Some("right".into())
        } else if center_l.iter().any(|w| w == name) {
            Some("center".into())
        } else {
            None
        }
    };

    let _ = app.set_property("position", Value::String(position.into()));
    if let Some(sec) = find_section("clock") {
        let _ = app.set_property("show-clock", Value::Bool(true));
        let _ = app.set_property("clock-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-clock", Value::Bool(false));
    }
    if let Some(sec) = find_section("launcher") {
        let _ = app.set_property("show-launcher", Value::Bool(true));
        let _ = app.set_property("launcher-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-launcher", Value::Bool(false));
    }
    if let Some(sec) = find_section("conversation") {
        let _ = app.set_property("show-conversation", Value::Bool(true));
        let _ = app.set_property("conversation-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-conversation", Value::Bool(false));
    }
    let show_volume_out = find_section("volume-out").is_some();
    if let Some(sec) = find_section("volume-out") {
        let _ = app.set_property("show-volume-out", Value::Bool(true));
        let _ = app.set_property("volume-out-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-volume-out", Value::Bool(false));
    }
    let show_volume_in = find_section("volume-in").is_some();
    if let Some(sec) = find_section("volume-in") {
        let _ = app.set_property("show-volume-in", Value::Bool(true));
        let _ = app.set_property("volume-in-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-volume-in", Value::Bool(false));
    }

    let show_network = find_section("network").is_some();
    if let Some(sec) = find_section("network") {
        let _ = app.set_property("show-network", Value::Bool(true));
        let _ = app.set_property("network-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-network", Value::Bool(false));
    }
    let show_alerts = find_section("alerts").is_some();
    if let Some(sec) = find_section("alerts") {
        let _ = app.set_property("show-alerts", Value::Bool(true));
        let _ = app.set_property("alerts-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-alerts", Value::Bool(false));
    }
    let show_bluetooth = find_section("bluetooth").is_some();
    if let Some(sec) = find_section("bluetooth") {
        let _ = app.set_property("show-bluetooth", Value::Bool(true));
        let _ = app.set_property("bluetooth-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-bluetooth", Value::Bool(false));
    }
    let show_cpu = find_section("cpu").is_some();
    if let Some(sec) = find_section("cpu") {
        let _ = app.set_property("show-cpu", Value::Bool(true));
        let _ = app.set_property("cpu-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-cpu", Value::Bool(false));
    }
    let show_memory = find_section("memory").is_some();
    if let Some(sec) = find_section("memory") {
        let _ = app.set_property("show-memory", Value::Bool(true));
        let _ = app.set_property("memory-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-memory", Value::Bool(false));
    }
    let show_disk = find_section("disk").is_some();
    if let Some(sec) = find_section("disk") {
        let _ = app.set_property("show-disk", Value::Bool(true));
        let _ = app.set_property("disk-section", Value::String(sec.into()));
    } else {
        let _ = app.set_property("show-disk", Value::Bool(false));
    }

    let _ = app.set_property(
        "launcher-label",
        Value::String(bar_cfg.launcher_label.clone().into()),
    );
    let _ = app.set_property(
        "left-align",
        Value::String(bar_cfg.left_align.clone().into()),
    );
    let _ = app.set_property(
        "center-align",
        Value::String(bar_cfg.center_align.clone().into()),
    );
    let _ = app.set_property(
        "right-align",
        Value::String(bar_cfg.right_align.clone().into()),
    );
    let _ = app.set_property(
        "background-opacity",
        Value::Number(bar_cfg.background_opacity as f64),
    );
    let _ = app.set_property("show-border", Value::Bool(bar_cfg.show_border));
    if let Some(color) = bar_cfg.background.as_ref().and_then(|s| beca_ui::theme::parse_color(s)) {
        let _ = app.set_property("bar-background", Value::Brush(color.into()));
    }

    let initial = format_clock(&bar_cfg.clock_format);
    let _ = app.set_property("clock-text", Value::String(initial.into()));
    if show_volume_out {
        let v = read_volume("@DEFAULT_AUDIO_SINK@");
        let _ = app.set_property("volume-out-text", Value::String(v.text.into()));
        let _ = app.set_property("volume-out-muted", Value::Bool(v.muted));
    }
    if show_volume_in {
        let v = read_volume("@DEFAULT_AUDIO_SOURCE@");
        let _ = app.set_property("volume-in-text", Value::String(v.text.into()));
        let _ = app.set_property("volume-in-muted", Value::Bool(v.muted));
    }
    if show_network { let _ = app.set_property("network-text", Value::String(read_network().into())); }
    if show_alerts { let _ = app.set_property("alerts-text", Value::String(read_alerts().into())); }
    if show_bluetooth { let _ = app.set_property("bluetooth-text", Value::String(read_bluetooth().into())); }
    if show_cpu { let _ = app.set_property("cpu-text", Value::String(read_cpu().into())); }
    if show_memory { let _ = app.set_property("memory-text", Value::String(read_memory().into())); }
    if show_disk { let _ = app.set_property("disk-text", Value::String(read_disk().into())); }

    let _ = app.set_callback("launcher-clicked", |_args| {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "beca".to_string());
        let _ = Command::new(exe).args(["mod", "launcher:show"]).spawn();
        Value::Void
    });

    let converse_slot: Arc<std::sync::Mutex<Option<std::process::Child>>> =
        Arc::new(std::sync::Mutex::new(None));

    {
        let converse_slot = converse_slot.clone();
        let weak_converse = app.as_weak();
        let _ = app.set_callback("conversation-clicked", move |_args| {
            let active = converse_spawn_or_signal(&converse_slot);
            let weak2 = weak_converse.clone();
            let _ = ui_loader::invoke_on_event_loop(move || {
                if let Some(app) = weak2.upgrade() {
                    let _ = app.set_property("conversation-active", Value::Bool(active));
                }
            });
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let _ = app.set_callback("volume-out-scrolled", move |args| {
            let delta = args
                .first()
                .and_then(|v| match v {
                    Value::Number(n) => Some(*n as f32),
                    _ => None,
                })
                .unwrap_or(0.0);
            if delta != 0.0 {
                adjust_volume("@DEFAULT_AUDIO_SINK@", delta > 0.0);
                let v = read_volume("@DEFAULT_AUDIO_SINK@");
                let weak2 = weak.clone();
                let _ = ui_loader::invoke_on_event_loop(move || {
                    if let Some(app) = weak2.upgrade() {
                        let _ = app.set_property("volume-out-text", Value::String(v.text.into()));
                        let _ = app.set_property("volume-out-muted", Value::Bool(v.muted));
                    }
                });
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let _ = app.set_callback("volume-in-scrolled", move |args| {
            let delta = args
                .first()
                .and_then(|v| match v {
                    Value::Number(n) => Some(*n as f32),
                    _ => None,
                })
                .unwrap_or(0.0);
            if delta != 0.0 {
                adjust_volume("@DEFAULT_AUDIO_SOURCE@", delta > 0.0);
                let v = read_volume("@DEFAULT_AUDIO_SOURCE@");
                let weak2 = weak.clone();
                let _ = ui_loader::invoke_on_event_loop(move || {
                    if let Some(app) = weak2.upgrade() {
                        let _ = app.set_property("volume-in-text", Value::String(v.text.into()));
                        let _ = app.set_property("volume-in-muted", Value::Bool(v.muted));
                    }
                });
            }
            Value::Void
        });
    }

    let running = Arc::new(AtomicBool::new(true));

    {
        let converse_slot = converse_slot.clone();
        let weak_poll = app.as_weak();
        let running_flag = running.clone();
        thread::spawn(move || {
            let mut was_running = false;
            while running_flag.load(Ordering::Relaxed) {
                let mut run = false;
                if let Ok(mut slot) = converse_slot.lock() {
                    if let Some(child) = slot.as_mut() {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                *slot = None;
                            }
                            Ok(None) => {
                                run = true;
                            }
                            Err(_) => {
                                run = true;
                            }
                        }
                    }
                }
                if run != was_running {
                    was_running = run;
                    let weak2 = weak_poll.clone();
                    let _ = ui_loader::invoke_on_event_loop(move || {
                        if let Some(app) = weak2.upgrade() {
                            let _ = app.set_property("conversation-active", Value::Bool(run));
                        }
                    });
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
    }

    let weak_clock = app.as_weak();
    let fmt = bar_cfg.clock_format.clone();
    let show_clock = bar_cfg.show_clock;
    if show_clock {
        let running_flag = running.clone();
        thread::spawn(move || {

            let interval = if fmt.contains("%S") {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(30)
            };
            while running_flag.load(Ordering::Relaxed) {
                let text = format_clock(&fmt);
                let weak2 = weak_clock.clone();
                let _ = ui_loader::invoke_on_event_loop(move || {
                    if let Some(app) = weak2.upgrade() {
                        let _ = app.set_property("clock-text", Value::String(text.into()));
                    }
                });
                thread::sleep(interval);
            }
        });
    }

    let show_volume_out = bar_cfg.show_volume_out;
    let show_volume_in = bar_cfg.show_volume_in;
    if show_volume_out || show_volume_in {
        let running_flag = running.clone();
        let weak_vol = app.as_weak();
        thread::spawn(move || {
            while running_flag.load(Ordering::Relaxed) {
                let out = if show_volume_out {
                    Some(read_volume("@DEFAULT_AUDIO_SINK@"))
                } else {
                    None
                };
                let inp = if show_volume_in {
                    Some(read_volume("@DEFAULT_AUDIO_SOURCE@"))
                } else {
                    None
                };
                let weak2 = weak_vol.clone();
                let _ = ui_loader::invoke_on_event_loop(move || {
                    if let Some(app) = weak2.upgrade() {
                        if let Some(v) = out {
                            let _ = app.set_property("volume-out-text", Value::String(v.text.into()));
                            let _ = app.set_property("volume-out-muted", Value::Bool(v.muted));
                        }
                        if let Some(v) = inp {
                            let _ = app.set_property("volume-in-text", Value::String(v.text.into()));
                            let _ = app.set_property("volume-in-muted", Value::Bool(v.muted));
                        }
                    }
                });
                thread::sleep(Duration::from_secs(2));
            }
        });
    }

    if show_network || show_alerts || show_bluetooth || show_cpu || show_memory || show_disk {
        let running_flag = running.clone();
        let weak_stats = app.as_weak();
        thread::spawn(move || {
            while running_flag.load(Ordering::Relaxed) {
                let net = if show_network { Some(read_network()) } else { None };
                let al = if show_alerts { Some(read_alerts()) } else { None };
                let bt = if show_bluetooth { Some(read_bluetooth()) } else { None };
                let cpu = if show_cpu { Some(read_cpu()) } else { None };
                let mem = if show_memory { Some(read_memory()) } else { None };
                let disk = if show_disk { Some(read_disk()) } else { None };
                let weak2 = weak_stats.clone();
                let _ = ui_loader::invoke_on_event_loop(move || {
                    if let Some(app) = weak2.upgrade() {
                        if let Some(v) = net { let _ = app.set_property("network-text", Value::String(v.into())); }
                        if let Some(v) = al { let _ = app.set_property("alerts-text", Value::String(v.into())); }
                        if let Some(v) = bt { let _ = app.set_property("bluetooth-text", Value::String(v.into())); }
                        if let Some(v) = cpu { let _ = app.set_property("cpu-text", Value::String(v.into())); }
                        if let Some(v) = mem { let _ = app.set_property("memory-text", Value::String(v.into())); }
                        if let Some(v) = disk { let _ = app.set_property("disk-text", Value::String(v.into())); }
                    }
                });
                thread::sleep(Duration::from_secs(3));
            }
        });
    }

    let pos = position.to_string();
    let dock_h = bar_cfg.height;
    let exclusive = bar_cfg.exclusive;
    let dock_monitor: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let dock_monitor_set = dock_monitor.clone();
    thread::spawn(move || {
        let mon = dock_bar_hyprland(&pos, dock_h, exclusive);
        if let Ok(mut slot) = dock_monitor_set.lock() {
            *slot = mon;
        }
    });

    ui_loader::show(&app).map_err(|e| anyhow::anyhow!(e))?;
    ui_loader::run_event_loop().map_err(|e| anyhow::anyhow!(e))?;
    running.store(false, Ordering::Relaxed);

    if exclusive {
        if let Ok(slot) = dock_monitor.lock() {
            if let Some(mon) = slot.as_ref() {
                set_monitor_reserved(mon, position, dock_h.max(24.0) as i64, false);
            } else {
                clear_reserved_for_position(position);
            }
        } else {
            clear_reserved_for_position(position);
        }
    }
    Ok(())
}


fn read_network() -> String {
    let out = Command::new("nmcli").args(["-t", "-f", "TYPE,STATE,CONNECTION", "device"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            for line in s.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 && parts[1] == "connected" {
                    let name = parts[2];
                    if !name.is_empty() && name != "--" {
                        let short = if name.chars().count() > 12 { name.chars().take(12).collect::<String>() + "…" } else { name.to_string() };
                        return short;
                    }
                }
            }
            for line in s.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 && parts[1] == "connected" { return "on".into(); }
            }
        }
    }
    let out = Command::new("ip").args(["-o", "link", "show", "up"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.lines().any(|l| !l.contains("lo:")) { return "on".into(); }
        }
    }
    "off".into()
}

fn read_bluetooth() -> String {
    let out = Command::new("bluetoothctl").args(["show"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let powered = s.lines().any(|l| l.trim() == "Powered: yes");
            if !powered { return "off".into(); }
            if let Ok(dev) = Command::new("bluetoothctl").args(["devices", "Connected"]).output() {
                let ds = String::from_utf8_lossy(&dev.stdout);
                let count = ds.lines().filter(|l| !l.trim().is_empty()).count();
                if count > 0 { return format!("{count}"); }
            }
            return "on".into();
        }
    }
    "--".into()
}

fn read_alerts() -> String {
    if let Ok(o) = Command::new("swaync-client").args(["-c"]).output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() { return s; }
        }
    }
    if let Ok(o) = Command::new("makoctl").arg("list").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let count = s.matches("\n\n").count() + if s.trim().is_empty() { 0 } else { 1 };
            return count.to_string();
        }
    }
    if let Ok(o) = Command::new("dunstctl").args(["count", "displayed"]).output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() { return s; }
        }
    }
    "0".into()
}

fn read_cpu() -> String {
    static PREV: std::sync::OnceLock<std::sync::Mutex<(u64, u64)>> = std::sync::OnceLock::new();
    let lock = PREV.get_or_init(|| std::sync::Mutex::new((0, 0)));
    let Ok(content) = std::fs::read_to_string("/proc/stat") else { return "--%".into(); };
    let Some(line) = content.lines().next() else { return "--%".into(); };
    let nums: Vec<u64> = line.split_whitespace().skip(1).filter_map(|n| n.parse().ok()).collect();
    if nums.len() < 4 { return "--%".into(); }
    let idle = nums.get(3).copied().unwrap_or(0) + nums.get(4).copied().unwrap_or(0);
    let total: u64 = nums.iter().sum();
    let mut prev = match lock.lock() { Ok(g) => g, Err(_) => return "--%".into() };
    let (pt, pi) = *prev;
    *prev = (total, idle);
    if pt == 0 || total <= pt { return "--%".into(); }
    let dt = total - pt;
    let di = idle.saturating_sub(pi);
    let usage = ((dt.saturating_sub(di)) as f64 / dt as f64 * 100.0).round().clamp(0.0, 100.0) as i64;
    format!("{usage}%")
}

fn read_memory() -> String {
    let Ok(content) = std::fs::read_to_string("/proc/meminfo") else { return "--%".into(); };
    let mut total: u64 = 0;
    let mut avail: u64 = 0;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            total = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            avail = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        }
    }
    if total == 0 { return "--%".into(); }
    let used = total.saturating_sub(avail);
    let pct = (used as f64 / total as f64 * 100.0).round().clamp(0.0, 100.0) as i64;
    format!("{pct}%")
}

fn read_disk() -> String {
    let out = Command::new("df").args(["-h", "/"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = s.lines().nth(1) {
                if let Some(pct) = line.split_whitespace().nth(4) {
                    return pct.to_string();
                }
            }
        }
    }
    "--%".into()
}
