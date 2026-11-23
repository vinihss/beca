use anyhow::{Context, Result};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub fn hyprland_paths() -> Option<(String, String)> {
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

pub fn hyprland_dispatch(command: &str) -> Result<(bool, String), String> {
    let (sock, _) = hyprland_paths().ok_or_else(|| "hyprland not available".to_string())?;
    let mut stream = UnixStream::connect(&sock).map_err(|e| format!("connect: {e}"))?;
    stream
        .write_all(command.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
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

pub fn stream_hyprland_events(mut writer: UnixStream) -> Result<()> {
    let (_, sock2) = hyprland_paths().ok_or_else(|| anyhow::anyhow!("hyprland not available"))?;
    let stream = UnixStream::connect(&sock2).with_context(|| format!("connect {}", sock2))?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }
        let (event, data) = if let Some((a, b)) = line.split_once(">>") {
            (a.to_string(), b.to_string())
        } else {
            (line.clone(), String::new())
        };
        let msg = json!({
            "type": "hyprland.event",
            "event": event,
            "data": data,
            "raw": line,
        });
        if writeln!(writer, "{}", msg).is_err() {
            break;
        }
        if writer.flush().is_err() {
            break;
        }
    }
    Ok(())
}
