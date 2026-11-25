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
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    /// Every widget id in canonical layout order.
    pub const ALL: [Self; 11] = [
        Self::Launcher,
        Self::Clock,
        Self::Conversation,
        Self::VolumeOut,
        Self::VolumeIn,
        Self::Network,
        Self::Alerts,
        Self::Bluetooth,
        Self::Cpu,
        Self::Memory,
        Self::Disk,
    ];

    fn index(self) -> usize {
        match self {
            Self::Launcher => 0,
            Self::Clock => 1,
            Self::Conversation => 2,
            Self::VolumeOut => 3,
            Self::VolumeIn => 4,
            Self::Network => 5,
            Self::Alerts => 6,
            Self::Bluetooth => 7,
            Self::Cpu => 8,
            Self::Memory => 9,
            Self::Disk => 10,
        }
    }

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

/// A value written by a widget source to a renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetValue {
    Text(String),
    Muted(bool),
    Active(bool),
}

/// Renderer-agnostic batch of widget changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WidgetUpdate {
    values: Vec<(WidgetId, WidgetValue)>,
}

impl WidgetUpdate {
    pub fn text(id: WidgetId, text: impl Into<String>) -> Self {
        Self::default().with(id, WidgetValue::Text(text.into()))
    }

    pub fn muted(id: WidgetId, muted: bool) -> Self {
        Self::default().with(id, WidgetValue::Muted(muted))
    }

    pub fn active(id: WidgetId, active: bool) -> Self {
        Self::default().with(id, WidgetValue::Active(active))
    }

    pub fn volume(id: WidgetId, info: VolumeInfo) -> Self {
        Self::default()
            .with(id, WidgetValue::Text(info.text))
            .with(id, WidgetValue::Muted(info.muted))
    }

    /// Appends another value to this update.
    pub fn with(mut self, id: WidgetId, value: WidgetValue) -> Self {
        self.values.push((id, value));
        self
    }

    /// Writes every value through `sink`.
    pub fn apply<S: WidgetSink>(&self, sink: &S) {
        for (id, value) in &self.values {
            match value {
                WidgetValue::Text(text) => sink.set_text(*id, text),
                WidgetValue::Muted(muted) => sink.set_muted(*id, *muted),
                WidgetValue::Active(active) => sink.set_active(*id, *active),
            }
        }
    }
}

/// Minimal renderer contract used by the widget controller.
///
/// Both bar renderers implement this trait; neither needs to know where the
/// data came from or which command produced it.
pub trait WidgetSink {
    fn set_visible(&self, id: WidgetId, visible: bool);
    fn set_section(&self, id: WidgetId, section: &'static str);
    fn set_text(&self, id: WidgetId, text: &str);
    fn set_muted(&self, id: WidgetId, muted: bool);
    fn set_active(&self, id: WidgetId, active: bool);
}

/// Resolved layout for widget ids.
#[derive(Clone, Debug, Default)]
pub struct WidgetLayout {
    sections: [Option<&'static str>; 11],
}

impl WidgetLayout {
    /// Builds a layout by querying a resolver such as `BarProperties::section`.
    pub fn from_lookup<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Option<&'static str>,
    {
        let mut sections = [None; 11];
        for id in WidgetId::ALL {
            sections[id.index()] = lookup(id.as_str());
        }
        Self { sections }
    }

    pub fn section(&self, id: WidgetId) -> Option<&'static str> {
        self.sections[id.index()]
    }

    pub fn visible(&self, id: WidgetId) -> bool {
        self.section(id).is_some()
    }

    pub fn any_visible(&self, ids: &[WidgetId]) -> bool {
        ids.iter().copied().any(|id| self.visible(id))
    }
}

/// Grouping used by the renderer timers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetGroup {
    Clock,
    Volume,
    Conversation,
    Stats,
}

impl WidgetGroup {
    pub const ALL: [Self; 4] = [Self::Clock, Self::Volume, Self::Conversation, Self::Stats];
}

/// User-triggered widget action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetAction {
    Launcher,
    ConversationToggle,
    VolumeScroll { id: WidgetId, up: bool },
    VolumeMute { id: WidgetId },
    Alerts,
    Network,
    Bluetooth,
}

/// Owns widget state and produces updates for any renderer.
#[derive(Clone)]
pub struct WidgetController {
    layout: Arc<WidgetLayout>,
    clock_format: Arc<str>,
    converse_child: Arc<Mutex<Option<Child>>>,
}

impl WidgetController {
    pub fn new(layout: WidgetLayout, clock_format: impl Into<String>) -> Self {
        Self {
            layout: Arc::new(layout),
            clock_format: Arc::from(clock_format.into().as_str()),
            converse_child: Arc::new(Mutex::new(None)),
        }
    }

