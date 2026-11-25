use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

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
    let t_pipeline = Instant::now();
    let (tx, rx) = sync_channel::<String>(8);
    let tts = spawn_tts_worker(opts.clone(), rx);

    let mut answer = String::new();
    let mut pending = String::new();
    let mut error: Option<String> = None;
    let mut first_token_at: Option<Instant> = None;

    match read_sse(opts, user_text, cancel, &mut |delta| {
        if first_token_at.is_none() {
            let at = Instant::now();
            first_token_at = Some(at);
            eprintln!("[voice][timing] llm ttft {:?}", at.duration_since(t_pipeline));
        }
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
    eprintln!("[voice][timing] llm total {:?}", t_pipeline.elapsed());

    if !pending.trim().is_empty() {
        let _ = tx.send(pending);
    }
    drop(tx);
    let _ = tts.join();
    eprintln!("[voice][timing] respond pipeline {:?}", t_pipeline.elapsed());

    match error {
        Some(e) => Err(e),
        None => Ok(answer),
    }
}

const ABBREVIATIONS: &[&str] = &[
    "sr", "sra", "srta", "dr", "dra", "prof", "profa", "eng", "exmo", "exma", "av", "est", "apto",
    "ref", "tel", "cel", "ltda", "inc", "vs",
];

fn is_abbreviation(prefix: &str) -> bool {
    let token = prefix
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("");
    if token.is_empty() {
        return false;
    }
    let normalized = token.to_lowercase();
    if normalized.chars().count() == 1 && normalized.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return true;
    }
    ABBREVIATIONS.contains(&normalized.as_str())
}

fn take_chunk(pending: &str) -> Option<(String, String)> {
    let trimmed = pending.trim_start();
    let bytes = trimmed.as_bytes();

    let mut cut = None;
    for (idx, byte) in bytes.iter().enumerate() {
        if matches!(byte, b'.' | b'!' | b'?' | b'\n') {
            let next = bytes.get(idx + 1).copied();
            if matches!(byte, b'.') && is_abbreviation(&trimmed[..idx]) {
                continue;
            }
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
                if let Some(content) = event.get("content").and_then(|c| c.as_str())
                    && !content.is_empty()
                {
                    on_delta(content);
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
            let t = Instant::now();
            match speak(&opts, &chunk) {
                Ok(()) => eprintln!(
                    "[voice][timing] tts sentence {} chars in {:?}",
                    chunk.chars().count(),
                    t.elapsed()
                ),
                Err(e) => eprintln!("[voice] tts failed: {e}"),
            }
        }
    })
}

