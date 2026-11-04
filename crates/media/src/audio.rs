//! Audio decoding to interleaved f32 PCM.
//!
//! Opus (Ogg, WebM / Matroska) uses our own decoder; everything else — MP3, AAC (M4A / MP4 / MOV /
//! ADTS), ALAC, FLAC, WAV, AIFF, CAF, Vorbis, PCM in MP4 / Matroska — goes through symphonia
//! (MPL-2.0, used unmodified), with gapless trimming on.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::{Bytes, Container, MediaError, Result};

/// Decoded audio: interleaved f32 samples in −1..1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

const MAX_PCM_SAMPLES: usize = 64 * 1024 * 1024;

impl Pcm {
    /// Sample frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }
    /// Sample `frame` of channel `ch` (0 past the end).
    pub fn at(&self, frame: usize, ch: usize) -> f32 {
        let c = self.channels.max(1) as usize;
        self.samples.get(frame * c + ch.min(c - 1)).copied().unwrap_or(0.0)
    }
    /// Root-mean-square level over all samples.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        (self.samples.iter().map(|s| s * s).sum::<f32>() / self.samples.len() as f32).sqrt()
    }

    fn push_planar(planes: &[Vec<f32>], out: &mut Vec<f32>) {
        let n = planes.iter().map(Vec::len).min().unwrap_or(0);
        let remaining = MAX_PCM_SAMPLES.saturating_sub(out.len());
        let to_push = n.min(remaining / planes.len().max(1));
        out.reserve(to_push * planes.len());
        for i in 0..to_push {
            for p in planes {
                out.push(p[i]);
            }
        }
    }
}

/// Bytes as a symphonia source without copying them.
struct Shared(Bytes);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Decode the audio of `bytes` (an audio file, or the audio track of a video) to PCM.
pub fn decode(bytes: &Bytes) -> Result<Pcm> {
    match Container::sniff(bytes) {
        Container::Asf => Err(MediaError::Unsupported("Windows Media Audio (WMA)".into())),
        Container::Ogg => match decode_ogg_opus(bytes) {
            Some(r) => r,
            None => contained(|| decode_symphonia(bytes, "ogg")),
        },
        Container::Matroska => match decode_mkv_opus(bytes) {
            Some(r) => r,
            None => contained(|| decode_symphonia(bytes, "mkv")),
        },
        Container::Mp4 if !crate::mp4guard::tables_fit(bytes) => Err(MediaError::Corrupt("the MP4's sample tables do not fit the file".into())),
        c => contained(|| decode_symphonia(bytes, c.name())),
    }
}

/// Ogg Opus with our decoder; `None` when the file holds no Opus stream.
fn decode_ogg_opus(bytes: &Bytes) -> Option<Result<Pcm>> {
    let src: &[u8] = bytes;
    let file = deckcraft_ogg::open(src).ok()?;
    let s = file.stream_of(deckcraft_ogg::Codec::Opus)?;
    let stream = file.streams.get(s)?;
    Some((|| {
        let head = stream.headers.first().ok_or(MediaError::Corrupt("Opus header missing".into()))?;
        let mut dec = deckcraft_opus::Decoder::new(head).map_err(|e| MediaError::Corrupt(e.to_string()))?;
        dec.set_trim_pre_skip(true);
        let channels = dec.channels() as u16;
        let timing = deckcraft_ogg::OpusTiming::of(stream, dec.pre_skip() as u32).map_err(|e| MediaError::Corrupt(e.to_string()))?;
        let mut out = Vec::new();
        for i in 0..stream.packets.len() {
            if MAX_PCM_SAMPLES.saturating_sub(out.len()) < channels.max(1) as usize {
                break;
            }
            let Ok(p) = file.read_packet(src, s, i) else { continue };
            if p.is_empty() {
                continue;
            }
            match dec.decode(Some(&p)) {
                Ok(planes) => Pcm::push_planar(&planes, &mut out),
                Err(e) => log::debug!("opus packet {i}: {e}"),
            }
        }
        // End trimming (RFC 7845 §4.5).
        let total = timing.total.max(0) as usize * channels.max(1) as usize;
        if total > 0 && total < out.len() {
            out.truncate(total);
        }
        Ok(Pcm { rate: 48_000, channels, samples: out })
    })())
}

/// WebM / Matroska Opus with our decoder; `None` when there is no Opus track.
fn decode_mkv_opus(bytes: &Bytes) -> Option<Result<Pcm>> {
    let src: &[u8] = bytes;
    let file = deckcraft_matroska::open(src).ok()?;
    let (ti, head) = file.tracks.iter().enumerate().find_map(|(i, t)| match &t.codec {
        deckcraft_matroska::Codec::Opus { head } => Some((i, head.clone())),
        _ => None,
    })?;
    Some((|| {
        let mut dec = deckcraft_opus::Decoder::new(&head).map_err(|e| MediaError::Corrupt(e.to_string()))?;
        dec.set_trim_pre_skip(true);
        let channels = dec.channels() as u16;
        let mut out = Vec::new();
        let n = file.tracks.get(ti).map(|t| t.samples.len()).unwrap_or(0);
        for i in 0..n {
            if MAX_PCM_SAMPLES.saturating_sub(out.len()) < channels.max(1) as usize {
                break;
            }
            let Ok(p) = file.read_sample(src, ti, i) else { continue };
            if p.is_empty() {
                continue;
            }
            match dec.decode(Some(&p)) {
                Ok(planes) => Pcm::push_planar(&planes, &mut out),
                Err(e) => log::debug!("opus packet {i}: {e}"),
            }
        }
        Ok(Pcm { rate: 48_000, channels, samples: out })
    })())
}

