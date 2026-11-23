use anyhow::{Result, anyhow, bail};

/// Handles `beca mod alert:*`. Alerts are always local: they are one-shot layer-shell
/// surfaces and must not be adopted by the daemon's supervised processes.
pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    match action {
        "show" => {
            let mut title = "Beca".to_string();
            let mut message = String::new();
            let mut timeout_ms = 5000u64;
            let mut it = args.iter();

            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "-t" | "--title" => {
                        let Some(value) = it.next() else {
                            bail!("{arg} requires a value");
                        };
                        title = value.clone();
                    }
                    "-m" | "--message" => {
                        let Some(value) = it.next() else {
                            bail!("{arg} requires a value");
                        };
                        message = value.clone();
                    }
                    "--timeout-ms" => {
                        let Some(value) = it.next() else {
                            bail!("{arg} requires a value");
                        };
                        timeout_ms = value
                            .parse::<u64>()
                            .map_err(|_| anyhow!("invalid timeout-ms: {value}"))?;
                    }
                    other if !other.starts_with('-') && message.is_empty() => {
                        message = other.to_string();
                    }
                    other => bail!("unknown alert option: {other}"),
                }
            }

            beca_layer_shell::run_alert(&beca_layer_shell::AlertOptions {
                title,
                message,
                timeout_ms,
                accent: None,
            })
            .map_err(anyhow::Error::msg)
        }
        other => bail!("unknown alert action: {other} (expected: show)"),
    }
}
