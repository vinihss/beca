use serde_json::Value as JsonValue;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Volume dos feedbacks sonoros do microfone: baixo o suficiente para não
/// incomodar, mas audível sobre o áudio do sistema.
const SOUND_VOLUME: u32 = 18_000;
const SOUND_VOLUME_DB: &str = "-11.0";

pub fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["beca voice", msg])
        .status();
}

fn play_sound(paths: &[&str], fallback_id: &str) {
    let volume = SOUND_VOLUME.to_string();
    for path in paths {
        if !Path::new(path).exists() {
            continue;
        }
        let _ = Command::new("paplay")
            .arg("--volume")
            .arg(&volume)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        return;
    }

    let _ = Command::new("canberra-gtk-play")
        .args(["-i", fallback_id, "-V", SOUND_VOLUME_DB])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Som curto de confirmação ao iniciar a captura de microfone.
pub fn play_start_sound() {
    play_sound(
        &[
            "/usr/share/sounds/freedesktop/stereo/audio-volume-change.oga",
            "/usr/share/sounds/freedesktop/stereo/bell.oga",
            "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
        ],
        "audio-volume-change",
    );
}

/// Som curto ao detectar o fim da fala e iniciar a transcrição.
pub fn play_stop_sound() {
    play_sound(
        &[
            "/usr/share/sounds/freedesktop/stereo/complete.oga",
            "/usr/share/sounds/freedesktop/stereo/message.oga",
            "/usr/share/sounds/freedesktop/stereo/dialog-information.oga",
        ],
        "complete",
    );
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
    base.join(format!("beca-voice-{ts}.wav"))
}

pub fn transcribe(base_url: &str, audio_path: &Path) -> Result<String, String> {
    let bytes = fs::read(audio_path).map_err(|e| format!("read audio: {e}"))?;
    if bytes.is_empty() {
        return Err("empty audio file".to_string());
    }
    let url = format!("{}/transcribe", base_url.trim_end_matches('/'));
    eprintln!("[voice] transcribing...");
    let resp = ureq::post(&url)
        .timeout(Duration::from_secs(60))
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
            "empty transcript (silent or too-short audio). Speak louder or longer.".to_string(),
        );
    }
    Ok(text)
}

pub fn handle_transcript(text: &str, run: Option<&str>, cleanup_path: Option<&Path>) {
    if text.is_empty() {
        eprintln!("ERROR: empty transcript");
        notify("⚠️ Empty transcript");
        if let Some(p) = cleanup_path {
            let _ = fs::remove_file(p);
        }
        std::process::exit(1);
    }
    print!("{text}");
    let _ = std::io::stdout().flush();
    if !copy_to_clipboard(text) {
        eprintln!("ERROR: clipboard copy failed (no wl-copy/xclip/xsel)");
    }
    if let Some(cmd) = run {
        let status = Command::new(cmd).arg(text).status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("ERROR: --run exited with status {s}"),
            Err(e) => eprintln!("ERROR: failed to run command: {e}"),
        }
    }
    if let Some(p) = cleanup_path {
        let _ = fs::remove_file(p);
    }
}
