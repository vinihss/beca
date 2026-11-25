use beca_widgets::WidgetId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::theme::{load_theme, Theme};

pub const DEFAULT_CONFIG_TOML: &str = include_str!("../../../docs/schema/config.example.toml");
pub const DEFAULT_THEME_TOML: &str =
    include_str!("../../../docs/schema/theme.default.example.toml");

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

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.themes_dir)?;
        fs::create_dir_all(&self.scripts_dir)?;
        Ok(())
    }
}

fn seed_file(path: &Path, content: &str, force: bool) -> Result<bool, String> {
    if path.exists() && !force {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(true)
}

pub fn ensure_defaults(force: bool) -> Result<Vec<PathBuf>, String> {
    ensure_defaults_with_paths(&BecaPaths::resolve(), force)
}

pub fn ensure_defaults_with_paths(paths: &BecaPaths, force: bool) -> Result<Vec<PathBuf>, String> {
    paths
        .ensure_dirs()
        .map_err(|e| format!("create {}: {e}", paths.config_dir.display()))?;

    let mut created = Vec::new();
    let seeds = [
        (&paths.config_file, DEFAULT_CONFIG_TOML),
        (&paths.theme_file("default"), DEFAULT_THEME_TOML),
    ];
    for (path, content) in seeds {
        if seed_file(path, content, force)? {
            created.push(path.clone());
        }
    }
    Ok(created)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub core: CoreConfig,
    pub shell: ShellConfig,
    pub modules: ModulesConfig,
    pub bar: BarsConfig,
    pub widget: WidgetsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            core: CoreConfig::default(),
            shell: ShellConfig::default(),
            modules: ModulesConfig::default(),
            bar: BarsConfig::default(),
            widget: WidgetsConfig::default(),
        }
    }
}

impl Config {
    pub fn bar_config_for(&self, position: &str) -> BarModuleConfig {
        self.bar.config_for(position)
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
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            voice: VoiceModuleConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BarsConfig {
    pub top: Option<BarModuleConfig>,
    pub bottom: Option<BarModuleConfig>,
}

impl BarsConfig {
    /// Returns the effective configuration for `top` or `bottom`.
    ///
    /// Missing sections keep their historical widget defaults. A section that
    /// exists but omits all three lists is also filled with those defaults;
    /// once at least one list is present, omitted lists mean "empty".
    pub fn config_for(&self, position: &str) -> BarModuleConfig {
        let mut cfg = if position == "bottom" {
            self.bottom.clone()
        } else {
            self.top.clone()
        }
        .unwrap_or_default();

        if cfg.begin.is_none() && cfg.center.is_none() && cfg.end.is_none() {
            let (begin, center, end) = Self::default_layout(position);
            cfg.begin = Some(begin);
            cfg.center = Some(center);
            cfg.end = Some(end);
        }
        cfg
    }

    fn default_layout(position: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        if position == "bottom" {
            (
                vec!["launcher".into()],
                Vec::new(),
                vec!["volume-out".into(), "volume-in".into(), "network".into()],
            )
        } else {
            (
                vec!["cpu".into(), "memory".into(), "disk".into()],
                vec!["clock".into(), "conversation".into(), "alerts".into()],
                vec!["bluetooth".into()],
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BarModuleConfig {
    pub enabled: bool,
    pub height: f32,

    pub exclusive: bool,

    #[serde(alias = "opacity", alias = "bar_opacity", alias = "background-opacity")]
    pub background_opacity: f32,

    #[serde(alias = "border", alias = "show-border")]
    pub show_border: bool,

    #[serde(
        alias = "background-color",
        alias = "background_color",
        alias = "bar-background"
    )]
    pub background: Option<String>,

    pub begin_align: String,
    pub center_align: String,
    pub end_align: String,

    pub begin: Option<Vec<String>>,
    pub center: Option<Vec<String>>,
    pub end: Option<Vec<String>>,
}

impl Default for BarModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 34.0,
            exclusive: true,
            background_opacity: 1.0,
            show_border: false,
            background: None,
            begin_align: "start".into(),
            center_align: "center".into(),
            end_align: "end".into(),
            begin: None,
            center: None,
            end: None,
        }
    }
}

impl BarModuleConfig {
    pub fn resolved_layout(&self) -> (Vec<String>, Vec<String>, Vec<String>) {
        let norm = |items: &Option<Vec<String>>| -> Vec<String> {
            items
                .as_ref()
                .map(|v| v.iter().filter_map(|s| Self::normalize_widget(s)).collect())
                .unwrap_or_default()
        };
        (norm(&self.begin), norm(&self.center), norm(&self.end))
    }

