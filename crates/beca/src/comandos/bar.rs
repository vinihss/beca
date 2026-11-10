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

fn format_clock(format: &str) -> String {
    // Use `date` for local-time strftime formatting (no extra deps).
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
    // e.g. "Volume: 0.39" or "Volume: 0.72 [MUTED]"
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
            // best-effort: query daemon if available
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
            // lifecycle (stop/hide tracked instance) is managed by daemon; local process cannot
            // hide an already-running bar instance. Try to route via daemon.
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
    // 0.57+: socket accepts `eval <lua>`
    hypr_dispatch_socket(&format!("eval {lua}"))
}

fn set_monitor_reserved(monitor_name: &str, position: &str, height: i64, enable: bool) {
    // Preserve other reserved edges (array observed: [left, top, right, bottom]).
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

/// Clear reserved edge for a bar position on all monitors (best-effort, daemon stop path).
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
        // preserve other edges? we only manage top/bottom for bars; set both managed edges as needed.
        // Safer: read current reserved [l,t,r,b] and zero only the relevant edge.
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

/// Best-effort Hyprland 0.57+ docking for the bar window (top/bottom).
/// Returns the monitor name if docked (used for exclusive-zone cleanup).
fn dock_bar_hyprland(position: &str, height: f32, exclusive: bool) -> Option<String> {
    let pid = std::process::id();
    let h = (height.max(24.0)) as i64;

    // Wait for the bar window to map and appear in Hyprland clients (fixes displaced spawn).
    // Poll quickly at start to reduce visible time at wrong coordinates.
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

    // Resolve monitor geometry.
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

    // Remember active window to restore focus (bar must not steal focus / be killable via Super+Q).
    let prev_active = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("address").and_then(|a| a.as_str()).map(|s| s.to_string()));

    // Hyprland 0.57+ Lua dispatch API: `dispatch <dispatcher-expr>` (hl.dsp.*).
    // Prefer address-targeted operations; avoid leaving focus on the bar.
    let focus = format!("dispatch hl.dsp.focus({{window = 'address:{addr}'}})");
    let move_cmd = format!("dispatch hl.dsp.window.move({{x={x},y={y}}})");
    let resize = format!("dispatch hl.dsp.window.resize({{x={mw},y={h}}})");

    // Multiple passes to win races with initial map/layout and keep it docked.
    for i in 0..8 {
        let _ = hypr_dispatch_socket(&focus);
        let _ = hypr_dispatch_socket(&move_cmd);
        let _ = hypr_dispatch_socket(&resize);
        // move quickly at first to minimize visible wrong placement
        let wait = if i < 3 { 30 } else { 80 };
        thread::sleep(Duration::from_millis(wait));
    }

    // Fallback: legacy string dispatch with explicit target address.
    let _ = hypr_dispatch_socket(&format!(
        "dispatch movewindowpixel exact {x} {y},address:{addr}"
    ));
    let _ = hypr_dispatch_socket(&format!(
        "dispatch resizewindowpixel exact {mw} {h},address:{addr}"
    ));

    // Pin (toggle dispatcher): only pin if not already pinned.
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

    // Restore focus so Super+Q (killactive) doesn't target the bar.
    if let Some(prev) = prev_active {
        if prev != addr {
            let _ = hypr_dispatch_socket(&format!("dispatch focuswindow address:{prev}"));
        } else {
            let _ = hypr_dispatch_socket("dispatch cyclenext");
        }
    } else {
        let _ = hypr_dispatch_socket("dispatch cyclenext");
    }

    // Exclusive zone: reserve space so tiled windows don't overlap the bar.
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
    let bar_cfg = &resolved.config.modules.bar;
    let theme = resolved.theme.voice_theme();

    let app = ui_loader::create("BarWindow").map_err(|e| anyhow::anyhow!(e))?;
    ui_loader::apply_theme_global(&app, &theme);

    let _ = app.set_property("position", Value::String(position.into()));
    let _ = app.set_property("show-clock", Value::Bool(bar_cfg.show_clock));
    let _ = app.set_property("show-launcher", Value::Bool(bar_cfg.show_launcher));
    let _ = app.set_property(
        "launcher-label",
        Value::String(bar_cfg.launcher_label.clone().into()),
    );
    let _ = app.set_property("show-volume-out", Value::Bool(bar_cfg.show_volume_out));
    let _ = app.set_property("show-volume-in", Value::Bool(bar_cfg.show_volume_in));
    let _ = app.set_property("show-voice", Value::Bool(bar_cfg.show_voice));
    let _ = app.set_property(
        "voice-label",
        Value::String(bar_cfg.voice_label.clone().into()),
    );
    let _ = app.set_property("show-voice-assistant", Value::Bool(true));
    let _ = app.set_property("voice-assistant-label", Value::String("VA".into()));
    // layout: 3 sections (left/center/right) with per-section alignment and widget placement
    let _ = app.set_property(
        "clock-section",
        Value::String(bar_cfg.clock_section.clone().into()),
    );
    let _ = app.set_property(
        "launcher-section",
        Value::String(bar_cfg.launcher_section.clone().into()),
    );
    let _ = app.set_property(
        "volume-out-section",
        Value::String(bar_cfg.volume_out_section.clone().into()),
    );
    let _ = app.set_property(
        "volume-in-section",
        Value::String(bar_cfg.volume_in_section.clone().into()),
    );
    let _ = app.set_property(
        "voice-section",
        Value::String(bar_cfg.voice_section.clone().into()),
    );
    let _ = app.set_property(
        "voice-assistant-section",
        Value::String(bar_cfg.voice_section.clone().into()),
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
    // set initial clock/volume text
    let initial = format_clock(&bar_cfg.clock_format);
    let _ = app.set_property("clock-text", Value::String(initial.into()));
    if bar_cfg.show_volume_out {
        let v = read_volume("@DEFAULT_AUDIO_SINK@");
        let _ = app.set_property("volume-out-text", Value::String(v.text.into()));
        let _ = app.set_property("volume-out-muted", Value::Bool(v.muted));
    }
    if bar_cfg.show_volume_in {
        let v = read_volume("@DEFAULT_AUDIO_SOURCE@");
        let _ = app.set_property("volume-in-text", Value::String(v.text.into()));
        let _ = app.set_property("volume-in-muted", Value::Bool(v.muted));
    }

    // launcher click -> spawn launcher in separate process (bar stays alive)
    let _ = app.set_callback("launcher-clicked", |_args| {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "beca".to_string());
        let _ = Command::new(exe).args(["mod", "launcher:show"]).spawn();
        Value::Void
    });

    // voice click -> ask resident voice to show (daemon-routed), avoiding duplicate windows.
    // default: respond+TTS assistant flow (configurable via modules.voice.respond)
    let _ = app.set_callback("voice-clicked", |_args| {
        // If a Voice Capture window already exists, focus it instead of spawning another.
        if let Ok(out) = Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
        {
            if out.status.success() {
                if let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
                    if let Some(addr) = clients.iter().find_map(|c| {
                        let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        if title == "Voice Capture" || title == "Voice Assistant" {
                            c.get("address").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    }) {
                        let _ = Command::new("hyprctl")
                            .args(["dispatch", "focuswindow", &format!("address:{addr}")])
                            .spawn();
                        return Value::Void;
                    }
                }
            }
        }
        // Ask daemon/resident directly (avoids spawning an extra CLI process).
        let req = serde_json::json!({"type":"mod","target":"voice:show","args":[]});
        let _ = crate::daemon::send_request(&req);
        Value::Void
    });

    let _ = app.set_callback("voice-assistant-clicked", |_args| {
        if let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() {
            if out.status.success() {
                if let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
                    if let Some(addr) = clients.iter().find_map(|c| {
                        let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        if title == "Voice Assistant" {
                            c.get("address").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    }) {
                        let _ = Command::new("hyprctl")
                            .args(["dispatch", "focuswindow", &format!("address:{addr}")])
                            .spawn();
                        return Value::Void;
                    }
                }
            }
        }
        let req = serde_json::json!({"type":"mod","target":"voice:assistant","args":[]});
        let _ = crate::daemon::send_request(&req);
        Value::Void
    });

    let running = Arc::new(AtomicBool::new(true));

    // clock updater thread
    let weak_clock = app.as_weak();
    let fmt = bar_cfg.clock_format.clone();
    let show_clock = bar_cfg.show_clock;
    if show_clock {
        let running_flag = running.clone();
        thread::spawn(move || {
            // update roughly every 30s normally, 1s if format includes seconds
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

    // volume updater thread (output/input)
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

    // Best-effort: dock/anchor bar on Hyprland (compatible with 0.57+ lua dispatch).
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
    // Best-effort cleanup of exclusive zone on normal exit.
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
