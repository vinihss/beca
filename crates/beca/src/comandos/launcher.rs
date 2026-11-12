use anyhow::{Context, Result};
use mlua::{Function, Lua, Table};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{ComponentHandle, Global, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
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

pub fn send_control(cmd: &str) -> Result<()> {
    let path = control_socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("launcher control socket unavailable: {}", path.display()))?;
    writeln!(stream, "{cmd}")?;
    stream.flush()?;
    Ok(())
}

pub fn show() -> Result<()> {

    if send_control("show").is_ok() {
        return Ok(());
    }

    run_launcher(false)
}

pub fn serve() -> Result<()> {
    run_launcher(true)
}

fn run_launcher(resident: bool) -> Result<()> {

    let queue: Arc<Mutex<VecDeque<Rc<MinimalSoftwareWindow>>>> = Arc::new(Mutex::new(VecDeque::new()));
    beca_layer_shell::install_platform(queue.clone());
    let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    queue.lock().unwrap().push_back(msw.clone());

    let window = SearchWindow::new().context("could not create SearchWindow")?;
    apply_beca_theme(&window);
    window.hide().ok();
    let quit = Arc::new(AtomicBool::new(false));

    let lua = Lua::new();
    let beca = beca_core::lua::bridge::install_beca_base(&lua).map_err(lua_err)?;
    let search_slot: Rc<RefCell<Option<Table>>> = Rc::new(RefCell::new(None));

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

                    if let Some(search) = slot_show.borrow().clone() {
                        if let Ok(cb) = search.get::<Function>("on_search") {
                            let _ = cb.call::<()>(String::new());
                        }
                    }
                    if let Some(w) = weak_show.upgrade() {
                        let _ = w.show();
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

            if let Some(w) = weak_hide.upgrade() {
                let _ = w.hide();
            }
        });
    }

    beca_core::lua::runtime::exec_module_script(&lua, "launcher", EMBEDDED_LAUNCHER)
        .context("launcher script failed")?;

    {
        thread::spawn(move || {
            let initial = beca_apps::search("");
            for app in initial.iter().take(40) {
                let _ = beca_apps::desktop_entry::resolve_icon(&app.icon);
            }
        });
    }

    if resident {

        window
            .window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        start_control_listener(&window, search_slot.clone(), quit.clone())?;

        let _ = window.hide();
    } else {

        let weak_quit = window.as_weak();
        let quit_close = quit.clone();
        window.window().on_close_requested(move || {
            if let Some(w) = weak_quit.upgrade() {
                let _ = w.hide();
            }
            quit_close.store(true, Ordering::Relaxed);
            slint::CloseRequestResponse::HideWindow
        });
        let _ = window.show();
    }

    beca_layer_shell::run_launcher(msw, quit).map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

fn start_control_listener(
    window: &SearchWindow,
    slot: Rc<RefCell<Option<Table>>>,
    quit: Arc<AtomicBool>,
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

                    if let Some(search) = slot.borrow().clone() {
                        if let Ok(cb) = search.get::<Function>("on_search") {
                            let _ = cb.call::<()>(String::new());
                        }
                    }
                    if let Some(w) = weak.upgrade() {
                        let _ = w.show();
                    }
                }
                "hide" => {
                    if let Some(w) = weak.upgrade() {
                        let _ = w.hide();
                    }
                }
                "quit" | "stop" => {
                    if let Some(w) = weak.upgrade() {
                        let _ = w.hide();
                    }
                    quit.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    });

    std::mem::forget(timer);

    Ok(())
}