    fn normalize_widget(name: &str) -> Option<String> {
        WidgetId::parse(name).map(|id| id.as_str().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetsConfig {
    pub clock: ClockWidgetConfig,
    pub launcher: LauncherWidgetConfig,
}

impl Default for WidgetsConfig {
    fn default() -> Self {
        Self {
            clock: ClockWidgetConfig::default(),
            launcher: LauncherWidgetConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockWidgetConfig {
    pub format: String,
}

impl Default for ClockWidgetConfig {
    fn default() -> Self {
        Self {
            format: "%H:%M".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherWidgetConfig {
    pub label: String,
}

impl Default for LauncherWidgetConfig {
    fn default() -> Self {
        Self {
            label: "Apps".into(),
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
    if let Err(e) = ensure_defaults_with_paths(&paths, false) {
        eprintln!("[beca][config] could not seed defaults: {e}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_sections_use_begin_center_end() {
        let cfg: Config = toml::from_str(
            r#"
[bar.top]
enabled = true
height = 40.0
background_opacity = 0.5
begin = ["cpu"]
center = ["clock"]
end = ["bluetooth"]

[bar.bottom]
enabled = false

[widget.clock]
format = "%H:%M:%S"

[widget.launcher]
label = ""
"#,
        )
        .expect("config should parse");

        let top = cfg.bar_config_for("top");
        assert!(top.enabled);
        assert_eq!(top.height, 40.0);
        assert_eq!(top.background_opacity, 0.5);
        let (begin, center, end) = top.resolved_layout();
        assert_eq!(begin, vec!["cpu".to_string()]);
        assert_eq!(center, vec!["clock".to_string()]);
        assert_eq!(end, vec!["bluetooth".to_string()]);

        let bottom = cfg.bar_config_for("bottom");
        assert!(!bottom.enabled);
        let (begin, center, end) = bottom.resolved_layout();
        assert_eq!(begin, vec!["launcher".to_string()]);
        assert!(center.is_empty());
        assert_eq!(
            end,
            vec![
                "volume-out".to_string(),
                "volume-in".to_string(),
                "network".to_string()
            ]
        );

        assert_eq!(cfg.widget.clock.format, "%H:%M:%S");
        assert_eq!(cfg.widget.launcher.label, "");
    }

    #[test]
    fn partial_bar_section_keeps_position_defaults() {
        let cfg: Config = toml::from_str(
            r#"
[bar.bottom]
background_opacity = 0.75
center = ["launcher"]
"#,
        )
        .expect("config should parse");

        let bottom = cfg.bar_config_for("bottom");
        assert_eq!(bottom.background_opacity, 0.75);
        let (begin, center, end) = bottom.resolved_layout();
        assert!(begin.is_empty());
        assert_eq!(center, vec!["launcher".to_string()]);
        assert!(end.is_empty());
    }

    #[test]
    fn omitted_sections_keep_historical_defaults() {
        let cfg = Config::default();
        let top = cfg.bar_config_for("top");
        let (begin, center, end) = top.resolved_layout();
        assert_eq!(
            begin,
            vec!["cpu".to_string(), "memory".to_string(), "disk".to_string()]
        );
        assert_eq!(
            center,
            vec![
                "clock".to_string(),
                "conversation".to_string(),
                "alerts".to_string()
            ]
        );
        assert_eq!(end, vec!["bluetooth".to_string()]);

        let bottom = cfg.bar_config_for("bottom");
        let (begin, center, end) = bottom.resolved_layout();
        assert_eq!(begin, vec!["launcher".to_string()]);
        assert!(center.is_empty());
        assert_eq!(
            end,
            vec![
                "volume-out".to_string(),
                "volume-in".to_string(),
                "network".to_string()
            ]
        );
        assert_eq!(cfg.widget.clock.format, "%H:%M");
        assert_eq!(cfg.widget.launcher.label, "Apps");
    }
}
