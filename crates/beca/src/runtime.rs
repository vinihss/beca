use anyhow::Result;
use mlua::{Function, Lua, Table, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Load a module script from XDG (`$XDG_CONFIG_HOME/beca/scripts/<name>.lua`)
/// falling back to the embedded default.
pub fn load_module_script(name: &str, embedded: &str) -> String {
    let paths = beca_core::BecaPaths::resolve();
    let filename = if name.ends_with(".lua") || name.ends_with(".luau") {
        name.to_string()
    } else {
        format!("{name}.lua")
    };
    let disk = paths.scripts_dir.join(&filename);
    match fs::read_to_string(&disk) {
        Ok(s) => {
            eprintln!("[beca] loaded script: {}", disk.display());
            s
        }
        Err(_) => embedded.to_string(),
    }
}

fn json_to_lua(lua: &Lua, v: &serde_json::Value) -> mlua::Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Number(f)
            } else {
                Value::Nil
            }
        }
        serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
        serde_json::Value::Array(arr) => {
            let t = lua.create_table()?;
            for (i, item) in arr.iter().enumerate() {
                t.set(i + 1, json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, val) in map {
                t.set(k.as_str(), json_to_lua(lua, val)?)?;
            }
            Value::Table(t)
        }
    })
}

fn install_system(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let system: Table = beca.get("system")?;

    system.set(
        "notify",
        lua.create_function(|_, (title, body): (Option<String>, Option<String>)| {
            let title = title.unwrap_or_else(|| "beca".to_string());
            let body = body.unwrap_or_default();
            let _ = Command::new("notify-send").args([&title, &body]).status();
            Ok(())
        })?,
    )?;

    system.set(
        "clipboard_copy",
        lua.create_function(|_, text: String| {
            // try wl-copy, xclip, xsel
            for (cmd, args) in [
                ("wl-copy", vec![]),
                ("xclip", vec!["-selection", "clipboard"]),
                ("xsel", vec!["--clipboard", "--input"]),
            ] {
                if let Ok(mut child) = Command::new(cmd)
                    .args(&args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    if child.wait().is_ok_and(|s| s.success()) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })?,
    )?;

    system.set(
        "exec",
        lua.create_function(|lua, (cmd, args): (String, Option<Table>)| {
            let mut command = Command::new(&cmd);
            if let Some(args) = args {
                for v in args.sequence_values::<String>() {
                    if let Ok(s) = v {
                        command.arg(s);
                    }
                }
            }
            match command.output() {
                Ok(out) => {
                    let t = lua.create_table()?;
                    t.set("ok", out.status.success())?;
                    t.set("code", out.status.code().unwrap_or(-1))?;
                    t.set("stdout", String::from_utf8_lossy(&out.stdout).to_string())?;
                    t.set("stderr", String::from_utf8_lossy(&out.stderr).to_string())?;
                    Ok(Value::Table(t))
                }
                Err(e) => {
                    let t = lua.create_table()?;
                    t.set("ok", false)?;
                    t.set("code", -1)?;
                    t.set("stdout", "")?;
                    t.set("stderr", e.to_string())?;
                    Ok(Value::Table(t))
                }
            }
        })?,
    )?;

    system.set(
        "env",
        lua.create_function(|_, key: String| Ok(std::env::var(&key).ok()))?,
    )?;

    system.set(
        "paths",
        lua.create_function(|lua, ()| {
            let p = beca_core::BecaPaths::resolve();
            let t = lua.create_table()?;
            t.set("config_dir", p.config_dir.to_string_lossy().to_string())?;
            t.set("config_file", p.config_file.to_string_lossy().to_string())?;
            t.set("themes_dir", p.themes_dir.to_string_lossy().to_string())?;
            t.set("scripts_dir", p.scripts_dir.to_string_lossy().to_string())?;
            Ok(t)
        })?,
    )?;

    system.set(
        "log",
        lua.create_function(|_, (level, msg): (String, String)| {
            eprintln!("[beca:{}] {}", level, msg);
            Ok(())
        })?,
    )?;

    Ok(())
}

fn install_voice(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let voice: Table = beca.get("voice")?;

    voice.set(
        "transcribe",
        lua.create_function(|lua, (path, url): (String, Option<String>)| {
            let base = url.unwrap_or_else(|| {
                let resolved = beca_core::load();
                resolved.config.modules.voice.daemon_url
            });
            match voice_capture::transcribe(&base, Path::new(&path)) {
                Ok(text) => Ok(Value::String(lua.create_string(&text)?)),
                Err(e) => Err(mlua::Error::RuntimeError(e)),
            }
        })?,
    )?;

    voice.set(
        "config",
        lua.create_function(|lua, ()| {
            let resolved = beca_core::load();
            let v = &resolved.config.modules.voice;
            let t = lua.create_table()?;
            t.set("enabled", v.enabled)?;
            t.set("daemon_url", v.daemon_url.clone())?;
            t.set(
                "mode",
                match v.mode {
                    beca_core::VoiceMode::Pipe => "pipe",
                    beca_core::VoiceMode::Hold => "hold",
                },
            )?;
            t.set("prefer_sample_rate", v.prefer_sample_rate)?;
            t.set("channels", v.channels)?;
            let ui = lua.create_table()?;
            ui.set("title", v.ui.title.clone())?;
            ui.set("status_recording", v.ui.status_recording.clone())?;
            ui.set("status_transcribing", v.ui.status_transcribing.clone())?;
            t.set("ui", ui)?;
            Ok(t)
        })?,
    )?;

    Ok(())
}

fn install_events(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let events: Table = beca.get("events")?;
    let handlers = lua.create_table()?;
    let next_id = lua.create_table()?;
    next_id.set("v", 1i64)?;
    events.set("_handlers", handlers)?;
    events.set("_next_id", next_id)?;

    events.set(
        "on",
        lua.create_function(|lua, (name, func): (String, Function)| {
            let events: Table = lua.globals().get::<Table>("beca")?.get("events")?;
            let handlers: Table = events.get("_handlers")?;
            let next_id: Table = events.get("_next_id")?;
            let id: i64 = next_id.get("v")?;
            next_id.set("v", id + 1)?;
            let list: Table = match handlers.get::<Table>(name.as_str()) {
                Ok(t) => t,
                Err(_) => {
                    let t = lua.create_table()?;
                    handlers.set(name.as_str(), t.clone())?;
                    t
                }
            };
            let entry = lua.create_table()?;
            entry.set("id", id)?;
            entry.set("fn", func)?;
            let n = list.len()? + 1;
            list.set(n, entry)?;
            Ok(id)
        })?,
    )?;

    events.set(
        "off",
        lua.create_function(|lua, (name, id): (String, i64)| {
            let events: Table = lua.globals().get::<Table>("beca")?.get("events")?;
            let handlers: Table = events.get("_handlers")?;
            if let Ok(list) = handlers.get::<Table>(name.as_str()) {
                let mut kept = Vec::new();
                for pair in list.sequence_values::<Table>() {
                    if let Ok(entry) = pair {
                        let eid: i64 = entry.get("id").unwrap_or(-1);
                        if eid != id {
                            kept.push(entry);
                        }
                    }
                }
                let new_list = lua.create_table()?;
                for (i, e) in kept.into_iter().enumerate() {
                    new_list.set(i + 1, e)?;
                }
                handlers.set(name.as_str(), new_list)?;
            }
            Ok(())
        })?,
    )?;

    events.set(
        "emit",
        lua.create_function(|lua, (name, payload): (String, Option<Value>)| {
            let events: Table = lua.globals().get::<Table>("beca")?.get("events")?;
            let handlers: Table = events.get("_handlers")?;
            if let Ok(list) = handlers.get::<Table>(name.as_str()) {
                for pair in list.sequence_values::<Table>() {
                    if let Ok(entry) = pair {
                        if let Ok(func) = entry.get::<Function>("fn") {
                            let p = payload.clone().unwrap_or(Value::Nil);
                            let _ = func.call::<()>(p);
                        }
                    }
                }
            }
            Ok(())
        })?,
    )?;

    Ok(())
}

use mlua::UserData;
use mlua::UserDataMethods;
use slint::ComponentHandle;

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    for (cmd, args) in [
        ("wl-copy", vec![] as Vec<&str>),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }
    }
    false
}

