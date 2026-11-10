use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mlua::{Lua, Table, Value};
use serde_json::Value as JsonValue;
use slint::{ComponentHandle, Global};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use ui_lib::VoiceCaptureWindow;

const BEHAVIOR_LUAU: &str = include_str!("../scripts/voice_capture_behavior.luau");

#[derive(Debug, Clone)]
pub struct Args {
    /// Output WAV path (temp file used if omitted)
    pub output: Option<PathBuf>,
    /// Transcribe an existing audio file (no capture/UI)
    pub input: Option<PathBuf>,
    /// Run command with transcript as a single argument
    pub run: Option<String>,
    /// Keep UI open and show transcript after success (breaks q-voice pipe/exit contract)
    pub hold: bool,
}

#[derive(Clone, Debug)]
pub enum CaptureOutcome {
    Cancel,
    Sent(Result<String, String>),
}

#[derive(Clone, Debug)]
struct UiState {
    status_text: String,
    status_color: slint::Brush,
}

fn parse_hex_color(hex: &str) -> Option<slint::Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(slint::Color::from_rgb_u8(r, g, b))
}

struct Behavior {
    lua: Lua,
    module: Option<Table>,
}

impl Behavior {
    fn load() -> Self {
        let lua = Lua::new();
        let module = lua
            .load(BEHAVIOR_LUAU)
            .set_name("voice_capture_behavior.luau")
            .eval::<Table>()
            .ok();
        if module.is_none() {
            eprintln!("[voice-capture] luau behavior load failed; using defaults");
        } else {
            eprintln!("[voice-capture] luau behavior loaded");
        }
        Self { lua, module }
    }

    fn call_state(&self, func: &str, arg: Option<&str>, fallback: UiState) -> UiState {
        let Some(module) = &self.module else {
            return fallback;
        };
        let Ok(f): Result<mlua::Function, _> = module.get(func) else {
            return fallback;
        };
        let res: Result<Table, _> = match arg {
            Some(a) => f.call(a),
            None => f.call(()),
        };
        let Ok(t) = res else {
            return fallback;
        };
        let status_text: String = t
            .get::<Value>("status_text")
            .ok()
            .and_then(|v| self.lua.coerce_string(v).ok().flatten())
            .and_then(|s| s.to_str().ok().map(|s| s.to_string()))
            .unwrap_or(fallback.status_text);
        let status_color = t
            .get::<Value>("status_color")
            .ok()
            .and_then(|v| self.lua.coerce_string(v).ok().flatten())
            .and_then(|s| s.to_str().ok().map(|s| s.to_string()))
            .and_then(|h| parse_hex_color(&h))
            .map(slint::Brush::SolidColor)
            .unwrap_or(fallback.status_color);
        UiState {
            status_text,
            status_color,
        }
    }

    fn recording(&self) -> UiState {
        self.call_state(
            "on_recording",
            None,
            UiState {
                status_text: "Gravando...".into(),
                status_color: slint::Brush::SolidColor(slint::Color::from_rgb_u8(247, 118, 142)),
            },
        )
    }

    fn transcribing(&self) -> UiState {
        self.call_state(
            "on_transcribing",
            None,
            UiState {
                status_text: "Enviando para transcrição...".into(),
                status_color: slint::Brush::SolidColor(slint::Color::from_rgb_u8(122, 162, 247)),
            },
        )
    }

    fn success(&self, text: &str) -> UiState {
        self.call_state(
            "on_success",
            Some(text),
            UiState {
                status_text: "Transcrição concluída".into(),
                status_color: slint::Brush::SolidColor(slint::Color::from_rgb_u8(80, 180, 120)),
            },
        )
    }
}

fn apply_state(app: &VoiceCaptureWindow, st: &UiState) {
    app.set_status_text(st.status_text.clone().into());
    app.set_status_color(st.status_color.clone());
}

pub fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["Voice Capture", msg])
        .status();
}

pub fn copy_to_clipboard(text: &str) -> bool {
    for (cmd, args) in [
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if let Ok(mut child) = Command::new(cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }
    }
    false
}

pub fn default_output_path() -> PathBuf {
    let base = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/opencode"));
    let _ = fs::create_dir_all(&base);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    base.join(format!("voice-capture-{ts}.wav"))
}

