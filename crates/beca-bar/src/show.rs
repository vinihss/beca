use super::*;
use crate::hyprland::{clear_reserved_for_position, dock_bar_hyprland, set_monitor_reserved};
use beca_core::BarProperties;
use beca_widgets::{WidgetAction, WidgetController, WidgetLayout, WidgetSink};
use slint_interpreter::{ComponentInstance, Value};

struct InterpreterSink<'a> {
    app: &'a ComponentInstance,
}

impl InterpreterSink<'_> {
    fn write(&self, property: String, value: Value) {
        let _ = self.app.set_property(property.as_str(), value);
    }
}

impl WidgetSink for InterpreterSink<'_> {
    fn set_visible(&self, id: beca_widgets::WidgetId, visible: bool) {
        self.write(format!("show-{}", id.as_str()), Value::Bool(visible));
    }

    fn set_section(&self, id: beca_widgets::WidgetId, section: &'static str) {
        self.write(
            format!("{}-section", id.as_str()),
            Value::String(section.into()),
        );
    }

    fn set_text(&self, id: beca_widgets::WidgetId, text: &str) {
        self.write(format!("{}-text", id.as_str()), Value::String(text.into()));
    }

    fn set_muted(&self, id: beca_widgets::WidgetId, muted: bool) {
        let property = match id {
            beca_widgets::WidgetId::VolumeOut => "volume-out-muted",
            beca_widgets::WidgetId::VolumeIn => "volume-in-muted",
            _ => return,
        };
        self.write(property.into(), Value::Bool(muted));
    }

    fn set_active(&self, id: beca_widgets::WidgetId, active: bool) {
        if id != beca_widgets::WidgetId::Conversation {
            return;
        }
        self.write("conversation-active".into(), Value::Bool(active));
    }
}

fn apply_action(app: &ComponentInstance, controller: &WidgetController, action: WidgetAction) {
    let sink = InterpreterSink { app };
    if let Some(update) = controller.act(action) {
        update.apply(&sink);
    }
}

fn bind_callbacks(app: &ComponentInstance, controller: WidgetController) {
    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("launcher-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(&app, &controller, WidgetAction::Launcher);
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("alerts-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(&app, &controller, WidgetAction::Alerts);
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("conversation-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(&app, &controller, WidgetAction::ConversationToggle);
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("volume-out-scrolled", move |args| {
            let delta = scroll_delta(args);
            if delta == 0.0 {
                return Value::Void;
            }
            if let Some(app) = weak.upgrade() {
                apply_action(
                    &app,
                    &controller,
                    WidgetAction::VolumeScroll {
                        id: beca_widgets::WidgetId::VolumeOut,
                        up: delta > 0.0,
                    },
                );
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("volume-in-scrolled", move |args| {
            let delta = scroll_delta(args);
            if delta == 0.0 {
                return Value::Void;
            }
            if let Some(app) = weak.upgrade() {
                apply_action(
                    &app,
                    &controller,
                    WidgetAction::VolumeScroll {
                        id: beca_widgets::WidgetId::VolumeIn,
                        up: delta > 0.0,
                    },
                );
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("volume-out-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(
                    &app,
                    &controller,
                    WidgetAction::VolumeMute {
                        id: beca_widgets::WidgetId::VolumeOut,
                    },
                );
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("volume-in-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(
                    &app,
                    &controller,
                    WidgetAction::VolumeMute {
                        id: beca_widgets::WidgetId::VolumeIn,
                    },
                );
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("network-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(&app, &controller, WidgetAction::Network);
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let controller = controller.clone();
        let _ = app.set_callback("bluetooth-clicked", move |_args| {
            if let Some(app) = weak.upgrade() {
                apply_action(&app, &controller, WidgetAction::Bluetooth);
            }
            Value::Void
        });
    }
}

fn scroll_delta(args: &[Value]) -> f32 {
    args.first()
        .and_then(|value| match value {
            Value::Number(number) => Some(*number as f32),
            _ => None,
        })
        .unwrap_or(0.0)
}

pub fn show(position: &str) -> Result<()> {
    let resolved = beca_core::config::load();
    let bar_cfg = resolved.config.bar_config_for(position);
    let widgets = &resolved.config.widget;
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
            match beca_layer_shell::run_bar(pos, &bar_cfg, widgets, theme) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    eprintln!(
                        "[beca-bar] layer-shell unavailable, falling back to window mode: {e}"
                    );

                    if (e.contains("dispatch:")
                        || e.contains("roundtrip:")
                        || e.contains("flush:")
                        || e.contains("Backend error")
                        || e.contains("Connection reset"))
                        && let Ok(exe) = std::env::current_exe()
                    {
                        let status = Command::new(exe)
                            .env("BECA_NO_DAEMON", "1")
                            .env("BECA_BAR_FORCE_WINDOW", "1")
                            .args(["mod", "--local", &format!("bar:show-{}", position)])
                            .status();
                        match status {
                            Ok(s) if s.success() => return Ok(()),
                            Ok(s) => {
                                anyhow::bail!("bar window fallback process exited with status {s}")
                            }
                            Err(err) => {
                                anyhow::bail!("failed to spawn bar window fallback: {err}")
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
    let layout = WidgetLayout::from_lookup(|name| props.section(name));
    let controller = WidgetController::new(layout, widgets.clock.format.clone());

    {
        let sink = InterpreterSink { app: &app };
        controller.apply_layout(&sink);
        controller.sample_all(&sink);
    }

    let _ = app.set_property("position", Value::String(position.into()));
    let _ = app.set_property(
        "launcher-label",
        Value::String(widgets.launcher.label.clone().into()),
    );
    let _ = app.set_property(
        "begin-align",
        Value::String(bar_cfg.begin_align.clone().into()),
    );
    let _ = app.set_property(
        "center-align",
        Value::String(bar_cfg.center_align.clone().into()),
    );
    let _ = app.set_property("end-align", Value::String(bar_cfg.end_align.clone().into()));
    let _ = app.set_property(
        "background-opacity",
        Value::Number(bar_cfg.background_opacity as f64),
    );
    let _ = app.set_property("show-border", Value::Bool(bar_cfg.show_border));
    if let Some(color) = bar_cfg
        .background
        .as_ref()
        .and_then(|s| beca_ui::theme::parse_color(s))
    {
        let _ = app.set_property("bar-background", Value::Brush(color.into()));
    }

    bind_callbacks(&app, controller.clone());

    let running = Arc::new(AtomicBool::new(true));
    for (interval, group) in controller.timer_specs() {
        let running_flag = running.clone();
        let controller = controller.clone();
        let weak = app.as_weak();
        thread::spawn(move || {
            while running_flag.load(Ordering::Relaxed) {
                if let Some(update) = controller.sample(group) {
                    let weak2 = weak.clone();
                    let _ = ui_loader::invoke_on_event_loop(move || {
                        if let Some(app) = weak2.upgrade() {
                            let sink = InterpreterSink { app: &app };
                            update.apply(&sink);
                        }
                    });
                }
                thread::sleep(interval);
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
