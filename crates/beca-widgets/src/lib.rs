//! Shared widget data providers and actions for the Beca bar.
//!
//! This crate owns the system-facing code behind bar widgets: reading clocks,
//! volume, network, stats and notification counts, plus the matching actions.
//! It has no dependency on either renderer, so `beca-bar` (window fallback)
//! and `beca-layer-shell` can use the same implementations without duplicating
//! them.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;

/// Canonical widget names accepted in bar layout lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetId {
    Launcher,
    Clock,
    Conversation,
    VolumeOut,
    VolumeIn,
    Network,
    Alerts,
    Bluetooth,
    Cpu,
    Memory,
    Disk,
}

impl WidgetId {
    /// Parses a configured name, accepting the same aliases as before.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "launcher" | "apps" | "app" => Some(Self::Launcher),
            "clock" | "time" => Some(Self::Clock),
            "conversation" | "voice-live" | "voicelive" | "live" | "voice" => {
                Some(Self::Conversation)
            }
            "volume-out" | "volume_out" | "audio-out" | "output" | "sink" => Some(Self::VolumeOut),
            "volume-in" | "volume_in" | "audio-in" | "input" | "source" | "mic" => {
                Some(Self::VolumeIn)
            }
            "network" | "net" | "wifi" | "ethernet" => Some(Self::Network),
            "alerts" | "notifications" | "notif" => Some(Self::Alerts),
            "bluetooth" | "bt" => Some(Self::Bluetooth),
            "cpu" => Some(Self::Cpu),
            "memory" | "mem" | "ram" => Some(Self::Memory),
            "disk" | "storage" | "fs" => Some(Self::Disk),
            _ => None,
        }
    }

    /// Returns the canonical name used in resolved layouts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Launcher => "launcher",
            Self::Clock => "clock",
            Self::Conversation => "conversation",
            Self::VolumeOut => "volume-out",
            Self::VolumeIn => "volume-in",
            Self::Network => "network",
            Self::Alerts => "alerts",
            Self::Bluetooth => "bluetooth",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Disk => "disk",
        }
    }
}

pub fn format_clock(format: &str) -> String {
    let out = Command::new("date").arg(format!("+{}", format)).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "--:--".to_string(),
    }
}

#[derive(Clone, Debug)]
pub struct VolumeInfo {
    pub text: String,
    pub muted: bool,
}

pub fn read_volume_wpctl(target: &str) -> Option<VolumeInfo> {
    let out = Command::new("wpctl")
        .args(["get-volume", target])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);

    let muted = s.contains("[MUTED]");
    let vol = s
        .split_whitespace()
        .find_map(|part| part.parse::<f32>().ok())
        .unwrap_or(0.0);
    let pct = (vol * 100.0).round().clamp(0.0, 999.0) as i32;
    Some(VolumeInfo {
        text: format!("{pct}%"),
        muted,
    })
}

pub fn read_volume_pactl(target: &str) -> Option<VolumeInfo> {
    let (cmd_target, mute_flag) = if target == "@DEFAULT_AUDIO_SINK@" {
        ("@DEFAULT_SINK@", "get-sink-mute")
    } else {
        ("@DEFAULT_SOURCE@", "get-source-mute")
    };
    let vol_out = Command::new("pactl")
        .args(["get-sink-volume", cmd_target])
        .output()
        .ok();
    let vol_out = if target == "@DEFAULT_AUDIO_SINK@" {
        vol_out
    } else {
        Command::new("pactl")
            .args(["get-source-volume", cmd_target])
            .output()
            .ok()
    };
    let vol_out = vol_out?;
    if !vol_out.status.success() {
        return None;
    }
    let vs = String::from_utf8_lossy(&vol_out.stdout);
    let pct = vs
        .split('%')
        .next()
        .and_then(|head| head.split_whitespace().last())
        .and_then(|n| n.parse::<i32>().ok())
        .unwrap_or(0);
    let mute_out = Command::new("pactl")
        .args([mute_flag, cmd_target])
        .output()
        .ok();
    let muted = mute_out
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("yes"))
        .unwrap_or(false);
    Some(VolumeInfo {
        text: format!("{pct}%"),
        muted,
    })
}

