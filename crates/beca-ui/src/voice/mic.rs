use cpal::traits::{HostTrait, StreamTrait};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use super::capture;
use super::transcribe::{self, default_output_path};

const POLL_INTERVAL: Duration = Duration::from_millis(15);
const WARMUP: Duration = Duration::from_millis(200);
const NOISE_FLOOR_MIN: f32 = 0.01;
const NOISE_FLOOR_MAX: f32 = 0.20;
const SPEECH_DELTA: f32 = 0.10;
const SILENCE_DELTA: f32 = 0.04;
const MIN_SPEECH_POLLS: u32 = 8;
const SILENCE_POLLS: u32 = 12;
const MAX_RECORDING: Duration = Duration::from_secs(20);

/// Limiares adaptativos: o silêncio é medido em relação ao ruído de fundo
/// capturado no início, em vez de um valor absoluto que falha em ambientes
/// ruidosos (ou com microfone muito sensível).
fn voice_thresholds(noise_floor: f32) -> (f32, f32) {
    let floor = noise_floor.clamp(NOISE_FLOOR_MIN, NOISE_FLOOR_MAX);
    let speech = (floor + SPEECH_DELTA).clamp(0.10, 0.35);
    let silence = (floor + SILENCE_DELTA).clamp(0.05, speech - 0.02);
    (speech, silence)
}

/// Detecta o fim da fala com histerese: exige fala mínima antes de contar
/// silêncio. Níveis na zona cinzenta apenas desaceleram a contagem, para que
/// ruído de fundo não impeça o endpoint nem um pico isolado o dispare cedo.
fn utterance_ended(
    level: f32,
    noise_floor: f32,
    saw_speech: &mut bool,
    speech_polls: &mut u32,
    silent_polls: &mut u32,
) -> bool {
    let (speech_threshold, silence_threshold) = voice_thresholds(noise_floor);
    if level > speech_threshold {
        *saw_speech = true;
        *speech_polls += 1;
        *silent_polls = 0;
        return false;
    }
    if !*saw_speech || *speech_polls < MIN_SPEECH_POLLS {
        return false;
    }
    if level < silence_threshold {
        *silent_polls += 1;
        return *silent_polls >= SILENCE_POLLS;
    }
    // Zona cinzenta: não reinicia, apenas reduz a contagem de silêncio.
    *silent_polls = silent_polls.saturating_sub(1);
    false
}

fn recent_level(recent: &Mutex<Vec<f32>>) -> Option<f32> {
    let buffer = recent.lock().ok()?;
    if buffer.is_empty() {
        return None;
    }
    let sum: f32 = buffer.iter().map(|sample| sample * sample).sum();
    Some(((sum / buffer.len() as f32).sqrt() / 0.085).min(1.0))
}

pub struct RecordedAudio {
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
}

impl RecordedAudio {
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn transcribe(self, base_url: &str) -> Result<String, String> {
        if self.samples.is_empty() {
            return Err("empty audio capture".to_string());
        }

        let path = default_output_path();
        let result = (|| {
            capture::write_wav(&path, &self.samples, self.channels, self.sample_rate)?;
            transcribe::transcribe(base_url, &path)
        })();
        let _ = fs::remove_file(&path);
        result
    }
}

pub struct MicCapture {
    stream: cpal::Stream,
    recording: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    monitor_stop: Arc<AtomicBool>,
    sample_rate: u32,
}

impl MicCapture {
    pub fn start() -> Result<Self, String> {
        Self::start_with_silence_detection(|| {})
    }

