use serde_json::Value as JsonValue;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn notify(msg: &str) {
    let _ = Command::new("notify-send")
        .args(["beca voice", msg])
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
    base.join(format!("beca-voice-{ts}.wav"))
}

pub fn transcribe(base_url: &str, audio_path: &Path) -> Result<String, String> {
    let bytes = fs::read(audio_path).map_err(|e| format!("read audio: {e}"))?;
    if bytes.is_empty() {
        return Err("empty audio file".to_string());
    }
    let url = format!("{}/transcribe", base_url.trim_end_matches('/'));
    eprintln!("[beca-voice] transcrevendo...");
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

pub fn handle_transcript(text: &str, run: Option<&str>, cleanup_path: Option<&Path>) {
    if text.is_empty() {
        eprintln!("ERROR: empty transcript");
        notify("⚠️ Transcrição vazia");
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
