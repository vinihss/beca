//! Voice capture and speech-to-text (moved from beca-voice crate).

pub mod captura;
pub mod transcricao;
pub mod view;

pub use captura::{
    Args, CaptureOutcome, RespondConfig, VoiceTheme, VoiceUiTexts, apply_theme, run,
    run_capture_ui, run_capture_ui_with, run_configured,
};
pub use transcricao::{
    copy_to_clipboard, default_output_path, handle_transcript, notify, transcribe,
};
pub use view::VoiceView;
