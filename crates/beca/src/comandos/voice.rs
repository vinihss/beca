use anyhow::{Result, bail};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn run_voice_layer_shell_show(action: &str, args: &[String]) -> Result<()> {
    let force_window = std::env::var_os("BECA_VOICE_FORCE_WINDOW").is_some();
    #[cfg(target_os = "linux")]
    if !force_window && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use std::{collections::VecDeque, rc::Rc, sync::Mutex};

        if std::env::var_os("BECA_VOICE_COMPONENT").is_none() {
            let name = if action.starts_with("assistant") {
                "VoiceAssistantWindow"
            } else {
                "VoiceCaptureWindow"
            };
            unsafe { std::env::set_var("BECA_VOICE_COMPONENT", name) };
        }

        let queue = Arc::new(Mutex::new(VecDeque::<Rc<MinimalSoftwareWindow>>::new()));
        beca_layer_shell::install_platform(queue.clone());
        let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        queue.lock().unwrap().push_back(msw.clone());

        beca_ui::ui_loader::set_custom_event_loop(move || {
            beca_layer_shell::run_voice_layer_shell(msw.clone())
        });

        unsafe { std::env::set_var("BECA_VOICE_LAYER_SHELL", "1") };
        let r = run_voice_session(args);
        unsafe { std::env::remove_var("BECA_VOICE_LAYER_SHELL") };

        return match r {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                let fatal = msg.contains("flush")
                    || msg.contains("roundtrip")
                    || msg.contains("wayland connect")
                    || msg.contains("registry")
                    || msg.contains("wlr-layer-shell")
                    || msg.contains("wl_shm");
                eprintln!("[voice] layer-shell session failed: {msg}");
                if fatal {
                    if let Ok(exe) = std::env::current_exe() {
                        let target = format!("voice:{action}");
                        let mut cmd = std::process::Command::new(exe);
                        cmd.arg("mod").arg("--local").arg(target);
                        for a in args {
                            cmd.arg(a);
                        }
                        cmd.env("BECA_VOICE_FORCE_WINDOW", "1")
                            .env("BECA_NO_DAEMON", "1");
                        let status = cmd.status();
                        match status {
                            Ok(s) if s.success() => return Ok(()),
                            Ok(s) => {
                                anyhow::bail!("voice layer-shell fallback process exited with status {s}")
                            }
                            Err(err) => {
                                anyhow::bail!("failed to spawn voice layer-shell fallback: {err}")
                            }
                        }
                    }
                    anyhow::bail!("could not resolve current executable for fallback");
                }
                Err(e)
            }
        };
    }

    let component_name = if action.starts_with("assistant") {
        "VoiceAssistantWindow"
    } else {
        "VoiceCaptureWindow"
    };
    unsafe { std::env::set_var("BECA_VOICE_COMPONENT", component_name) };
    run_voice_session(args)
}

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

    let resolved = beca_core::config::load();
    eprintln!("[voice][timing] config+theme loaded in {:?}", t0.elapsed());
    let voice = &resolved.config.modules.voice;
    let theme = resolved.theme.voice_theme();
    let ui_texts = ui_texts_from_config(&voice.ui);
    let respond_cfg = beca_ui::voice::RespondConfig {
        respond_command: voice.respond_command.clone(),
        tts_command: voice.tts_command.clone(),
        respond_stream: voice.respond_stream,
        llm_url: voice.llm_url.clone(),
        llm_model: voice.llm_model.clone(),
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

fn run_quick_session(args: &[String]) -> Result<()> {
    let resolved = beca_core::config::load();
    let voice = &resolved.config.modules.voice;
    let theme = resolved.theme.voice_theme();
    let ui_texts = ui_texts_from_config(&voice.ui);
    let respond_cfg = beca_ui::voice::RespondConfig {
        respond_command: voice.respond_command.clone(),
        tts_command: voice.tts_command.clone(),
        respond_stream: voice.respond_stream,
        llm_url: voice.llm_url.clone(),
        llm_model: voice.llm_model.clone(),
    };
    let mut respond = voice.respond;
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--no-respond" | "--no-tts" => respond = false,
            "--respond" | "--assist" | "--tts" => respond = true,
            "-o" | "--output" => {
                if let Some(v) = it.next() {
                    out = Some(PathBuf::from(v));
                }
            }
            _ => {}
        }
    }
    let out_path = out.unwrap_or_else(beca_ui::voice::default_output_path);
    beca_ui::voice::quick::run_quick(
        &out_path,
        &voice.daemon_url,
        respond,
        respond_cfg,
        theme,
        ui_texts,
    )
    .map_err(|e| anyhow::anyhow!(e))
}

