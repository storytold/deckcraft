//! The Whisper model: loading `openai/whisper-tiny` from the model directory and transcribing a
//! 30-second window with greedy decoding and no timestamps.
//!
//! candle runs the weights on the CPU; the files are the user's (see [`deckcraft_asr::model`]).

use std::io::Read;
use std::path::Path;

use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self, Config, N_FRAMES, N_SAMPLES, audio, model::Whisper};
use deckcraft_asr::settings::LANGUAGE_CODES;
use deckcraft_asr::{AsrError, model, whisper_tiny};
use tokenizers::Tokenizer;

use crate::mel;

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
    ///
    /// The files are checked against the manifest (size and SHA-256) before anything is read or
    /// parsed: the directory may be one the user picked, and a file that is present is not
    /// necessarily the model. Every read is bounded by the manifest's recorded size, so a
    /// replaced or truncated file cannot ask for an unbounded allocation.
    pub fn load(dir: &Path) -> Result<WhisperEngine, AsrError> {
        let manifest = whisper_tiny();
        manifest.validate().map_err(|e| AsrError::new(format!("the model manifest is damaged: {e}")))?;
        for file in &manifest.files {
            model::verify_file(dir, file).map_err(|e| AsrError::new(format!("the model files do not match the manifest: {e}")))?;
        }
        let limit = |name: &str| manifest.file(name).map_or(0, |f| f.bytes);
        let config_bytes = bounded_read(&dir.join("config.json"), limit("config.json"))?;
        let config: Config = serde_json::from_slice(&config_bytes).map_err(|e| AsrError::new(format!("the model config is damaged: {e}")))?;
        validate_config(&config)?;
        let weights_path = dir.join("model.safetensors");
        let tokenizer_path = dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| AsrError::new(format!("cannot read the tokenizer: {e}")))?;
        let weights = bounded_read(&weights_path, limit("model.safetensors"))?;
        if weights.is_empty() {
            return Err(AsrError::new("the weights file is empty"));
        }
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
        // Look the tag up among the model's languages: the tokenizer also knows special tokens
        // that are not languages (`<|transcribe|>`), which must not become the prompt.
        self.languages
            .iter()
            .find(|(code, _)| code == tag)
            .map(|(_, id)| *id)
            .ok_or_else(|| AsrError::new(format!("`{tag}` is not a Whisper language")))
    }

    fn encode(&mut self, pcm: &[f32]) -> Result<Tensor, AsrError> {
        let n_mel = self.config.num_mel_bins;
        if n_mel == 0 {
            return Err(AsrError::new("the model config has no mel bins"));
        }
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
        // A NaN has no amplitude, so it becomes silence; an infinity is clamped like any other
        // out-of-range sample.
        *slot = if sample.is_nan() { 0.0 } else { sample.clamp(-1.0, 1.0) };
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

/// Read at most `limit` bytes, refusing a file that claims more before allocating. The manifest's
/// recorded size is the limit; `0` (no manifest entry) reads nothing.
fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, AsrError> {
    if limit == 0 {
        return Err(AsrError::new(format!("{} is not part of the model manifest", path.display())));
    }
    let meta = std::fs::metadata(path).map_err(|e| AsrError::new(format!("cannot read {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(AsrError::new(format!("{} is not a file", path.display())));
    }
    if meta.len() > limit {
        return Err(AsrError::new(format!("{} is larger than the manifest says", path.display())));
    }
    let mut f = std::fs::File::open(path).map_err(|e| AsrError::new(format!("cannot read {}: {e}", path.display())))?;
    let mut out = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    // One byte past the limit, so a file that grew between `metadata` and the read is refused
    // rather than truncated silently.
    f.by_ref().take(limit.saturating_add(1)).read_to_end(&mut out).map_err(|e| AsrError::new(format!("cannot read {}: {e}", path.display())))?;
    if out.len() as u64 > limit {
        return Err(AsrError::new(format!("{} is larger than the manifest says", path.display())));
    }
    Ok(out)
}

/// Check the dimensions `config.json` asks for before they reach an allocation or a tensor shape.
/// The file is user-writable, so every count is bounded and the mel filter table is sized with
/// checked arithmetic.
fn validate_config(c: &Config) -> Result<(), AsrError> {
    let out_of_range = |what: &str, value: usize| AsrError::new(format!("the model config's {what} ({value}) is out of range"));
    if !matches!(c.num_mel_bins, 80 | 128) {
        return Err(out_of_range("mel bins", c.num_mel_bins));
    }
    if c.num_mel_bins.checked_mul(mel::FFT_BINS).is_none() {
        return Err(out_of_range("mel bins", c.num_mel_bins));
    }
    if c.vocab_size == 0 || c.vocab_size > 60_000 {
        return Err(out_of_range("vocabulary size", c.vocab_size));
    }
    if c.max_target_positions == 0 || c.max_target_positions > 1_024 {
        return Err(out_of_range("context length", c.max_target_positions));
    }
    if c.max_source_positions == 0 || c.max_source_positions > 3_000 {
        return Err(out_of_range("audio context length", c.max_source_positions));
    }
    if c.d_model == 0 || c.d_model > 2_048 {
        return Err(out_of_range("model width", c.d_model));
    }
    for (what, heads) in [("encoder heads", c.encoder_attention_heads), ("decoder heads", c.decoder_attention_heads)] {
        if heads == 0 || heads > 64 || !c.d_model.is_multiple_of(heads) {
            return Err(out_of_range(what, heads));
        }
    }
    for (what, layers) in [("encoder layers", c.encoder_layers), ("decoder layers", c.decoder_layers)] {
        if layers == 0 || layers > 64 {
            return Err(out_of_range(what, layers));
        }
    }
    if c.suppress_tokens.iter().any(|t| *t as usize >= c.vocab_size) {
        return Err(AsrError::new("the model config suppresses a token the vocabulary does not have"));
    }
    Ok(())
}

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

    fn tiny_config() -> Config {
        Config {
            num_mel_bins: 80,
            max_source_positions: 1_500,
            d_model: 384,
            encoder_attention_heads: 6,
            encoder_layers: 4,
            vocab_size: 51_865,
            max_target_positions: 448,
            decoder_attention_heads: 6,
            decoder_layers: 4,
            suppress_tokens: vec![],
        }
    }

    #[test]
    fn hostile_config_dimensions_are_refused_before_allocation() {
        assert!(validate_config(&tiny_config()).is_ok());
        let cases: Vec<(&str, Box<dyn Fn(&mut Config)>)> = vec![
            ("mel bins", Box::new(|c: &mut Config| c.num_mel_bins = usize::MAX)),
            ("vocabulary size", Box::new(|c: &mut Config| c.vocab_size = usize::MAX)),
            ("context length", Box::new(|c: &mut Config| c.max_target_positions = usize::MAX)),
            ("audio context length", Box::new(|c: &mut Config| c.max_source_positions = 0)),
            ("model width", Box::new(|c: &mut Config| c.d_model = 0)),
            ("encoder heads", Box::new(|c: &mut Config| c.encoder_attention_heads = 0)),
            ("decoder heads", Box::new(|c: &mut Config| c.decoder_attention_heads = 5)),
            ("encoder layers", Box::new(|c: &mut Config| c.encoder_layers = usize::MAX)),
            ("decoder layers", Box::new(|c: &mut Config| c.decoder_layers = 65)),
        ];
        for (what, change) in cases {
            let mut c = tiny_config();
            change(&mut c);
            assert!(validate_config(&c).is_err(), "{what} was accepted");
        }
        let mut c = tiny_config();
        c.suppress_tokens = vec![u32::MAX];
        assert!(validate_config(&c).is_err(), "a suppressed token outside the vocabulary was accepted");
    }

    #[test]
    fn a_read_is_bounded_by_the_manifest_size() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("deckcraft-asr-candle-bounded-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("weights.bin");
        std::fs::write(&file, b"0123456789").unwrap();
        assert_eq!(bounded_read(&file, 10).unwrap().len(), 10);
        assert!(bounded_read(&file, 9).is_err(), "a file past the limit is refused");
        assert!(bounded_read(&file, 0).is_err(), "a file with no manifest entry is refused");
        assert!(bounded_read(&dir.join("missing"), 10).is_err());
        // The read itself must not exceed the limit even when the file grew after the metadata
        // check; a file written between the two calls is caught by the second bound.
        assert!(bounded_read(&dir, 10).is_err(), "a directory is not a file");
        let _ = std::fs::remove_dir_all(&dir);
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
