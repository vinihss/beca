use anyhow::{Context, Result};
use mlua::{Function, Lua, Table};
use slint::{ComponentHandle, Global, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    thread,
    time::Duration,
};

use beca_ui::{AppEntry, SearchWindow};

const EMBEDDED_LAUNCHER: &str = include_str!("../../../../scripts/launcher.lua");

fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!(e.to_string())
}

fn apply_beca_theme(app: &SearchWindow) {
    let resolved = beca_core::config::load();
    let tokens = beca_ui::theme::ThemeTokens::from_voice_theme(&resolved.theme.voice_theme());
    tokens.apply_typed(&beca_ui::Theme::get(app));
}

pub fn control_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("launcher.ctl")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("launcher.ctl")
    }
}

/// Send a control command to a resident launcher, if running.
pub fn send_control(cmd: &str) -> Result<()> {
    let path = control_socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("launcher control socket unavailable: {}", path.display()))?;
    writeln!(stream, "{cmd}")?;
    stream.flush()?;
    Ok(())
}

fn hypr_dispatch_socket(cmd: &str) -> Option<String> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let path = runtime.join("hypr").join(sig).join(".socket.sock");
    let mut stream = UnixStream::connect(path).ok()?;
    stream.write_all(cmd.as_bytes()).ok()?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(400)));
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut stream, &mut buf);
    Some(buf.trim_end().to_string())
}

/// Best-effort: make launcher window floating and centered (Hyprland).
fn float_launcher_window() {
    thread::spawn(|| {
        let pid = std::process::id();
        // Retry to handle remap/timing (resident show after hide).
        for attempt in 0..10 {
            thread::sleep(std::time::Duration::from_millis(if attempt == 0 { 100 } else { 80 }));
            let clients_out = std::process::Command::new("hyprctl")
                .args(["clients", "-j"])
                .output()
                .ok();
            let monitors_out = std::process::Command::new("hyprctl")
                .args(["monitors", "-j"])
                .output()
                .ok();
            let (Some(clients_out), Some(monitors_out)) = (clients_out, monitors_out) else {
                continue;
            };
            if !clients_out.status.success() || !monitors_out.status.success() {
                continue;
            }
            let clients: Vec<serde_json::Value> =
                serde_json::from_slice(&clients_out.stdout).unwrap_or_default();
            let monitors: Vec<serde_json::Value> =
                serde_json::from_slice(&monitors_out.stdout).unwrap_or_default();
            let client = clients
                .iter()
                .find(|c| c.get("pid").and_then(|v| v.as_u64()) == Some(pid as u64))
                .or_else(|| {
                    clients
                        .iter()
                        .find(|c| c.get("title").and_then(|v| v.as_str()) == Some("beca"))
                });
            let Some(client) = client else { continue };
            let Some(addr) = client.get("address").and_then(|v| v.as_str()) else {
                continue;
            };
            let mon_id = client.get("monitor").and_then(|v| v.as_i64()).unwrap_or(0);
            let monitor = monitors
                .iter()
                .find(|m| m.get("id").and_then(|v| v.as_i64()) == Some(mon_id))
                .or_else(|| {
                    monitors
                        .iter()
                        .find(|m| m.get("focused").and_then(|v| v.as_bool()) == Some(true))
                })
                .or_else(|| monitors.first());
            let Some(monitor) = monitor else { continue };
            let mx = monitor.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
            let my = monitor.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
            let mw = monitor.get("width").and_then(|v| v.as_i64()).unwrap_or(1280);
            let mh = monitor.get("height").and_then(|v| v.as_i64()).unwrap_or(720);
            let ww = 640i64;
            let wh = 460i64;
            let x = mx + (mw - ww) / 2;
            let y = my + (mh - wh) / 3; // slightly above center
            let is_floating = client
                .get("floating")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let focus = format!("dispatch hl.dsp.focus({{window = 'address:{addr}'}})");
            let _ = hypr_dispatch_socket(&focus);
            if !is_floating {
                let _ = hypr_dispatch_socket("dispatch hl.dsp.window.float()");
            }
            let _ = hypr_dispatch_socket(&format!(
                "dispatch hl.dsp.window.resize({{x={ww},y={wh}}})"
            ));
            let _ = hypr_dispatch_socket(&format!(
                "dispatch hl.dsp.window.move({{x={x},y={y}}})"
            ));
            // keep focus on launcher for immediate typing
            let _ = hypr_dispatch_socket(&focus);
            // stop retrying once floating and roughly correct
            if is_floating {
                break;
            }
        }
    });
}

