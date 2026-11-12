use slint_interpreter::{ComponentInstance, Value};

use crate::theme::parse_color;

pub const IDLE_WAVEFORM: &str = "M 0 80 L 508 80";

pub const PROPERTIES: &[&str] = &[
    "status-text",
    "status-color",
    "busy",
    "is-recording",
    "show-result",
    "transcript-text",
    "response-text",
    "show-settings",
    "respond-enabled",
    "hold-enabled",
    "audio-path-data",
    "reveal",
];

pub struct VoiceView<'a> {
    app: &'a ComponentInstance,
}

impl<'a> VoiceView<'a> {
    pub fn new(app: &'a ComponentInstance) -> Self {
        Self { app }
    }

    fn set_bool(&self, name: &'static str, value: bool) {
        let _ = self.app.set_property(name, Value::Bool(value));
    }

    fn set_string(&self, name: &'static str, value: &str) {
        let _ = self.app.set_property(name, Value::String(value.into()));
    }

    pub fn set_status(&self, text: &str, color_hex: &str) {
        self.set_string("status-text", text);
        if let Some(c) = parse_color(color_hex) {
            let _ = self
                .app
                .set_property("status-color", Value::Brush(c.into()));
        }
    }

    pub fn idle(&self) {
        self.set_bool("busy", false);
        self.set_bool("is-recording", false);
    }

    pub fn start_recording(&self) {
        self.set_bool("is-recording", true);
        self.set_bool("busy", false);
    }

    pub fn begin_processing(&self) {
        self.set_bool("is-recording", false);
        self.set_bool("busy", true);
    }

    pub fn set_busy(&self, busy: bool) {
        self.set_bool("busy", busy);
    }

    pub fn show_result(&self, transcript: &str) {
        self.set_bool("busy", false);
        self.set_bool("show-result", true);
        self.set_string("transcript-text", transcript);
    }

    pub fn hide_result(&self) {
        self.set_bool("show-result", false);
    }

    pub fn begin_response(&self) {
        self.set_bool("show-result", false);
        self.set_bool("busy", true);
        self.set_string("response-text", "");
    }

    pub fn clear_result(&self) {
        self.set_bool("show-result", false);
        self.set_string("transcript-text", "");
        self.set_string("response-text", "");
    }

    pub fn set_response(&self, text: &str) {
        self.set_string("response-text", text);
    }

    pub fn clear_response(&self) {
        self.set_string("response-text", "");
    }

    pub fn reset(&self) {
        self.idle();
        self.clear_result();
        self.set_string("audio-path-data", IDLE_WAVEFORM);
    }

    pub fn set_audio_path(&self, data: &str) {
        self.set_string("audio-path-data", data);
    }

    pub fn set_reveal(&self, reveal: bool) {
        self.set_bool("reveal", reveal);
    }

    pub fn set_settings(&self, respond: bool, hold: bool) {
        self.set_bool("respond-enabled", respond);
        self.set_bool("hold-enabled", hold);
    }

    pub fn set_show_settings(&self, open: bool) {
        self.set_bool("show-settings", open);
    }

    pub fn respond_enabled(&self, default: bool) -> bool {
        match self.app.get_property("respond-enabled") {
            Ok(Value::Bool(v)) => v,
            _ => default,
        }
    }

    pub fn hold_enabled(&self, default: bool) -> bool {
        match self.app.get_property("hold-enabled") {
            Ok(Value::Bool(v)) => v,
            _ => default,
        }
    }

    pub fn transcript(&self) -> String {
        match self.app.get_property("transcript-text") {
            Ok(Value::String(s)) => s.to_string(),
            _ => String::new(),
        }
    }
}
