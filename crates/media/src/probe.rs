//! Media probing: container, duration, audio and video stream parameters — without decoding
//! (except for audio files whose container doesn't state a length).

use crate::video::{codec_name, mkv_codec_name};
use crate::{ASF_HEADER, Bytes, Container, MediaError, Result};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// DeckCraft can decode it.
    pub decodable: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: u64,
    pub decodable: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaInfo {
    pub container: &'static str,
    /// Duration in milliseconds (0 when unknown).
    pub duration_ms: u64,
    pub audio: Option<AudioInfo>,
    pub video: Option<VideoInfo>,
}

impl MediaInfo {
    pub fn duration(&self) -> f64 {
        self.duration_ms as f64 / 1000.0
    }
    /// Something in it can be played.
    pub fn playable(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.decodable) || self.video.as_ref().is_some_and(|v| v.decodable)
    }
}

fn ms(secs: f64) -> u64 {
    if secs.is_finite() && secs > 0.0 { (secs * 1000.0).round() as u64 } else { 0 }
}

/// Probe `bytes`.
pub fn probe(bytes: &Bytes) -> Result<MediaInfo> {
    let mut info = probe_headers(bytes)?;
    if info.duration_ms == 0 && info.audio.as_ref().is_some_and(|a| a.decodable) && info.video.is_none() {
        // No length in the header (raw MP3 / ADTS without an index): decode to measure.
        if let Ok(pcm) = crate::audio::decode(bytes) {
            info.duration_ms = ms(pcm.duration());
        }
    }
    Ok(info)
}

/// [`probe`] from the headers alone, without decoding: a raw MP3 or ADTS stream without an
/// index reports a `duration_ms` of 0.
pub fn probe_headers(bytes: &Bytes) -> Result<MediaInfo> {
    let c = Container::sniff(bytes);
    let mut info = match c {
        Container::Mp4 => probe_mp4(bytes)?,
        Container::Matroska => probe_mkv(bytes)?,
        Container::Asf => probe_asf(bytes)?,
        Container::Ogg => probe_ogg(bytes).unwrap_or_else(|| MediaInfo { container: "ogg", ..Default::default() }),
        _ => MediaInfo { container: c.name(), ..Default::default() },
    };
    if info.audio.is_none() && info.video.is_none() || info.audio.as_ref().is_some_and(|a| a.sample_rate == 0) {
        // Plain audio files (and anything our demuxers didn't describe): symphonia's probe.
        if c == Container::Mp4 && !crate::mp4guard::tables_fit(bytes) {
            return Err(MediaError::Corrupt("the MP4's sample tables do not fit the file".into()));
        }
        let (codec, rate, channels, frames) = std::panic::catch_unwind(|| crate::audio::probe_symphonia(bytes, c.name()))
            .ok()
            .flatten()
            .ok_or_else(|| MediaError::Unsupported("this kind of media file".into()))?;
        info.audio = Some(AudioInfo { codec: codec_label(&codec), sample_rate: rate, channels, decodable: true });
        if info.duration_ms == 0
            && let Some(n) = frames
            && rate > 0
        {
            info.duration_ms = ms(n as f64 / rate as f64);
        }
    }
    Ok(info)
}

fn codec_label(short: &str) -> String {
    match short {
        "mp3" => "MP3".into(),
        "mp2" => "MP2".into(),
        "mp1" => "MP1".into(),
        "aac" => "AAC".into(),
        "alac" => "ALAC".into(),
        "flac" => "FLAC".into(),
        "vorbis" => "Vorbis".into(),
        s if s.starts_with("pcm") => "PCM".into(),
        s if s.starts_with("adpcm") => "ADPCM".into(),
        s => s.to_string(),
    }
}

