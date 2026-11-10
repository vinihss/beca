//! beca-core: config, theme, XDG paths, and load/merge logic.
//! Precedence: CLI (applied by caller) > ENV > user config/theme files > defaults.

mod config;
mod load;
mod paths;
mod theme;

pub use config::*;
pub use load::*;
pub use paths::*;
pub use theme::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_example_config() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schema/config.example.toml");
        let (cfg, src) = load_config_from(&p);
        assert!(src.is_some(), "example config should parse");
        assert_eq!(cfg.core.theme, "default");
        assert_eq!(cfg.modules.voice.daemon_url, "http://127.0.0.1:4100");
        assert_eq!(cfg.modules.voice.mode, VoiceMode::Pipe);
        assert_eq!(cfg.modules.voice.ui.title, "Voice Capture");
    }

    #[test]
    fn parse_example_theme() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/schema/theme.default.example.toml");
        let (t, src) = load_theme_from(&p);
        assert!(src.is_some(), "example theme should parse");
        assert_eq!(t.meta.name, "default");
        assert_eq!(t.meta.variant, "dark");
        assert_eq!(t.colors.background, "#1E1E1E");
        assert_eq!(t.metrics.radius, 6.0);
    }

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.core.theme, "default");
        assert!(cfg.modules.voice.enabled);
        assert_eq!(cfg.modules.voice.mode, VoiceMode::Pipe);
        let t = Theme::default();
        assert_eq!(t.colors.accent, "#007ACC");
    }
}
