use super::*;
use crate::converse::converse_spawn_or_signal;
use crate::hyprland::{clear_reserved_for_position, dock_bar_hyprland, set_monitor_reserved};
use beca_core::BarProperties;
use crate::widgets::*;

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

    let props = BarProperties::from_config(&bar_cfg);
    let set_widget = |widget: &str, show_key: &str, section_key: &str| {
        if let Some(section) = props.section(widget) {
            let _ = app.set_property(show_key, Value::Bool(true));
            let _ = app.set_property(section_key, Value::String(section.into()));
        } else {
            let _ = app.set_property(show_key, Value::Bool(false));
        }
    };

    let _ = app.set_property("position", Value::String(position.into()));
    set_widget("clock", "show-clock", "clock-section");
    set_widget("launcher", "show-launcher", "launcher-section");
    set_widget("conversation", "show-conversation", "conversation-section");
    set_widget("volume-out", "show-volume-out", "volume-out-section");
    set_widget("volume-in", "show-volume-in", "volume-in-section");
    set_widget("network", "show-network", "network-section");
    set_widget("alerts", "show-alerts", "alerts-section");
    set_widget("bluetooth", "show-bluetooth", "bluetooth-section");
    set_widget("cpu", "show-cpu", "cpu-section");
    set_widget("memory", "show-memory", "memory-section");
    set_widget("disk", "show-disk", "disk-section");

    let show_volume_out = props.visible("volume-out");
    let show_volume_in = props.visible("volume-in");
    let show_network = props.visible("network");
    let show_alerts = props.visible("alerts");
    let show_bluetooth = props.visible("bluetooth");
    let show_cpu = props.visible("cpu");
    let show_memory = props.visible("memory");
    let show_disk = props.visible("disk");

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
    let _ = app.set_callback("alerts-clicked", |_args| {
        open_alerts_panel();
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

    {
        let weak = app.as_weak();
        let _ = app.set_callback("volume-out-clicked", move |_args| {
            toggle_volume_mute("@DEFAULT_AUDIO_SINK@");
            let v = read_volume("@DEFAULT_AUDIO_SINK@");
            let weak2 = weak.clone();
            let _ = ui_loader::invoke_on_event_loop(move || {
                if let Some(app) = weak2.upgrade() {
                    let _ = app.set_property("volume-out-text", Value::String(v.text.into()));
                    let _ = app.set_property("volume-out-muted", Value::Bool(v.muted));
                }
            });
            Value::Void
        });
    }
    {
        let weak = app.as_weak();
        let _ = app.set_callback("volume-in-clicked", move |_args| {
            toggle_volume_mute("@DEFAULT_AUDIO_SOURCE@");
            let v = read_volume("@DEFAULT_AUDIO_SOURCE@");
            let weak2 = weak.clone();
            let _ = ui_loader::invoke_on_event_loop(move || {
                if let Some(app) = weak2.upgrade() {
                    let _ = app.set_property("volume-in-text", Value::String(v.text.into()));
                    let _ = app.set_property("volume-in-muted", Value::Bool(v.muted));
                }
            });
            Value::Void
        });
    }
    let _ = app.set_callback("network-clicked", |_args| {
        open_network_panel();
        Value::Void
    });
    let _ = app.set_callback("bluetooth-clicked", |_args| {
        open_bluetooth_panel();
        Value::Void
    });

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
