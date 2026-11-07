//! The Whisper model: loading `openai/whisper-tiny` from the model directory and transcribing a
//! 30-second window with greedy decoding and no timestamps.
//!
//! candle runs the weights on the CPU; the files are the user's (see [`deckcraft_asr::model`]).

use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self, Config, N_FRAMES, N_SAMPLES, audio, model::Whisper};
use deckcraft_asr::{Accelerator, AsrError};
use tokenizers::Tokenizer;

use crate::mel;

/// The Whisper language tokens, as the tokenizer spells them. Detection picks one of these.
pub const LANGUAGE_CODES: [&str; 99] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms",
    "cs", "ro", "da", "hu", "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc", "ka", "be",
    "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha",
    "ba", "jw", "su",
];

/// A loaded Whisper model.
pub struct WhisperEngine {
    model: Whisper,
    tokenizer: Tokenizer,
    config: Config,
    mel_filters: Vec<f32>,
    device: Device,
    sot: u32,
    transcribe: u32,
    no_ts: u32,
    eot: u32,
    no_speech: Vec<u32>,
    suppress: Vec<u32>,
    languages: Vec<(String, u32)>,
}

impl WhisperEngine {
    /// Load `model.safetensors`, `config.json` and `tokenizer.json` from `dir`.
    pub fn load(dir: &Path) -> Result<WhisperEngine, AsrError> {
        let config_path = dir.join("config.json");
        let config_bytes =
            std::fs::read(&config_path).map_err(|e| AsrError::new(format!("cannot read the model config ({}): {e}", config_path.display())))?;
        let config: Config = serde_json::from_slice(&config_bytes).map_err(|e| AsrError::new(format!("the model config is damaged: {e}")))?;
        let tokenizer_path = dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| AsrError::new(format!("cannot read the tokenizer: {e}")))?;
        let weights_path = dir.join("model.safetensors");
        let meta =
            std::fs::metadata(&weights_path).map_err(|e| AsrError::new(format!("cannot read the weights ({}): {e}", weights_path.display())))?;
        if !meta.is_file() || meta.len() == 0 {
            return Err(AsrError::new("the weights file is empty"));
        }
        let weights = std::fs::read(&weights_path).map_err(|e| AsrError::new(format!("cannot load the weights: {e}")))?;
        let device = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(weights, whisper::DTYPE, &device)
            .map_err(|e| AsrError::new(format!("the weights are damaged: {e}")))?;
        let model = Whisper::load(&vb, config.clone()).map_err(|e| AsrError::new(format!("the model could not be built: {e}")))?;
        let special = |name: &str| -> Result<u32, AsrError> {
            tokenizer.token_to_id(name).ok_or_else(|| AsrError::new(format!("the tokenizer has no {name}")))
        };
        let no_speech = whisper::NO_SPEECH_TOKENS.iter().filter_map(|t| tokenizer.token_to_id(t)).collect();
        let suppress = config.suppress_tokens.clone();
        let languages = LANGUAGE_CODES
            .iter()
            .filter_map(|code| tokenizer.token_to_id(&format!("<|{code}|>")).map(|id| ((*code).to_string(), id)))
            .collect::<Vec<_>>();
        let n_mel = config.num_mel_bins;
        Ok(WhisperEngine {
            sot: special(whisper::SOT_TOKEN)?,
            transcribe: special(whisper::TRANSCRIBE_TOKEN)?,
            no_ts: special(whisper::NO_TIMESTAMPS_TOKEN)?,
            eot: special(whisper::EOT_TOKEN)?,
            no_speech,
            suppress,
            languages,
            mel_filters: mel::mel_filters(n_mel),
            model,
            tokenizer,
            config,
            device,
        })
    }

    /// The model's vocabulary size (for diagnostics).
    pub fn vocab_size(&self) -> usize {
        self.config.vocab_size
    }

    /// Transcribe up to 30 seconds of 16 kHz mono audio. `language` is a Whisper language token
    /// (`"en"`, `"es"`); `None` detects it from the audio.
    pub fn transcribe(&mut self, audio: &[f32], language: Option<&str>) -> Result<String, AsrError> {
        let pcm = window(audio);
        let encode_start = std::time::Instant::now();
        let features = self.encode(&pcm)?;
        log::debug!("subtitles: encoder {:?}", encode_start.elapsed());
        let lang = match language.map(str::trim).filter(|l| !l.is_empty()) {
            Some(tag) => self.language_token(tag)?,
            None => self.detect_language(&features)?,
        };
        let mut tokens = vec![self.sot, lang, self.transcribe, self.no_ts];
        let decode_start = std::time::Instant::now();
        let max = self.config.max_target_positions.min(224);
        let mut ids: Vec<u32> = Vec::new();
        for step in 0..max {
            let input = Tensor::new(tokens.as_slice(), &self.device).map_err(tensor_error)?.unsqueeze(0).map_err(tensor_error)?;
            let hidden = self.model.decoder.forward(&input, &features, step == 0).map_err(tensor_error)?;
            let (_, seq_len, _) = hidden.dims3().map_err(tensor_error)?;
            // `forward` returns the decoder's hidden state; the vocabulary logits come from the
            // tied input embedding (the final linear), at the last position only.
            let last = hidden.narrow(1, seq_len.saturating_sub(1), 1).map_err(tensor_error)?;
            let logits = self.model.decoder.final_linear(&last).map_err(tensor_error)?;
            let mut row = logits.i((0, 0)).map_err(tensor_error)?.to_vec1::<f32>().map_err(tensor_error)?;
            for id in &self.suppress {
                if let Some(slot) = row.get_mut(*id as usize) {
                    *slot = f32::NEG_INFINITY;
                }
            }
            let Some(next) = argmax(&row) else { break };
            if next == self.eot || self.no_speech.contains(&next) {
                break;
            }
            tokens.push(next);
            ids.push(next);
        }
        log::debug!("subtitles: {} tokens in {:?}", ids.len(), decode_start.elapsed());
        let text = self.tokenizer.decode(&ids, true).map_err(|e| AsrError::new(format!("cannot decode the transcript: {e}")))?;
        Ok(text.trim().to_string())
    }

    fn language_token(&self, tag: &str) -> Result<u32, AsrError> {
        let name = format!("<|{tag}|>");
        self.tokenizer.token_to_id(&name).ok_or_else(|| AsrError::new(format!("`{tag}` is not a Whisper language")))
    }

    fn encode(&mut self, pcm: &[f32]) -> Result<Tensor, AsrError> {
        let n_mel = self.config.num_mel_bins;
        let mel = audio::pcm_to_mel(&self.config, pcm, &self.mel_filters);
        let frames = mel.len().checked_div(n_mel).unwrap_or(0);
        if frames < N_FRAMES {
            return Err(AsrError::new("the audio window is too short for the model"));
        }
        let mel = Tensor::from_vec(mel, (1, n_mel, frames), &self.device).map_err(tensor_error)?;
        let mel = mel.narrow(2, 0, N_FRAMES).map_err(tensor_error)?;
        self.model.reset_kv_cache();
        self.model.encoder.forward(&mel, true).map_err(tensor_error)
    }

    /// Whisper's own language detection: the most likely language token after the start token.
    fn detect_language(&mut self, features: &Tensor) -> Result<u32, AsrError> {
        self.model.decoder.reset_kv_cache();
        let input = Tensor::new(&[self.sot][..], &self.device).map_err(tensor_error)?.unsqueeze(0).map_err(tensor_error)?;
        let hidden = self.model.decoder.forward(&input, features, true).map_err(tensor_error)?;
        let logits = self.model.decoder.final_linear(&hidden).map_err(tensor_error)?;
        let row = logits.i((0, 0)).map_err(tensor_error)?.to_vec1::<f32>().map_err(tensor_error)?;
        let mut best: Option<(f32, u32)> = None;
        for (_, id) in &self.languages {
            let Some(p) = row.get(*id as usize).copied().filter(|p| p.is_finite()) else { continue };
            if best.is_none_or(|(b, _)| p > b) {
                best = Some((p, *id));
            }
        }
        best.map(|(_, id)| id).ok_or_else(|| AsrError::new("the model recognized no language"))
    }
}

