//! End-to-end proof that the Whisper backend recognises real speech.
//!
//! Ignored by default: it needs a downloaded model and an audio file.
//!
//! ```text
//! DECKCRAFT_WHISPER_MODEL=<dir with model.safetensors, config.json, tokenizer.json> \
//! DECKCRAFT_WHISPER_WAV=<16 kHz mono WAV> \
//! cargo test -p deckcraft-asr-candle --test whisper_real_audio --release -- --ignored --nocapture
//! ```
//!
//! The WAV this was verified with is `samples_jfk.wav` from
//! <https://huggingface.co/datasets/Narsil/candle-examples> (11 s, 16 kHz mono 16-bit PCM, a
//! public-domain US government recording): "And so my fellow Americans, ask not what your country
//! can do for you, ask what you can do for your country."

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Instant;

use deckcraft_asr::AudioBuffer;
use deckcraft_asr_candle::WhisperEngine;

/// A 16-bit PCM WAV, chunks walked rather than assumed at fixed offsets.
fn read_wav(path: &Path) -> Result<(Vec<f32>, u32, u16), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12usize;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        let start = pos + 8;
        let end = start.saturating_add(len).min(bytes.len());
        match id {
            b"fmt " if end >= start + 16 => {
                let tag = u16::from_le_bytes([bytes[start], bytes[start + 1]]);
                let channels = u16::from_le_bytes([bytes[start + 2], bytes[start + 3]]);
                let rate = u32::from_le_bytes([bytes[start + 4], bytes[start + 5], bytes[start + 6], bytes[start + 7]]);
                let bits = u16::from_le_bytes([bytes[start + 14], bytes[start + 15]]);
                format = Some((tag, channels, rate, bits));
            }
            b"data" => {
                let (tag, channels, rate, bits) = format.ok_or("the data chunk comes before fmt")?;
                if tag != 1 || bits != 16 {
                    return Err(format!("only 16-bit PCM is read here (format {tag}, {bits} bits)"));
                }
                let samples = bytes[start..end]
                    .chunks(2)
                    .take_while(|s| s.len() == 2)
                    .map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0)
                    .collect();
                return Ok((samples, rate, channels));
            }
            _ => {}
        }
        pos = start.saturating_add(len + (len & 1));
    }
    Err("no data chunk".into())
}

#[test]
#[ignore = "needs DECKCRAFT_WHISPER_MODEL (a downloaded whisper-tiny) and DECKCRAFT_WHISPER_WAV"]
fn whisper_tiny_transcribes_the_jfk_recording() {
    let model = std::env::var("DECKCRAFT_WHISPER_MODEL").expect("set DECKCRAFT_WHISPER_MODEL to the model directory");
    let wav = std::env::var("DECKCRAFT_WHISPER_WAV").expect("set DECKCRAFT_WHISPER_WAV to the recording");
    let (samples, rate, channels) = read_wav(Path::new(&wav)).expect("the recording reads");
    let pcm = AudioBuffer::new(samples, rate, channels).mono_16k();
    assert!(pcm.len() > 16_000, "the recording is longer than a second");

    let load = Instant::now();
    let mut engine = WhisperEngine::load(Path::new(&model)).expect("the model loads");
    let loaded = load.elapsed();

    let run = Instant::now();
    let text = engine.transcribe(&pcm, Some("en")).expect("the recording decodes");
    let elapsed = run.elapsed();
    eprintln!("load: {loaded:?}, transcribe {} ms of audio: {elapsed:?}", pcm.len() / 16);
    eprintln!("transcript: {text}");

    let normalised: String = text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    assert!(normalised.contains("and so my fellow americans"), "expected the President's sentence, got: {text}");
    assert!(normalised.contains("ask not what your country can do for you"), "the rest of the sentence: {text}");
}

#[test]
#[ignore = "needs DECKCRAFT_WHISPER_MODEL and DECKCRAFT_WHISPER_WAV; prints latency by window length"]
fn whisper_tiny_latency_by_window() {
    let model = std::env::var("DECKCRAFT_WHISPER_MODEL").expect("model");
    let wav = std::env::var("DECKCRAFT_WHISPER_WAV").expect("wav");
    let (samples, rate, channels) = read_wav(Path::new(&wav)).expect("reads");
    let pcm = AudioBuffer::new(samples, rate, channels).mono_16k();
    let mut engine = WhisperEngine::load(Path::new(&model)).expect("loads");
    let warm = Instant::now();
    let _ = engine.transcribe(&vec![0.0f32; 16_000], Some("en")).expect("warms up");
    let warm = warm.elapsed();
    eprintln!("silent warm-up: {warm:?}");
    // A bound, not a benchmark: the measurement in docs/captions.md is 3.1 s on a loaded laptop;
    // 60 s catches a stalled decode without failing on a busy CI machine.
    assert!(warm < std::time::Duration::from_secs(60), "the warm-up stalled: {warm:?}");
    for secs in [2usize, 4, 8, 16, 30] {
        let n = (secs * 16_000).min(pcm.len());
        let slice = &pcm[pcm.len() - n..];
        let t = Instant::now();
        let text = engine.transcribe(slice, Some("en")).expect("decodes");
        let elapsed = t.elapsed();
        eprintln!("{secs}s window: {elapsed:?}, {} chars: {text}", text.chars().count());
        assert!(elapsed < std::time::Duration::from_secs(120), "a {secs}s window took {elapsed:?}");
        assert!(text.chars().count() <= 400, "a decode ran away: {} chars", text.chars().count());
    }
}

/// Not ignored: the loader's verification is pure file handling and needs no weights. A directory
/// with none of the manifest's files must be refused with an explanation, not a panic.
#[test]
fn loading_an_empty_directory_is_an_error_not_a_panic() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("deckcraft-whisper-empty-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let Err(err) = WhisperEngine::load(&dir) else { panic!("an empty directory loaded as a model") };
    assert!(err.message().contains("manifest"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