fn strip_drop_tokens(text: &str) -> String {
    text.split_whitespace()
        .filter(|token| {
            let t = token.to_lowercase();
            !(t.contains("http://")
                || t.contains("https://")
                || t.starts_with("www.")
                || t.contains('@') && t.contains('.'))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn pt_under_100(n: u64) -> String {
    const UNITS: [&str; 20] = [
        "zero", "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", "nove",
        "dez", "onze", "doze", "treze", "catorze", "quinze", "dezesseis", "dezessete",
        "dezoito", "dezenove",
    ];
    const TENS: [&str; 10] = [
        "", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa",
    ];
    if n < 20 {
        return UNITS[n as usize].to_string();
    }
    let tens = (n / 10) as usize;
    let unit = (n % 10) as usize;
    if unit == 0 {
        TENS[tens].to_string()
    } else {
        format!("{} e {}", TENS[tens], UNITS[unit])
    }
}

fn pt_under_1000(n: u64) -> String {
    const HUNDREDS: [&str; 10] = [
        "", "cento", "duzentos", "trezentos", "quatrocentos", "quinhentos", "seiscentos",
        "setecentos", "oitocentos", "novecentos",
    ];
    if n == 100 {
        return "cem".to_string();
    }
    let hundreds = (n / 100) as usize;
    let rest = n % 100;
    if hundreds == 0 {
        return pt_under_100(rest);
    }
    if rest == 0 {
        return HUNDREDS[hundreds].to_string();
    }
    format!("{} e {}", HUNDREDS[hundreds], pt_under_100(rest))
}

fn pt_rest(n: u64) -> String {
    let thousands = n / 1000;
    let rest = n % 1000;
    if thousands == 0 {
        return pt_under_1000(rest);
    }
    let head = if thousands == 1 { "mil".to_string() } else { format!("{} mil", pt_under_1000(thousands)) };
    if rest == 0 {
        return head;
    }
    let sep = if rest < 100 || rest.is_multiple_of(100) { " e " } else { " " };
    format!("{head}{sep}{}", pt_under_1000(rest))
}

fn pt_int_words(n: u64) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    let millions = n / 1_000_000;
    let rest = n % 1_000_000;
    if millions == 0 {
        return pt_rest(rest);
    }
    let head = if millions == 1 {
        "um milhão".to_string()
    } else {
        format!("{} milhões", pt_under_1000(millions))
    };
    if rest == 0 {
        return head;
    }
    let sep = if rest < 100 || rest.is_multiple_of(100) { " e " } else { " " };
    format!("{head}{sep}{}", pt_rest(rest))
}

fn parse_number(int_token: &str, frac_token: Option<&str>) -> String {
    let digits: String = int_token.chars().filter(|c| c.is_ascii_digit()).collect();
    let value = digits.parse::<u64>().unwrap_or(0);
    let mut out = pt_int_words(value);
    if let Some(frac) = frac_token {
        let frac = frac.to_string();
        if frac.chars().all(|c| c == '0') {
            return out;
        }
        match frac.parse::<u64>() {
            Ok(f) => {
                out.push_str(" vírgula ");
                out.push_str(&pt_int_words(f));
            }
            Err(_) => {
                out.push_str(" vírgula ");
                for c in frac.chars() {
                    out.push_str(pt_int_words((c as u64) - 48).as_str());
                    out.push(' ');
                }
                let _ = out.pop();
            }
        }
    }
    out
}

fn wordify_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 64);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !c.is_ascii_digit() {
            if c == '-' || c == '‐' || c == '–' || c == '—' {
                let next_digit = chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
                let prev_alnum = out
                    .chars()
                    .rev()
                    .find(|d| *d != ' ')
                    .is_some_and(|d| d.is_ascii_alphanumeric());
                if next_digit && prev_alnum {
                    out.push_str(" a ");
                } else if next_digit {
                    out.push_str("menos ");
                } else {
                    out.push(' ');
                }
            } else {
                out.push(c);
            }
            i += 1;
            continue;
        }
        let mut int_token = String::new();
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                int_token.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '.' {
                let d1 = chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
                let d2 = chars.get(i + 2).is_some_and(|d| d.is_ascii_digit());
                let d3 = chars.get(i + 3).is_some_and(|d| d.is_ascii_digit());
                let d4 = chars.get(i + 4).is_some_and(|d| d.is_ascii_digit());
                let boundary = chars.get(i + 4).is_none_or(|d| !d.is_ascii_digit());
                if d1 && d2 && d3 && boundary && !d4 {
                    int_token.push('.');
                    i += 1;
                    continue;
                }
            }
            break;
        }
        let mut frac_token: Option<String> = None;
        if chars.get(i) == Some(&',') && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
            i += 1;
            let mut f = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                f.push(chars[i]);
                i += 1;
            }
            frac_token = Some(f);
        }
        out.push_str(&parse_number(&int_token, frac_token.as_deref()));
    }
    out
}

fn sanitize_for_speech(text: &str) -> String {
    let mut out = strip_drop_tokens(text);
    for ch in ['*', '#', '`', '_', '~', '[', ']', '{', '}', '<', '>', '|', '•', '"', '\'', '“', '”', '‘', '’', '«', '»', '(', ')', '/', '→'] {
        out = out.replace(ch, " ");
    }
    for ch in ["R$", "US$", "U$", "$"] {
        out = out.replace(ch, " reais ");
    }
    out = out.replace('%', " por cento");
    out = wordify_numbers(&out);
    let mut collapsed = String::with_capacity(out.len());
    let mut last_space = true;
    for ch in out.chars() {
        if ch == ' ' || ch == '\t' {
            if !last_space {
                collapsed.push(' ');
            }
            last_space = true;
            continue;
        }
        collapsed.push(ch);
        last_space = false;
    }
    let mut s = collapsed.trim_matches(|c| c == ' ' || c == ',' || c == '.').to_string();
    while s.contains(", ,") {
        s = s.replace(", ,", ",");
    }
    while s.contains(",  ") {
        s = s.replace(",  ", ", ");
    }
    while s.contains(" .") {
        s = s.replace(" .", ".");
    }
    while s.contains(" ,") {
        s = s.replace(" ,", ",");
    }
    s
}

