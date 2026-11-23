use anyhow::{Result, bail};

use super::{alert, bar, registry};
use crate::cli::parse_target;
use crate::output;

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
        bail!("unknown module: {module} (available: alert, bar, launcher, voice)");
    }

    let no_daemon = local || std::env::var_os("BECA_NO_DAEMON").is_some();
    if !no_daemon && module != "alert" && target != "voice:transcribe" {
        if let Some(response) = beca_daemon::try_route_mod(target, args) {
            output::ok_or_error(&response, "daemon routing failed")?;
            return output::print_json(&response);
        }
    }

    dispatch(&module, &action, args)
}

/// Dispatches a module action locally, bypassing the daemon.
fn dispatch(module: &str, action: &str, args: &[String]) -> Result<()> {
    match module {
        "alert" => alert::dispatch(action, args),
        "launcher" | "apps" => match action {
            "show" => beca_launcher::show(),
            "serve" => beca_launcher::serve(),
            "hide" => beca_launcher::send_control("hide"),
            "stop" | "quit" => beca_launcher::send_control("quit"),
            other => {
                bail!("unknown launcher action: {other} (expected: show, serve, hide, stop)")
            }
        },
        "bar" => bar::dispatch(action, args),
        "voice" => beca_voice::dispatch(action, args),
        other => bail!("unknown module: {other} (available: alert, bar, launcher, voice)"),
    }
}
