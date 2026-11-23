use crate::config::BarModuleConfig;

/// Pure, resolved widget layout shared by both bar renderers.
///
/// Turning the configured widget lists into a single query object keeps the
/// layer-shell surface and the window fallback in sync without duplicating
/// section lookup rules.
#[derive(Debug, Clone, Default)]
pub struct BarProperties {
    left: Vec<String>,
    center: Vec<String>,
    right: Vec<String>,
}

impl BarProperties {
    pub fn from_config(config: &BarModuleConfig) -> Self {
        let (left, center, right) = config.resolved_layout();
        Self {
            left,
            center,
            right,
        }
    }

    /// Returns the section containing `widget`, if any.
    ///
    /// The lookup order (left, right, center) matches the historical behavior
    /// of both renderers when a widget appears in more than one section.
    pub fn section(&self, widget: &str) -> Option<&'static str> {
        if self.left.iter().any(|name| name == widget) {
            Some("left")
        } else if self.right.iter().any(|name| name == widget) {
            Some("right")
        } else if self.center.iter().any(|name| name == widget) {
            Some("center")
        } else {
            None
        }
    }

    pub fn visible(&self, widget: &str) -> bool {
        self.section(widget).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties() -> BarProperties {
        BarProperties {
            left: vec!["clock".into(), "launcher".into()],
            center: vec!["volume-out".into()],
            right: vec!["clock".into(), "network".into()],
        }
    }

    #[test]
    fn section_prefers_left_over_right() {
        assert_eq!(properties().section("clock"), Some("left"));
    }

    #[test]
    fn section_finds_center_and_right_widgets() {
        let props = properties();
        assert_eq!(props.section("volume-out"), Some("center"));
        assert_eq!(props.section("network"), Some("right"));
    }

    #[test]
    fn missing_widget_has_no_section() {
        let props = properties();
        assert_eq!(props.section("cpu"), None);
        assert!(!props.visible("cpu"));
    }
}