/// Copy the last 30 seconds of `audio` into a fixed window, padding the end with silence (the
/// window the encoder always takes) and clamping anything that is not a sample.
fn window(audio: &[f32]) -> Vec<f32> {
    let mut pcm = vec![0.0f32; N_SAMPLES];
    let tail = &audio[audio.len().saturating_sub(N_SAMPLES)..];
    for (slot, sample) in pcm.iter_mut().zip(tail) {
        *slot = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };
    }
    pcm
}

fn tensor_error(e: candle_core::Error) -> AsrError {
    AsrError::new(format!("the model failed: {e}"))
}

fn argmax(values: &[f32]) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for (i, &p) in values.iter().enumerate() {
        if !p.is_finite() {
            continue;
        }
        if best.is_none_or(|(b, _)| p > b) {
            best = Some((p, u32::try_from(i).unwrap_or(u32::MAX)));
        }
    }
    best.map(|(_, i)| i)
}

/// The float type candle stores the model in.
pub const DTYPE: DType = whisper::DTYPE;

/// The accelerator this backend runs on. A future OpenVINO backend implements the same
/// [`WhisperEngine`] surface and returns its own; callers ask
/// [`crate::capabilities`] rather than assuming this.
pub const ACCELERATOR: Accelerator = Accelerator::Cpu;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_exactly_thirty_seconds() {
        assert_eq!(window(&[0.5; 100]).len(), N_SAMPLES);
        let long = vec![0.25f32; N_SAMPLES + 10];
        let w = window(&long);
        assert_eq!(w.len(), N_SAMPLES);
        assert!(w.iter().all(|s| (*s - 0.25).abs() < 1e-9), "the last 30 seconds are kept");
        let hostile = [f32::NAN, f32::INFINITY, -4.0, 0.5];
        let w = window(&hostile);
        assert_eq!(&w[..4], &[0.0, 1.0, -1.0, 0.5]);
    }

    #[test]
    fn argmax_skips_non_finite_logits() {
        assert_eq!(argmax(&[f32::NAN, 0.1, 0.9, f32::NEG_INFINITY]), Some(2));
        assert_eq!(argmax(&[f32::NEG_INFINITY; 4]), None);
        assert_eq!(argmax(&[]), None);
    }

    #[test]
    #[ignore = "needs a downloaded model in DECKCRAFT_WHISPER_MODEL"]
    fn whisper_tiny_loads_and_decodes_a_silent_window() {
        let dir = std::env::var("DECKCRAFT_WHISPER_MODEL").expect("set DECKCRAFT_WHISPER_MODEL to the model directory");
        let mut engine = WhisperEngine::load(Path::new(&dir)).expect("model loads");
        assert!(engine.vocab_size() > 50_000);
        let text = engine.transcribe(&vec![0.0; 16_000], Some("en")).expect("silence decodes");
        assert!(text.chars().count() < 160, "silence is not a speech: {text}");
        let detected = engine.transcribe(&vec![0.0; 16_000], None).expect("detection runs");
        assert!(detected.chars().count() < 160);
    }
}
