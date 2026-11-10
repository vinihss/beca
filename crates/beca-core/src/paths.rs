use std::path::PathBuf;

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
