use mlua::{Function, Lua, UserData, UserDataMethods, Value};
use slint::ComponentHandle;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static TEST_MODE: AtomicBool = AtomicBool::new(false);

fn copy_to_clipboard(text: &str) -> bool {
    for (cmd, args) in [
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if let Ok(mut child) = Command::new(cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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

fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["Voice Capture", msg])
        .status();
}

fn transcribe_file(base_url: &str, audio_path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(audio_path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let url = format!("{}/transcribe", base_url.trim_end_matches('/'));
    let resp = ureq::post(&url)
        .set("Content-Type", "audio/wav")
        .send_bytes(&buf)
        .map_err(|e| e.to_string())?;
    let body: String = resp.into_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    v.get("text")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("invalid response: {body}"))
}

enum AppHandle {
    VoiceCapture(ui_lib::VoiceCaptureWindow),
    Stub(String),
}

impl AppHandle {
    fn set_prop(&self, key: &str, value: &str) {
        if let AppHandle::VoiceCapture(app) = self {
            match key {
                "status-text" => app.set_status_text(value.into()),
                "transcript-text" => app.set_transcript_text(value.into()),
                "audio-path-data" => app.set_audio_path_data(value.into()),
                _ => {}
            }
        }
    }
    fn get_prop(&self, key: &str) -> String {
        if let AppHandle::VoiceCapture(app) = self {
            match key {
                "transcript-text" => app.get_transcript_text().to_string(),
                "status-text" => app.get_status_text().to_string(),
                _ => String::new(),
            }
        } else {
            String::new()
        }
    }
    fn set_bool(&self, key: &str, val: bool) {
        if let AppHandle::VoiceCapture(app) = self {
            match key {
                "busy" => app.set_busy(val),
                "show-result" => app.set_show_result(val),
                _ => {}
            }
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
            if let AppHandle::VoiceCapture(app) = &this.0 {
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
                        if !TEST_MODE.load(Ordering::Relaxed) {
                            let _ = slint::quit_event_loop();
                        }
                        Ok(Value::Nil)
                    }
                    "clipboard.copy" => {
                        let text = arg1.unwrap_or_else(|| this.0.get_prop("transcript-text"));
                        Ok(Value::Boolean(copy_to_clipboard(&text)))
                    }
                    "transcribe.send" => {
                        let path = arg1.unwrap_or_default();
                        let url = arg2.unwrap_or_else(|| "http://127.0.0.1:4100".to_string());
                        match transcribe_file(&url, Path::new(&path)) {
                            Ok(t) => Ok(Value::String(lua.create_string(&t)?)),
                            Err(e) => Err(mlua::Error::RuntimeError(e)),
                        }
                    }
                    "ui.notify" => {
                        notify(&arg1.unwrap_or_default());
                        Ok(Value::Nil)
                    }
                    "ui.set" => {
                        let key = arg1.unwrap_or_default();
                        let val = arg2.unwrap_or_default();
                        this.0.set_prop(&key, &val);
                        if key == "busy" || key == "show-result" {
                            this.0.set_bool(&key, val == "true" || val == "1");
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
            if !TEST_MODE.load(Ordering::Relaxed) {
                if let AppHandle::VoiceCapture(app) = &this.0 {
                    let _ = app.run();
                }
            }
            Ok(())
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argv = env::args().skip(1);
    let script_path = match argv.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: sdk-run <script.luau> [--test]");
            std::process::exit(1);
        }
    };
    let test = argv.any(|a| a == "--test");
    TEST_MODE.store(test, Ordering::Relaxed);

    let lua = Lua::new();
    let sdk = lua.create_table()?;
    sdk.set(
        "window",
        lua.create_function(|_, name: String| -> mlua::Result<AppUserData> {
            if TEST_MODE.load(Ordering::Relaxed) {
                return Ok(AppUserData(AppHandle::Stub(name)));
            }
            match name.as_str() {
                "VoiceCaptureWindow" => {
                    let app = ui_lib::VoiceCaptureWindow::new()
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    Ok(AppUserData(AppHandle::VoiceCapture(app)))
                }
                _ => Err(mlua::Error::RuntimeError(format!("unknown window: {name}"))),
            }
        })?,
    )?;
    sdk.set(
        "transcribe",
        lua.create_function(
            |lua, (path, url): (String, Option<String>)| -> mlua::Result<Value> {
                let base = url.unwrap_or_else(|| "http://127.0.0.1:4100".to_string());
                match transcribe_file(&base, Path::new(&path)) {
                    Ok(t) => Ok(Value::String(lua.create_string(&t)?)),
                    Err(e) => Err(mlua::Error::RuntimeError(e)),
                }
            },
        )?,
    )?;
    lua.globals().set("sdk", sdk)?;
    lua.globals()
        .set("VOICE_DAEMON_URL", "http://127.0.0.1:4100")?;

    let script = fs::read_to_string(&script_path)?;
    lua.load(&script).set_name(&script_path).exec()?;
    if test {
        println!("sdk-run: script loaded OK (--test)");
    }
    Ok(())
}
