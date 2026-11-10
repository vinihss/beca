use anyhow::{Result, bail};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn ui_texts_from_config(ui: &beca_core::config::VoiceUiConfig) -> beca_ui::voice::VoiceUiTexts {
    let opt = |s: &str| -> Option<String> {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    beca_ui::voice::VoiceUiTexts {
        status_recording: opt(&ui.status_recording),
        status_transcribing: opt(&ui.status_transcribing),
        status_responding: opt(&ui.status_responding),
        status_speaking: opt(&ui.status_speaking),
    }
}

fn voice_ctl_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("voice.ctl")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("voice.ctl")
    }
}

fn run_voice_session(args: &[String]) -> Result<()> {
    let t0 = std::time::Instant::now();
    // Reload config/theme each session; UI compile cache auto-invalidates on file changes.
    let resolved = beca_core::config::load();
    eprintln!("[beca-voice][timing] config+theme loaded in {:?}", t0.elapsed());
    let voice = &resolved.config.modules.voice;
    let theme = resolved.theme.voice_theme();
    let ui_texts = ui_texts_from_config(&voice.ui);
    let respond_cfg = beca_ui::voice::RespondConfig {
        respond_command: voice.respond_command.clone(),
        tts_command: voice.tts_command.clone(),
    };

    let mut hold = voice.mode == beca_core::config::VoiceMode::Hold;
    let mut respond = voice.respond;
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut run_cmd: Option<String> = None;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hold" => hold = true,
            "--respond" | "--assist" | "--tts" => respond = true,
            "--no-respond" | "--no-tts" => respond = false,
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

    let vc_args = beca_ui::voice::Args {
        output,
        input,
        run: run_cmd,
        hold,
        respond,
    };
    beca_ui::voice::run_configured(
        vc_args,
        theme,
        ui_texts,
        Some(voice.daemon_url.clone()),
        respond_cfg,
    )
    .map_err(|e| anyhow::anyhow!(e))
}

/// Resident voice service: listens for `show` on ctl socket and runs capture sessions.
/// Keeps process warm and reuses compiled UI definitions across sessions.
pub fn serve() -> Result<()> {
    let path = voice_ctl_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("failed to bind voice ctl socket {}: {e}", path.display()))?;
    eprintln!("[beca voice] serving on {}", path.display());

    // Local cache of compiled UI defs (avoids recompilation across sessions).
    let mut ui_defs: std::collections::HashMap<String, slint_interpreter::ComponentDefinition> =
        std::collections::HashMap::new();

    // Pre-warm default voice UI.
    let t_warm = std::time::Instant::now();
    match beca_ui::ui_loader::compile_component("VoiceCaptureWindow") {
        Ok(def) => {
            ui_defs.insert("VoiceCaptureWindow".to_string(), def.clone());
            beca_ui::ui_loader::set_preloaded_component("VoiceCaptureWindow", def);
            eprintln!("[beca voice][timing] ui prewarm in {:?}", t_warm.elapsed());
        }
        Err(e) => eprintln!("[beca voice] ui prewarm failed: {e}"),
    }

    let running = Arc::new(AtomicBool::new(true));
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                });
                let mut writer = stream;
                let mut line = String::new();
                if reader.read_line(&mut line).ok().unwrap_or(0) == 0 {
                    continue;
                }
                let cmd = line.trim().to_string();
                let (component, resp): (&str, &str) = match cmd.as_str() {
                    "show" | "" => ("VoiceCaptureWindow", "ok"),
                    "show-assistant" | "assistant" => ("VoiceAssistantWindow", "ok"),
                    "quit" | "stop" => {
                        let _ = writeln!(writer, "ok");
                        let _ = writer.flush();
                        break;
                    }
                    "hide" => {
                        let _ = writeln!(writer, "ok");
                        let _ = writer.flush();
                        continue;
                    }
                    "ping" => {
                        let _ = writeln!(writer, "ok");
                        let _ = writer.flush();
                        continue;
                    }
                    _ => {
                        let _ = writeln!(writer, "unknown: {cmd}");
                        let _ = writer.flush();
                        continue;
                    }
                };

                let _ = writeln!(writer, "{resp}");
                let _ = writer.flush();

                // Ensure compiled def is available and pinned for this session.
                if !ui_defs.contains_key(component) {
                    match beca_ui::ui_loader::compile_component(component) {
                        Ok(def) => { ui_defs.insert(component.to_string(), def); }
                        Err(e) => {
                            eprintln!("[beca voice] compile {component} failed: {e}");
                        }
                    }
                }
                if let Some(def) = ui_defs.get(component).cloned() {
                    beca_ui::ui_loader::set_preloaded_component(component, def);
                    eprintln!("[beca voice] preloaded set for {component}");
                } else {
                    eprintln!("[beca voice] no preloaded def available for {component}");
                }
                unsafe { std::env::set_var("BECA_VOICE_COMPONENT", component) };
                if let Err(e) = run_voice_session(&[]) {
                    eprintln!("[beca voice] session error: {e}");
                }
                unsafe { std::env::remove_var("BECA_VOICE_COMPONENT") };
            }
            Err(e) => {
                eprintln!("[beca voice] accept error: {e}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    beca_ui::ui_loader::clear_preloaded_component();
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// `beca mod voice:show` / `voice:transcribe` / `voice:serve`
pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    match action {
        "serve" => serve(),
        "show" => run_voice_session(args),
        "assistant" | "show-assistant" => {
            // Safety: best-effort, single-use in CLI dispatch path.
            unsafe { std::env::set_var("BECA_VOICE_COMPONENT", "VoiceAssistantWindow") };
            let r = run_voice_session(args);
            unsafe { std::env::remove_var("BECA_VOICE_COMPONENT") };
            r
        }
        "transcribe" => {
            let resolved = beca_core::config::load();
            let voice = &resolved.config.modules.voice;
            let theme = resolved.theme.voice_theme();
            let ui_texts = ui_texts_from_config(&voice.ui);
            let respond_cfg = beca_ui::voice::RespondConfig {
                respond_command: voice.respond_command.clone(),
                tts_command: voice.tts_command.clone(),
            };
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
            let vc_args = beca_ui::voice::Args {
                output: None,
                input: Some(input),
                run: None,
                hold: false,
                respond: false,
            };
            beca_ui::voice::run_configured(
                vc_args,
                theme,
                ui_texts,
                Some(voice.daemon_url.clone()),
                respond_cfg,
            )
            .map_err(|e| anyhow::anyhow!(e))
        }
        other => bail!("unknown voice action: {other} (expected: show, serve, transcribe)"),
    }
}
