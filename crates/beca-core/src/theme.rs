use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub meta: ThemeMeta,
    pub colors: ThemeColors,
    pub metrics: ThemeMetrics,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            meta: ThemeMeta::default(),
            colors: ThemeColors::default(),
            metrics: ThemeMetrics::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeMeta {
    pub name: String,
    pub variant: String,
}

impl Default for ThemeMeta {
    fn default() -> Self {
        Self {
            name: "default".into(),
            variant: "dark".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    pub background: String,
    pub text_main: String,
    pub text_strong: String,
    pub card_bg: String,
    pub card_border: String,
    pub surface: String,
    pub surface_alt: String,
    pub border: String,
    pub accent: String,
    pub accent_success: String,
    pub accent_danger: String,
    pub recording: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            background: "#1E1E1E".into(),
            text_main: "#FFFFFF".into(),
            text_strong: "#FFFFFF".into(),
            card_bg: "#2D2D2D".into(),
            card_border: "#3F3F3F".into(),
            surface: "#2D2D2D".into(),
            surface_alt: "#131522".into(),
            border: "#3F3F3F".into(),
            accent: "#007ACC".into(),
            accent_success: "#2f6f4f".into(),
            accent_danger: "#a04040".into(),
            recording: "#f7768e".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeMetrics {
    pub radius: f32,
}

impl Default for ThemeMetrics {
    fn default() -> Self {
        Self { radius: 6.0 }
    }
}
