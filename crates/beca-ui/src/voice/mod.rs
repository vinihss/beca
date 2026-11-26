pub mod capture;
pub mod conversation;
pub mod mic;
pub mod quick;
pub mod streaming;
pub mod transcribe;
pub mod view;

pub use capture::{
    Args, CaptureOutcome, RespondConfig, VoiceTheme, VoiceUiTexts, apply_theme, run,
    run_capture_ui, run_capture_ui_with, run_configured,
};
pub use conversation::run_conversation;
pub use mic::{MicCapture, RecordedAudio};
pub use transcribe::{
    copy_to_clipboard, default_output_path, handle_transcript, notify, play_start_sound,
    play_stop_sound, transcribe,
};
pub use view::VoiceView;