    pub fn layout(&self) -> &WidgetLayout {
        &self.layout
    }

    pub fn clock_interval(&self) -> Duration {
        if self.clock_format.contains("%S") {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(30)
        }
    }

    /// Timer intervals for the widgets enabled in the current layout.
    pub fn timer_specs(&self) -> Vec<(Duration, WidgetGroup)> {
        let mut specs = vec![(Duration::from_millis(250), WidgetGroup::Conversation)];
        if self.layout.visible(WidgetId::Clock) {
            specs.push((self.clock_interval(), WidgetGroup::Clock));
        }
        if self
            .layout
            .any_visible(&[WidgetId::VolumeOut, WidgetId::VolumeIn])
        {
            specs.push((Duration::from_secs(2), WidgetGroup::Volume));
        }
        if self.layout.any_visible(&[
            WidgetId::Network,
            WidgetId::Alerts,
            WidgetId::Bluetooth,
            WidgetId::Cpu,
            WidgetId::Memory,
            WidgetId::Disk,
        ]) {
            specs.push((Duration::from_secs(3), WidgetGroup::Stats));
        }
        specs
    }

    /// Writes the resolved visibility and section of every widget.
    pub fn apply_layout<S: WidgetSink>(&self, sink: &S) {
        for id in WidgetId::ALL {
            let visible = self.layout.visible(id);
            sink.set_visible(id, visible);
            if visible && let Some(section) = self.layout.section(id) {
                sink.set_section(id, section);
            }
        }
    }

    /// Samples one group of widgets.
    pub fn sample(&self, group: WidgetGroup) -> Option<WidgetUpdate> {
        match group {
            WidgetGroup::Clock => self
                .layout
                .visible(WidgetId::Clock)
                .then(|| WidgetUpdate::text(WidgetId::Clock, format_clock(&self.clock_format))),
            WidgetGroup::Volume => {
                let mut update = WidgetUpdate::default();
                if self.layout.visible(WidgetId::VolumeOut) {
                    let info = read_volume("@DEFAULT_AUDIO_SINK@");
                    update = update
                        .with(WidgetId::VolumeOut, WidgetValue::Text(info.text))
                        .with(WidgetId::VolumeOut, WidgetValue::Muted(info.muted));
                }
                if self.layout.visible(WidgetId::VolumeIn) {
                    let info = read_volume("@DEFAULT_AUDIO_SOURCE@");
                    update = update
                        .with(WidgetId::VolumeIn, WidgetValue::Text(info.text))
                        .with(WidgetId::VolumeIn, WidgetValue::Muted(info.muted));
                }
                (!update.is_empty()).then_some(update)
            }
            WidgetGroup::Conversation => Some(WidgetUpdate::active(
                WidgetId::Conversation,
                conversation_active(&self.converse_child),
            )),
            WidgetGroup::Stats => {
                let mut update = WidgetUpdate::default();
                if self.layout.visible(WidgetId::Network) {
                    update = update.with(WidgetId::Network, WidgetValue::Text(read_network()));
                }
                if self.layout.visible(WidgetId::Alerts) {
                    update = update.with(WidgetId::Alerts, WidgetValue::Text(read_alerts()));
                }
                if self.layout.visible(WidgetId::Bluetooth) {
                    update = update.with(WidgetId::Bluetooth, WidgetValue::Text(read_bluetooth()));
                }
                if self.layout.visible(WidgetId::Cpu) {
                    update = update.with(WidgetId::Cpu, WidgetValue::Text(read_cpu()));
                }
                if self.layout.visible(WidgetId::Memory) {
                    update = update.with(WidgetId::Memory, WidgetValue::Text(read_memory()));
                }
                if self.layout.visible(WidgetId::Disk) {
                    update = update.with(WidgetId::Disk, WidgetValue::Text(read_disk()));
                }
                (!update.is_empty()).then_some(update)
            }
        }
    }

    /// Samples all groups and writes them to `sink`.
    pub fn sample_all<S: WidgetSink>(&self, sink: &S) {
        for group in WidgetGroup::ALL {
            if let Some(update) = self.sample(group) {
                update.apply(sink);
            }
        }
    }

