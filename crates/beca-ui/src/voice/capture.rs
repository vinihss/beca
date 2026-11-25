use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mlua::{Lua, Table, Value as LuaValue};
use slint_interpreter::{ComponentHandle, ComponentInstance, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::ui_loader;
use crate::voice::view::VoiceView;

const BEHAVIOR_LUAU: &str = include_str!("../../scripts/voice_behavior.luau");

fn scripts_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from(".config"));
    base.join("beca").join("scripts")
}

fn load_behavior_script() -> (String, String) {
    let dir = scripts_dir();
    let candidates = ["voice_behavior.lua", "voice_behavior.luau"];
    for name in candidates {
        let path = dir.join(name);
        if let Ok(code) = std::fs::read_to_string(&path) {
            eprintln!("[voice] behavior script: {}", path.display());
            return (code, path.to_string_lossy().to_string());
        }
    }

    let _ = std::fs::create_dir_all(&dir);
    let seed = dir.join("voice_behavior.lua");
    if !seed.exists() {
        let _ = std::fs::write(&seed, BEHAVIOR_LUAU);
    }
    eprintln!(
        "[voice] behavior script: embedded (seeded {})",
        seed.display()
    );
    (
        BEHAVIOR_LUAU.to_string(),
        "voice_behavior.lua (embedded)".into(),
    )
}

#[derive(Debug, Clone)]
pub struct Args {
    pub output: Option<PathBuf>,

    pub input: Option<PathBuf>,

    pub run: Option<String>,

    pub hold: bool,

    pub respond: bool,
}

#[derive(Clone, Debug)]
pub enum CaptureOutcome {
    Cancel,
    Sent(Result<String, String>),

    Responded(Result<String, String>),
}

#[derive(Clone, Debug, Default)]
pub struct RespondConfig {
    pub respond_command: String,
    pub tts_command: String,

    pub respond_stream: bool,

    pub llm_url: String,

    pub llm_model: String,
}

#[derive(Clone, Debug)]
pub(crate) struct UiState {
    pub(crate) status_text: String,
    pub(crate) status_color: String,
}

#[derive(Clone, Debug, Default)]
pub struct VoiceUiTexts {
    pub status_recording: Option<String>,
    pub status_transcribing: Option<String>,
    pub status_responding: Option<String>,
    pub status_speaking: Option<String>,
}

pub(crate) struct Behavior {
    lua: Lua,
    module: Option<Table>,
    ui: VoiceUiTexts,
}

impl Behavior {
    pub(crate) fn load_with(ui: VoiceUiTexts) -> Self {
        let lua = Lua::new();
        let (code, source_name) = load_behavior_script();
        let module = lua.load(&code).set_name(&source_name).eval::<Table>().ok();
        if module.is_none() {
            eprintln!("[voice] luau behavior load failed; using defaults");
        } else {
            eprintln!("[voice] luau behavior loaded ({source_name})");
        }
        Self { lua, module, ui }
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
            .get::<LuaValue>("status_text")
            .ok()
            .and_then(|v| self.lua.coerce_string(v).ok().flatten())
            .and_then(|s| s.to_str().ok().map(|s| s.to_string()))
            .unwrap_or(fallback.status_text);
        let status_color = t
            .get::<LuaValue>("status_color")
            .ok()
            .and_then(|v| self.lua.coerce_string(v).ok().flatten())
            .and_then(|s| s.to_str().ok().map(|s| s.to_string()))
            .unwrap_or(fallback.status_color);
        UiState {
            status_text,
            status_color,
        }
    }

    pub(crate) fn idle(&self) -> UiState {
        self.call_state(
            "on_idle",
            None,
            UiState {
                status_text: "Ready to record".into(),
                status_color: "#7aa2f7".into(),
            },
        )
    }

    pub(crate) fn recording(&self) -> UiState {
        let mut st = self.call_state(
            "on_recording",
            None,
            UiState {
                status_text: "Recording...".into(),
                status_color: "#f7768e".into(),
            },
        );
        if let Some(t) = &self.ui.status_recording {
            st.status_text = t.clone();
        }
        st
    }

    pub(crate) fn transcribing(&self) -> UiState {
        let mut st = self.call_state(
            "on_transcribing",
            None,
            UiState {
                status_text: "Sending for transcription...".into(),
                status_color: "#7aa2f7".into(),
            },
        );
        if let Some(t) = &self.ui.status_transcribing {
            st.status_text = t.clone();
        }
        st
    }

    pub(crate) fn responding(&self) -> UiState {
        let mut st = self.call_state(
            "on_responding",
            None,
            UiState {
                status_text: "Thinking...".into(),
                status_color: "#e0af68".into(),
            },
        );
        if let Some(t) = &self.ui.status_responding {
            st.status_text = t.clone();
        }
        st
    }

    pub(crate) fn speaking(&self) -> UiState {
        let mut st = self.call_state(
            "on_speaking",
            None,
            UiState {
                status_text: "Speaking...".into(),
                status_color: "#bb9af7".into(),
            },
        );
        if let Some(t) = &self.ui.status_speaking {
            st.status_text = t.clone();
        }
        st
    }

    pub(crate) fn success(&self, text: &str) -> UiState {
        self.call_state(
            "on_success",
            Some(text),
            UiState {
                status_text: "Transcription finished".into(),
                status_color: "#50b478".into(),
            },
        )
    }
}

