use slint::{Brush, Color};
use slint_interpreter::{ComponentInstance, Value};

use crate::voice::VoiceTheme;

pub fn parse_color(hex: &str) -> Option<Color> {
    let h = hex.trim().trim_start_matches('#');
    match h.len() {
        6 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some(Color::from_rgb_u8(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            let a = u8::from_str_radix(&h[6..8], 16).ok()?;
            Some(Color::from_argb_u8(a, r, g, b))
        }
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeTokens {
    pub dark_mode: bool,
    pub radius: f32,
    pub background: Color,
    pub text_main: Color,
    pub card_bg: Color,
    pub card_border: Color,
    pub surface: Brush,
    pub surface_alt: Brush,
    pub border: Brush,
    pub text_strong: Brush,
    pub accent: Brush,
    pub accent_success: Brush,
    pub accent_danger: Brush,
    pub recording: Brush,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        let c = |hex: &str| parse_color(hex).expect("valid default color");
        Self {
            dark_mode: true,
            radius: 12.0,
            background: c("#141218"),
            text_main: c("#E6E0E9"),
            card_bg: c("#211F26"),
            card_border: c("#49454F"),
            surface: Brush::SolidColor(c("#211F26")),
            surface_alt: Brush::SolidColor(c("#2B2930")),
            border: Brush::SolidColor(c("#49454F")),
            text_strong: Brush::SolidColor(c("#E6E0E9")),
            accent: Brush::SolidColor(c("#D0BCFF")),
            accent_success: Brush::SolidColor(c("#4CAF50")),
            accent_danger: Brush::SolidColor(c("#F2B8B5")),
            recording: Brush::SolidColor(c("#F2B8B5")),
        }
    }
}

impl ThemeTokens {

    pub fn from_voice_theme(t: &VoiceTheme) -> Self {
        let d = Self::default();
        let color = |hex: &str, fallback: Color| parse_color(hex).unwrap_or(fallback);
        let brush = |hex: &str, fallback: &Brush| {
            parse_color(hex)
                .map(Brush::SolidColor)
                .unwrap_or_else(|| fallback.clone())
        };
        Self {
            dark_mode: t.dark_mode,
            radius: t.radius,
            background: color(&t.background, d.background),
            text_main: color(&t.text_main, d.text_main),
            card_bg: color(&t.card_bg, d.card_bg),
            card_border: color(&t.card_border, d.card_border),
            surface: brush(&t.surface, &d.surface),
            surface_alt: brush(&t.surface_alt, &d.surface_alt),
            border: brush(&t.border, &d.border),
            text_strong: brush(&t.text_strong, &d.text_strong),
            accent: brush(&t.accent, &d.accent),
            accent_success: brush(&t.accent_success, &d.accent_success),
            accent_danger: brush(&t.accent_danger, &d.accent_danger),
            recording: brush(&t.recording, &d.recording),
        }
    }

    pub fn apply_typed(&self, g: &crate::Theme) {
        g.set_dark_mode(self.dark_mode);
        g.set_radius(self.radius);
        g.set_background(self.background);
        g.set_text_main(self.text_main);
        g.set_card_bg(self.card_bg);
        g.set_card_border(self.card_border);
        g.set_surface(self.surface.clone());
        g.set_surface_alt(self.surface_alt.clone());
        g.set_border(self.border.clone());
        g.set_text_strong(self.text_strong.clone());
        g.set_accent(self.accent.clone());
        g.set_accent_success(self.accent_success.clone());
        g.set_accent_danger(self.accent_danger.clone());
        g.set_recording(self.recording.clone());
    }

    pub fn apply_interpreter(&self, app: &ComponentInstance) {
        let set = |name: &str, value: Value| {
            let _ = app.set_global_property("Theme", name, value);
        };
        set("dark-mode", Value::Bool(self.dark_mode));
        set("radius", Value::Number(self.radius as f64));
        set("background", Value::Brush(self.background.into()));
        set("text-main", Value::Brush(self.text_main.into()));
        set("card-bg", Value::Brush(self.card_bg.into()));
        set("card-border", Value::Brush(self.card_border.into()));
        set("surface", Value::Brush(self.surface.clone()));
        set("surface-alt", Value::Brush(self.surface_alt.clone()));
        set("border", Value::Brush(self.border.clone()));
        set("text-strong", Value::Brush(self.text_strong.clone()));
        set("accent", Value::Brush(self.accent.clone()));
        set("accent-success", Value::Brush(self.accent_success.clone()));
        set("accent-danger", Value::Brush(self.accent_danger.clone()));
        set("recording", Value::Brush(self.recording.clone()));
    }
}

pub fn apply_voice_theme_typed(g: &crate::Theme, t: &VoiceTheme) {
    ThemeTokens::from_voice_theme(t).apply_typed(g);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rgb_and_rgba() {
        let rgb = parse_color("#1E1E1E").expect("rgb");
        assert_eq!((rgb.red(), rgb.green(), rgb.blue(), rgb.alpha()), (30, 30, 30, 255));
        let rgba = parse_color("1E1E1EB8").expect("rgba");
        assert_eq!((rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()), (30, 30, 30, 184));
        assert!(parse_color("#12345").is_none());
        assert!(parse_color("not-a-color").is_none());
    }

    #[test]
    fn default_voice_theme_matches_default_tokens() {
        let from_voice = ThemeTokens::from_voice_theme(&crate::voice::VoiceTheme::default());
        assert_eq!(from_voice, ThemeTokens::default());
    }

    #[test]
    fn invalid_colors_fall_back_to_defaults() {
        let mut t = crate::voice::VoiceTheme::default();
        t.accent = "garbage".into();
        t.radius = 12.0;
        let tokens = ThemeTokens::from_voice_theme(&t);
        assert_eq!(tokens.accent, ThemeTokens::default().accent);
        assert_eq!(tokens.radius, 12.0);
    }
}
