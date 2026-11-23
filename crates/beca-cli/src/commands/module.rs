use anyhow::{Result, bail};

use super::{bar, launcher, registry, voice};
use crate::cli::parse_target;
use crate::{ipc, output};

/// Handles `beca mod <target> [args...]`.
pub fn execute(target: &str, local: bool, args: &[String]) -> Result<()> {
    if target == "list" {
        for (name, description) in registry::list_modules() {
            println!("{name}\t{description}");
        }
        return Ok(());
    }

    let (module, action) = parse_target(target)?;
    if action == "info" {
        if let Some(info) = registry::module_info(&module) {
            println!("{info}");
            return Ok(());
        }
        bail!("unknown module: {module} (available: bar, launcher, voice)");
    }

    let no_daemon = local || std::env::var_os("BECA_NO_DAEMON").is_some();
    if !no_daemon && target != "voice:transcribe" {
        if let Some(response) = ipc::try_route_mod(target, args) {
            output::ok_or_error(&response, "daemon routing failed")?;
            return output::print_json(&response);
        }
    }

    dispatch(&module, &action, args)
}

/// Dispatches a module action locally, bypassing the daemon.
fn dispatch(module: &str, action: &str, args: &[String]) -> Result<()> {
    match module {
        "launcher" | "apps" => match action {
            "show" => launcher::show(),
            "serve" => launcher::serve(),
            "hide" => launcher::send_control("hide"),
            "stop" | "quit" => launcher::send_control("quit"),
            other => {
                bail!("unknown launcher action: {other} (expected: show, serve, hide, stop)")
            }
        },
        "bar" => bar::dispatch(action, args),
        "voice" => voice::dispatch(action, args),
        other => bail!("unknown module: {other} (available: bar, launcher, voice)"),
    }
}
