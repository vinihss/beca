use serde::{Deserialize, Serialize};

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
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            voice: VoiceModuleConfig::default(),
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
}

impl Default for VoiceUiConfig {
    fn default() -> Self {
        Self {
            title: "Voice Capture".into(),
            status_recording: "Gravando...".into(),
            status_transcribing: "Transcrevendo áudio...".into(),
        }
    }
}