fn notify_msg(msg: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["beca", msg])
        .status();
}

fn parse_hex_color(hex: &str) -> Option<slint::Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(slint::Color::from_rgb_u8(r, g, b))
}

fn apply_theme_voice(app: &ui_lib::VoiceCaptureWindow) {
    use slint::Global;
    let resolved = beca_core::load();
    let t = &resolved.theme;
    let g = ui_lib::Theme::get(app);
    let to_brush = |s: &str| -> slint::Brush {
        parse_hex_color(s)
            .map(slint::Brush::SolidColor)
            .unwrap_or_else(|| slint::Brush::SolidColor(slint::Color::from_rgb_u8(0, 0, 0)))
    };
    let to_color = |s: &str| -> slint::Color {
        parse_hex_color(s).unwrap_or_else(|| slint::Color::from_rgb_u8(0, 0, 0))
    };
    g.set_background(to_color(&t.colors.background));
    g.set_text_main(to_color(&t.colors.text_main));
    g.set_card_bg(to_color(&t.colors.card_bg));
    g.set_card_border(to_color(&t.colors.card_border));
    g.set_surface(to_brush(&t.colors.surface));
    g.set_surface_alt(to_brush(&t.colors.surface_alt));
    g.set_border(to_brush(&t.colors.border));
    g.set_text_strong(to_brush(&t.colors.text_strong));
    g.set_accent(to_brush(&t.colors.accent));
    g.set_accent_success(to_brush(&t.colors.accent_success));
    g.set_accent_danger(to_brush(&t.colors.accent_danger));
    g.set_recording(to_brush(&t.colors.recording));
    g.set_radius(t.metrics.radius as f32);
    g.set_dark_mode(t.meta.variant != "light");
}