pub(crate) fn apply_state(app: &ComponentInstance, st: &UiState) {
    VoiceView::new(app).set_status(&st.status_text, &st.status_color);
}

fn persist_voice_setting(key: &str, value: &str) {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from(".config"));
    let path = base.join("beca").join("config.toml");
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    let mut out = String::with_capacity(content.len() + 32);
    let mut in_voice = false;
    let mut replaced = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_voice && !replaced {
                out.push_str(&format!("{key} = {value}\n"));
                replaced = true;
            }
            in_voice = trimmed == "[modules.voice]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_voice {
            let prefix = format!("{key} ");
            let prefix_eq = format!("{key}=");
            if trimmed.starts_with(&prefix) || trimmed.starts_with(&prefix_eq) {
                out.push_str(&format!("{key} = {value}\n"));
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_voice && !replaced {
        out.push_str(&format!("{key} = {value}\n"));
    }
    let _ = fs::write(path, out);
}

pub(crate) fn write_wav(
    path: &Path,
    samples: &[f32],
    channels: u16,
    sample_rate: u32,
) -> Result<(), String> {
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

fn kill_child_process(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run_shell_stdout(
    cmd: &str,
    envs: &[(&str, &str)],
    cancel: &Arc<AtomicBool>,
    active_child: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<String, String> {
    let mut c = std::process::Command::new("bash");
    c.arg("-lc").arg(cmd);
    c.stdout(std::process::Stdio::piped());
    c.stderr(std::process::Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    let child = c.spawn().map_err(|e| format!("exec failed: {e}"))?;
    if let Ok(mut slot) = active_child.lock() {
        *slot = Some(child);
    } else {
        return Err("failed to track child process".to_string());
    }
    loop {
        if cancel.load(Ordering::Relaxed) {
            if let Ok(mut slot) = active_child.lock()
                && let Some(mut c) = slot.take()
            {
                kill_child_process(&mut c);
            }
            return Err("cancelled".to_string());
        }
        let status = {
            if let Ok(mut slot) = active_child.lock() {
                if let Some(c) = slot.as_mut() {
                    match c.try_wait() {
                        Ok(Some(s)) => Some((s, slot.take())),
                        Ok(None) => None,
                        Err(e) => return Err(format!("wait failed: {e}")),
                    }
                } else {
                    return Err("cancelled".to_string());
                }
            } else {
                return Err("failed to track child process".to_string());
            }
        };
        if let Some((status, child_opt)) = status {
            let mut child = child_opt.ok_or_else(|| "cancelled".to_string())?;
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut o) = child.stdout.take() {
                use std::io::Read;
                let _ = o.read_to_end(&mut stdout);
            }
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_end(&mut stderr);
            }
            let _ = child.wait();
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            if !status.success() {
                let stderr_s = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(if stderr_s.is_empty() {
                    format!("command failed: {}", status)
                } else {
                    stderr_s
                });
            }
            return Ok(String::from_utf8_lossy(&stdout).trim().to_string());
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn run_shell_status(
    cmd: &str,
    envs: &[(&str, &str)],
    cancel: &Arc<AtomicBool>,
    active_child: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<(), String> {
    let mut c = std::process::Command::new("bash");
    c.arg("-lc").arg(cmd);
    c.stdout(std::process::Stdio::null());
    c.stderr(std::process::Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    let child = c.spawn().map_err(|e| format!("exec failed: {e}"))?;
    if let Ok(mut slot) = active_child.lock() {
        *slot = Some(child);
    } else {
        return Err("failed to track child process".to_string());
    }
    loop {
        if cancel.load(Ordering::Relaxed) {
            if let Ok(mut slot) = active_child.lock()
                && let Some(mut c) = slot.take()
            {
                kill_child_process(&mut c);
            }
            return Err("cancelled".to_string());
        }
        let status = {
            if let Ok(mut slot) = active_child.lock() {
                if let Some(c) = slot.as_mut() {
                    match c.try_wait() {
                        Ok(Some(s)) => Some((s, slot.take())),
                        Ok(None) => None,
                        Err(e) => return Err(format!("wait failed: {e}")),
                    }
                } else {
                    return Err("cancelled".to_string());
                }
            } else {
                return Err("failed to track child process".to_string());
            }
        };
        if let Some((status, child_opt)) = status {
            let mut child = child_opt.ok_or_else(|| "cancelled".to_string())?;
            let mut stderr = Vec::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_end(&mut stderr);
            }
            let _ = child.wait();
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            if status.success() {
                return Ok(());
            } else {
                let stderr_s = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(if stderr_s.is_empty() {
                    format!("command failed: {status}")
                } else {
                    stderr_s
                });
            }
        }
        thread::sleep(Duration::from_millis(40));
    }
}

pub(crate) fn generate_response(
    transcript: &str,
    cfg: &RespondConfig,
    cancel: &Arc<AtomicBool>,
    active_child: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<String, String> {
    if !cfg.respond_command.trim().is_empty() {
        return run_shell_stdout(
            &cfg.respond_command,
            &[("TRANSCRIPT", transcript)],
            cancel,
            active_child,
        );
    }

    let script = r#"
set -euo pipefail
if command -v q-rs >/dev/null 2>&1; then
  if command -v q-context_builder >/dev/null 2>&1; then
    system_prompt="$(q-context_builder assist)"
    q-rs --prompt "$system_prompt" run "$TRANSCRIPT"
  else
    q-rs run "$TRANSCRIPT"
  fi
else
  echo "q-rs not found to generate response" >&2
  exit 127
fi
"#;
    run_shell_stdout(script, &[("TRANSCRIPT", transcript)], cancel, active_child)
}

pub(crate) fn generate_response_streaming(
    transcript: &str,
    cfg: &RespondConfig,
    daemon_url: &str,
    cancel: &Arc<AtomicBool>,
    weak: &slint_interpreter::Weak<ComponentInstance>,
) -> Result<String, String> {
    let system_prompt = system_prompt_assist();
    let opts = super::streaming::StreamOptions {
        llm_url: cfg.llm_url.clone(),
        model: cfg.llm_model.clone(),
        system_prompt,
        tts_url: daemon_url.to_string(),
        tts_command: cfg.tts_command.clone(),
    };
    let weak = weak.clone();
    super::streaming::stream_and_speak(&opts, transcript, cancel, &|partial| {
        let partial = partial.to_string();
        let _ = weak.upgrade_in_event_loop(move |app| {
            VoiceView::new(&app).set_response(&partial);
        });
    })
}

const SPEECH_SUFFIX: &str = "You are speaking with the user via voice (the text will be synthesized and played aloud). Answer in Brazilian Portuguese. Use short, direct sentences, no more than one or two breaths each. No markdown, no lists, no asterisks, no code, no emoji. Do not repeat the question. Prefer two or three sentences.";

fn system_prompt_assist() -> String {
    let script = "q-context_builder assist 2>/dev/null";
    let out = std::process::Command::new("bash")
        .arg("-lc")
        .arg(script)
        .output();
    let base = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };
    if base.is_empty() {
        SPEECH_SUFFIX.to_string()
    } else {
        format!("{base}\n\n{SPEECH_SUFFIX}")
    }
}

pub(crate) fn speak_response(
    text: &str,
    cfg: &RespondConfig,
    cancel: &Arc<AtomicBool>,
    active_child: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    if !cfg.tts_command.trim().is_empty() {
        return run_shell_status(&cfg.tts_command, &[("TEXT", text)], cancel, active_child);
    }

    let script = r#"
set -euo pipefail
tts=""
for c in \
  "/home/vinicius/office-agent/main/scripts/voice/voice-cli.sh" \
  "$HOME/office-agent/main/scripts/voice/voice-cli.sh"
do
  if [ -x "$c" ]; then tts="$c"; break; fi
done
if [ -n "$tts" ]; then
  "$tts" api tts "$TEXT"
  exit 0
fi
if command -v voice-cli.sh >/dev/null 2>&1; then
  voice-cli.sh api tts "$TEXT"
  exit 0
fi
echo "TTS unavailable (voice-cli.sh not found)" >&2
exit 127
"#;
    run_shell_status(script, &[("TEXT", text)], cancel, active_child)
}

pub(crate) fn animate_waveform(
    recent: &Arc<Mutex<Vec<f32>>>,
    running: &Arc<AtomicBool>,
    speaking: bool,
) {
    let mut t = 0.0f32;
    while running.load(Ordering::Relaxed) {
        if let Ok(mut r) = recent.lock() {
            let n = r.len();
            let amp = if speaking { 0.9 } else { 0.5 };
            let freq = if speaking { 4.5 } else { 2.2 };
            for (i, v) in r.iter_mut().enumerate() {
                let x = i as f32 / n as f32;
                let wave = (x * std::f32::consts::TAU * freq + t).sin()
                    * (x * std::f32::consts::TAU * (freq * 0.55) + t * 0.7).cos();
                *v = wave * amp;
            }
        }
        t += if speaking { 0.35 } else { 0.18 };
        thread::sleep(Duration::from_millis(33));
    }
}

pub(crate) fn pick_input_config(
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

fn push_mono(mono: &[f32], recorded: &Arc<Mutex<Vec<f32>>>, recent: &Arc<Mutex<Vec<f32>>>) {
    if let Ok(mut all) = recorded.lock() {
        all.extend_from_slice(mono);
    }
    if let Ok(mut r) = recent.lock() {
        let n = r.len();
        if mono.len() >= n {
            r.copy_from_slice(&mono[mono.len() - n..]);
        } else {
            r.drain(0..mono.len());
            r.extend_from_slice(mono);
        }
    }
}

pub(crate) fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    recording: Arc<AtomicBool>,
    recorded: Arc<Mutex<Vec<f32>>>,
    recent: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let err_fn = move |err| eprintln!("[voice] stream error: {err}");
    match sample_format {
        cpal::SampleFormat::F32 => {
            let recording = recording.clone();
            let recorded = recorded.clone();
            let recent = recent.clone();
            device
                .build_input_stream(
                    config,
                    move |data: &[f32], _| {
                        if !recording.load(Ordering::Relaxed) {
                            return;
                        }
                        let mono: Vec<f32> = if channels <= 1 {
                            data.to_vec()
                        } else {
                            data.chunks(channels)
                                .map(|c| c.iter().copied().sum::<f32>() / c.len() as f32)
                                .collect()
                        };
                        push_mono(&mono, &recorded, &recent);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::I16 => {
            let recording = recording.clone();
            let recorded = recorded.clone();
            let recent = recent.clone();
            device
                .build_input_stream(
                    config,
                    move |data: &[i16], _| {
                        if !recording.load(Ordering::Relaxed) {
                            return;
                        }
                        let mono: Vec<f32> = if channels <= 1 {
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect()
                        } else {
                            data.chunks(channels)
                                .map(|c| {
                                    c.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>()
                                        / c.len() as f32
                                })
                                .collect()
                        };
                        push_mono(&mono, &recorded, &recent);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::U16 => {
            let recording = recording.clone();
            let recorded = recorded.clone();
            let recent = recent.clone();
            device
                .build_input_stream(
                    config,
                    move |data: &[u16], _| {
                        if !recording.load(Ordering::Relaxed) {
                            return;
                        }
                        let mono: Vec<f32> = if channels <= 1 {
                            data.iter()
                                .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                .collect()
                        } else {
                            data.chunks(channels)
                                .map(|c| {
                                    c.iter()
                                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                        .sum::<f32>()
                                        / c.len() as f32
                                })
                                .collect()
                        };
                        push_mono(&mono, &recorded, &recent);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("unsupported sample format: {sample_format:?}")),
    }
}

#[derive(Clone, Debug)]
pub struct VoiceTheme {
    pub background: String,
    pub text_main: String,
    pub card_bg: String,
    pub card_border: String,
    pub surface: String,
    pub surface_alt: String,
    pub border: String,
    pub text_strong: String,
    pub accent: String,
    pub accent_success: String,
    pub accent_danger: String,
    pub recording: String,
    pub radius: f32,
    pub dark_mode: bool,
}

impl Default for VoiceTheme {
    fn default() -> Self {
        Self {
            background: "#141218".into(),
            text_main: "#E6E0E9".into(),
            card_bg: "#211F26".into(),
            card_border: "#49454F".into(),
            surface: "#211F26".into(),
            surface_alt: "#2B2930".into(),
            border: "#49454F".into(),
            text_strong: "#E6E0E9".into(),
            accent: "#D0BCFF".into(),
            accent_success: "#4CAF50".into(),
            accent_danger: "#F2B8B5".into(),
            recording: "#F2B8B5".into(),
            radius: 12.0,
            dark_mode: true,
        }
    }
}

pub fn apply_theme(app: &ComponentInstance, t: &VoiceTheme) {
    ui_loader::apply_theme_global(app, t);
}

fn start_position_keeper(win_w: i32, running: Arc<AtomicBool>) {
    let script = format!(
        r#"
while :; do
  addr=$(hyprctl clients -j 2>/dev/null | python3 -c 'import sys,json
for c in json.load(sys.stdin):
  t=(c.get("title") or "")
  cls=(c.get("class") or "")
  it=(c.get("initialTitle") or "")
  if t=="Voice Capture" or it=="Voice Capture" or "voice_capture" in cls.lower() or "Voice Capture" in cls:
    print(c.get("address","")); break' 2>/dev/null)
  if [ -n "$addr" ]; then
    eval "$(hyprctl monitors -j 2>/dev/null | python3 -c 'import sys,json
ms=json.load(sys.stdin)
m=next((x for x in ms if x.get("focused")), ms[0] if ms else None)
if m:
  x=int(m.get("x",0) + (m.get("width",0)-{win_w})/2)
  y=int(m.get("y",0) + 40)
  print("X=%d;Y=%d" % (x,y))' 2>/dev/null)"
    if [ -n "${{X:-}}" ] && [ -n "${{Y:-}}" ]; then
      if [ "${{X:-}}" != "${{LASTX:-}}" ] || [ "${{Y:-}}" != "${{LASTY:-}}" ]; then
        hyprctl dispatch "hl.dsp.focus({{window = 'address:$addr'}})" >/dev/null 2>&1 || true
        hyprctl dispatch "hl.dsp.window.move({{x=$X,y=$Y}})" >/dev/null 2>&1 || \
        hyprctl dispatch movewindowpixel "exact $X $Y,address:$addr" >/dev/null 2>&1 || true
        LASTX=$X; LASTY=$Y
      fi
    fi
  fi
  sleep 0.5
done
"#
    );
    std::thread::spawn(move || {
        let mut child = match std::process::Command::new("bash")
            .arg("-lc")
            .arg(script)
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = child.kill();
        let _ = child.wait();
    });
}

pub fn run_capture_ui(
    out_path: &Path,
    base_url: &str,
    hold: bool,
) -> Result<CaptureOutcome, String> {
    run_capture_ui_with(
        out_path,
        base_url,
        hold,
        false,
        RespondConfig::default(),
        VoiceTheme::default(),
        VoiceUiTexts::default(),
    )
}

pub fn run_capture_ui_with(
    out_path: &Path,
    base_url: &str,
    hold: bool,
    respond: bool,
    respond_cfg: RespondConfig,
    theme: VoiceTheme,
    ui_texts: VoiceUiTexts,
) -> Result<CaptureOutcome, String> {
    {
        let base_url = base_url.to_string();
        std::thread::spawn(move || {
            let url = format!("{}/health", base_url.trim_end_matches('/'));
            let _ = ureq::get(&url).call();
            if respond {
                let _ = std::process::Command::new("bash")
                    .arg("-c")
                    .arg("command -v q-context_builder >/dev/null && q-context_builder assist >/dev/null 2>&1; command -v q-rs >/dev/null && q-rs run 'ping' >/dev/null 2>&1")
                    .status();
            }
        });
    }
    let t0 = std::time::Instant::now();
    let t_step = std::time::Instant::now();
    let behavior = Behavior::load_with(ui_texts);
    eprintln!("[voice][timing] behavior loaded in {:?}", t_step.elapsed());
    let t_step = std::time::Instant::now();
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no capture device (microphone not found)".to_string())?;
    eprintln!(
        "[voice][timing] audio device selected in {:?}",
        t_step.elapsed()
    );
    let t_step = std::time::Instant::now();
    let (config, sample_format) = pick_input_config(&device)?;
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;
    eprintln!(
        "[voice][timing] audio config picked in {:?}",
        t_step.elapsed()
    );

    eprintln!(
        "[voice] preparing capture device ({channels}ch {sample_rate}Hz, {sample_format:?}) (total {:?})",
        t0.elapsed()
    );

    let recorded: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let recent: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0f32; 508]));

    let recording = Arc::new(AtomicBool::new(false));

    let stream = build_input_stream(
        &device,
        &config,
        sample_format,
        recording.clone(),
        recorded.clone(),
        recent.clone(),
    )?;
    let t_step = std::time::Instant::now();

    stream.play().map_err(|e| e.to_string())?;
    let _ = stream.pause();
    let stream = Rc::new(stream);
    eprintln!(
        "[voice][timing] audio stream ready(paused) in {:?} (total {:?})",
        t_step.elapsed(),
        t0.elapsed()
    );

    let t_step = std::time::Instant::now();
    let component_name = std::env::var("BECA_VOICE_COMPONENT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "VoiceCaptureWindow".to_string());
    let app = ui_loader::create(&component_name).map_err(|e| e.to_string())?;
    eprintln!(
        "[voice][timing] ui create in {:?} (total {:?})",
        t_step.elapsed(),
        t0.elapsed()
    );
    ui_loader::apply_theme_global(&app, &theme);
    apply_state(&app, &behavior.idle());
    let view = VoiceView::new(&app);
    view.idle();
    view.set_settings(respond, hold);
    view.set_show_settings(false);
    let outcome: Arc<Mutex<Option<CaptureOutcome>>> = Arc::new(Mutex::new(None));
    let finishing = Arc::new(AtomicBool::new(false));
    let generation: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancel_flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let active_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let finishing = finishing.clone();
        let recording = recording.clone();
        let recorded = recorded.clone();
        let recent_for_send = recent.clone();
        let out_path = out_path.to_path_buf();
        let base_url = base_url.to_string();
        let idle_state = behavior.idle();
        let recording_state = behavior.recording();
        let transcribing_state = behavior.transcribing();
        let responding_state = behavior.responding();
        let speaking_state = behavior.speaking();
        let success_state = behavior.success("");
        let respond_cfg = respond_cfg.clone();
        let respond_default = respond;
        let generation = generation.clone();
        let cancel_flag = cancel_flag.clone();
        let active_child = active_child.clone();
        let stream_ctl = stream.clone();
        let _ = app.set_callback("send", move |_args| {
            if finishing.load(Ordering::SeqCst) {
                return Value::Void;
            }

            if !recording.load(Ordering::Relaxed) {
                if let Ok(mut all) = recorded.lock() {
                    all.clear();
                }
                if let Ok(mut r) = recent_for_send.lock() {
                    for v in r.iter_mut() {
                        *v = 0.0;
                    }
                }
                let _ = stream_ctl.play();
                recording.store(true, Ordering::Relaxed);
                if let Some(app) = weak.upgrade() {
                    VoiceView::new(&app).start_recording();
                    apply_state(&app, &recording_state);
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
            let outcome = outcome.clone();
            let recent_for_send = recent_for_send.clone();
            let out_path = out_path.clone();
            let base_url = base_url.clone();
            let finishing = finishing.clone();
            let idle_state = idle_state.clone();
            let transcribing_state = transcribing_state.clone();
            let responding_state = responding_state.clone();
            let speaking_state = speaking_state.clone();
            let success_state = success_state.clone();
            let respond_cfg = respond_cfg.clone();
            let generation = generation.clone();
            let cancel_flag = cancel_flag.clone();
            let active_child = active_child.clone();
            cancel_flag.store(false, Ordering::SeqCst);
            let my_gen = generation.load(Ordering::SeqCst);
            let (respond_now, hold_now) = if let Some(app) = weak.upgrade() {
                let view = VoiceView::new(&app);
                view.begin_processing();
                apply_state(&app, &transcribing_state);
                (
                    view.respond_enabled(respond_default),
                    view.hold_enabled(hold),
                )
            } else {
                (respond_default, hold)
            };
            thread::spawn(move || {
                let is_stale = || {
                    generation.load(Ordering::SeqCst) != my_gen
                        || cancel_flag.load(Ordering::Relaxed)
                };
                if is_stale() {
                    return;
                }
                let t_stt = std::time::Instant::now();
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
                    super::transcribe::transcribe(&base_url, &out_path)
                })();
                eprintln!("[voice][timing] stt {:?}", t_stt.elapsed());

                match result {
                    Ok(text) if respond_now => {
                        let transcript = text.clone();
                        let st = responding_state.clone();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            VoiceView::new(&app).begin_response();
                            apply_state(&app, &st);
                        });

                        let anim_run = Arc::new(AtomicBool::new(true));
                        let anim_flag = anim_run.clone();
                        let recent_anim = recent_for_send.clone();
                        let anim_handle = thread::spawn(move || {
                            animate_waveform(&recent_anim, &anim_flag, false)
                        });

                        if respond_cfg.respond_stream {
                            let resp = generate_response_streaming(
                                &transcript,
                                &respond_cfg,
                                &base_url,
                                &cancel_flag,
                                &weak,
                            );
                            anim_run.store(false, Ordering::Relaxed);
                            let _ = anim_handle.join();
                            if is_stale() {
                                return;
                            }
                            match resp {
                                Ok(answer) => {
                                    if is_stale() {
                                        return;
                                    }
                                    if let Ok(mut o) = outcome.lock() {
                                        *o = Some(CaptureOutcome::Responded(Ok(answer.clone())));
                                    }
                                    println!("{answer}");
                                    let _ = std::io::Write::flush(&mut std::io::stdout());
                                    let st = success_state.clone();
                                    let idle = idle_state.clone();
                                    let _ = weak.upgrade_in_event_loop(move |app| {
                                        VoiceView::new(&app).idle();
                                        apply_state(&app, &st);
                                    });
                                    finishing.store(false, Ordering::SeqCst);
                                    thread::sleep(Duration::from_millis(1200));
                                    if is_stale() {
                                        return;
                                    }
                                    let _ = weak.upgrade_in_event_loop(move |app| {
                                        let view = VoiceView::new(&app);
                                        view.idle();
                                        view.clear_response();
                                        apply_state(&app, &idle);
                                    });
                                }
                                Err(e) => {
                                    if is_stale() || e == "cancelled" {
                                        return;
                                    }
                                    eprintln!("[voice] respond failed: {e}");
                                    if let Ok(mut o) = outcome.lock() {
                                        *o = Some(CaptureOutcome::Responded(Err(e.clone())));
                                    }
                                    let err_state = UiState {
                                        status_text: "Failed to respond".into(),
                                        status_color: "#f7768e".into(),
                                    };
                                    let _ = weak.upgrade_in_event_loop(move |app| {
                                        VoiceView::new(&app).set_busy(false);
                                        apply_state(&app, &err_state);
                                    });
                                    finishing.store(false, Ordering::SeqCst);
                                }
                            }
                            return;
                        }

                        let resp = generate_response(
                            &transcript,
                            &respond_cfg,
                            &cancel_flag,
                            &active_child,
                        );
                        anim_run.store(false, Ordering::Relaxed);
                        let _ = anim_handle.join();
                        if is_stale() {
                            return;
                        }

                        match resp {
                            Ok(answer) => {
                                if is_stale() {
                                    return;
                                }
                                let answer_ui = answer.clone();
                                let st = speaking_state.clone();
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    VoiceView::new(&app).set_response(&answer_ui);
                                    apply_state(&app, &st);
                                });

                                let anim_run = Arc::new(AtomicBool::new(true));
                                let anim_flag = anim_run.clone();
                                let recent_anim = recent_for_send.clone();
                                let anim_handle = thread::spawn(move || {
                                    animate_waveform(&recent_anim, &anim_flag, true)
                                });
                                let tts_res = speak_response(
                                    &answer,
                                    &respond_cfg,
                                    &cancel_flag,
                                    &active_child,
                                );
                                anim_run.store(false, Ordering::Relaxed);
                                let _ = anim_handle.join();
                                if is_stale() {
                                    return;
                                }

                                if let Ok(mut o) = outcome.lock() {
                                    *o = Some(CaptureOutcome::Responded(Ok(answer.clone())));
                                }

                                println!("{answer}");
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                                let _ = tts_res;
                                let st = success_state.clone();
                                let idle = idle_state.clone();
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    VoiceView::new(&app).idle();
                                    apply_state(&app, &st);
                                });
                                finishing.store(false, Ordering::SeqCst);

                                thread::sleep(Duration::from_millis(1200));
                                if is_stale() {
                                    return;
                                }
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    let view = VoiceView::new(&app);
                                    view.idle();
                                    view.clear_response();
                                    apply_state(&app, &idle);
                                });
                            }
                            Err(e) => {
                                if is_stale() || e == "cancelled" {
                                    return;
                                }
                                eprintln!("[voice] respond failed: {e}");
                                if let Ok(mut o) = outcome.lock() {
                                    *o = Some(CaptureOutcome::Responded(Err(e.clone())));
                                }
                                let err_state = UiState {
                                    status_text: "Failed to respond".into(),
                                    status_color: "#f7768e".into(),
                                };
                                let _ = weak.upgrade_in_event_loop(move |app| {
                                    VoiceView::new(&app).set_busy(false);
                                    apply_state(&app, &err_state);
                                });
                                finishing.store(false, Ordering::SeqCst);
                            }
                        }
                    }
                    Ok(text) => {
                        if is_stale() {
                            return;
                        }
                        if let Ok(mut o) = outcome.lock() {
                            *o = Some(CaptureOutcome::Sent(Ok(text.clone())));
                        }
                        let idle = idle_state.clone();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            let view = VoiceView::new(&app);
                            if hold_now {
                                view.show_result(&text);
                                apply_state(&app, &success_state);
                            } else {
                                view.idle();
                                view.hide_result();
                                apply_state(&app, &idle);
                            }
                        });
                        finishing.store(false, Ordering::SeqCst);
                    }
                    Err(e) => {
                        if is_stale() || e == "cancelled" {
                            return;
                        }
                        if let Ok(mut o) = outcome.lock() {
                            *o = Some(CaptureOutcome::Sent(Err(e.clone())));
                        }

                        finishing.store(false, Ordering::SeqCst);
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            VoiceView::new(&app).idle();
                            apply_state(
                                &app,
                                &UiState {
                                    status_text: format!("Error: {e}"),
                                    status_color: "#f7768e".into(),
                                },
                            );
                        });
                    }
                }
            });
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let finishing = finishing.clone();
        let recording = recording.clone();
        let generation = generation.clone();
        let cancel_flag = cancel_flag.clone();
        let active_child = active_child.clone();
        let idle_state = behavior.idle();
        let recent_for_stop = recent.clone();
        let stream_ctl = stream.clone();
        let _ = app.set_callback("cancel", move |_args| {
            generation.fetch_add(1, Ordering::SeqCst);
            cancel_flag.store(true, Ordering::SeqCst);
            recording.store(false, Ordering::Relaxed);
            let _ = stream_ctl.pause();
            finishing.store(false, Ordering::SeqCst);

            if let Ok(mut slot) = active_child.lock()
                && let Some(mut c) = slot.take()
            {
                let _ = c.kill();
                let _ = c.wait();
            }

            if let Ok(mut r) = recent_for_stop.lock() {
                for v in r.iter_mut() {
                    *v = 0.0;
                }
            }

            if let Ok(mut o) = outcome.lock() {
                *o = Some(CaptureOutcome::Cancel);
            }
            if let Some(app) = weak.upgrade() {
                VoiceView::new(&app).reset();
                apply_state(&app, &idle_state);
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let _ = app.set_callback("copy-text", move |_args| {
            if let Some(app) = weak.upgrade() {
                let text = VoiceView::new(&app).transcript();
                if super::transcribe::copy_to_clipboard(&text) {
                    super::transcribe::notify("📋 Transcript copied");
                } else {
                    super::transcribe::notify("❌ couldn't copy (wl-copy/xclip/xsel)");
                }
            }
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let recording = recording.clone();
        let finishing = finishing.clone();
        let generation = generation.clone();
        let cancel_flag = cancel_flag.clone();
        let active_child = active_child.clone();
        let stream_ctl = stream.clone();
        let _ = app.set_callback("close-app", move |_args| {
            generation.fetch_add(1, Ordering::SeqCst);
            cancel_flag.store(true, Ordering::SeqCst);
            recording.store(false, Ordering::Relaxed);
            let _ = stream_ctl.pause();
            finishing.store(false, Ordering::SeqCst);
            if let Ok(mut slot) = active_child.lock()
                && let Some(mut c) = slot.take()
            {
                let _ = c.kill();
                let _ = c.wait();
            }
            if let Ok(mut o) = outcome.lock()
                && o.is_none()
            {
                *o = Some(CaptureOutcome::Cancel);
            }
            if let Some(app) = weak.upgrade() {
                let _ = app.hide();
            }
            ui_loader::quit_event_loop();
            Value::Void
        });
    }

    {
        let weak = app.as_weak();
        let finishing = finishing.clone();
        let recording = recording.clone();
        let idle_state = behavior.idle();
        let stream_ctl = stream.clone();
        let _ = app.set_callback("new-recording", move |_args| {
            recording.store(false, Ordering::Relaxed);
            let _ = stream_ctl.pause();
            finishing.store(false, Ordering::SeqCst);
            if let Some(app) = weak.upgrade() {
                let view = VoiceView::new(&app);
                view.reset();
                apply_state(&app, &idle_state);
            }
            Value::Void
        });
    }

    {
        let _ = app.set_callback("respond-toggled", move |args| {
            if let Some(Value::Bool(v)) = args.first() {
                persist_voice_setting("respond", if *v { "true" } else { "false" });
            }
            Value::Void
        });
    }
    {
        let _ = app.set_callback("hold-toggled", move |args| {
            if let Some(Value::Bool(v)) = args.first() {
                let mode = if *v { "\"hold\"" } else { "\"pipe\"" };
                persist_voice_setting("mode", mode);
            }
            Value::Void
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

    if let Some(c) = ui_loader::parse_color(&theme.background) {
        let a = ((c.alpha() as f32) * 0.72).round().clamp(40.0, 255.0) as u8;
        let tc = slint_interpreter::Color::from_argb_u8(a, c.red(), c.green(), c.blue());
        let _ = app.set_global_property("Theme", "background", Value::Brush(tc.into()));
    } else if let Some(tc) = ui_loader::parse_color("#1E1E1EB8") {
        let _ = app.set_global_property("Theme", "background", Value::Brush(tc.into()));
    }

    let layer_shell = std::env::var_os("BECA_VOICE_LAYER_SHELL").is_some();

    let position_running = Arc::new(AtomicBool::new(true));
    if !layer_shell {
        start_position_keeper(540, position_running.clone());
    }

    let focus_watch_running = Arc::new(AtomicBool::new(true));
    if !layer_shell {
        let weak = app.as_weak();
        let outcome = outcome.clone();
        let recording = recording.clone();
        let finishing = finishing.clone();
        let generation = generation.clone();
        let cancel_flag = cancel_flag.clone();
        let active_child = active_child.clone();
        let focus_watch_flag = focus_watch_running.clone();
        let retracting = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let mut seen_focused = false;
            let mut misses = 0u32;
            while focus_watch_flag.load(Ordering::Relaxed) {
                let active_title = std::process::Command::new("hyprctl")
                    .args(["activewindow", "-j"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
                    .and_then(|v| {
                        v.get("title")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    });
                let is_voice = active_title.as_deref() == Some("Voice Capture");
                if is_voice {
                    seen_focused = true;
                    misses = 0;
                } else if seen_focused {
                    misses += 1;
                }

                if seen_focused && misses >= 2 && !retracting.swap(true, Ordering::SeqCst) {
                    focus_watch_flag.store(false, Ordering::Relaxed);

                    generation.fetch_add(1, Ordering::SeqCst);
                    cancel_flag.store(true, Ordering::SeqCst);
                    recording.store(false, Ordering::Relaxed);
                    finishing.store(false, Ordering::SeqCst);
                    if let Ok(mut slot) = active_child.lock()
                        && let Some(mut c) = slot.take()
                    {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    if let Ok(mut o) = outcome.lock()
                        && o.is_none()
                    {
                        *o = Some(CaptureOutcome::Cancel);
                    }
                    let weak2 = weak.clone();
                    let _ = slint_interpreter::invoke_from_event_loop(move || {
                        if let Some(app) = weak2.upgrade() {
                            VoiceView::new(&app).set_reveal(false);
                            let _ = app.hide();
                        }
                    });
                    std::thread::sleep(std::time::Duration::from_millis(240));
                    ui_loader::quit_event_loop();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        });
    }

    VoiceView::new(&app).set_reveal(false);
    let t_show = std::time::Instant::now();
    ui_loader::show(&app).map_err(|e| e.to_string())?;
    eprintln!(
        "[voice][timing] show() in {:?} (total {:?})",
        t_show.elapsed(),
        t0.elapsed()
    );

    let weak_reveal = app.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = slint_interpreter::invoke_from_event_loop(move || {
            if let Some(app) = weak_reveal.upgrade() {
                VoiceView::new(&app).set_reveal(true);
            }
        });
    });
    ui_loader::run_event_loop().map_err(|e| e.to_string())?;

    generation.fetch_add(1, Ordering::SeqCst);
    cancel_flag.store(true, Ordering::SeqCst);
    focus_watch_running.store(false, Ordering::Relaxed);
    position_running.store(false, Ordering::Relaxed);
    if let Ok(mut slot) = active_child.lock()
        && let Some(mut c) = slot.take()
    {
        let _ = c.kill();
        let _ = c.wait();
    }
    waveform_running.store(false, Ordering::Relaxed);
    recording.store(false, Ordering::Relaxed);
    drop(stream);

    outcome
        .lock()
        .ok()
        .and_then(|o| o.clone())
        .ok_or_else(|| "capture ended without action".to_string())
}

pub fn run(args: Args) -> Result<(), String> {
    run_configured(
        args,
        VoiceTheme::default(),
        VoiceUiTexts::default(),
        None,
        RespondConfig::default(),
    )
}

pub fn run_configured(
    args: Args,
    theme: VoiceTheme,
    ui_texts: VoiceUiTexts,
    daemon_url: Option<String>,
    respond_cfg: RespondConfig,
) -> Result<(), String> {
    let base_url = daemon_url
        .or_else(|| std::env::var("VOICE_DAEMON_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:4100".to_string());

    if let Some(input) = args.input.clone() {
        match super::transcribe::transcribe(&base_url, &input) {
            Ok(text) => super::transcribe::handle_transcript(&text, args.run.as_deref(), None),
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Err(e);
            }
        }
        return Ok(());
    }

    let out = args
        .output
        .clone()
        .unwrap_or_else(super::transcribe::default_output_path);
    let cleanup = if args.output.is_none() {
        Some(out.as_path())
    } else {
        None
    };
    match run_capture_ui_with(
        &out,
        &base_url,
        args.hold,
        args.respond,
        respond_cfg,
        theme,
        ui_texts,
    ) {
        Ok(CaptureOutcome::Cancel) => {
            let _ = fs::remove_file(&out);
            super::transcribe::notify("❌ Capture cancelled");
            Ok(())
        }
        Ok(CaptureOutcome::Sent(Ok(text))) => {
            super::transcribe::handle_transcript(&text, args.run.as_deref(), cleanup);
            Ok(())
        }
        Ok(CaptureOutcome::Sent(Err(e))) => {
            eprintln!("ERROR: {e}");
            if let Some(p) = cleanup {
                let _ = fs::remove_file(p);
            }
            Err(e)
        }
        Ok(CaptureOutcome::Responded(Ok(_))) => {
            if let Some(p) = cleanup {
                let _ = fs::remove_file(p);
            }
            Ok(())
        }
        Ok(CaptureOutcome::Responded(Err(e))) => {
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
