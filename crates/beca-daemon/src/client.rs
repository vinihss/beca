use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::protocol::Request;

/// Path of the daemon control socket.
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("beca.sock")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("beca.sock")
    }
}

/// Sends a raw JSON request and reads one response line.
pub fn send_request(req: &Value) -> Result<Value> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to daemon at {}", path.display()))?;
    writeln!(stream, "{req}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    serde_json::from_str(line.trim()).context("invalid daemon response")
}

/// Sends a typed request and reads one response line.
pub fn send(req: &Request) -> Result<Value> {
    send_request(&serde_json::to_value(req)?)
}

/// Returns `None` when no daemon is listening; otherwise forwards the mod request.
pub fn try_route_mod(target: &str, args: &[String]) -> Option<Value> {
    let path = socket_path();
    if !path.exists() {
        return None;
    }
    let req = Request::Mod {
        target: target.to_string(),
        args: args.to_vec(),
    };
    send(&req).ok()
}

/// Opens a stream for `hyprland.subscribe` and returns the ack line.
pub fn subscribe_stream() -> Result<UnixStream> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to daemon at {}", path.display()))?;
    writeln!(stream, "{}", json!(Request::HyprlandSubscribe))?;
    stream.flush()?;
    Ok(stream)
}
