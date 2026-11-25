pub mod bar;
pub mod config;
pub mod error;
pub mod lua;
pub mod theme;

pub use bar::BarProperties;
pub use config::{
    BecaPaths, Config, CoreConfig, DEFAULT_CONFIG_TOML, DEFAULT_THEME_TOML, ModulesConfig,
    Resolved, ShellConfig, VoiceMode, VoiceModuleConfig, VoiceUiConfig, apply_env, ensure_defaults,
    ensure_defaults_with_paths, load, load_config, load_config_from, load_with_paths,
};
pub use error::BecaError;
pub use theme::{Theme, ThemeColors, ThemeMeta, ThemeMetrics, load_theme, load_theme_from};

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

    #[test]
    fn ensure_defaults_seeds_xdg_files() {
        let base = std::env::temp_dir().join(format!("beca-cfg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let paths = BecaPaths {
            config_file: base.join("beca/config.toml"),
            themes_dir: base.join("beca/themes"),
            scripts_dir: base.join("beca/scripts"),
            config_dir: base.join("beca"),
        };

        let created = ensure_defaults_with_paths(&paths, false).expect("seed defaults");
        assert_eq!(created.len(), 2, "config + default theme should be created");
        assert!(paths.scripts_dir.is_dir());

        let (cfg, src) = load_config_from(&paths.config_file);
        assert!(src.is_some(), "seeded config must parse");
        assert_eq!(cfg.core.theme, "default");

        let (theme, theme_src) = load_theme_from(&paths.theme_file("default"));
        assert!(theme_src.is_some(), "seeded theme must parse");
        assert_eq!(theme.meta.name, "default");

        let again = ensure_defaults_with_paths(&paths, false).expect("idempotent seed");
        assert!(again.is_empty(), "existing files must not be overwritten");

        let forced = ensure_defaults_with_paths(&paths, true).expect("forced seed");
        assert_eq!(forced.len(), 2);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn load_with_paths_auto_seeds_missing_files() {
        let base = std::env::temp_dir().join(format!("beca-cfg-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let paths = BecaPaths {
            config_file: base.join("beca/config.toml"),
            themes_dir: base.join("beca/themes"),
            scripts_dir: base.join("beca/scripts"),
            config_dir: base.join("beca"),
        };
        assert!(!paths.config_file.exists());

        let resolved = load_with_paths(paths.clone());
        assert!(
            paths.config_file.is_file(),
            "config.toml should be auto-seeded"
        );
        assert!(
            paths.theme_file("default").is_file(),
            "themes/default.toml should be auto-seeded"
        );
        assert_eq!(resolved.config.core.theme, "default");
        assert!(resolved.config_source.is_some());
        assert!(resolved.theme_source.is_some());

        let _ = std::fs::remove_dir_all(&base);
    }
}
