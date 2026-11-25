//! Live integration test for the streaming LLM -> TTS pipeline.
//!
//! Requires the q-rs server (127.0.0.1:8787) and the voice daemon
//! (127.0.0.1:4100) to be running. Ignored by default: run with
//! `cargo test -p beca-ui --test streaming_live -- --ignored --nocapture`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use beca_ui::voice::streaming::{StreamOptions, stream_and_speak};

#[test]
#[ignore = "requires live q-rs server and voice daemon"]
fn streams_answer_and_speaks_chunks() {
    let opts = StreamOptions {
        llm_url: "http://127.0.0.1:8787".into(),
        model: "openrouter/openrouter/free".into(),
        system_prompt: "You are an assistant. Answer in English, at most 3 sentences.".into(),
        tts_url: "http://127.0.0.1:4100".into(),
        tts_command: String::new(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let answer = stream_and_speak(
        &opts,
        "talk about the sea in 3 short sentences",
        &cancel,
        &|partial| {
            seen.lock().unwrap().push(partial.to_string());
        },
    )
    .expect("streaming failed");

    assert!(!answer.trim().is_empty(), "answer is empty");
    let updates = seen.lock().unwrap();
    assert!(updates.len() >= 2, "expected progressive UI updates");
    assert!(
        updates.last().unwrap().contains(answer.trim()),
        "last update should contain the full answer"
    );
    println!("answer: {answer}");
}
