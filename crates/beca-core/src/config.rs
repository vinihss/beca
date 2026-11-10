use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tema::{Theme, load_theme};

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
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            voice: VoiceModuleConfig::default(),
            bar: BarModuleConfig::default(),
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
    /// Reserve space (exclusive zone/struts) so tiled windows don't overlap the bar.
    pub exclusive: bool,
    /// Section placement: "left" | "center" | "right"
    pub clock_section: String,
    pub launcher_section: String,
    /// Section alignment: "start" | "center" | "end"
    pub left_align: String,
    pub center_align: String,
    pub right_align: String,
    /// Opacidade do fundo da barra (0.0 = transparente, 1.0 = opaco).
    #[serde(alias = "opacity", alias = "bar_opacity", alias = "background-opacity")]
    pub background_opacity: f32,
    /// Exibir contorno/borda da barra.
    #[serde(alias = "border", alias = "show-border")]
    pub show_border: bool,
    /// Widgets de volume (saída/entrada)
    pub show_volume_out: bool,
    pub show_volume_in: bool,
    pub volume_out_section: String,
    pub volume_in_section: String,
    /// Widget de voz (abre `beca mod voice:show`)
    pub show_voice: bool,
    pub voice_label: String,
    pub voice_section: String,
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
            show_volume_out: true,
            show_volume_in: true,
            volume_out_section: "right".into(),
            volume_in_section: "right".into(),
            show_voice: true,
            voice_label: "Voice".into(),
            voice_section: "left".into(),
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
    /// Responder via LLM e falar via TTS após transcrição (fluxo assistente/kokoro).
    pub respond: bool,
    /// Comando para gerar resposta (recebe transcrição em $TRANSCRIPT; stdout = resposta).
    /// Se vazio, usa fluxo padrão (q-context_builder assist + q-rs) quando disponível.
    pub respond_command: String,
    /// Comando TTS para falar a resposta (recebe texto em $TEXT ou como argumento).
    /// Se vazio, usa voice-cli.sh api tts quando disponível.
    pub tts_command: String,
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
            status_recording: "Gravando...".into(),
            status_transcribing: "Transcrevendo áudio...".into(),
            status_responding: "Pensando resposta...".into(),
            status_speaking: "Falando resposta...".into(),
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

/// Apply environment overrides (ENV > file > defaults).
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

/// Load config + theme from XDG paths, apply env overrides. Caller applies CLI overrides.
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
