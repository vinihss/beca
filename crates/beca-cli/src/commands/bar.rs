use anyhow::{Result, bail};

/// Handles bar actions. Lifecycle actions are delegated to the daemon because
/// only it owns the supervised bar processes.
pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    match action {
        "show-top" => beca_bar::show("top"),
        "show-bottom" => beca_bar::show("bottom"),
        "show" => {
            let position = args
                .iter()
                .find_map(|arg| {
                    if arg == "top" || arg == "bottom" {
                        Some(arg.as_str())
                    } else {
                        None
                    }
                })
                .or_else(|| args.first().map(String::as_str))
                .unwrap_or("top");
            beca_bar::show(if position == "bottom" {
                "bottom"
            } else {
                "top"
            })
        }
        "status" => match beca_daemon::send(&beca_daemon::protocol::Request::BarStatus) {
            Ok(response) => {
                println!(
                    "{}",
                    serde_json::to_string(&response).unwrap_or_else(|_| "{}".into())
                );
                Ok(())
            }
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"ok": false, "error": error.to_string()})
                );
                Ok(())
            }
        },
        "hide" | "hide-top" | "hide-bottom" | "stop" | "stop-top" | "stop-bottom" => {
            let target = format!("bar:{action}");
            if let Some(response) = beca_daemon::try_route_mod(&target, args) {
                println!(
                    "{}",
                    serde_json::to_string(&response).unwrap_or_else(|_| "{}".into())
                );
                if response.get("ok").and_then(|value| value.as_bool()) == Some(true) {
                    Ok(())
                } else {
                    bail!(
                        "{}",
                        response
                            .get("error")
                            .and_then(|value| value.as_str())
                            .unwrap_or("bar lifecycle action failed")
                    )
                }
            } else {
                bail!(
                    "bar {action} requires daemon-managed lifecycle (start `beca daemon` or run via `beca mod bar:{action}`)"
                )
            }
        }
        other => bail!(
            "unknown bar action: {other} (expected: show, show-top, show-bottom, hide, hide-top, hide-bottom, status)"
        ),
    }
}