fn speak(opts: &StreamOptions, chunk: &str) -> Result<(), String> {
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return Ok(());
    }
    let chunk_owned = sanitize_for_speech(chunk);
    let chunk = chunk_owned.trim();
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

    #[test]
    fn does_not_split_on_common_abbreviations() {
        assert!(take_chunk("fale com Dr. Silva e me avise hoje cedo").is_none());
        assert!(take_chunk("a Sra. Maria ligou agora a pouco").is_none());
        assert!(take_chunk("conforme a ref. 123 do processo").is_none());
        let (chunk, rest) = take_chunk("fale com Dr. Silva. depois avise a equipe toda").expect("chunk");
        assert_eq!(chunk, "fale com Dr. Silva.");
        assert_eq!(rest, "depois avise a equipe toda");
    }

    #[test]
    fn ellipsis_still_splits() {
        let (chunk, rest) = take_chunk("Hmm... pronto.").expect("chunk");
        assert_eq!(chunk, "Hmm...");
        assert_eq!(rest, "pronto.");
    }

    #[test]
    fn sanitize_strips_markdown_and_symbols() {
        assert_eq!(sanitize_for_speech("*bold* and `code`"), "bold and code");
        assert_eq!(sanitize_for_speech("(test) [array] {map}"), "test array map");
        assert_eq!(sanitize_for_speech("olá — tudo bem?"), "olá tudo bem?");
        assert_eq!(sanitize_for_speech("a/b"), "a b");
        assert_eq!(sanitize_for_speech("“quote”"), "quote");
    }

    #[test]
    fn sanitize_drops_urls_and_emails() {
        assert_eq!(sanitize_for_speech("veja www.example.com agora"), "veja agora");
        assert_eq!(sanitize_for_speech("acesse https://example.com/a?b=1 fim"), "acesse fim");
        assert_eq!(sanitize_for_speech("escreva para me@example.com hoje"), "escreva para hoje");
    }

    #[test]
    fn sanitize_wordifies_pt_numbers() {
        assert_eq!(sanitize_for_speech("custa 2500 reais"), "custa dois mil e quinhentos reais");
        assert_eq!(sanitize_for_speech("hoje é 15"), "hoje é quinze");
        assert_eq!(sanitize_for_speech("R$ 1.234,00"), "reais mil duzentos e trinta e quatro");
        assert_eq!(sanitize_for_speech("3,5 por cento"), "três vírgula cinco por cento");
        assert_eq!(sanitize_for_speech("50%"), "cinquenta por cento");
        assert_eq!(sanitize_for_speech("1.000.000"), "um milhão");
        assert_eq!(sanitize_for_speech("2.500"), "dois mil e quinhentos");
        assert_eq!(sanitize_for_speech("1250"), "mil duzentos e cinquenta");
        assert_eq!(sanitize_for_speech("112"), "cento e doze");
        assert_eq!(sanitize_for_speech("101"), "cento e um");
        assert_eq!(sanitize_for_speech("100"), "cem");
    }

    #[test]
    fn sanitize_handles_ranges_and_negatives() {
        assert_eq!(sanitize_for_speech("de 10-20 itens"), "de dez a vinte itens");
        assert_eq!(sanitize_for_speech("-3 graus"), "menos três graus");
    }

    #[test]
    fn sanitize_collapses_spaces() {
        assert_eq!(sanitize_for_speech("a   b    c"), "a b c");
        assert_eq!(sanitize_for_speech("  trim "), "trim");
    }

    #[test]
    fn decimal_numbers_do_not_split() {
        let (chunk, rest) = take_chunk("o total foi R$ 2.500,00 hoje. ok valeu").expect("chunk");
        assert_eq!(chunk, "o total foi R$ 2.500,00 hoje.");
        assert_eq!(rest, "ok valeu");
    }
}