enum AppHandle {
    VoiceCapture(ui_lib::VoiceCaptureWindow),
}

impl AppHandle {
    fn set_prop(&self, key: &str, value: &str) {
        let AppHandle::VoiceCapture(app) = self;
        match key {
            "status-text" => app.set_status_text(value.into()),
            "transcript-text" => app.set_transcript_text(value.into()),
            "audio-path-data" => app.set_audio_path_data(value.into()),
            _ => {}
        }
    }
    fn get_prop(&self, key: &str) -> String {
        let AppHandle::VoiceCapture(app) = self;
        match key {
            "transcript-text" => app.get_transcript_text().to_string(),
            "status-text" => app.get_status_text().to_string(),
            _ => String::new(),
        }
    }
    fn set_bool(&self, key: &str, val: bool) {
        let AppHandle::VoiceCapture(app) = self;
        match key {
            "busy" => app.set_busy(val),
            "show-result" => app.set_show_result(val),
            _ => {}
        }
    }
}

struct AppUserData(AppHandle);

impl UserData for AppUserData {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set", |_, this, (key, value): (String, String)| {
            this.0.set_prop(&key, &value);
            Ok(())
        });
        methods.add_method("set_bool", |_, this, (key, value): (String, bool)| {
            this.0.set_bool(&key, value);
            Ok(())
        });
        methods.add_method("get", |_, this, key: String| -> mlua::Result<String> {
            Ok(this.0.get_prop(&key))
        });
        methods.add_method("on", |_, this, (event, func): (String, Function)| {
            let AppHandle::VoiceCapture(app) = &this.0;
            match event.as_str() {
                "send" => {
                    let f = func;
                    app.on_send(move || {
                        let _ = f.call::<()>(());
                    });
                }
                "cancel" => {
                    let f = func;
                    app.on_cancel(move || {
                        let _ = f.call::<()>(());
                    });
                }
                "copy-text" => {
                    let f = func;
                    app.on_copy_text(move || {
                        let _ = f.call::<()>(());
                    });
                }
                "close-app" => {
                    let f = func;
                    app.on_close_app(move || {
                        let _ = f.call::<()>(());
                    });
                }
                _ => {}
            }
            Ok(())
        });
        methods.add_method(
            "call",
            |lua,
             this,
             (action, arg1, arg2): (String, Option<String>, Option<String>)|
             -> mlua::Result<Value> {
                match action.as_str() {
                    "app.close" => {
                        let _ = slint::quit_event_loop();
                        Ok(Value::Nil)
                    }
                    "clipboard.copy" => {
                        let text = arg1.unwrap_or_else(|| this.0.get_prop("transcript-text"));
                        Ok(Value::Boolean(copy_to_clipboard(&text)))
                    }
                    "transcribe.send" => {
                        let path = arg1.unwrap_or_default();
                        let url = arg2
                            .unwrap_or_else(|| beca_core::load().config.modules.voice.daemon_url);
                        match voice_capture::transcribe(&url, std::path::Path::new(&path)) {
                            Ok(t) => Ok(Value::String(lua.create_string(&t)?)),
                            Err(e) => Err(mlua::Error::RuntimeError(e)),
                        }
                    }
                    "ui.notify" => {
                        notify_msg(&arg1.unwrap_or_default());
                        Ok(Value::Nil)
                    }
                    "ui.set" => {
                        let key = arg1.unwrap_or_default();
                        let val = arg2.unwrap_or_default();
                        if key == "busy" || key == "show-result" {
                            this.0.set_bool(&key, val == "true" || val == "1");
                        } else {
                            this.0.set_prop(&key, &val);
                        }
                        Ok(Value::Nil)
                    }
                    _ => Err(mlua::Error::RuntimeError(format!(
                        "unknown action: {action}"
                    ))),
                }
            },
        );
        methods.add_method("run", |_, this, ()| {
            let AppHandle::VoiceCapture(app) = &this.0;
            let _ = app.run();
            Ok(())
        });
        methods.add_method("show", |_, this, ()| {
            let AppHandle::VoiceCapture(app) = &this.0;
            let _ = app.show();
            Ok(())
        });
        methods.add_method("hide", |_, this, ()| {
            let AppHandle::VoiceCapture(app) = &this.0;
            let _ = app.hide();
            Ok(())
        });
    }
}

