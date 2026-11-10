use anyhow::{Result, bail};
use beca_core::VoiceMode;
use std::path::PathBuf;

/// `beca mod voice:show` / `voice:transcribe`
/// Native module: uses voice-capture engine + beca-core config.
pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    let resolved = beca_core::load();
    let voice = &resolved.config.modules.voice;

    match action {
        "show" => {
            let mut hold = voice.mode == VoiceMode::Hold;
            let mut input: Option<PathBuf> = None;
            let mut output: Option<PathBuf> = None;
            let mut run_cmd: Option<String> = None;

            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--hold" => hold = true,
                    "-i" | "--input" => {
                        if let Some(v) = it.next() {
                            input = Some(PathBuf::from(v));
                        }
                    }
                    "-o" | "--output" => {
                        if let Some(v) = it.next() {
                            output = Some(PathBuf::from(v));
                        }
                    }
                    "--run" => {
                        if let Some(v) = it.next() {
                            run_cmd = Some(v.clone());
                        }
                    }
                    _ => {}
                }
            }

            // SAFETY: single-threaded at this point; voice engine reads it during run.
            unsafe { std::env::set_var("VOICE_DAEMON_URL", &voice.daemon_url) };
            let vc_args = voice_capture::Args {
                output,
                input,
                run: run_cmd,
                hold,
            };
            voice_capture::run(vc_args).map_err(|e| anyhow::anyhow!(e))
        }
        "transcribe" => {
            let mut input: Option<PathBuf> = None;
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-i" | "--input" => {
                        if let Some(v) = it.next() {
                            input = Some(PathBuf::from(v));
                        }
                    }
                    other if !other.starts_with('-') && input.is_none() => {
                        input = Some(PathBuf::from(other));
                    }
                    _ => {}
                }
            }
            let Some(input) = input else {
                bail!("voice:transcribe requires -i <wav>");
            };
            // SAFETY: single-threaded at this point; voice engine reads it during run.
            unsafe { std::env::set_var("VOICE_DAEMON_URL", &voice.daemon_url) };
            let vc_args = voice_capture::Args {
                output: None,
                input: Some(input),
                run: None,
                hold: false,
            };
            voice_capture::run(vc_args).map_err(|e| anyhow::anyhow!(e))
        }
        other => bail!("unknown voice action: {other} (expected: show, transcribe)"),
    }
}
