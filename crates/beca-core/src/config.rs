use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::theme::{Theme, load_theme};

#[derive(Debug, Clone)]
pub struct BecaPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub themes_dir: PathBuf,
    pub scripts_dir: PathBuf,
}

impl BecaPaths {
    pub fn resolve() -> Self {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        let config_dir = base.join("beca");
        Self {
            config_file: config_dir.join("config.toml"),
            themes_dir: config_dir.join("themes"),
            scripts_dir: config_dir.join("scripts"),
            config_dir,
        }
    }

    pub fn theme_file(&self, name: &str) -> PathBuf {
        let name = if name.ends_with(".toml") {
            name.to_string()
        } else {
            format!("{name}.toml")
        };
        self.themes_dir.join(name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub core: CoreConfig,
    pub shell: ShellConfig,
    pub modules: ModulesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            core: CoreConfig::default(),
            shell: ShellConfig::default(),
            modules: ModulesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreConfig {
    pub theme: String,
    pub log_level: String,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            log_level: "info".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    pub voice: VoiceModuleConfig,
    pub bar: BarModuleConfig,
    pub bar_bottom: Option<BarModuleConfig>,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            voice: VoiceModuleConfig::default(),
            bar: BarModuleConfig::default(),
            bar_bottom: None,
        }
    }
}

impl ModulesConfig {
    pub fn effective_bottom_bar(&self) -> BarModuleConfig {
        if let Some(mut b) = self.bar_bottom.clone() {
            if b.background.is_none() {
                b.background = self.bar.background.clone();
            }
            if (b.background_opacity - 1.0).abs() < f32::EPSILON && self.bar.background_opacity < 1.0 {
                b.background_opacity = self.bar.background_opacity;
            }
            b
        } else {
            let mut b = BarModuleConfig::default_bottom();
            b.background = self.bar.background.clone();
            b.background_opacity = self.bar.background_opacity;
            b.show_border = self.bar.show_border;
            b
        }
    }

    pub fn bottom_bar_enabled(&self) -> bool {
        self.bar.enabled && self.effective_bottom_bar().enabled
    }

    pub fn effective_top_bar(&self) -> BarModuleConfig {
        let mut c = self.bar.clone();
        if self.bottom_bar_enabled() {
            c.show_volume_out = false;
            c.show_volume_in = false;
            c.show_launcher = false;
        }
        c
    }

    pub fn bar_config_for(&self, position: &str) -> BarModuleConfig {
        if position == "bottom" {
            let mut c = self.effective_bottom_bar();
            if self.bottom_bar_enabled() && self.bar_bottom.as_ref().is_none() {
                c.show_launcher = true;
                c.launcher_label = self.bar.launcher_label.clone();
                c.launcher_section = "left".into();
            }
            c
        } else {
            self.effective_top_bar()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BarModuleConfig {
    pub enabled: bool,
    pub clock_format: String,
    pub show_clock: bool,
    pub show_launcher: bool,
    pub launcher_label: String,
    pub height: f32,

    pub exclusive: bool,

    pub clock_section: String,
    pub launcher_section: String,

    pub left_align: String,
    pub center_align: String,
    pub right_align: String,

    #[serde(alias = "opacity", alias = "bar_opacity", alias = "background-opacity")]
    pub background_opacity: f32,

    #[serde(alias = "border", alias = "show-border")]
    pub show_border: bool,

    #[serde(alias = "background-color", alias = "background_color", alias = "bar-background")]
    pub background: Option<String>,

    pub show_volume_out: bool,
    pub show_volume_in: bool,
    pub volume_out_section: String,
    pub volume_in_section: String,

    pub show_voice: bool,
    pub voice_label: String,
    pub voice_section: String,

    pub left: Option<Vec<String>>,
    pub center: Option<Vec<String>>,
    pub right: Option<Vec<String>>,
}

impl Default for BarModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            clock_format: "%H:%M".into(),
            show_clock: true,
            show_launcher: true,
            launcher_label: "Apps".into(),
            height: 34.0,
            exclusive: true,
            clock_section: "center".into(),
            launcher_section: "left".into(),
            left_align: "start".into(),
            center_align: "center".into(),
            right_align: "end".into(),
            background_opacity: 1.0,
            show_border: false,
            background: None,
            show_volume_out: true,
            show_volume_in: true,
            volume_out_section: "right".into(),
            volume_in_section: "right".into(),
            show_voice: true,
            voice_label: "Voice".into(),
            voice_section: "center".into(),
            left: None,
            center: None,
            right: None,
        }
    }
}

impl BarModuleConfig {
    pub fn default_bottom() -> Self {
        Self {
            enabled: true,
            clock_format: "%H:%M".into(),
            show_clock: false,
            show_launcher: false,
            launcher_label: "Apps".into(),
            height: 34.0,
            exclusive: true,
            clock_section: "center".into(),
            launcher_section: "left".into(),
            left_align: "start".into(),
            center_align: "center".into(),
            right_align: "end".into(),
            background_opacity: 1.0,
            show_border: false,
            background: None,
            show_volume_out: true,
            show_volume_in: true,
            volume_out_section: "right".into(),
            volume_in_section: "right".into(),
            show_voice: false,
            voice_label: "Voice".into(),
            voice_section: "center".into(),
            left: None,
            center: None,
            right: None,
        }
    }