fn hyprland_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let base = runtime.join("hypr").join(sig);
    Some((base.join(".socket.sock"), base.join(".socket2.sock")))
}

fn install_hyprland(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let hypr: Table = beca.get("hyprland")?;

    hypr.set(
        "available",
        lua.create_function(|_, ()| Ok(hyprland_dirs().is_some()))?,
    )?;

    hypr.set(
        "socket_paths",
        lua.create_function(|lua, ()| {
            if let Some((cmd, ev)) = hyprland_dirs() {
                let t = lua.create_table()?;
                t.set("socket", cmd.to_string_lossy().to_string())?;
                t.set("socket2", ev.to_string_lossy().to_string())?;
                Ok(Value::Table(t))
            } else {
                Ok(Value::Nil)
            }
        })?,
    )?;

    hypr.set(
        "dispatch",
        lua.create_function(|lua, command: String| {
            let (sock, _) = hyprland_dirs()
                .ok_or_else(|| mlua::Error::RuntimeError("hyprland not available".into()))?;
            use std::io::{Read, Write};
            use std::os::unix::net::UnixStream;
            let mut stream = UnixStream::connect(&sock).map_err(|e| {
                mlua::Error::RuntimeError(format!("connect {}: {e}", sock.display()))
            })?;
            stream
                .write_all(command.as_bytes())
                .map_err(|e| mlua::Error::RuntimeError(format!("write: {e}")))?;
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
            let mut buf = String::new();
            let _ = stream.read_to_string(&mut buf);
            let resp = buf.trim_end().to_string();
            let lower = resp.to_lowercase();
            let ok = resp.is_empty()
                || resp.eq_ignore_ascii_case("ok")
                || lower.starts_with("ok")
                || resp.starts_with('{')
                || resp.starts_with('[');
            let t = lua.create_table()?;
            t.set("ok", ok)?;
            t.set("response", resp)?;
            Ok(t)
        })?,
    )?;

    // convenience alias
    hypr.set(
        "exec",
        lua.create_function(|lua, command: String| {
            let f: mlua::Function = lua
                .globals()
                .get::<Table>("beca")?
                .get::<Table>("hyprland")?
                .get("dispatch")?;
            f.call::<Table>(command)
        })?,
    )?;

    // Read events from .socket2.sock for up to timeout_ms (default 200).
    // Returns array of { event = "...", data = "..." }.
    hypr.set(
        "read_events",
        lua.create_function(|lua, timeout_ms: Option<u64>| {
            let (_, sock2) = hyprland_dirs()
                .ok_or_else(|| mlua::Error::RuntimeError("hyprland not available".into()))?;
            use std::io::{BufRead, BufReader};
            use std::os::unix::net::UnixStream;
            let stream = UnixStream::connect(&sock2).map_err(|e| {
                mlua::Error::RuntimeError(format!("connect {}: {e}", sock2.display()))
            })?;
            let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(200));
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            let mut reader = BufReader::new(stream);
            let out = lua.create_table()?;
            let mut i = 1usize;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = line.trim_end_matches(['\n', '\r']).to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let (event, data) = if let Some((a, b)) = line.split_once(">>") {
                            (a.to_string(), b.to_string())
                        } else {
                            (line.clone(), String::new())
                        };
                        let t = lua.create_table()?;
                        t.set("event", event)?;
                        t.set("data", data)?;
                        t.set("raw", line)?;
                        out.set(i, t)?;
                        i += 1;
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        break;
                    }
                    Err(_) => break,
                }
            }
            Ok(out)
        })?,
    )?;

    Ok(())
}