fn write_wav(path: &Path, samples: &[f32], channels: u16, sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        let amp = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        if channels == 1 {
            writer.write_sample(amp).map_err(|e| e.to_string())?;
        } else {
            for _ in 0..channels {
                writer.write_sample(amp).map_err(|e| e.to_string())?;
            }
        }
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn transcribe(base_url: &str, audio_path: &Path) -> Result<String, String> {
    let bytes = fs::read(audio_path).map_err(|e| format!("read audio: {e}"))?;
    if bytes.is_empty() {
        return Err("empty audio file".to_string());
    }
    let url = format!("{}/transcribe", base_url.trim_end_matches('/'));
    eprintln!("[voice-capture] transcrevendo...");
    let resp = ureq::post(&url)
        .set("Content-Type", "audio/wav")
        .send(Cursor::new(bytes))
        .map_err(|e| format!("transcribe request failed: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("read response: {e}"))?;
    let json: JsonValue =
        serde_json::from_str(&body).map_err(|e| format!("parse response: {e}"))?;
    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(
            "transcrição vazia (áudio silencioso/curto). Fale mais alto ou mais longo.".to_string(),
        );
    }
    Ok(text)
}

fn pick_input_config(
    device: &cpal::Device,
) -> Result<(cpal::StreamConfig, cpal::SampleFormat), String> {
    let desired_rate = cpal::SampleRate(16_000);
    let mut candidates: Vec<cpal::SupportedStreamConfigRange> = device
        .supported_input_configs()
        .map_err(|e| format!("input configs: {e}"))?
        .filter(|r| {
            r.channels() == 1
                && r.min_sample_rate() <= desired_rate
                && desired_rate <= r.max_sample_rate()
        })
        .collect();
    candidates.sort_by_key(|r| match r.sample_format() {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::I16 => 1,
        cpal::SampleFormat::U16 => 2,
        _ => 3,
    });
    if let Some(range) = candidates.into_iter().next() {
        let supported = range.with_sample_rate(desired_rate);
        let fmt = supported.sample_format();
        return Ok((supported.into(), fmt));
    }
    let supported = device
        .default_input_config()
        .map_err(|e| format!("input config: {e}"))?;
    let fmt = supported.sample_format();
    Ok((supported.into(), fmt))
}

fn apply_beca_theme(app: &ui_lib::VoiceCaptureWindow) {
    let resolved = beca_core::load();
    let t = &resolved.theme;
    let g = ui_lib::Theme::get(app);
    let to_brush = |s: &str| -> slint::Brush {
        parse_hex_color(s)
            .map(slint::Brush::SolidColor)
            .unwrap_or_else(|| slint::Brush::SolidColor(slint::Color::from_rgb_u8(0, 0, 0)))
    };
    let to_color = |s: &str| -> slint::Color {
        parse_hex_color(s).unwrap_or_else(|| slint::Color::from_rgb_u8(0, 0, 0))
    };
    g.set_background(to_color(&t.colors.background));
    g.set_text_main(to_color(&t.colors.text_main));
    g.set_card_bg(to_color(&t.colors.card_bg));
    g.set_card_border(to_color(&t.colors.card_border));
    g.set_surface(to_brush(&t.colors.surface));
    g.set_surface_alt(to_brush(&t.colors.surface_alt));
    g.set_border(to_brush(&t.colors.border));
    g.set_text_strong(to_brush(&t.colors.text_strong));
    g.set_accent(to_brush(&t.colors.accent));
    g.set_accent_success(to_brush(&t.colors.accent_success));
    g.set_accent_danger(to_brush(&t.colors.accent_danger));
    g.set_recording(to_brush(&t.colors.recording));
    g.set_radius(t.metrics.radius as f32);
    g.set_dark_mode(t.meta.variant != "light");
}

pub fn run_capture_ui(
    out_path: &Path,
    base_url: &str,
    hold: bool,
) -> Result<CaptureOutcome, String> {
    let behavior = Behavior::load();
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no capture device (microphone not found)".to_string())?;
    let (config, sample_format) = pick_input_config(&device)?;
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;

    eprintln!(
        "[voice-capture] iniciando captura ({channels}ch {sample_rate}Hz, {sample_format:?})"
    );

    let recorded: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let recent: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0f32; 508]));
    let recording = Arc::new(AtomicBool::new(true));

    let recorded_cb = recorded.clone();
    let recent_cb = recent.clone();
    let recording_cb = recording.clone();

    let err_fn = move |err| eprintln!("[voice-capture] stream error: {err}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &config,
                move |data: &[f32], _| {
                    if !recording_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono: Vec<f32> = if channels <= 1 {
                        data.to_vec()
                    } else {
                        data.chunks(channels as usize)
                            .map(|c| c.iter().copied().sum::<f32>() / c.len() as f32)
                            .collect()
                    };
                    if let Ok(mut all) = recorded_cb.lock() {
                        all.extend_from_slice(&mono);
                    }
                    if let Ok(mut r) = recent_cb.lock() {
                        let n = r.len();
                        if mono.len() >= n {
                            r.copy_from_slice(&mono[mono.len() - n..]);
                        } else {
                            r.drain(0..mono.len());
                            r.extend_from_slice(&mono);
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &config,
                move |data: &[i16], _| {
                    if !recording_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono: Vec<f32> = if channels <= 1 {
                        data.iter().map(|&s| s as f32 / i16::MAX as f32).collect()
                    } else {
                        data.chunks(channels as usize)
                            .map(|c| {
                                c.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>()
                                    / c.len() as f32
                            })
                            .collect()
                    };
                    if let Ok(mut all) = recorded_cb.lock() {
                        all.extend_from_slice(&mono);
                    }
                    if let Ok(mut r) = recent_cb.lock() {
                        let n = r.len();
                        if mono.len() >= n {
                            r.copy_from_slice(&mono[mono.len() - n..]);
                        } else {
                            r.drain(0..mono.len());
                            r.extend_from_slice(&mono);
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                &config,
                move |data: &[u16], _| {
                    if !recording_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono: Vec<f32> = if channels <= 1 {
                        data.iter()
                            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect()
                    } else {
                        data.chunks(channels as usize)
                            .map(|c| {
                                c.iter()
                                    .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                    .sum::<f32>()
                                    / c.len() as f32
                            })
                            .collect()
                    };
                    if let Ok(mut all) = recorded_cb.lock() {
                        all.extend_from_slice(&mono);
                    }
                    if let Ok(mut r) = recent_cb.lock() {
                        let n = r.len();
                        if mono.len() >= n {
                            r.copy_from_slice(&mono[mono.len() - n..]);
                        } else {
                            r.drain(0..mono.len());
                            r.extend_from_slice(&mono);
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        _ => return Err(format!("unsupported sample format: {sample_format:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;

    let app = VoiceCaptureWindow::new().map_err(|e| e.to_string())?;
    apply_beca_theme(&app);
    apply_state(&app, &behavior.recording());
    let outcome: Arc<Mutex<Option<CaptureOutcome>>> = Arc::new(Mutex::new(None));
    let finishing = Arc::new(AtomicBool::new(false));

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let finishing = finishing.clone();
        let recording = recording.clone();
        let recorded = recorded.clone();
        let out_path = out_path.to_path_buf();
        let base_url = base_url.to_string();
        let transcribing_state = behavior.transcribing();
        let success_state = behavior.success("");
        app.on_send(move || {
            if finishing.swap(true, Ordering::SeqCst) {
                return;
            }
            recording.store(false, Ordering::Relaxed);
            let samples = recorded.lock().ok().map(|v| v.clone()).unwrap_or_default();
            let weak = weak.clone();
            let outcome = outcome.clone();
            let out_path = out_path.clone();
            let base_url = base_url.clone();
            let transcribing_state = transcribing_state.clone();
            let success_state = success_state.clone();
            if let Some(app) = weak.upgrade() {
                app.set_busy(true);
                apply_state(&app, &transcribing_state);
            }
            thread::spawn(move || {
                let result = (|| {
                    if samples.is_empty() {
                        return Err("empty audio capture".to_string());
                    }
                    write_wav(&out_path, &samples, 1, sample_rate)?;
                    if fs::metadata(&out_path)
                        .map(|m| m.len() == 0)
                        .unwrap_or(true)
                    {
                        return Err("empty audio file".to_string());
                    }
                    transcribe(&base_url, &out_path)
                })();
                if let Ok(mut o) = outcome.lock() {
                    *o = Some(CaptureOutcome::Sent(result.clone()));
                }
                let _ = weak.upgrade_in_event_loop(move |app| match result {
                    Ok(text) if hold => {
                        app.set_busy(false);
                        app.set_show_result(true);
                        app.set_transcript_text(text.into());
                        apply_state(&app, &success_state);
                    }
                    _ => {
                        let _ = slint::quit_event_loop();
                    }
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let finishing = finishing.clone();
        let recording = recording.clone();
        app.on_cancel(move || {
            if finishing.swap(true, Ordering::SeqCst) {
                return;
            }
            recording.store(false, Ordering::Relaxed);
            if let Ok(mut o) = outcome.lock() {
                *o = Some(CaptureOutcome::Cancel);
            }
            if let Some(app) = weak.upgrade() {
                let _ = app.hide();
            }
            let _ = slint::quit_event_loop();
        });
    }
    {
        let weak = app.as_weak();
        app.on_copy_text(move || {
            if let Some(app) = weak.upgrade() {
                let text = app.get_transcript_text().to_string();
                if copy_to_clipboard(&text) {
                    notify("📋 Transcrição copiada");
                } else {
                    notify("❌ Não foi possível copiar (wl-copy/xclip/xsel)");
                }
            }
        });
    }
    {
        let weak = app.as_weak();
        app.on_close_app(move || {
            if let Some(app) = weak.upgrade() {
                let _ = app.hide();
            }
            let _ = slint::quit_event_loop();
        });
    }

    let weak = app.as_weak();
    let recent_ui = recent.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(33),
        move || {
            if let Some(app) = weak.upgrade()
                && let Ok(buf) = recent_ui.lock()
            {
                let mut path = String::new();
                let w = 508.0f32;
                let h = 160.0f32;
                let mid = h / 2.0;
                for (i, &s) in buf.iter().enumerate() {
                    let x = (i as f32 / buf.len() as f32) * w;
                    let y = mid - (s * 68.0);
                    if i == 0 {
                        path.push_str(&format!("M {x} {y}"));
                    } else {
                        path.push_str(&format!(" L {x} {y}"));
                    }
                }
                app.set_audio_path_data(path.into());
            }
        },
    );

    app.run().map_err(|e| e.to_string())?;
    recording.store(false, Ordering::Relaxed);
    drop(stream);

    outcome
        .lock()
        .ok()
        .and_then(|o| o.clone())
        .ok_or_else(|| "capture ended without action".to_string())
}

pub fn handle_transcript(text: &str, run: Option<&str>, cleanup_path: Option<&Path>) {
    eprintln!("[voice-capture] transcript: {text}");
    println!("{text}");
    if let Some(cmd) = run {
        let status = Command::new(cmd).arg(text).status();
        if let Ok(s) = status {
            if !s.success() {
                eprintln!("[voice-capture] --run exited with {s}");
            }
        } else if let Err(e) = status {
            eprintln!("[voice-capture] --run failed: {e}");
        }
    }
    if let Some(p) = cleanup_path {
        let _ = fs::remove_file(p);
    }
    notify("📝 Transcrição concluída");
}

/// High-level entry: same behavior as the CLI binary.
pub fn run(args: Args) -> Result<(), String> {
    let base_url =
        std::env::var("VOICE_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:4100".to_string());

    if let Some(input) = args.input.clone() {
        match transcribe(&base_url, &input) {
            Ok(text) => handle_transcript(&text, args.run.as_deref(), None),
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(e);
            }
        }
        return Ok(());
    }

    let out = args.output.clone().unwrap_or_else(default_output_path);
    let cleanup = if args.output.is_none() {
        Some(out.as_path())
    } else {
        None
    };
    match run_capture_ui(&out, &base_url, args.hold) {
        Ok(CaptureOutcome::Cancel) => {
            let _ = fs::remove_file(&out);
            notify("❌ Captura cancelada");
            Ok(())
        }
        Ok(CaptureOutcome::Sent(Ok(text))) => {
            handle_transcript(&text, args.run.as_deref(), cleanup);
            Ok(())
        }
        Ok(CaptureOutcome::Sent(Err(e))) => {
            eprintln!("ERROR: {e}");
            if let Some(p) = cleanup {
                let _ = fs::remove_file(p);
            }
            Err(e)
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            Err(e)
        }
    }
}
