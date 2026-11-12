use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::thread;

use serde_json::Value as JsonValue;

#[derive(Clone, Debug, Default)]
pub struct StreamOptions {

    pub llm_url: String,

    pub model: String,

    pub system_prompt: String,

    pub tts_url: String,

    pub tts_command: String,
}

const MAX_CHUNK_CHARS: usize = 160;

pub fn stream_and_speak(
    opts: &StreamOptions,
    user_text: &str,
    cancel: &Arc<AtomicBool>,
    on_text: &dyn Fn(&str),
) -> Result<String, String> {
    if user_text.trim().is_empty() {
        return Err("empty transcript".to_string());
    }
    let (tx, rx) = sync_channel::<String>(8);
    let tts = spawn_tts_worker(opts.clone(), rx);

    let mut answer = String::new();
    let mut pending = String::new();
    let mut error: Option<String> = None;

    match read_sse(opts, user_text, cancel, &mut |delta| {
        answer.push_str(delta);
        on_text(&answer);
        pending.push_str(delta);
        while let Some((chunk, rest)) = take_chunk(&pending) {
            pending = rest;
            if tx.send(chunk).is_err() {
                break;
            }
        }
    }) {
        Ok(()) => {}
        Err(e) => error = Some(e),
    }

    if !pending.trim().is_empty() {
        let _ = tx.send(pending);
    }
    drop(tx);
    let _ = tts.join();

    match error {
        Some(e) => Err(e),
        None => Ok(answer),
    }
}

fn take_chunk(pending: &str) -> Option<(String, String)> {
    let trimmed = pending.trim_start();
    let bytes = trimmed.as_bytes();

    let mut cut = None;
    for (idx, byte) in bytes.iter().enumerate() {
        if matches!(byte, b'.' | b'!' | b'?' | b'\n') {
            let next = bytes.get(idx + 1).copied();
            if next.is_none() || next == Some(b' ') || next == Some(b'\n') {
                cut = Some(idx + 1);
                break;
            }
        }
    }

    if cut.is_none() && trimmed.chars().count() >= MAX_CHUNK_CHARS {
        let limit = MAX_CHUNK_CHARS.min(trimmed.len());
        let slice = &trimmed[..limit];
        let cut_at = slice.rfind(' ').unwrap_or(limit);
        cut = Some(cut_at);
    }
    let cut = cut?;
    if trimmed[..cut].trim().is_empty() {
        return None;
    }
    let chunk = trimmed[..cut].trim().to_string();
    let rest = trimmed[cut..].trim_start().to_string();
    Some((chunk, rest))
}

fn read_sse(
    opts: &StreamOptions,
    user_text: &str,
    cancel: &Arc<AtomicBool>,
    on_delta: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let base = opts.llm_url.trim_end_matches('/');
    if base.is_empty() {
        return Err("llm_url is not configured".to_string());
    }
    let url = format!("{base}/v1/chat/completions");

    let mut messages: Vec<JsonValue> = Vec::new();
    if !opts.system_prompt.trim().is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": opts.system_prompt,
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": user_text,
    }));

    let body = serde_json::json!({
        "model": opts.model,
        "messages": messages,
        "stream": true,
    })
    .to_string();

    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| format!("llm request failed: {e}"))?;

    let reader = BufReader::new(response.into_reader());
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let line = line.map_err(|e| format!("stream read failed: {e}"))?;
        let Some(data) = line.trim().strip_prefix("data: ") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<JsonValue>(data) else {
            continue;
        };
        if let Some(err) = event.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str())
        {
            return Err(format!("llm stream error: {err}"));
        }
        match event.get("type").and_then(|t| t.as_str()) {
            Some("delta") => {
                if let Some(content) = event.get("content").and_then(|c| c.as_str()) {
                    if !content.is_empty() {
                        on_delta(content);
                    }
                }
            }
            Some("finished") => break,
            _ => {}
        }
    }
    Ok(())
}

fn spawn_tts_worker(opts: StreamOptions, rx: Receiver<String>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(chunk) = rx.recv() {
            if let Err(e) = speak(&opts, &chunk) {
                eprintln!("[voice] tts failed: {e}");
            }
        }
    })
}

fn speak(opts: &StreamOptions, chunk: &str) -> Result<(), String> {
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return Ok(());
    }
    if !opts.tts_command.trim().is_empty() {
        let status = std::process::Command::new("bash")
            .arg("-lc")
            .arg(&opts.tts_command)
            .env("TEXT", chunk)
            .status()
            .map_err(|e| format!("tts command failed: {e}"))?;
        if !status.success() {
            return Err(format!("tts command exited with {status}"));
        }
        return Ok(());
    }
    let base = opts.tts_url.trim_end_matches('/');
    if base.is_empty() {
        return Err("tts_url is not configured".to_string());
    }
    let url = format!("{base}/tts");
    let body = serde_json::json!({ "text": chunk }).to_string();
    ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| format!("tts request failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_sentence_boundary() {
        let (chunk, rest) = take_chunk("Hello. How are you?").expect("chunk");
        assert_eq!(chunk, "Hello.");
        assert_eq!(rest, "How are you?");
    }

    #[test]
    fn keeps_partial_sentence_buffered() {
        assert!(take_chunk("Hello, all").is_none());
    }

    #[test]
    fn force_splits_long_text() {
        let long = "word ".repeat(40);
        let (chunk, rest) = take_chunk(&long).expect("chunk");
        assert!(!chunk.is_empty());
        assert!(!rest.is_empty());
        assert!(chunk.chars().count() <= MAX_CHUNK_CHARS);
    }
}