    /// Inicia a captura e avisa quando o usuário para de falar (ou quando o
    /// tempo máximo é atingido), para que a transcrição possa ser disparada.
    pub fn start_with_silence_detection(
        on_silence: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no capture device (microphone not found)".to_string())?;
        let (config, sample_format) = capture::pick_input_config(&device)?;
        let sample_rate = config.sample_rate.0;

        let recording = Arc::new(AtomicBool::new(true));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let recent = Arc::new(Mutex::new(vec![0.0f32; 508]));
        let monitor_stop = Arc::new(AtomicBool::new(true));

        let monitor_recent = recent.clone();
        let monitor_flag = monitor_stop.clone();
        thread::spawn(move || {
            let mut saw_speech = false;
            let mut speech_polls = 0u32;
            let mut silent_polls = 0u32;
            let mut baseline = Vec::new();
            let mut noise_floor = None;
            let started = Instant::now();

            while monitor_flag.load(Ordering::Relaxed) {
                thread::sleep(POLL_INTERVAL);
                if !monitor_flag.load(Ordering::Relaxed) {
                    break;
                }
                let Some(level) = recent_level(&monitor_recent) else {
                    continue;
                };

                // O transitório de abertura do stream serve para medir o ruído
                // de fundo; ele não é usado como fala.
                if started.elapsed() < WARMUP {
                    baseline.push(level);
                    continue;
                }
                if noise_floor.is_none() {
                    baseline.sort_by(f32::total_cmp);
                    let index = (baseline.len() as f32 * 0.2) as usize;
                    noise_floor = Some(
                        baseline
                            .get(index)
                            .copied()
                            .unwrap_or(0.03)
                            .clamp(NOISE_FLOOR_MIN, NOISE_FLOOR_MAX),
                    );
                }
                let floor = noise_floor.unwrap_or(0.03);

                if utterance_ended(
                    level,
                    floor,
                    &mut saw_speech,
                    &mut speech_polls,
                    &mut silent_polls,
                ) || started.elapsed() >= MAX_RECORDING
                {
                    monitor_flag.store(false, Ordering::Relaxed);
                    on_silence();
                    break;
                }
            }
        });

        let stream = match capture::build_input_stream(
            &device,
            &config,
            sample_format,
            recording.clone(),
            samples.clone(),
            recent,
        ) {
            Ok(stream) => stream,
            Err(e) => {
                monitor_stop.store(false, Ordering::Relaxed);
                return Err(e);
            }
        };
        if let Err(e) = stream.play() {
            monitor_stop.store(false, Ordering::Relaxed);
            return Err(e.to_string());
        }

        Ok(Self {
            stream,
            recording,
            samples,
            monitor_stop,
            sample_rate,
        })
    }

    pub fn finish(self) -> Result<RecordedAudio, String> {
        let Self {
            stream,
            recording,
            samples,
            monitor_stop,
            sample_rate,
        } = self;

        monitor_stop.store(false, Ordering::Relaxed);
        recording.store(false, Ordering::Relaxed);
        let _ = stream.pause();
        drop(stream);

        let samples = samples
            .lock()
            .map_err(|_| "audio capture lock poisoned".to_string())?
            .clone();
        Ok(RecordedAudio {
            samples,
            channels: 1,
            sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordedAudio, utterance_ended, voice_thresholds};

    const FLOOR: f32 = 0.03;

    #[test]
    fn silence_endpoint_requires_minimum_speech_then_quiet_frames() {
        let mut saw_speech = false;
        let mut speech_polls = 0;
        let mut silent_polls = 0;

        assert!(!utterance_ended(
            0.01,
            FLOOR,
            &mut saw_speech,
            &mut speech_polls,
            &mut silent_polls
        ));
        assert!(!saw_speech);

        for _ in 0..8 {
            assert!(!utterance_ended(
                0.30,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls
            ));
        }
        assert!(saw_speech);

        for _ in 0..11 {
            assert!(!utterance_ended(
                0.01,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls
            ));
        }
        assert!(utterance_ended(
            0.01,
            FLOOR,
            &mut saw_speech,
            &mut speech_polls,
            &mut silent_polls
        ));
    }

    #[test]
    fn uncertain_levels_do_not_reset_silence_counter() {
        let mut saw_speech = false;
        let mut speech_polls = 0;
        let mut silent_polls = 0;

        for _ in 0..8 {
            utterance_ended(
                0.30,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls,
            );
        }
        for _ in 0..6 {
            assert!(!utterance_ended(
                0.01,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls
            ));
        }
        // 0.10 fica entre o limiar de silêncio (0.07) e o de fala (0.13).
        for _ in 0..2 {
            assert!(!utterance_ended(
                0.10,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls
            ));
        }
        for _ in 0..7 {
            assert!(!utterance_ended(
                0.01,
                FLOOR,
                &mut saw_speech,
                &mut speech_polls,
                &mut silent_polls
            ));
        }
        assert!(utterance_ended(
            0.01,
            FLOOR,
            &mut saw_speech,
            &mut speech_polls,
            &mut silent_polls
        ));
    }

    #[test]
    fn thresholds_follow_noise_floor() {
        let (speech, silence) = voice_thresholds(0.12);
        assert!(speech > 0.18);
        assert!(silence > 0.08);
        assert!(silence < speech);

        let (speech, silence) = voice_thresholds(0.02);
        assert!(speech < 0.18);
        assert!(silence < speech);
    }

    #[test]
    fn empty_audio_is_rejected_before_writing_a_wav() {
        let audio = RecordedAudio {
            samples: Vec::new(),
            channels: 1,
            sample_rate: 16_000,
        };

        let err = audio.transcribe("http://127.0.0.1:1").unwrap_err();
        assert_eq!(err, "empty audio capture");
    }
}