fn run_voice_quick(args: &[String]) -> Result<()> {
    let force_window = std::env::var_os("BECA_QUICK_FORCE_WINDOW").is_some();
    #[cfg(target_os = "linux")]
    if !force_window && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use std::{collections::VecDeque, rc::Rc, sync::Mutex};

        let queue = Arc::new(Mutex::new(VecDeque::<Rc<MinimalSoftwareWindow>>::new()));
        beca_layer_shell::install_platform(queue.clone());
        let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        queue.lock().unwrap().push_back(msw.clone());

        beca_ui::ui_loader::set_custom_event_loop(move || {
            beca_layer_shell::run_quick_layer_shell(msw.clone())
        });
        unsafe { std::env::set_var("BECA_VOICE_LAYER_SHELL", "1") };
        let r = run_quick_session(args);
        unsafe { std::env::remove_var("BECA_VOICE_LAYER_SHELL") };

        return match r {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                let fatal = msg.contains("flush")
                    || msg.contains("roundtrip")
                    || msg.contains("wayland connect")
                    || msg.contains("registry")
                    || msg.contains("wlr-layer-shell")
                    || msg.contains("wl_shm");
                eprintln!("[beca-quick] layer-shell session failed: {msg}");
                if fatal {
                    if let Ok(exe) = std::env::current_exe() {
                        let mut cmd = std::process::Command::new(exe);
                        cmd.arg("mod").arg("--local").arg("voice:quick");
                        for a in args {
                            cmd.arg(a);
                        }
                        cmd.env("BECA_QUICK_FORCE_WINDOW", "1")
                            .env("BECA_NO_DAEMON", "1");
                        let status = cmd.status();
                        match status {
                            Ok(s) if s.success() => return Ok(()),
                            Ok(s) => {
                                anyhow::bail!("quick layer-shell fallback process exited with status {s}")
                            }
                            Err(err) => {
                                anyhow::bail!("failed to spawn quick fallback: {err}")
                            }
                        }
                    }
                    anyhow::bail!("could not resolve current executable for fallback");
                }
                Err(e)
            }
        };
    }

    run_quick_session(args)
}

fn run_converse_session(_args: &[String]) -> Result<()> {
    let resolved = beca_core::config::load();
    let voice = &resolved.config.modules.voice;
    let theme = resolved.theme.voice_theme();
    let respond_cfg = beca_ui::voice::RespondConfig {
        respond_command: voice.respond_command.clone(),
        tts_command: voice.tts_command.clone(),
        respond_stream: voice.respond_stream,
        llm_url: voice.llm_url.clone(),
        llm_model: voice.llm_model.clone(),
    };
    let base_url = voice.daemon_url.clone();
    beca_ui::voice::run_conversation(&base_url, respond_cfg, theme).map_err(|e| anyhow::anyhow!(e))
}

fn run_voice_converse(args: &[String]) -> Result<()> {
    let force_window = std::env::var_os("BECA_CONVERSE_FORCE_WINDOW").is_some();
    #[cfg(target_os = "linux")]
    if !force_window && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use std::{collections::VecDeque, rc::Rc, sync::Mutex};

        let queue = Arc::new(Mutex::new(VecDeque::<Rc<MinimalSoftwareWindow>>::new()));
        beca_layer_shell::install_platform(queue.clone());
        let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        queue.lock().unwrap().push_back(msw.clone());

        beca_ui::ui_loader::set_custom_event_loop(move || {
            beca_layer_shell::run_orb_layer_shell(msw.clone())
        });
        unsafe { std::env::set_var("BECA_VOICE_LAYER_SHELL", "1") };
        let r = run_converse_session(args);
        unsafe { std::env::remove_var("BECA_VOICE_LAYER_SHELL") };

        return match r {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                let fatal = msg.contains("flush")
                    || msg.contains("roundtrip")
                    || msg.contains("wayland connect")
                    || msg.contains("registry")
                    || msg.contains("wlr-layer-shell")
                    || msg.contains("wl_shm");
                eprintln!("[beca-converse] layer-shell session failed: {msg}");
                if fatal {
                    if let Ok(exe) = std::env::current_exe() {
                        let mut cmd = std::process::Command::new(exe);
                        cmd.arg("mod").arg("--local").arg("voice:converse");
                        cmd.env("BECA_CONVERSE_FORCE_WINDOW", "1")
                            .env("BECA_NO_DAEMON", "1");
                        let status = cmd.status();
                        match status {
                            Ok(s) if s.success() => return Ok(()),
                            Ok(s) => {
                                anyhow::bail!("converse layer-shell fallback process exited with status {s}")
                            }
                            Err(err) => {
                                anyhow::bail!("failed to spawn converse fallback: {err}")
                            }
                        }
                    }
                    anyhow::bail!("could not resolve current executable for fallback");
                }
                Err(e)
            }
        };
    }

    run_converse_session(args)
}

pub fn serve() -> Result<()> {
    let path = voice_ctl_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("failed to bind voice ctl socket {}: {e}", path.display()))?;
    eprintln!("[beca voice] serving on {}", path.display());

    let mut ui_defs: std::collections::HashMap<String, slint_interpreter::ComponentDefinition> =
        std::collections::HashMap::new();

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

pub fn dispatch(action: &str, args: &[String]) -> Result<()> {
    match action {
        "serve" => serve(),
        "show" => run_voice_layer_shell_show("show", args),
        "assistant" | "show-assistant" => {

            unsafe { std::env::set_var("BECA_VOICE_COMPONENT", "VoiceAssistantWindow") };
            let r = run_voice_layer_shell_show(action, args);
            unsafe { std::env::remove_var("BECA_VOICE_COMPONENT") };
            r
        }
        "quick" => run_voice_quick(args),
        "converse" => run_voice_converse(args),
        "transcribe" => {
            let resolved = beca_core::config::load();
            let voice = &resolved.config.modules.voice;
            let theme = resolved.theme.voice_theme();
            let ui_texts = ui_texts_from_config(&voice.ui);
            let respond_cfg = beca_ui::voice::RespondConfig {
                respond_command: voice.respond_command.clone(),
                tts_command: voice.tts_command.clone(),
                respond_stream: voice.respond_stream,
                llm_url: voice.llm_url.clone(),
                llm_model: voice.llm_model.clone(),
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