fn probe_mp4(bytes: &Bytes) -> Result<MediaInfo> {
    use deckcraft_isobmff::{CodecConfig, TrackKind};
    let src: &[u8] = bytes;
    let f = deckcraft_isobmff::open(src).map_err(|e| MediaError::Corrupt(e.to_string()))?;
    let mut info = MediaInfo { container: if f.is_quicktime { "mov" } else { "mp4" }, ..Default::default() };
    let mut dur = if f.timescale > 0 { f.duration as f64 / f.timescale as f64 } else { 0.0 };
    if let Some(ti) = f.track_of_kind(TrackKind::Video) {
        let t = &f.tracks[ti];
        let secs = if t.timescale > 0 { t.duration as f64 / t.timescale as f64 } else { 0.0 };
        if let Some(e) = t.entries.first() {
            let (w, h) = e.video.as_ref().map(|v| (v.width as u32, v.height as u32)).unwrap_or((t.width, t.height));
            let decodable = matches!(e.codec, CodecConfig::Avc(_) | CodecConfig::Hevc(_) | CodecConfig::Vp9(_) | CodecConfig::Av1(_));
            let frames = t.samples.len() as u64;
            info.video = Some(VideoInfo {
                codec: codec_name(&e.codec),
                width: w,
                height: h,
                fps: if secs > 0.0 { frames as f64 / secs } else { 0.0 },
                frames,
                decodable,
            });
        }
        dur = dur.max(secs);
    }
    if let Some(ti) = f.track_of_kind(TrackKind::Audio) {
        let t = &f.tracks[ti];
        if let Some(e) = t.entries.first() {
            let (rate, channels) = e.audio.as_ref().map(|a| (a.sample_rate.round() as u32, a.channels as u16)).unwrap_or((0, 0));
            let decodable =
                matches!(e.codec, CodecConfig::Aac(_) | CodecConfig::Mp3 | CodecConfig::Pcm(_) | CodecConfig::Alac { .. } | CodecConfig::Flac(_));
            info.audio = Some(AudioInfo { codec: codec_name(&e.codec), sample_rate: rate, channels, decodable });
        }
        if t.timescale > 0 {
            dur = dur.max(t.duration as f64 / t.timescale as f64);
        }
    }
    info.duration_ms = ms(dur);
    Ok(info)
}

fn probe_mkv(bytes: &Bytes) -> Result<MediaInfo> {
    use deckcraft_matroska::{Codec, TrackKind};
    let src: &[u8] = bytes;
    let f = deckcraft_matroska::open(src).map_err(|e| MediaError::Corrupt(e.to_string()))?;
    let mut info = MediaInfo { container: if f.is_webm() { "webm" } else { "matroska" }, ..Default::default() };
    let dur = f.duration_ns().map(|n| n as f64 / 1e9).unwrap_or(0.0);
    info.duration_ms = ms(dur);
    if let Some(t) = f.tracks.iter().find(|t| t.kind == TrackKind::Video) {
        let (w, h) = t.video.as_ref().map(|v| (v.pixel_width, v.pixel_height)).unwrap_or((0, 0));
        let frames = t.samples.len() as u64;
        let fps = match t.default_duration_ns {
            Some(d) if d > 0 => 1e9 / d as f64,
            _ if dur > 0.0 => frames as f64 / dur,
            _ => 0.0,
        };
        let decodable = matches!(t.codec, Codec::Avc { .. } | Codec::Hevc { .. } | Codec::Vp9 { .. } | Codec::Av1 { .. });
        info.video = Some(VideoInfo { codec: mkv_codec_name(&t.codec), width: w, height: h, fps, frames, decodable });
    }
    if let Some(t) = f.tracks.iter().find(|t| t.kind == TrackKind::Audio) {
        let (rate, channels) = t
            .audio
            .as_ref()
            .map(|a| (a.output_sampling_frequency.unwrap_or(a.sampling_frequency).round() as u32, a.channels as u16))
            .unwrap_or((0, 0));
        let rate = if matches!(t.codec, Codec::Opus { .. }) { 48_000 } else { rate };
        let decodable =
            matches!(t.codec, Codec::Opus { .. } | Codec::Vorbis { .. } | Codec::Aac { .. } | Codec::Flac { .. } | Codec::Mp3 | Codec::Pcm { .. });
        info.audio = Some(AudioInfo { codec: mkv_codec_name(&t.codec), sample_rate: rate, channels, decodable });
    }
    Ok(info)
}