    /// Performs an action and returns the immediate update, if any.
    pub fn act(&self, action: WidgetAction) -> Option<WidgetUpdate> {
        match action {
            WidgetAction::Launcher => {
                let exe = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "beca".to_string());
                let _ = Command::new(exe).args(["mod", "launcher:show"]).spawn();
                None
            }
            WidgetAction::ConversationToggle => {
                let active = converse_spawn_or_signal(&self.converse_child);
                Some(WidgetUpdate::active(WidgetId::Conversation, active))
            }
            WidgetAction::VolumeScroll { id, up } => {
                let target = volume_target(id)?;
                adjust_volume(target, up);
                let info = read_volume(target);
                Some(WidgetUpdate::volume(id, info))
            }
            WidgetAction::VolumeMute { id } => {
                let target = volume_target(id)?;
                toggle_volume_mute(target);
                let info = read_volume(target);
                Some(WidgetUpdate::volume(id, info))
            }
            WidgetAction::Alerts => {
                open_alerts_panel();
                None
            }
            WidgetAction::Network => {
                open_network_panel();
                None
            }
            WidgetAction::Bluetooth => {
                open_bluetooth_panel();
                None
            }
        }
    }
}

impl WidgetUpdate {
    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

fn volume_target(id: WidgetId) -> Option<&'static str> {
    match id {
        WidgetId::VolumeOut => Some("@DEFAULT_AUDIO_SINK@"),
        WidgetId::VolumeIn => Some("@DEFAULT_AUDIO_SOURCE@"),
        _ => None,
    }
}

fn conversation_active(slot: &Mutex<Option<Child>>) -> bool {
    match slot.lock() {
        Ok(mut slot) => match slot.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) | Err(_) => true,
                Ok(Some(_)) => {
                    *slot = None;
                    false
                }
            },
            None => false,
        },
        Err(_) => false,
    }
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

    #[derive(Default)]
    struct RecordingSink {
        values: std::cell::RefCell<Vec<(WidgetId, WidgetValue)>>,
    }

    impl WidgetSink for RecordingSink {
        fn set_visible(&self, id: WidgetId, visible: bool) {
            self.values
                .borrow_mut()
                .push((id, WidgetValue::Active(visible)));
        }
        fn set_section(&self, id: WidgetId, section: &'static str) {
            self.values
                .borrow_mut()
                .push((id, WidgetValue::Text(section.into())));
        }
        fn set_text(&self, id: WidgetId, text: &str) {
            self.values
                .borrow_mut()
                .push((id, WidgetValue::Text(text.into())));
        }
        fn set_muted(&self, id: WidgetId, muted: bool) {
            self.values
                .borrow_mut()
                .push((id, WidgetValue::Muted(muted)));
        }
        fn set_active(&self, id: WidgetId, active: bool) {
            self.values
                .borrow_mut()
                .push((id, WidgetValue::Active(active)));
        }
    }

    #[test]
    fn layout_uses_the_shared_widget_ids() {
        let layout = WidgetLayout::from_lookup(|name| match name {
            "clock" => Some("center"),
            "volume-out" => Some("end"),
            _ => None,
        });
        let controller = WidgetController::new(layout, "%H:%M");
        let sink = RecordingSink::default();

        controller.apply_layout(&sink);

        assert!(controller.layout().visible(WidgetId::Clock));
        assert!(controller.layout().visible(WidgetId::VolumeOut));
        assert!(!controller.layout().visible(WidgetId::Cpu));
        assert_eq!(controller.layout().section(WidgetId::Clock), Some("center"));
        assert!(!sink.values.borrow().is_empty());
    }

    #[test]
    fn timer_specs_include_only_enabled_groups() {
        let layout = WidgetLayout::from_lookup(|name| match name {
            "clock" => Some("begin"),
            "network" => Some("end"),
            _ => None,
        });
        let controller = WidgetController::new(layout, "%H:%M");
        let specs = controller.timer_specs();

        assert_eq!(specs.len(), 3);
        assert!(specs.iter().any(|(_, g)| *g == WidgetGroup::Conversation));
        assert!(specs.iter().any(|(_, g)| *g == WidgetGroup::Clock));
        assert!(specs.iter().any(|(_, g)| *g == WidgetGroup::Stats));
        assert!(!specs.iter().any(|(_, g)| *g == WidgetGroup::Volume));
    }

    #[test]
    fn hidden_groups_are_not_sampled() {
        let layout = WidgetLayout::from_lookup(|_| None);
        let controller = WidgetController::new(layout, "%H:%M");

        assert!(controller.sample(WidgetGroup::Clock).is_none());
        assert!(controller.sample(WidgetGroup::Volume).is_none());
        assert!(controller.sample(WidgetGroup::Stats).is_none());
        assert_eq!(
            controller.sample(WidgetGroup::Conversation),
            Some(WidgetUpdate::active(WidgetId::Conversation, false))
        );
    }
}