fn sym_err(e: SymError) -> MediaError {
    match e {
        SymError::Unsupported(what) => MediaError::Unsupported(what.to_string()),
        other => MediaError::Corrupt(other.to_string()),
    }
}

/// Runs symphonia, whose demuxers can panic on damaged files (Matroska's EBML reader asserts
/// its position, for one): the panic becomes a corrupt-file error instead of ending the app.
fn contained<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(MediaError::Corrupt("the audio could not be read".into())))
}

/// Demux errors in a row after which a file is taken to have no more audio.
const MAX_DEMUX_ERRORS: u32 = 64;

fn decode_symphonia(bytes: &Bytes, ext: &str) -> Result<Pcm> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(Shared(bytes.clone()))), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    match ext {
        "unknown" => {}
        "mp4" => {
            hint.with_extension("mp4");
        }
        e => {
            hint.with_extension(e);
        }
    }
    let opts = FormatOptions { enable_gapless: true, ..Default::default() };
    let probed = symphonia::default::get_probe().format(&hint, mss, &opts, &MetadataOptions::default()).map_err(|e| match e {
        SymError::Unsupported(_) => MediaError::Unsupported("this audio format".into()),
        other => sym_err(other),
    })?;
    let mut format = probed.format;
    let codecs = symphonia::default::get_codecs();
    // The first track we can decode (a video's audio track follows its video track).
    let (track_id, mut decoder) = format
        .tracks()
        .iter()
        .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .find_map(|t| codecs.make(&t.codec_params, &DecoderOptions::default()).ok().map(|d| (t.id, d)))
        .ok_or(MediaError::Missing("audio track we can decode"))?;
    let mut out: Vec<f32> = Vec::new();
    let mut rate = 0;
    let mut channels = 0u16;
    let mut buf: Option<SampleBuffer<f32>> = None;
    // A damaged file can make the demuxer report the same error forever without moving on.
    let mut demux_errors = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => {
                demux_errors = 0;
                p
            }
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(SymError::DecodeError(e)) => {
                log::debug!("audio demux: {e}");
                demux_errors += 1;
                if demux_errors > MAX_DEMUX_ERRORS {
                    break;
                }
                continue;
            }
            Err(e) => {
                if out.is_empty() {
                    return Err(sym_err(e));
                }
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let ch = spec.channels.count() as u16;
                if rate == 0 {
                    rate = spec.rate;
                    channels = ch;
                }
                if ch != channels || decoded.frames() == 0 {
                    continue;
                }
                let need = decoded.capacity() as u64;
                if buf.as_ref().is_none_or(|b| (b.capacity() as u64) < need * ch as u64) {
                    buf = Some(SampleBuffer::<f32>::new(need, spec));
                }
                if let Some(b) = buf.as_mut() {
                    b.copy_interleaved_ref(decoded);
                    out.extend_from_slice(b.samples());
                }
            }
            Err(SymError::DecodeError(e)) => log::debug!("audio decode: {e}"),
            Err(SymError::IoError(_)) => continue,
            Err(e) => {
                if out.is_empty() {
                    return Err(sym_err(e));
                }
                break;
            }
        }
    }
    if rate == 0 {
        return Err(MediaError::Corrupt("no audio could be decoded".into()));
    }
    Ok(Pcm { rate, channels: channels.max(1), samples: out })
}

/// Sample rate, channels and frame count from symphonia's container probe (no decoding).
pub(crate) fn probe_symphonia(bytes: &Bytes, ext: &str) -> Option<(String, u32, u16, Option<u64>)> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(Shared(bytes.clone()))), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if ext != "unknown" {
        hint.with_extension(ext);
    }
    let opts = FormatOptions { enable_gapless: true, ..Default::default() };
    let probed = symphonia::default::get_probe().format(&hint, mss, &opts, &MetadataOptions::default()).ok()?;
    let codecs = symphonia::default::get_codecs();
    let t = probed.format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL)?;
    let p = &t.codec_params;
    let name = codecs.get_codec(p.codec).map(|d| d.short_name.to_string()).unwrap_or_else(|| "unknown".into());
    Some((name, p.sample_rate.unwrap_or(0), p.channels.map(|c| c.count() as u16).unwrap_or(0), p.n_frames))
}

/// Interleaved PCM resampled (linear) to `rate`.
pub fn resample(pcm: &Pcm, rate: u32) -> Pcm {
    if pcm.rate == rate || pcm.rate == 0 || rate == 0 {
        return pcm.clone();
    }
    let c = pcm.channels.max(1) as usize;
    let n_in = pcm.frames();
    let n_out = (n_in as u64 * rate as u64 / pcm.rate as u64) as usize;
    let step = pcm.rate as f64 / rate as f64;
    let mut out = Vec::with_capacity(n_out * c);
    for i in 0..n_out {
        let x = i as f64 * step;
        let k = x as usize;
        let f = (x - k as f64) as f32;
        for ch in 0..c {
            let a = pcm.at(k, ch);
            let b = if k + 1 < n_in { pcm.at(k + 1, ch) } else { a };
            out.push(a + (b - a) * f);
        }
    }
    Pcm { rate, channels: pcm.channels, samples: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_duration_and_shape() {
        let pcm = Pcm { rate: 100, channels: 1, samples: (0..100).map(|i| i as f32 / 100.0).collect() };
        let r = resample(&pcm, 200);
        assert_eq!(r.frames(), 200);
        assert!((r.duration() - 1.0).abs() < 1e-9);
        assert!((r.at(101, 0) - 0.505).abs() < 1e-3);
    }

    #[test]
    fn wma_is_reported_unsupported() {
        let mut b = crate::ASF_HEADER.to_vec();
        b.extend_from_slice(&[0; 64]);
        assert!(matches!(decode(&std::sync::Arc::new(b)), Err(MediaError::Unsupported(_))));
    }
}