fn probe_ogg(bytes: &Bytes) -> Option<MediaInfo> {
    let src: &[u8] = bytes;
    let f = deckcraft_ogg::open(src).ok()?;
    let s = f.stream_of(deckcraft_ogg::Codec::Opus)?;
    let st = f.streams.get(s)?;
    let head = deckcraft_opus::OpusHead::parse(st.headers.first()?).ok()?;
    let timing = deckcraft_ogg::OpusTiming::of(st, head.pre_skip as u32).ok()?;
    Some(MediaInfo {
        container: "ogg",
        duration_ms: ms(timing.total as f64 / 48_000.0),
        audio: Some(AudioInfo { codec: "Opus".into(), sample_rate: 48_000, channels: head.channels as u16, decodable: true }),
        video: None,
    })
}

/// Windows Media (ASF): enough of the header to describe the file. We have no WMA/WMV decoder.
fn probe_asf(b: &[u8]) -> Result<MediaInfo> {
    const FILE_PROPS: [u8; 16] = [0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65];
    const STREAM_PROPS: [u8; 16] = [0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65];
    const AUDIO_MEDIA: [u8; 16] = [0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B];
    const VIDEO_MEDIA: [u8; 16] = [0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B];
    let u16le = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).unwrap_or(0);
    let u32le = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).unwrap_or(0);
    let u64le = |o: usize| b.get(o..o + 8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes).unwrap_or(0);
    if b.get(..16) != Some(&ASF_HEADER[..]) {
        return Err(MediaError::Corrupt("not an ASF file".into()));
    }
    let header_end = (u64le(16) as usize).min(b.len());
    let mut info = MediaInfo { container: "asf", ..Default::default() };
    let mut pos: usize = 30;
    while pos.checked_add(24).is_some_and(|end| end <= header_end) {
        let guid = &b[pos..pos + 16];
        let size = u64le(pos + 16) as usize;
        if size < 24 {
            break;
        }
        let d = pos + 24;
        if guid == FILE_PROPS {
            // file id 16, file size 8, creation 8, packets 8, play duration 8 (100 ns), send 8, preroll 8 (ms)
            let play = u64le(d + 40) as f64 / 1e7;
            let preroll = u64le(d + 56) as f64 / 1e3;
            info.duration_ms = ms(play - preroll);
        } else if guid == STREAM_PROPS {
            let kind = b.get(d..d + 16);
            // type 16, error correction type 16, time offset 8, type data len 4, ec len 4, flags 2, reserved 4
            let td = d + 54;
            if kind == Some(&AUDIO_MEDIA[..]) {
                let tag = u16le(td);
                let codec = match tag {
                    0x0160 => "WMA 1",
                    0x0161 => "WMA 2",
                    0x0162 => "WMA Pro",
                    0x0163 => "WMA Lossless",
                    0x000A => "WMA Voice",
                    _ => "WMA",
                };
                info.audio = Some(AudioInfo { codec: codec.into(), sample_rate: u32le(td + 4), channels: u16le(td + 2), decodable: false });
            } else if kind == Some(&VIDEO_MEDIA[..]) {
                info.video = Some(VideoInfo { codec: "WMV".into(), width: u32le(td), height: u32le(td + 4), decodable: false, ..Default::default() });
            }
        }
        // A hostile size must not wrap the walk (or walk off the end): an object that
        // overruns the header ends it, which is also the correct ASF semantics.
        let Some(next) = pos.checked_add(size).filter(|&p| p <= header_end) else { break };
        pos = next;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_asf_object_sizes_end_the_walk() {
        // The header object GUID followed by 48 × 0xFF: the walk reads usize::MAX back as
        // every object size and must end instead of wrapping `pos` (and panicking).
        let mut b = crate::ASF_HEADER.to_vec();
        b.extend_from_slice(&[0xFF; 48]);
        assert!(matches!(probe_asf(&b), Ok(info) if info.container == "asf"));
    }
}