pub fn read_volume(target: &str) -> VolumeInfo {
    read_volume_wpctl(target)
        .or_else(|| read_volume_pactl(target))
        .unwrap_or(VolumeInfo {
            text: "--%".to_string(),
            muted: false,
        })
}

pub fn adjust_volume_wpctl(target: &str, up: bool) -> bool {
    let step = if up { "5%+" } else { "5%-" };
    Command::new("wpctl")
        .args(["set-volume", "-l", "1.5", target, step])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn adjust_volume_pactl(target: &str, up: bool) -> bool {
    let (cmd, t) = if target == "@DEFAULT_AUDIO_SINK@" {
        ("set-sink-volume", "@DEFAULT_SINK@")
    } else {
        ("set-source-volume", "@DEFAULT_SOURCE@")
    };
    let step = if up { "+5%" } else { "-5%" };
    Command::new("pactl")
        .args([cmd, t, step])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn adjust_volume(target: &str, up: bool) {
    if !adjust_volume_wpctl(target, up) {
        let _ = adjust_volume_pactl(target, up);
    }
}

pub fn read_network() -> String {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "TYPE,STATE,CONNECTION", "device"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            for line in s.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 && parts[1] == "connected" {
                    let name = parts[2];
                    if !name.is_empty() && name != "--" {
                        let short = if name.chars().count() > 12 {
                            name.chars().take(12).collect::<String>() + "…"
                        } else {
                            name.to_string()
                        };
                        return short;
                    }
                }
            }
            for line in s.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 && parts[1] == "connected" {
                    return "on".into();
                }
            }
        }
    }
    let out = Command::new("ip")
        .args(["-o", "link", "show", "up"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.lines().any(|l| !l.contains("lo:")) {
                return "on".into();
            }
        }
    }
    "off".into()
}

pub fn read_bluetooth() -> String {
    let out = Command::new("bluetoothctl").args(["show"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let powered = s.lines().any(|l| l.trim() == "Powered: yes");
            if !powered {
                return "off".into();
            }
            if let Ok(dev) = Command::new("bluetoothctl")
                .args(["devices", "Connected"])
                .output()
            {
                let ds = String::from_utf8_lossy(&dev.stdout);
                let count = ds.lines().filter(|l| !l.trim().is_empty()).count();
                if count > 0 {
                    return format!("{count}");
                }
            }
            return "on".into();
        }
    }
    "--".into()
}

pub fn toggle_volume_mute(target: &str) {
    if !Command::new("wpctl")
        .args(["set-mute", target, "toggle"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let (cmd, t) = if target == "@DEFAULT_AUDIO_SINK@" {
            ("set-sink-mute", "@DEFAULT_SINK@")
        } else {
            ("set-source-mute", "@DEFAULT_SOURCE@")
        };
        let _ = Command::new("pactl").args([cmd, t, "toggle"]).status();
    }
}

pub fn open_network_panel() {
    if Command::new("nm-connection-editor").spawn().is_err() {
        let _ = Command::new("nm-applet").spawn();
    }
}

pub fn open_bluetooth_panel() {
    let _ = Command::new("blueman-manager").spawn();
}

pub fn open_alerts_panel() {
    if Command::new("swaync-client")
        .args(["-t", "-sw"])
        .spawn()
        .is_ok()
    {
        return;
    }
    let _ = Command::new("dunstctl").args(["history-pop"]).spawn();
}

pub fn read_alerts() -> String {
    if let Ok(o) = Command::new("swaync-client").args(["-c"]).output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    if let Ok(o) = Command::new("makoctl").arg("list").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let count = s.matches("\n\n").count() + if s.trim().is_empty() { 0 } else { 1 };
            return count.to_string();
        }
    }
    if let Ok(o) = Command::new("dunstctl")
        .args(["count", "displayed"])
        .output()
    {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "0".into()
}

