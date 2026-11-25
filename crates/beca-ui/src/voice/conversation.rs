use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use slint_interpreter::{ComponentHandle, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use crate::ui_loader;
use crate::voice::capture::{self, RespondConfig, VoiceTheme};
use crate::voice::transcribe;
use crate::voice::view::VoiceView;

const FRAME_SAMPLES: usize = 256;
const VAD_THRESHOLD: f32 = 0.6;
const ONSET_FRAMES: u32 = 3;
const OFFSET_FRAMES: u32 = 28;
const MAX_UTTERANCE_SECONDS: usize = 30;
const PRE_UTTERANCE_FRAMES: usize = 4;

fn converse_ctl_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("converse.ctl")
    } else {
        std::env::temp_dir().join("beca-converse.ctl")
    }
}

fn utterance_path() -> PathBuf {
    std::env::temp_dir().join("beca-converse-utterance.wav")
}

enum OrbState {
    Listening,
    Processing,
}

fn state_texts(state: &OrbState) -> (&'static str, &'static str) {
    match state {
        OrbState::Listening => ("Listening...", "#7aa2f7"),
        OrbState::Processing => ("Thinking...", "#e0af68"),
    }
}

fn set_level(weak: &slint_interpreter::Weak<slint_interpreter::ComponentInstance>, level: f32) {
    let weak = weak.clone();
    let level = level.clamp(0.0, 1.0);
    let _ = weak.upgrade_in_event_loop(move |app| {
        let _ = app.set_property("input-level", Value::Number(level as f64));
    });
}

fn set_transcript(weak: &slint_interpreter::Weak<slint_interpreter::ComponentInstance>, text: &str) {
    let weak = weak.clone();
    let text = text.to_string();
    let _ = weak.upgrade_in_event_loop(move |app| {
        let _ = app.set_property("transcript-text", Value::String(text.into()));
    });
}

fn set_status(weak: &slint_interpreter::Weak<slint_interpreter::ComponentInstance>, text: &str, color: &str) {
    let weak = weak.clone();
    let text = text.to_string();
    let color = color.to_string();
    let _ = weak.upgrade_in_event_loop(move |app| {
        VoiceView::new(&app).set_status(&text, &color);
    });
}

fn drain_channel(rx: &Receiver<Vec<f32>>) {
    while rx.try_recv().is_ok() {}
}

fn build_frame_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    tx: std::sync::mpsc::SyncSender<Vec<f32>>,
) -> Result<cpal::Stream, String> {
    fn norm_i16(v: i16) -> f32 {
        v as f32 / 32768.0
    }
    fn norm_u16(v: u16) -> f32 {
        (v as f32 - 32768.0) / 32768.0
    }
    fn mono_f32(data: &[f32], channels: usize) -> Vec<f32> {
        if channels <= 1 {
            return data.to_vec();
        }
        data.chunks(channels)
            .map(|c| c.iter().copied().sum::<f32>() / c.len() as f32)
            .collect()
    }
    fn mono_i16(data: &[i16], channels: usize) -> Vec<f32> {
        if channels <= 1 {
            return data.iter().map(|&s| norm_i16(s)).collect();
        }
        data.chunks(channels)
            .map(|c| c.iter().map(|&s| norm_i16(s)).sum::<f32>() / c.len() as f32)
            .collect()
    }
    fn mono_u16(data: &[u16], channels: usize) -> Vec<f32> {
        if channels <= 1 {
            return data.iter().map(|&s| norm_u16(s)).collect();
        }
        data.chunks(channels)
            .map(|c| c.iter().map(|&s| norm_u16(s)).sum::<f32>() / c.len() as f32)
            .collect()
    }
    let channels = config.channels as usize;
    match sample_format {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.try_send(mono_f32(data, channels));
                },
                |err| eprintln!("[beca-converse] input stream error: {err}"),
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.try_send(mono_i16(data, channels));
                },
                |err| eprintln!("[beca-converse] input stream error: {err}"),
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            device.build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.try_send(mono_u16(data, channels));
                },
                |err| eprintln!("[beca-converse] input stream error: {err}"),
                None,
            )
        }
        other => return Err(format!("unsupported sample format: {other:?}")),
    }
    .map_err(|e| e.to_string())
}

struct VadEngine {
    detector: earshot::Detector,
    pending: Vec<f32>,
    pre_ring: Vec<[f32; FRAME_SAMPLES]>,
    utterance: Vec<f32>,
    voiced_run: u32,
    silence_run: u32,
    in_speech: bool,
    max_utterance_samples: usize,
}

enum VadOutput {
    Continue,
    Endpoint(Vec<f32>),
}