fn install_ui(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let ui: Table = beca.get("ui")?;
    ui.set(
        "window",
        lua.create_function(|_, name: String| -> mlua::Result<AppUserData> {
            match name.as_str() {
                "VoiceCaptureWindow" => {
                    let app = ui_lib::VoiceCaptureWindow::new()
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    apply_theme_voice(&app);
                    Ok(AppUserData(AppHandle::VoiceCapture(app)))
                }
                _ => Err(mlua::Error::RuntimeError(format!("unknown window: {name}"))),
            }
        })?,
    )?;
    ui.set(
        "run",
        lua.create_function(|_, ()| {
            let _ = slint::run_event_loop();
            Ok(())
        })?,
    )?;
    ui.set(
        "quit",
        lua.create_function(|_, ()| {
            let _ = slint::quit_event_loop();
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_runtime(lua: &Lua, beca: &Table) -> mlua::Result<()> {
    let runtime = lua.create_table()?;

    runtime.set(
        "config",
        lua.create_function(|lua, ()| {
            let resolved = beca_core::load();
            let json = serde_json::to_value(&resolved.config).unwrap_or(serde_json::Value::Null);
            json_to_lua(lua, &json)
        })?,
    )?;

    runtime.set(
        "theme",
        lua.create_function(|lua, ()| {
            let resolved = beca_core::load();
            let json = serde_json::to_value(&resolved.theme).unwrap_or(serde_json::Value::Null);
            json_to_lua(lua, &json)
        })?,
    )?;

    runtime.set(
        "paths",
        lua.create_function(|lua, ()| {
            let p = beca_core::BecaPaths::resolve();
            let t = lua.create_table()?;
            t.set("config_dir", p.config_dir.to_string_lossy().to_string())?;
            t.set("config_file", p.config_file.to_string_lossy().to_string())?;
            t.set("themes_dir", p.themes_dir.to_string_lossy().to_string())?;
            t.set("scripts_dir", p.scripts_dir.to_string_lossy().to_string())?;
            Ok(t)
        })?,
    )?;

    runtime.set(
        "reload",
        lua.create_function(|lua, ()| {
            let beca: Table = lua.globals().get("beca")?;
            let runtime: Table = beca.get("runtime")?;
            let name: String = match runtime.get("_module") {
                Ok(s) => s,
                Err(_) => {
                    return Err(mlua::Error::RuntimeError(
                        "reload: no module context".into(),
                    ));
                }
            };
            let embedded: String = runtime.get("_embedded").unwrap_or_default();
            let code = load_module_script(&name, &embedded);
            // call on_unload hook if present
            if let Ok(hooks) = beca.get::<Table>("hooks") {
                if let Ok(f) = hooks.get::<Function>("on_unload") {
                    let _ = f.call::<()>(());
                }
            } else if let Ok(f) = beca.get::<Function>("on_unload") {
                let _ = f.call::<()>(());
            }
            lua.load(&code)
                .set_name(&name)
                .exec()
                .map_err(|e| mlua::Error::RuntimeError(format!("reload failed: {e}")))?;
            call_hook(lua, "on_load");
            Ok(())
        })?,
    )?;

    beca.set("runtime", runtime)?;
    Ok(())
}

/// Install base `beca` table with domain namespaces and core capabilities.
/// Domains: apps, ui, voice, events, system, hyprland.
pub fn install_beca_base(lua: &Lua) -> mlua::Result<mlua::Table> {
    let beca = lua.create_table()?;

    // Domain tables (extensible)
    let ui = lua.create_table()?;
    let apps = lua.create_table()?;
    let voice = lua.create_table()?;
    let events = lua.create_table()?;
    let system = lua.create_table()?;
    let hyprland = lua.create_table()?;
    let hooks = lua.create_table()?;

    beca.set("ui", ui)?;
    beca.set("apps", apps)?;
    beca.set("voice", voice)?;
    beca.set("events", events)?;
    beca.set("system", system)?;
    beca.set("hyprland", hyprland)?;
    beca.set("hooks", hooks)?;

    lua.globals().set("beca", beca.clone())?;

    install_system(lua, &beca)?;
    install_voice(lua, &beca)?;
    install_events(lua, &beca)?;
    install_runtime(lua, &beca)?;
    install_hyprland(lua, &beca)?;
    install_ui(lua, &beca)?;

    Ok(beca)
}

/// Call a lifecycle hook if defined: checks `beca.hooks.<name>` then `beca.<name>`.
pub fn call_hook(lua: &Lua, name: &str) {
    let Ok(beca) = lua.globals().get::<Table>("beca") else {
        return;
    };
    if let Ok(hooks) = beca.get::<Table>("hooks") {
        if let Ok(f) = hooks.get::<Function>(name) {
            if let Err(e) = f.call::<()>(()) {
                eprintln!("[beca] hook '{name}' failed: {e}");
            }
            return;
        }
    }
    if let Ok(f) = beca.get::<Function>(name) {
        if let Err(e) = f.call::<()>(()) {
            eprintln!("[beca] hook '{name}' failed: {e}");
        }
    }
}

/// Execute a named script with embedded fallback.
/// Sets module context for reload, then runs on_load hook.
pub fn exec_module_script(lua: &Lua, name: &str, embedded: &str) -> Result<()> {
    // store context for reload
    if let Ok(beca) = lua.globals().get::<Table>("beca") {
        if let Ok(runtime) = beca.get::<Table>("runtime") {
            let _ = runtime.set("_module", name);
            let _ = runtime.set("_embedded", embedded);
        }
    }
    let code = load_module_script(name, embedded);
    lua.load(&code)
        .set_name(name)
        .exec()
        .map_err(|e| anyhow::anyhow!("failed to execute script '{name}': {e}"))?;
    call_hook(lua, "on_load");
    Ok(())
}