pub fn read_cpu() -> String {
    static PREV: std::sync::OnceLock<Mutex<(u64, u64)>> = std::sync::OnceLock::new();
    let lock = PREV.get_or_init(|| Mutex::new((0, 0)));
    let Ok(content) = std::fs::read_to_string("/proc/stat") else {
        return "--%".into();
    };
    let Some(line) = content.lines().next() else {
        return "--%".into();
    };
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    if nums.len() < 4 {
        return "--%".into();
    }
    let idle = nums.get(3).copied().unwrap_or(0) + nums.get(4).copied().unwrap_or(0);
    let total: u64 = nums.iter().sum();
    let mut prev = match lock.lock() {
        Ok(g) => g,
        Err(_) => return "--%".into(),
    };
    let (pt, pi) = *prev;
    *prev = (total, idle);
    if pt == 0 || total <= pt {
        return "--%".into();
    }
    let dt = total - pt;
    let di = idle.saturating_sub(pi);
    let usage = ((dt.saturating_sub(di)) as f64 / dt as f64 * 100.0)
        .round()
        .clamp(0.0, 100.0) as i64;
    format!("{usage}%")
}

pub fn read_memory() -> String {
    let Ok(content) = std::fs::read_to_string("/proc/meminfo") else {
        return "--%".into();
    };
    let mut total: u64 = 0;
    let mut avail: u64 = 0;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            total = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            avail = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
    }
    if total == 0 {
        return "--%".into();
    }
    let used = total.saturating_sub(avail);
    let pct = (used as f64 / total as f64 * 100.0)
        .round()
        .clamp(0.0, 100.0) as i64;
    format!("{pct}%")
}

pub fn read_disk() -> String {
    let out = Command::new("df").args(["-h", "/"]).output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = s.lines().nth(1) {
                if let Some(pct) = line.split_whitespace().nth(4) {
                    return pct.to_string();
                }
            }
        }
    }
    "--%".into()
}

/// Toggles the conversation widget: signals the running session to quit, or
/// starts a new local `voice:converse` child and keeps its handle in `slot`.
pub fn converse_spawn_or_signal(slot: &Mutex<Option<Child>>) -> bool {
    let mut slot = match slot.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut alive = false;
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(None) => alive = true,
            Ok(Some(_)) => {
                *slot = None;
            }
            Err(_) => alive = true,
        }
    }
    if alive {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(dir).join("beca").join("converse.ctl");
            if let Ok(mut s) = UnixStream::connect(&path) {
                let _ = s.write_all(b"quit\n");
            } else {
                eprintln!("[beca-bar] converse ctl unreachable: {}", path.display());
            }
        }
        return false;
    }

    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .env("BECA_NO_DAEMON", "1")
            .arg("mod")
            .arg("--local")
            .arg("voice:converse")
            .spawn();
        match child {
            Ok(c) => {
                *slot = Some(c);
                return true;
            }
            Err(e) => eprintln!("[beca-bar] failed to spawn voice:converse: {e}"),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_names_and_aliases() {
        assert_eq!(WidgetId::parse(" clock "), Some(WidgetId::Clock));
        assert_eq!(WidgetId::parse("time"), Some(WidgetId::Clock));
        assert_eq!(WidgetId::parse("apps"), Some(WidgetId::Launcher));
        assert_eq!(WidgetId::parse("mic"), Some(WidgetId::VolumeIn));
        assert_eq!(WidgetId::parse("bt"), Some(WidgetId::Bluetooth));
        assert_eq!(WidgetId::parse("unknown"), None);
    }

    #[test]
    fn canonical_names_round_trip() {
        for name in [
            "launcher",
            "clock",
            "conversation",
            "volume-out",
            "volume-in",
            "network",
            "alerts",
            "bluetooth",
            "cpu",
            "memory",
            "disk",
        ] {
            let id = WidgetId::parse(name).expect("name should parse");
            assert_eq!(id.as_str(), name);
        }
    }
}