impl VadEngine {
    fn new(sample_rate: u32) -> Self {
        Self {
            detector: earshot::Detector::new(earshot::DefaultPredictor::new()),
            pending: Vec::with_capacity(FRAME_SAMPLES * 2),
            pre_ring: Vec::with_capacity(PRE_UTTERANCE_FRAMES + 1),
            utterance: Vec::new(),
            voiced_run: 0,
            silence_run: 0,
            in_speech: false,
            max_utterance_samples: MAX_UTTERANCE_SECONDS * sample_rate as usize,
        }
    }

    fn reset_to_listening(&mut self) {
        self.detector.reset();
        self.pending.clear();
        self.pre_ring.clear();
        self.utterance.clear();
        self.voiced_run = 0;
        self.silence_run = 0;
        self.in_speech = false;
    }

    fn push_chunk(&mut self, chunk: &[f32]) -> VadOutput {
        self.pending.extend_from_slice(chunk);
        let mut endpoint: Option<Vec<f32>> = None;
        while self.pending.len() >= FRAME_SAMPLES {
            let frame: Vec<f32> = self.pending.drain(..FRAME_SAMPLES).collect();
            let mut arr = [0.0f32; FRAME_SAMPLES];
            arr.copy_from_slice(&frame);
            let score = self.detector.predict_f32(&arr);
            let voiced = score >= VAD_THRESHOLD;

            if !self.in_speech {
                if voiced {
                    self.voiced_run += 1;
                    if self.pre_ring.len() > PRE_UTTERANCE_FRAMES {
                        self.pre_ring.remove(0);
                    }
                    self.pre_ring.push(arr);
                    if self.voiced_run >= ONSET_FRAMES {
                        self.in_speech = true;
                        self.silence_run = 0;
                        self.utterance = self.pre_ring.concat();
                        self.pre_ring.clear();
                    }
                } else {
                    self.voiced_run = 0;
                    if self.pre_ring.len() > PRE_UTTERANCE_FRAMES {
                        self.pre_ring.remove(0);
                    }
                    self.pre_ring.push(arr);
                }
            } else {
                self.utterance.extend_from_slice(&arr);
                if voiced {
                    self.silence_run = 0;
                } else {
                    self.silence_run += 1;
                }
                if self.silence_run >= OFFSET_FRAMES || self.utterance.len() >= self.max_utterance_samples {
                    endpoint = Some(std::mem::take(&mut self.utterance));
                    self.voiced_run = 0;
                    self.silence_run = 0;
                    self.in_speech = false;
                    self.pre_ring.clear();
                    break;
                }
            }
        }
        match endpoint {
            Some(samples) => VadOutput::Endpoint(samples),
            None => VadOutput::Continue,
        }
    }
}

fn process_utterance(
    samples: &[f32],
    sample_rate: u32,
    base_url: &str,
    respond_cfg: &RespondConfig,
    cancel: &Arc<AtomicBool>,
    weak: &slint_interpreter::Weak<slint_interpreter::ComponentInstance>,
) {
    let path = utterance_path();
    if let Err(e) = capture::write_wav(&path, samples, 1, sample_rate) {
        eprintln!("[beca-converse] wav failed: {e}");
        return;
    }
    let t = Instant::now();
    let text = match transcribe::transcribe(base_url, &path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("[beca-converse] transcribe failed: {e}");
            set_status(weak, "Transcription failed", "#f7768e");
            return;
        }
    };
    eprintln!("[beca-converse][timing] stt {:?} chars={}", t.elapsed(), text.chars().count());
    let text = text.trim().to_string();
    if text.is_empty() {
        set_status(weak, "Listening...", "#7aa2f7");
        return;
    }
    set_transcript(weak, &text);
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let (thinking, thinking_color) = state_texts(&OrbState::Processing);
    set_status(weak, thinking, thinking_color);
    eprintln!("[beca-converse] you said: {text}");

    match capture::generate_response_streaming(&text, respond_cfg, base_url, cancel, weak) {
        Ok(answer) => {
            eprintln!("[beca-converse] answer: {} chars", answer.chars().count());
        }
        Err(e) => {
            eprintln!("[beca-converse] respond failed: {e}");
            set_status(weak, "Error: response failed", "#f7768e");
        }
    }
}

