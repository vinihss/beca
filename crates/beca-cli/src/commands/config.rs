use anyhow::Result;

use crate::cli::ConfigAction;
use crate::output;
use beca_daemon::protocol::Request;

/// Handles `beca config init|reload`.
pub fn execute(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Init { force } => init(force),
        ConfigAction::Reload => reload(),
    }
}

fn init(force: bool) -> Result<()> {
    let paths = beca_core::config::BecaPaths::resolve();
    let created = beca_core::config::ensure_defaults_with_paths(&paths, force)
        .map_err(|error| anyhow::anyhow!(error))?;
    output::print_json(&serde_json::json!({
        "ok": true,
        "config_dir": paths.config_dir,
        "created": created,
        "force": force,
    }))
}

fn reload() -> Result<()> {
    let response = beca_daemon::send(&Request::ConfigReload)?;
    output::print_response(&response, "config reload failed")
}
