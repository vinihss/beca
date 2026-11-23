use anyhow::Result;

use crate::{ipc, output};

/// Handles `beca daemon` by starting the IPC server.
pub fn serve() -> Result<()> {
    ipc::run()
}

/// Handles `beca ping`.
pub fn ping() -> Result<()> {
    let response = ipc::send_request(&serde_json::json!({ "type": "ping" }))?;
    output::print_response(&response, "daemon ping failed")
}

/// Handles `beca subscribe`: streams Hyprland events from the daemon.
pub fn subscribe() -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let path = ipc::socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        anyhow::anyhow!("failed to connect to daemon at {}: {error}", path.display())
    })?;
    writeln!(stream, "{}", serde_json::json!({ "type": "hyprland.subscribe" }))?;
    stream.flush()?;

    for line in BufReader::new(stream).lines() {
        println!("{}", line?);
    }
    Ok(())
}
