pub mod client;
pub mod control;
pub mod handlers;
pub mod hyprland;
pub mod protocol;
pub mod state;

pub use client::{send, send_request, socket_path, subscribe_stream, try_route_mod};

use anyhow::{Context, Result};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

use handlers::handle_request;
use protocol::Request;

/// Starts the IPC daemon and blocks accepting client connections.
pub fn run() -> Result<()> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind daemon socket {}", path.display()))?;
    eprintln!("[beca daemon] listening on {}", path.display());

    thread::spawn(|| {
        thread::sleep(Duration::from_millis(150));
        match control::ensure_resident_launcher() {
            Ok(pid) => eprintln!("[beca daemon] resident launcher ready (pid {pid})"),
            Err(error) => eprintln!("[beca daemon] resident launcher warmup failed: {error}"),
        }
        match control::ensure_resident_voice() {
            Ok(pid) => eprintln!("[beca daemon] resident voice ready (pid {pid})"),
            Err(error) => eprintln!("[beca daemon] resident voice warmup failed: {error}"),
        }
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(error) = handle_client(stream) {
                        eprintln!("[beca daemon] client error: {error}");
                    }
                });
            }
            Err(error) => eprintln!("[beca daemon] accept error: {error}"),
        }
    }
    Ok(())
}

/// Reads newline-delimited requests from one client until it disconnects.
fn handle_client(stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: Request = match serde_json::from_str(line.trim()) {
            Ok(request) => request,
            Err(error) => {
                let response = json!({"ok": false, "error": format!("invalid json: {error}")});
                writeln!(writer, "{response}")?;
                writer.flush()?;
                continue;
            }
        };

        if matches!(request, Request::HyprlandSubscribe) {
            writeln!(writer, "{}", handle_request(request))?;
            writer.flush()?;
            return hyprland::stream_hyprland_events(writer);
        }

        let response = handle_request(request);
        writeln!(writer, "{response}")?;
        writer.flush()?;
    }
}
