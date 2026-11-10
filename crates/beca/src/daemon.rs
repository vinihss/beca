use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("beca.sock")
    } else {
        beca_core::BecaPaths::resolve().config_dir.join("beca.sock")
    }
}

pub fn run() -> Result<()> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind daemon socket {}", path.display()))?;
    eprintln!("[beca daemon] listening on {}", path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = handle_client(stream) {
                    eprintln!("[beca daemon] client error: {e}");
                }
            }
            Err(e) => eprintln!("[beca daemon] accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_client(stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let req: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({"ok": false, "error": format!("invalid json: {e}")});
                writeln!(writer, "{}", resp)?;
                line.clear();
                continue;
            }
        };
        let resp = handle_request(&req);
        writeln!(writer, "{}", resp)?;
        writer.flush()?;
        line.clear();
    }
    Ok(())
}

fn handle_request(req: &Value) -> Value {
    let t = req.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "ping" => json!({"ok": true, "type": "pong"}),
        "version" => json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}),
        "hyprland.dispatch" => {
            let cmd = req.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.is_empty() {
                return json!({"ok": false, "error": "missing command"});
            }
            match hyprland_dispatch(cmd) {
                Ok((ok, response)) => json!({"ok": ok, "response": response}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "hyprland.socket_paths" => match hyprland_paths() {
            Some((s1, s2)) => json!({"ok": true, "socket": s1, "socket2": s2}),
            None => json!({"ok": false, "error": "hyprland not available"}),
        },
        other => json!({"ok": false, "error": format!("unknown request type: {other}")}),
    }
}

fn hyprland_paths() -> Option<(String, String)> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let base = runtime.join("hypr").join(sig);
    Some((
        base.join(".socket.sock").to_string_lossy().to_string(),
        base.join(".socket2.sock").to_string_lossy().to_string(),
    ))
}

fn hyprland_dispatch(command: &str) -> Result<(bool, String), String> {
    let (sock, _) = hyprland_paths().ok_or_else(|| "hyprland not available".to_string())?;
    use std::io::{Read, Write as _};
    use std::os::unix::net::UnixStream as UStream;
    let mut stream = UStream::connect(&sock).map_err(|e| format!("connect: {e}"))?;
    stream
        .write_all(command.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
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
    Ok((ok, resp))
}

/// Send a request to a running daemon and return the response.
pub fn send_request(req: &Value) -> Result<Value> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to daemon at {}", path.display()))?;
    writeln!(stream, "{}", req)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let resp: Value = serde_json::from_str(line.trim()).context("invalid daemon response")?;
    Ok(resp)
}
