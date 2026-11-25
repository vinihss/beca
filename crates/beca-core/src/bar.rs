use crate::config::BarModuleConfig;

/// Pure, resolved widget layout shared by both bar renderers.
///
/// Turning the configured widget lists into a single query object keeps the
/// layer-shell surface and the window fallback in sync without duplicating
/// section lookup rules.
#[derive(Debug, Clone, Default)]
pub struct BarProperties {
    begin: Vec<String>,
    center: Vec<String>,
    end: Vec<String>,
}

impl BarProperties {
    pub fn from_config(config: &BarModuleConfig) -> Self {
        let (begin, center, end) = config.resolved_layout();
        Self { begin, center, end }
    }

    /// Returns the section containing `widget`, if any.
    ///
    /// The lookup order (begin, end, center) matches the historical behavior
    /// of both renderers when a widget appears in more than one section.
    pub fn section(&self, widget: &str) -> Option<&'static str> {
        if self.begin.iter().any(|name| name == widget) {
            Some("begin")
        } else if self.end.iter().any(|name| name == widget) {
            Some("end")
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
            begin: vec!["clock".into(), "launcher".into()],
            center: vec!["volume-out".into()],
            end: vec!["clock".into(), "network".into()],
        }
    }

    #[test]
    fn section_prefers_begin_over_end() {
        assert_eq!(properties().section("clock"), Some("begin"));
    }

    #[test]
    fn section_finds_center_and_end_widgets() {
        let props = properties();
        assert_eq!(props.section("volume-out"), Some("center"));
        assert_eq!(props.section("network"), Some("end"));
    }

    #[test]
    fn missing_widget_has_no_section() {
        let props = properties();
        assert_eq!(props.section("cpu"), None);
        assert!(!props.visible("cpu"));
    }
}