pub fn run_conversation(
    base_url: &str,
    respond_cfg: RespondConfig,
    theme: VoiceTheme,
) -> Result<(), String> {
    let t0 = std::time::Instant::now();

    {
        let base_url = base_url.to_string();
        std::thread::spawn(move || {
            let url = format!("{}/health", base_url.trim_end_matches('/'));
            let _ = ureq::get(&url).call();
        });
    }

    let device = {
        let host = cpal::default_host();
        host.default_input_device()
            .ok_or_else(|| "no capture device (microphone not found)".to_string())?
    };
    let (config, sample_format) = capture::pick_input_config(&device)?;
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;
    eprintln!("[beca-converse] audio: {channels}ch {sample_rate}Hz ({sample_format:?})");
    if sample_rate != 16_000 {
        eprintln!("[beca-converse] warning: device could not open 16 kHz; VAD timing tuned for 16 kHz degrades at {sample_rate}Hz");
    }

    let (tx, rx) = sync_channel::<Vec<f32>>(64);
    let stream = build_frame_stream(&device, &config, sample_format, tx)?;
    stream.play().map_err(|e| e.to_string())?;
    let _stream = Rc::new(stream);

    let app = ui_loader::create("ConversationOrb").map_err(|e| e.to_string())?;
    ui_loader::apply_theme_global(&app, &theme);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let weak_quit = app.as_weak();
    let stop_cb = stop_flag.clone();
    let _ = app.set_callback("quit-orb", move |_args| {
        stop_cb.store(true, Ordering::Relaxed);
        if let Some(app) = weak_quit.upgrade() {
            let _ = app.set_property("reveal", Value::Bool(false));
        }
        let weak_exit = weak_quit.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(260));
            let _ = weak_exit.upgrade_in_event_loop(|app| { let _ = app.hide().ok(); });
            thread::sleep(Duration::from_millis(60));
            ui_loader::quit_event_loop();
        });
        Value::Void
    });

    {
        let weak = app.as_weak();
        let stop = stop_flag.clone();
        let base_url = base_url.to_string();
        let respond_cfg = respond_cfg.clone();
        thread::spawn(move || {
            let mut vad = VadEngine::new(sample_rate);
            let mut last_level_ui = Instant::now() - Duration::from_millis(200);
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let chunk = match rx.recv_timeout(Duration::from_millis(120)) {
                    Ok(c) => c,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                if last_level_ui.elapsed() >= Duration::from_millis(70) {
                    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
                    set_level(&weak, (rms / 0.085).min(1.0));
                    last_level_ui = Instant::now();
                }
                match vad.push_chunk(&chunk) {
                    VadOutput::Continue => {}
                    VadOutput::Endpoint(samples) => {
                        let (listening, listening_color) = state_texts(&OrbState::Listening);
                        set_status(&weak, listening, listening_color);
                        if samples.len() < (sample_rate as usize) / 2 {
                            set_status(&weak, listening, listening_color);
                            continue;
                        }
                        set_status(&weak, "Transcribing...", "#7aa2f7");
                        process_utterance(&samples, sample_rate, &base_url, &respond_cfg, &stop, &weak);
                        drain_channel(&rx);
                        vad.reset_to_listening();
                        if !stop.load(Ordering::Relaxed) {
                            set_status(&weak, listening, listening_color);
                        }
                    }
                }
            }
        });
    }

    let ctl_running = Arc::new(AtomicBool::new(true));
    {
        let weak = app.as_weak();
        let ctl_running = ctl_running.clone();
        thread::spawn(move || {
            let path = converse_ctl_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::remove_file(&path);
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[beca-converse] ctl bind failed on {}: {e}", path.display());
                    return;
                }
            };
            if listener.set_nonblocking(true).is_err() {
                return;
            }
            eprintln!("[beca-converse] ctl listening on {}", path.display());
            while ctl_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let _ = conn.set_read_timeout(Some(Duration::from_millis(500)));
                        let mut line = String::new();
                        let mut reader = BufReader::new(&conn);
                        let _ = reader.read_line(&mut line);
                        match line.trim() {
                            "quit" => {
                                let _ = (&conn).write_all(b"ok\n");
                                let weak2 = weak.clone();
                                let _ = weak2.upgrade_in_event_loop(|app| {
                                    let _ = app.invoke("quit-orb", &[]);
                                });
                            }
                            "status" => {
                                let _ = (&conn).write_all(b"ok\n");
                            }
                            other => {
                                let _ = (&conn).write_all(b"err\n");
                                eprintln!("[beca-converse] ctl: unknown command {other:?}");
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        eprintln!("[beca-converse] ctl accept error: {e}");
                        thread::sleep(Duration::from_millis(200));
                    }
                }
            }
            let _ = fs::remove_file(&path);
        });
    }

    ui_loader::show(&app).map_err(|e| e.to_string())?;

    let weak_reveal = app.as_weak();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        let _ = weak_reveal.upgrade_in_event_loop(|app| {
            let _ = app.set_property("reveal", Value::Bool(true));
        });
    });

    eprintln!("[beca-converse][timing] session ready in {:?}", t0.elapsed());

    ui_loader::run_event_loop()?;

    ctl_running.store(false, Ordering::Relaxed);
    let _ = fs::remove_file(converse_ctl_path());
    stop_flag.store(true, Ordering::Relaxed);

    Ok(())
}
