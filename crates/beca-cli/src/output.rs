use anyhow::{Result, bail};
use serde_json::Value;

/// Prints a JSON value to stdout, one line per response.
pub fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

/// Prints a command response and maps `ok: false` to an error.
///
/// The daemon answers with `{"ok": false, "error": "..."}`; the error message
/// is forwarded so CLI callers see the real reason instead of a generic one.
pub fn print_response(response: &Value, fallback_error: &str) -> Result<()> {
    print_json(response)?;
    ok_or_error(response, fallback_error)
}

/// Maps `ok: false` to an error without printing the response.
pub fn ok_or_error(response: &Value, fallback_error: &str) -> Result<()> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    bail!(
        "{}",
        response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(fallback_error)
    )
}
