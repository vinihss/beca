use cpal::traits::{HostTrait, StreamTrait};
use slint_interpreter::{ComponentHandle, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::ui_loader;
use crate::voice::capture::{self, Behavior, RespondConfig, VoiceTheme, VoiceUiTexts};
use crate::voice::transcribe;
use crate::voice::view::{VoiceView, IDLE_WAVEFORM};

fn quick_ctl_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("quick.ctl")
    } else {
        std::env::temp_dir().join("beca-quick.ctl")
    }
}

pub fn run_quick(
    out_path: &Path,
    base_url: &str,
    respond: bool,
    respond_cfg: RespondConfig,
    theme: VoiceTheme,
    ui_texts: VoiceUiTexts,
) -> Result<(), String> {
    let t0 = std::time::Instant::now();

    {
        let base_url = base_url.to_string();
        std::thread::spawn(move || {
            let url = format!("{}/health", base_url.trim_end_matches('/'));
            let _ = ureq::get(&url).call();
            if respond {
                let _ = std::process::Command::new("bash")
                    .arg("-c")
                    .arg("command -v q-rs >/dev/null 2>&1 || true")
                    .status();
            }
        });
    }

    let behavior = Behavior::load_with(ui_texts);

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no capture device (microphone not found)".to_string())?;
    let (config, sample_format) = capture::pick_input_config(&device)?;
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;
    eprintln!("[beca-quick] audio: {channels}ch {sample_rate}Hz ({sample_format:?})");

    let recorded: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let recent: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0f32; 508]));
    let recording = Arc::new(AtomicBool::new(false));

    let stream = capture::build_input_stream(
        &device,
        &config,
        sample_format,
        recording.clone(),
        recorded.clone(),
        recent.clone(),
    )?;
    stream.play().map_err(|e| e.to_string())?;
    let _ = stream.pause();
    let stream = Rc::new(stream);

    let app = ui_loader::create("QuickVoiceWindow").map_err(|e| e.to_string())?;
    ui_loader::apply_theme_global(&app, &theme);
    VoiceView::new(&app).set_audio_path(IDLE_WAVEFORM);
    let st = behavior.idle();
    VoiceView::new(&app).set_status(&st.status_text, &st.status_color);
    eprintln!("[beca-quick][timing] ui ready in {:?}", t0.elapsed());

    let outcome: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let finishing = Arc::new(AtomicBool::new(false));
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let active_child = Arc::new(Mutex::new(None::<std::process::Child>));

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let finishing = finishing.clone();
        let recording = recording.clone();
        let recorded = recorded.clone();
        let recent = recent.clone();
        let out_path = out_path.to_path_buf();
        let base_url = base_url.to_string();
        let recording_state = behavior.recording();
        let transcribing_state = behavior.transcribing();
        let responding_state = behavior.responding();
        let speaking_state = behavior.speaking();
        let success_state = behavior.success("");
        let respond_cfg = respond_cfg.clone();
        let generation = generation.clone();
        let cancel_flag = cancel_flag.clone();
        let active_child = active_child.clone();
        let stream_ctl = stream.clone();
        let respond_default = respond;
        let _ = app.set_callback("send", move |_args| {
            if finishing.load(Ordering::SeqCst) {
                return Value::Void;
            }
            if !recording.load(Ordering::Relaxed) {
                if let Ok(mut all) = recorded.lock() { all.clear(); }
                if let Ok(mut r) = recent.lock() {
                    for v in r.iter_mut() { *v = 0.0; }
                }
                let _ = stream_ctl.play();
                recording.store(true, Ordering::Relaxed);
                if let Some(app) = weak.upgrade() {
                    VoiceView::new(&app).set_status(&recording_state.status_text, &recording_state.status_color);
                }
                return Value::Void;
            }

            if finishing.swap(true, Ordering::SeqCst) {
                return Value::Void;
            }
            recording.store(false, Ordering::Relaxed);
            let _ = stream_ctl.pause();
            let samples = recorded.lock().ok().map(|v| v.clone()).unwrap_or_default();
            let weak = weak.clone();
            let recent = recent.clone();
            let out_path = out_path.clone();
            let base_url = base_url.clone();
            let outcome = outcome.clone();
            let respond_cfg = respond_cfg.clone();
            let generation = generation.clone();
            let cancel_flag = cancel_flag.clone();
            let active_child = active_child.clone();
            let transcribing_state = transcribing_state.clone();
            let responding_state = responding_state.clone();
            let speaking_state = speaking_state.clone();
            let success_state = success_state.clone();
            cancel_flag.store(false, Ordering::SeqCst);
            let my_gen = generation.load(Ordering::SeqCst);
            if let Some(app) = weak.upgrade() {
                VoiceView::new(&app).set_status(&transcribing_state.status_text, &transcribing_state.status_color);
            }
            thread::spawn(move || {
                let is_stale = || generation.load(Ordering::SeqCst) != my_gen || cancel_flag.load(Ordering::Relaxed);
                if is_stale() { return; }
                let result = (|| {
                    if samples.is_empty() {
                        return Err("empty audio capture".to_string());
                    }
                    capture::write_wav(&out_path, &samples, 1, sample_rate)?;
                    transcribe::transcribe(&base_url, &out_path)
                })();

                match result {
                    Ok(text) if respond_default => {
                        let transcript = text.clone();
                        let st = responding_state.clone();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            VoiceView::new(&app).set_status(&st.status_text, &st.status_color);
                        });
                        let anim_run = Arc::new(AtomicBool::new(true));
                        let anim_flag = anim_run.clone();
                        let recent_anim = recent.clone();
                        let anim_handle = thread::spawn(move || capture::animate_waveform(&recent_anim, &anim_flag, false));

                        let resp = if respond_cfg.respond_stream {
                            capture::generate_response_streaming(&transcript, &respond_cfg, &base_url, &cancel_flag, &weak)
                        } else {
                            capture::generate_response(&transcript, &respond_cfg, &cancel_flag, &active_child)
                        };
                        anim_run.store(false, Ordering::Relaxed);
                        let _ = anim_handle.join();
                        if is_stale() { return; }
                        match resp {
                            Ok(answer) => {
                                if let Ok(mut o) = outcome.lock() {
                                    *o = Some(Ok(answer.clone()));
                                }
                                println!("{answer}");
                                let _ = std::io::Write::flush(&mut std::io::stdout());

                                if !respond_cfg.respond_stream {
                                    let st = speaking_state.clone();
                                    let _ = weak.upgrade_in_event_loop(move |app| {
                                        VoiceView::new(&app).set_status(&st.status_text, &st.status_color);
                                    });
                                    let _ = capture::speak_response(&answer, &respond_cfg, &cancel_flag, &active_child);
                                    if is_stale() { return; }
                                }
                                let st = success_state.clone();
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    VoiceView::new(&app).set_status(&st.status_text, &st.status_color);
                                });
                                thread::sleep(Duration::from_millis(1400));
                                if is_stale() { return; }
                                let _ = weak.upgrade_in_event_loop(|app| {
                                    let _ = app.set_property("reveal", Value::Bool(false));
                                });
                                thread::sleep(Duration::from_millis(280));
                                if is_stale() { return; }
                                let _ = weak.upgrade_in_event_loop(|app| { app.hide().ok(); });
                                ui_loader::quit_event_loop();
                            }
                            Err(e) => {
                                eprintln!("[beca-quick] respond failed: {e}");
                                if let Ok(mut o) = outcome.lock() {
                                    *o = Some(Err(e.clone()));
                                }
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    VoiceView::new(&app).set_status("Failed to respond", "#f7768e");
                                });
                                thread::sleep(Duration::from_millis(1600));
                                if is_stale() { return; }
                                let _ = weak.upgrade_in_event_loop(|app| {
                                    let _ = app.set_property("reveal", Value::Bool(false));
                                });
                                thread::sleep(Duration::from_millis(280));
                                if is_stale() { return; }
                                let _ = weak.upgrade_in_event_loop(|app| { app.hide().ok(); });
                                ui_loader::quit_event_loop();
                            }
                        }
                    }
                    Ok(text) => {
                        println!("{text}");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                        if let Ok(mut o) = outcome.lock() {
                            *o = Some(Ok(text.clone()));
                        }
                        let st = success_state.clone();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            VoiceView::new(&app).set_status(&st.status_text, &st.status_color);
                        });
                        thread::sleep(Duration::from_millis(1200));
                        if is_stale() { return; }
                        let _ = weak.upgrade_in_event_loop(|app| {
                            let _ = app.set_property("reveal", Value::Bool(false));
                        });
                        thread::sleep(Duration::from_millis(280));
                        if is_stale() { return; }
                        let _ = weak.upgrade_in_event_loop(|app| { app.hide().ok(); });
                        ui_loader::quit_event_loop();
                    }
                    Err(e) => {
                        eprintln!("[beca-quick] transcribe failed: {e}");
                        if let Ok(mut o) = outcome.lock() {
                            *o = Some(Err(e.clone()));
                        }
                        let weak2 = weak.clone();
                        let _ = weak2.upgrade_in_event_loop(move |app| {
                            VoiceView::new(&app).set_status("Error: transcription failed", "#f7768e");
                        });
                        thread::sleep(Duration::from_millis(1600));
                        if is_stale() { return; }
                        let _ = weak.upgrade_in_event_loop(|app| {
                            let _ = app.set_property("reveal", Value::Bool(false));
                        });
                        thread::sleep(Duration::from_millis(280));
                        if is_stale() { return; }
                        let _ = weak.upgrade_in_event_loop(|app| { app.hide().ok(); });
                        ui_loader::quit_event_loop();
                    }
                }
            });
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let recording = recording.clone();
        let cancel_flag = cancel_flag.clone();
        let generation = generation.clone();
        let active_child = active_child.clone();
        let _ = app.set_callback("quit-quick", move |_args| {
            generation.fetch_add(1, Ordering::SeqCst);
            cancel_flag.store(true, Ordering::SeqCst);
            recording.store(false, Ordering::Relaxed);
            if let Ok(mut o) = outcome.lock()
                && o.is_none()
            {
                *o = Some(Err("cancelled".to_string()));
            }
            if let Ok(mut slot) = active_child.lock()
                && let Some(mut c) = slot.take()
            {
                let _ = c.kill();
                let _ = c.wait();
            }

            if let Some(app) = weak.upgrade() {
                let _ = app.set_property("reveal", Value::Bool(false));
            }
            let weak_exit = weak.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(280));
                let _ = weak_exit.upgrade_in_event_loop(|app| { let _ = app.hide().ok(); });
                thread::sleep(Duration::from_millis(60));
                ui_loader::quit_event_loop();
            });
            Value::Void
        });
    }

    let ctl_running = Arc::new(AtomicBool::new(true));
    {
        let weak = app.as_weak();
        let ctl_running = ctl_running.clone();
        thread::spawn(move || {
            let path = quick_ctl_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::remove_file(&path);
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[beca-quick] ctl bind failed on {}: {e}", path.display());
                    return;
                }
            };
            if listener.set_nonblocking(true).is_err() {
                eprintln!("[beca-quick] ctl nonblocking failed");
                return;
            }
            eprintln!("[beca-quick] ctl listening on {}", path.display());
            while ctl_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let _ = conn.set_read_timeout(Some(Duration::from_millis(500)));
                        let mut line = String::new();
                        let mut reader = BufReader::new(&conn);
                        let _ = reader.read_line(&mut line);
                        let cmd = line.trim().to_string();
                        match cmd.as_str() {
                            "send" => {
                                let _ = (&conn).write_all(b"ok\n");
                                let weak2 = weak.clone();
                                let _ = weak2.upgrade_in_event_loop(|app| {
                                    let _ = app.invoke("send", &[]);
                                });
                            }
                            "cancel" | "quit" => {
                                let _ = (&conn).write_all(b"ok\n");
                                let weak2 = weak.clone();
                                let _ = weak2.upgrade_in_event_loop(|app| {
                                    let _ = app.invoke("quit-quick", &[]);
                                });
                            }
                            "status" => {
                                let _ = (&conn).write_all(b"ok\n");
                            }
                            other => {
                                let _ = (&conn).write_all(b"err\n");
                                eprintln!("[beca-quick] ctl: unknown command {other:?}");
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        eprintln!("[beca-quick] ctl accept error: {e}");
                        thread::sleep(Duration::from_millis(200));
                    }
                }
            }
            let _ = fs::remove_file(&path);
        });
    }

    let weak = app.as_weak();
    let recent_ui = recent.clone();
    let waveform_running = Arc::new(AtomicBool::new(true));
    let waveform_flag = waveform_running.clone();
    thread::spawn(move || {
        while waveform_flag.load(Ordering::Relaxed) {
            let path_str = if let Ok(buf) = recent_ui.lock() {
                let mut path = String::new();
                let w = 508.0f32;
                let h = 160.0f32;
                let mid = h / 2.0;
                for (i, &s) in buf.iter().enumerate() {
                    let x = (i as f32 / buf.len() as f32) * w;
                    let y = mid - (s * 115.0);
                    if i == 0 {
                        path.push_str(&format!("M {x} {y}"));
                    } else {
                        path.push_str(&format!(" L {x} {y}"));
                    }
                }
                path
            } else {
                String::new()
            };
            let weak2 = weak.clone();
            let _ = slint_interpreter::invoke_from_event_loop(move || {
                if let Some(app) = weak2.upgrade() {
                    VoiceView::new(&app).set_audio_path(&path_str);
                }
            });
            thread::sleep(Duration::from_millis(33));
        }
    });

    ui_loader::show(&app).map_err(|e| e.to_string())?;

    let weak_reveal = app.as_weak();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        let _ = weak_reveal.upgrade_in_event_loop(|app| {
            let _ = app.set_property("reveal", Value::Bool(true));
        });
    });
    let weak_start = app.as_weak();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        let _ = weak_start.upgrade_in_event_loop(|app| {
            let _ = app.invoke("send", &[]);
        });
    });

    ui_loader::run_event_loop()?;

    ctl_running.store(false, Ordering::Relaxed);
    let _ = fs::remove_file(quick_ctl_path());
    generation.fetch_add(1, Ordering::SeqCst);
    cancel_flag.store(true, Ordering::SeqCst);
    if let Ok(mut slot) = active_child.lock()
        && let Some(mut c) = slot.take()
    {
        let _ = c.kill();
        let _ = c.wait();
    }
    waveform_running.store(false, Ordering::Relaxed);
    recording.store(false, Ordering::Relaxed);
    drop(stream);

    Ok(())
}
