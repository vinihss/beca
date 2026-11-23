use anyhow::Result;
use std::io::{BufRead, BufReader};

use crate::output;
use beca_daemon::protocol::Request;

/// Handles `beca daemon` by starting the IPC server.
pub fn serve() -> Result<()> {
    beca_daemon::run()
}

/// Handles `beca ping`.
pub fn ping() -> Result<()> {
    let response = beca_daemon::send(&Request::Ping)?;
    output::print_response(&response, "daemon ping failed")
}

/// Handles `beca subscribe`: streams Hyprland events from the daemon.
pub fn subscribe() -> Result<()> {
    let stream = beca_daemon::subscribe_stream()?;
    for line in BufReader::new(stream).lines() {
        println!("{}", line?);
    }
    Ok(())
}
