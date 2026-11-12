pub mod capture;
pub mod quick;
pub mod streaming;
pub mod transcribe;
pub mod view;

pub use capture::{
    Args, CaptureOutcome, RespondConfig, VoiceTheme, VoiceUiTexts, apply_theme, run,
    run_capture_ui, run_capture_ui_with, run_configured,
};
pub use transcribe::{
    copy_to_clipboard, default_output_path, handle_transcript, notify, transcribe,
};
pub use view::VoiceView;
