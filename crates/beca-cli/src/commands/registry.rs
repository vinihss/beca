/// Descriptions used by `beca mod list`.
pub fn list_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bar", "Status bar (top/bottom) with widgets and layout"),
        ("launcher", "Application launcher overlay"),
        ("voice", "Voice capture, quick prompt, conversation and assistant"),
    ]
}

/// Help text used by `beca mod <module>:info`.
pub fn module_info(name: &str) -> Option<String> {
    match name {
        "bar" => Some(
            "module: bar\n\
             description: Status bar (top/bottom) with configurable sections and widgets\n\
             actions:\n\
               show [top|bottom]   Show bar (default: top)\n\
               show-top            Show top bar\n\
               show-bottom         Show bottom bar\n\
               hide [top|bottom]   Hide bar (default: top)\n\
               hide-top            Hide top bar\n\
               hide-bottom         Hide bottom bar\n\
               status              Show bar status\n\
               info                Show this info"
                .to_string(),
        ),
        "launcher" | "apps" => Some(
            "module: launcher (alias: apps)\n\
             description: Application launcher overlay\n\
             actions:\n\
               show                Show launcher\n\
               serve               Run launcher resident server\n\
               hide                Hide launcher\n\
               stop|quit           Stop launcher\n\
               info                Show this info"
                .to_string(),
        ),
        "voice" => Some(
            "module: voice\n\
             description: Voice capture, quick prompt, conversation and assistant\n\
             actions:\n\
               show                Open voice capture UI\n\
               assistant|show-assistant  Open voice assistant UI\n\
               quick               Quick voice prompt (orb)\n\
               converse            Live conversation (VAD/STT/LLM/TTS)\n\
               serve               Run voice resident server\n\
               transcribe -i <wav> Transcribe audio file\n\
               info                Show this info\n\
             flags: --hold, --respond/--assist/--tts, --no-respond, -i/--input, -o/--output, --run"
                .to_string(),
        ),
        _ => None,
    }
}
