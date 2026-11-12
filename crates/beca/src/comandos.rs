pub mod bar;
pub mod launcher;
pub mod runner;
pub mod voice;

use anyhow::Result;

pub fn dispatch(module: &str, action: &str, args: &[String]) -> Result<()> {
    match module {
        "launcher" | "apps" => match action {
            "show" => launcher::show(),
            "serve" => launcher::serve(),
            "hide" => launcher::send_control("hide"),
            "stop" | "quit" => launcher::send_control("quit"),
            other => {
                anyhow::bail!("unknown launcher action: {other} (expected: show, serve, hide, stop)")
            }
        },
        "bar" => bar::dispatch(action, args),
        "voice" => voice::dispatch(action, args),
        other => anyhow::bail!("unknown module: {other} (available: bar, launcher, voice)"),
    }
}

pub fn list_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bar", "Status bar (top/bottom) with widgets and layout"),
        ("launcher", "Application launcher overlay"),
        ("voice", "Voice capture, quick prompt, conversation and assistant"),
    ]
}

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
               info                Show this info".to_string(),
        ),
        "launcher" | "apps" => Some(
            "module: launcher (alias: apps)\n\
             description: Application launcher overlay\n\
             actions:\n\
               show                Show launcher\n\
               serve               Run launcher resident server\n\
               hide                Hide launcher\n\
               stop|quit           Stop launcher\n\
               info                Show this info".to_string(),
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
             flags: --hold, --respond/--assist/--tts, --no-respond, -i/--input, -o/--output, --run".to_string(),
        ),
        _ => None,
    }
}
