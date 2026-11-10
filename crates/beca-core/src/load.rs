use crate::{BecaPaths, Config, Theme};
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn load_theme(paths: &BecaPaths, name: &str) -> (Theme, Option<PathBuf>) {
    let path = paths.theme_file(name);
    load_theme_from(&path)
}

pub fn load_theme_from(path: &Path) -> (Theme, Option<PathBuf>) {
    match fs::read_to_string(path) {
        Ok(s) => match toml::from_str::<Theme>(&s) {
            Ok(t) => (t, Some(path.to_path_buf())),
            Err(_) => (Theme::default(), None),
        },
        Err(_) => (Theme::default(), None),
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