/// Default `launcher:show` action: ask resident launcher to show if available.
pub fn show() -> Result<()> {
    // If resident launcher is running, just ask it to show.
    if send_control("show").is_ok() {
        return Ok(());
    }
    // Fallback: one-shot launcher (non-resident)
    run_launcher(false)
}

/// Resident launcher service: keeps window/process alive and listens for show/hide/quit.
pub fn serve() -> Result<()> {
    run_launcher(true)
}

fn run_launcher(resident: bool) -> Result<()> {
    let window = SearchWindow::new().context("could not create SearchWindow")?;
    apply_beca_theme(&window);
    window.hide().ok();

    let lua = Lua::new();
    let beca = beca_core::lua::bridge::install_beca_base(&lua).map_err(lua_err)?;
    let search_slot: Rc<RefCell<Option<Table>>> = Rc::new(RefCell::new(None));

    // beca.apps
    let apps: Table = beca.get("apps").map_err(lua_err)?;
    apps.set(
        "search",
        lua.create_function(|lua, query: String| {
            let results = beca_apps::search(&query);
            let t = lua.create_table()?;
            for (i, app) in results.iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("name", app.name.clone())?;
                let icon = beca_apps::desktop_entry::resolve_icon(&app.icon).unwrap_or_default();
                entry.set("icon", icon)?;
                t.set(i + 1, entry)?;
            }
            Ok(t)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;
    apps.set(
        "launch",
        lua.create_function(|_, name: String| {
            beca_apps::launch(&name)
                .map(|_| ())
                .map_err(mlua::Error::external)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    // beca.ui.search_window(opts)
    let ui: Table = beca.get("ui").map_err(lua_err)?;
    let weak = window.as_weak();
    let slot_for_create = search_slot.clone();
    ui.set(
        "search_window",
        lua.create_function(move |lua, options: Option<Table>| {
            if let Some(options) = options {
                if let Ok(placeholder) = options.get::<String>("placeholder") {
                    if let Some(w) = weak.upgrade() {
                        w.set_placeholder(placeholder.into());
                    }
                }
                let _ = options.get::<String>("title");
            }
            let search = lua.create_table()?;
            let weak_results = weak.clone();
            let image_cache: Rc<RefCell<HashMap<String, slint::Image>>> =
                Rc::new(RefCell::new(HashMap::new()));
            search.set(
                "set_results",
                lua.create_function(move |_, args: mlua::MultiValue| {
                    // Support both method call `search:set_results(items)` and function call `search.set_results(items)`
                    let items_val = if args.len() >= 2 {
                        args.into_iter().nth(1).unwrap_or(mlua::Value::Nil)
                    } else {
                        args.into_iter().next().unwrap_or(mlua::Value::Nil)
                    };
                    if let mlua::Value::Table(items) = items_val {
                        let mut results: Vec<AppEntry> = Vec::new();
                        let len = items.len().unwrap_or(0);
                        for i in 1..=len {
                            match items.get::<mlua::Value>(i) {
                                Ok(mlua::Value::Table(t)) => {
                                    let name = t.get::<String>("name").unwrap_or_default();
                                    let icon_path = t.get::<String>("icon").unwrap_or_default();
                                    let icon = if !icon_path.is_empty() {
                                        if let Some(cached) = image_cache.borrow().get(&icon_path) {
                                            cached.clone()
                                        } else if Path::new(&icon_path).exists() {
                                            let img = slint::Image::load_from_path(Path::new(
                                                &icon_path,
                                            ))
                                            .unwrap_or_default();
                                            image_cache
                                                .borrow_mut()
                                                .insert(icon_path.clone(), img.clone());
                                            img
                                        } else {
                                            slint::Image::default()
                                        }
                                    } else {
                                        slint::Image::default()
                                    };
                                    results.push(AppEntry {
                                        name: name.into(),
                                        icon,
                                    });
                                }
                                Ok(mlua::Value::String(s)) => {
                                    if let Ok(s) = s.to_str() {
                                        results.push(AppEntry {
                                            name: s.to_string().into(),
                                            icon: slint::Image::default(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Some(w) = weak_results.upgrade() {
                            w.set_results(ModelRc::new(VecModel::from(results)));
                        }
                    }
                    Ok(())
                })?,
            )?;
            let weak_show = weak.clone();
            let slot_show = slot_for_create.clone();
            search.set(
                "show",
                lua.create_function(move |_, ()| {
                    // In resident mode, showing means reset search to full list and display.
                    if let Some(search) = slot_show.borrow().clone() {
                        if let Ok(cb) = search.get::<Function>("on_search") {
                            let _ = cb.call::<()>(String::new());
                        }
                    }
                    if let Some(w) = weak_show.upgrade() {
                        let _ = w.show();
                        float_launcher_window();
                    }
                    Ok(())
                })?,
            )?;
            let weak_hide = weak.clone();
            search.set(
                "hide",
                lua.create_function(move |_, ()| {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                    Ok(())
                })?,
            )?;
            *slot_for_create.borrow_mut() = Some(search.clone());
            Ok(search)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    // connect Slint events -> Lua callbacks
    {
        let slot = search_slot.clone();
        window.on_search_requested(move |query| {
            if let Some(search) = slot.borrow().clone() {
                if let Ok(cb) = search.get::<Function>("on_search") {
                    if let Err(e) = cb.call::<()>(query.to_string()) {
                        eprintln!("Lua on_search failed: {e}");
                    }
                }
            }
        });
    }
    {
        let slot = search_slot.clone();
        let weak_hide = window.as_weak();
        window.on_result_selected(move |item| {
            if let Some(search) = slot.borrow().clone() {
                if let Ok(cb) = search.get::<Function>("on_select") {
                    if let Err(e) = cb.call::<()>(item.to_string()) {
                        eprintln!("Lua on_select failed: {e}");
                    }
                }
            }
            // Hide after selection in resident mode behavior (feels snappy); harmless in one-shot.
            if let Some(w) = weak_hide.upgrade() {
                let _ = w.hide();
            }
        });
    }

    beca_core::lua::runtime::exec_module_script(&lua, "launcher", EMBEDDED_LAUNCHER)
        .context("launcher script failed")?;

    // Warm caches in background so first show is instant.
    // In resident mode the script already populated initial results via search(""),
    // but we also eagerly warm icon resolution for common entries.
    {
        thread::spawn(move || {
            let initial = beca_apps::search("");
            for app in initial.iter().take(40) {
                let _ = beca_apps::desktop_entry::resolve_icon(&app.icon);
            }
        });
    }

    if resident {
        // Don't quit when window is closed/hidden; keep serving.
        window
            .window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        start_control_listener(&window, search_slot.clone())?;
        // In resident mode, start hidden (daemon/user will request show).
        let _ = window.hide();
    } else {
        // One-shot: show immediately (script already called show, but ensure visible).
        let _ = window.show();
        float_launcher_window();
    }

    if resident {
        slint::run_event_loop_until_quit().context("Slint event loop failed")?;
    } else {
        slint::run_event_loop().context("Slint event loop failed")?;
    }
    Ok(())
}

fn start_control_listener(
    window: &SearchWindow,
    slot: Rc<RefCell<Option<Table>>>,
) -> Result<()> {
    let path = control_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind launcher control socket {}", path.display()))?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let cmd = line.trim().to_string();
            if cmd.is_empty() {
                continue;
            }
            if tx.send(cmd.clone()).is_err() {
                break;
            }
            if cmd == "quit" || cmd == "stop" {
                break;
            }
        }
        let _ = std::fs::remove_file(control_socket_path());
    });

    let weak = window.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(60), move || {
        while let Ok(cmd) = rx.try_recv() {
            match cmd.as_str() {
                "show" => {
                    // Refresh to full list and show.
                    if let Some(search) = slot.borrow().clone() {
                        if let Ok(cb) = search.get::<Function>("on_search") {
                            let _ = cb.call::<()>(String::new());
                        }
                    }
                    if let Some(w) = weak.upgrade() {
                        let _ = w.show();
                        float_launcher_window();
                    }
                }
                "hide" => {
                    if let Some(w) = weak.upgrade() {
                        let _ = w.hide();
                    }
                }
                "quit" | "stop" => {
                    slint::quit_event_loop().ok();
                }
                _ => {}
            }
        }
    });
    // Keep timer alive for the lifetime of the event loop.
    std::mem::forget(timer);

    Ok(())
}
