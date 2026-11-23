use serde::{Deserialize, Serialize};

/// Requests accepted by the daemon.
///
/// `#[serde(tag = "type")]` keeps the existing wire format, e.g.
/// `{"type":"mod","target":"bar:show","args":[]}`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Request {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "version")]
    Version,
    #[serde(rename = "hyprland.dispatch")]
    HyprlandDispatch { command: String },
    #[serde(rename = "hyprland.socket_paths")]
    HyprlandSocketPaths,
    #[serde(rename = "hyprland.subscribe")]
    HyprlandSubscribe,
    #[serde(rename = "mod")]
    Mod {
        target: String,
        #[serde(default)]
        args: Vec<String>,
    },
    #[serde(rename = "bar.status")]
    BarStatus,
    #[serde(rename = "launcher.status")]
    LauncherStatus,
    #[serde(rename = "config.reload")]
    ConfigReload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_wire_format_is_stable() {
        assert_eq!(
            serde_json::to_value(Request::Ping).unwrap(),
            json!({"type": "ping"})
        );
        assert_eq!(
            serde_json::to_value(Request::Mod {
                target: "bar:show".into(),
                args: vec![],
            })
            .unwrap(),
            json!({"type": "mod", "target": "bar:show", "args": []})
        );
    }

    #[test]
    fn parses_requests_without_optional_fields() {
        let request: Request =
            serde_json::from_value(json!({"type": "mod", "target": "voice:show"})).unwrap();
        assert_eq!(
            request,
            Request::Mod {
                target: "voice:show".into(),
                args: vec![],
            }
        );
    }
}