    pub fn resolved_layout(&self) -> (Vec<String>, Vec<String>, Vec<String>) {
        if self.left.is_some() || self.center.is_some() || self.right.is_some() {
            let norm = |items: &Option<Vec<String>>| -> Vec<String> {
                items
                    .as_ref()
                    .map(|v| v.iter().filter_map(|s| Self::normalize_widget(s)).collect())
                    .unwrap_or_default()
            };
            return (norm(&self.left), norm(&self.center), norm(&self.right));
        }
        let mut left = Vec::new();
        let mut center = Vec::new();
        let mut right = Vec::new();
        let mut push = |name: &str, show: bool, section: &str| {
            if !show {
                return;
            }
            match section {
                "left" => left.push(name.to_string()),
                "right" => right.push(name.to_string()),
                _ => center.push(name.to_string()),
            }
        };
        push("launcher", self.show_launcher, &self.launcher_section);
        push("clock", self.show_clock, &self.clock_section);
        push("conversation", self.show_voice, &self.voice_section);
        push("volume-out", self.show_volume_out, &self.volume_out_section);
        push("volume-in", self.show_volume_in, &self.volume_in_section);
        (left, center, right)
    }

    fn normalize_widget(name: &str) -> Option<String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "launcher" | "apps" | "app" => Some("launcher".into()),
            "clock" | "time" => Some("clock".into()),
            "conversation" | "voice-live" | "voicelive" | "live" | "voice" => Some("conversation".into()),
            "volume-out" | "volume_out" | "audio-out" | "output" | "sink" => Some("volume-out".into()),
            "volume-in" | "volume_in" | "audio-in" | "input" | "source" | "mic" => Some("volume-in".into()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VoiceMode {
    Pipe,
    Hold,
}

impl Default for VoiceMode {
    fn default() -> Self {
        VoiceMode::Pipe
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceModuleConfig {
    pub enabled: bool,
    pub daemon_url: String,
    pub mode: VoiceMode,
    pub prefer_sample_rate: u32,
    pub channels: u16,
    pub script: Option<String>,

    pub respond: bool,

    pub respond_command: String,

    pub tts_command: String,

    pub respond_stream: bool,

    pub llm_url: String,

    pub llm_model: String,
    pub ui: VoiceUiConfig,
}

impl Default for VoiceModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            daemon_url: "http://127.0.0.1:4100".into(),
            mode: VoiceMode::Pipe,
            prefer_sample_rate: 16000,
            channels: 1,
            script: None,
            respond: true,
            respond_command: String::new(),
            tts_command: String::new(),
            respond_stream: false,
            llm_url: "http://127.0.0.1:8787".into(),
            llm_model: "openrouter/openrouter/free".into(),
            ui: VoiceUiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceUiConfig {
    pub title: String,
    pub status_recording: String,
    pub status_transcribing: String,
    pub status_responding: String,
    pub status_speaking: String,
}

impl Default for VoiceUiConfig {
    fn default() -> Self {
        Self {
            title: "Voice Capture".into(),
            status_recording: "Recording...".into(),
            status_transcribing: "Transcribing...".into(),
            status_responding: "Thinking...".into(),
            status_speaking: "Speaking...".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub paths: BecaPaths,
    pub config: Config,
    pub theme: Theme,
    pub config_source: Option<PathBuf>,
    pub theme_source: Option<PathBuf>,
}

pub fn load_config(paths: &BecaPaths) -> (Config, Option<PathBuf>) {
    load_config_from(&paths.config_file)
}

pub fn load_config_from(path: &Path) -> (Config, Option<PathBuf>) {
    match fs::read_to_string(path) {
        Ok(s) => match toml::from_str::<Config>(&s) {
            Ok(cfg) => (cfg, Some(path.to_path_buf())),
            Err(_) => (Config::default(), None),
        },
        Err(_) => (Config::default(), None),
    }
}

pub fn apply_env(cfg: &mut Config) {
    if let Ok(v) = std::env::var("VOICE_DAEMON_URL") {
        if !v.is_empty() {
            cfg.modules.voice.daemon_url = v;
        }
    }
    if let Ok(v) = std::env::var("BECA_THEME") {
        if !v.is_empty() {
            cfg.core.theme = v;
        }
    }
    if let Ok(v) = std::env::var("BECA_LOG_LEVEL") {
        if !v.is_empty() {
            cfg.core.log_level = v;
        }
    }
}

pub fn load() -> Resolved {
    let paths = BecaPaths::resolve();
    load_with_paths(paths)
}

pub fn load_with_paths(paths: BecaPaths) -> Resolved {
    let (mut config, config_source) = load_config(&paths);
    apply_env(&mut config);
    let (theme, theme_source) = load_theme(&paths, &config.core.theme);
    Resolved {
        paths,
        config,
        theme,
        config_source,
        theme_source,
    }
}
