//! Video decoding: MP4/MOV and WebM/Matroska demuxing, H.264 / HEVC / VP9 / AV1 decoding (our own
//! decoders), RGBA frames and poster frames.

use std::collections::VecDeque;

use deckcraft_isobmff::CodecConfig;
use deckcraft_matroska::Codec as MkvCodec;

use crate::yuv::{self, Matrix, Planes};
use crate::{Bytes, Container, MediaError, Result};

/// A decoded picture, RGBA8 (straight alpha, always opaque), with its presentation time.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Presentation time in seconds from the start of the media.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A picture from a codec, before colour conversion; `pts` in track ticks.
struct Pic {
    pts: i64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

trait Codec: Send {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<Pic>>;
    fn flush(&mut self) -> Vec<Pic>;
    fn reset(&mut self);
}

fn dec_err(e: impl std::fmt::Display) -> MediaError {
    MediaError::Corrupt(e.to_string())
}

// ---------- codecs ----------

struct H264 {
    avcc: Vec<u8>,
    dec: deckcraft_h264::Decoder,
}

impl H264 {
    fn new(avcc: Vec<u8>) -> Result<H264> {
        let dec = deckcraft_h264::Decoder::from_avcc(&avcc).map_err(dec_err)?;
        Ok(H264 { avcc, dec })
    }
    fn convert(p: deckcraft_h264::Picture) -> Pic {
        let (w, h) = (p.width as usize, p.height as usize);
        let planes = Planes {
            y: &p.y,
            u: &p.u,
            v: &p.v,
            y_stride: p.y_stride,
            c_stride: p.uv_stride,
            width: w,
            height: h,
            sx: u32::from(p.chroma_width < p.width),
            sy: u32::from(p.chroma_height < p.height),
            bits: 8,
            mono: p.chroma_width == 0,
        };
        let rgba = yuv::to_rgba(&planes, Matrix::from_code(p.color.matrix, p.height), p.color.full_range);
        Pic { pts: p.pts, width: p.width, height: p.height, rgba }
    }
}

impl Codec for H264 {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<Pic>> {
        Ok(self.dec.decode(sample, pts).map_err(dec_err)?.into_iter().map(H264::convert).collect())
    }
    fn flush(&mut self) -> Vec<Pic> {
        self.dec.flush().into_iter().map(H264::convert).collect()
    }
    fn reset(&mut self) {
        if let Ok(d) = deckcraft_h264::Decoder::from_avcc(&self.avcc) {
            self.dec = d;
        }
    }
}

/// Convert an HEVC / VP9 style picture (planes of u8 or u16 with strides).
macro_rules! plane_pic {
    ($p:expr, $Plane:path, $sx:expr, $sy:expr, $matrix:expr, $full:expr, $mono:expr) => {{
        let p = $p;
        use $Plane as P;
        let (w, h) = (p.width as usize, p.height as usize);
        let rgba = match (&p.y, &p.u, &p.v) {
            (P::U8(y), P::U8(u), P::U8(v)) => yuv::to_rgba(
                &Planes { y, u, v, y_stride: p.y_stride, c_stride: p.uv_stride, width: w, height: h, sx: $sx, sy: $sy, bits: 8, mono: $mono },
                $matrix,
                $full,
            ),
            (P::U16(y), P::U16(u), P::U16(v)) => yuv::to_rgba(
                &Planes {
                    y,
                    u,
                    v,
                    y_stride: p.y_stride,
                    c_stride: p.uv_stride,
                    width: w,
                    height: h,
                    sx: $sx,
                    sy: $sy,
                    bits: p.bit_depth as u32,
                    mono: $mono,
                },
                $matrix,
                $full,
            ),
            _ => vec![0; w * h * 4],
        };
        Pic { pts: p.pts, width: p.width, height: p.height, rgba }
    }};
}

struct Hevc {
    hvcc: Vec<u8>,
    dec: deckcraft_hevc::Decoder,
}

impl Hevc {
    fn new(hvcc: Vec<u8>) -> Result<Hevc> {
        let dec = deckcraft_hevc::Decoder::from_hvcc(&hvcc).map_err(dec_err)?;
        Ok(Hevc { hvcc, dec })
    }
    fn convert(p: deckcraft_hevc::Picture) -> Pic {
        let sx = u32::from(p.chroma_width < p.width);
        let sy = u32::from(p.chroma_height < p.height);
        let m = Matrix::from_code(p.color.matrix, p.height);
        plane_pic!(&p, deckcraft_hevc::Plane, sx, sy, m, p.color.full_range, p.chroma_width == 0)
    }
}

impl Codec for Hevc {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<Pic>> {
        Ok(self.dec.decode(sample, pts).map_err(dec_err)?.into_iter().map(Hevc::convert).collect())
    }
    fn flush(&mut self) -> Vec<Pic> {
        self.dec.flush().into_iter().map(Hevc::convert).collect()
    }
    fn reset(&mut self) {
        if let Ok(d) = deckcraft_hevc::Decoder::from_hvcc(&self.hvcc) {
            self.dec = d;
        }
    }
}

struct Vp9 {
    dec: deckcraft_vp9::Decoder,
}

impl Vp9 {
    fn convert(p: deckcraft_vp9::Picture) -> Pic {
        let (w, h) = (p.width as usize, p.height as usize);
        if p.color.color_space == 7 {
            // RGB (4:4:4): the planes carry G, B, R.
            let shift = p.bit_depth.saturating_sub(8);
            let at = |pl: &deckcraft_vp9::Plane, i: usize| if i < pl.len() { (pl.get(i) >> shift) as u8 } else { 0 };
            let mut rgba = Vec::with_capacity(w * h * 4);
            for row in 0..h {
                for col in 0..w {
                    let i = row * p.y_stride + col;
                    let c = row * p.uv_stride + col;
                    rgba.extend_from_slice(&[at(&p.v, c), at(&p.y, i), at(&p.u, c), 255]);
                }
            }
            return Pic { pts: p.pts, width: p.width, height: p.height, rgba };
        }
        // color_space (VP9 §7.2.2): 1 BT.601, 2 BT.709, 3 SMPTE-170, 4 SMPTE-240, 5 BT.2020.
        let m = match p.color.color_space {
            1 | 3 => Matrix::Bt601,
            2 | 4 => Matrix::Bt709,
            5 => Matrix::Bt2020,
            _ => Matrix::from_code(2, p.height),
        };
        plane_pic!(&p, deckcraft_vp9::Plane, u32::from(p.subsampling_x), u32::from(p.subsampling_y), m, p.color.full_range, false)
    }
}

impl Codec for Vp9 {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<Pic>> {
        Ok(self.dec.decode(sample, pts).map_err(dec_err)?.into_iter().map(Vp9::convert).collect())
    }
    fn flush(&mut self) -> Vec<Pic> {
        self.dec.flush().into_iter().map(Vp9::convert).collect()
    }
    fn reset(&mut self) {
        self.dec.reset();
    }
}

struct Av1 {
    dec: deckcraft_av1::Decoder,
    config_obus: Vec<u8>,
    primed: bool,
}

impl Av1 {
    fn convert(p: deckcraft_av1::Picture) -> Pic {
        let (w, h) = (p.width as usize, p.height as usize);
        let cw = p.plane_width(1) as usize;
        let rgba = yuv::to_rgba(
            &Planes {
                y: &p.planes[0],
                u: &p.planes[1],
                v: &p.planes[2],
                y_stride: w,
                c_stride: cw,
                width: w,
                height: h,
                sx: p.subsampling_x as u32,
                sy: p.subsampling_y as u32,
                bits: p.bit_depth as u32,
                mono: p.mono_chrome,
            },
            Matrix::from_code(p.matrix_coefficients, p.height),
            p.full_range,
        );
        Pic { pts: p.pts, width: p.width, height: p.height, rgba }
    }
}

impl Codec for Av1 {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<Pic>> {
        if !self.primed {
            self.primed = true;
            if !self.config_obus.is_empty() {
                self.dec.decode(&self.config_obus).map_err(dec_err)?;
            }
        }
        Ok(self.dec.decode_pts(sample, pts).map_err(dec_err)?.into_iter().map(Av1::convert).collect())
    }
    fn flush(&mut self) -> Vec<Pic> {
        self.dec.flush().into_iter().map(Av1::convert).collect()
    }
    fn reset(&mut self) {
        self.dec = deckcraft_av1::Decoder::new();
        self.primed = false;
    }
}

// ---------- containers ----------

/// The video track's samples in decode order: (pts in ticks, sync).
struct Track {
    samples: Vec<(i64, bool)>,
    /// Seconds per tick (num, den).
    timebase: (i64, i64),
    width: u32,
    height: u32,
    codec: String,
}

enum Demux {
    Mp4(Box<deckcraft_isobmff::Mp4File>, usize),
    Mkv(Box<deckcraft_matroska::MkvFile>, usize),
}

impl Demux {
    fn read(&self, bytes: &[u8], i: usize) -> Result<Vec<u8>> {
        match self {
            Demux::Mp4(f, t) => f.read_sample(bytes, *t, i).map_err(dec_err),
            Demux::Mkv(f, t) => f.read_sample(bytes, *t, i).map_err(dec_err),
        }
    }
}

fn open_track(bytes: &[u8]) -> Result<(Demux, Track, Box<dyn Codec>)> {
    match Container::sniff(bytes) {
        Container::Mp4 => {
            let f = deckcraft_isobmff::open(bytes).map_err(dec_err)?;
            let ti = f.track_of_kind(deckcraft_isobmff::TrackKind::Video).ok_or(MediaError::Missing("video track"))?;
            let t = &f.tracks[ti];
            let entry = t.entries.first().ok_or(MediaError::Missing("video track"))?;
            let codec: Box<dyn Codec> = match &entry.codec {
                CodecConfig::Avc(a) => Box::new(H264::new(a.to_bytes())?),
                CodecConfig::Hevc(c) => Box::new(Hevc::new(c.to_bytes())?),
                CodecConfig::Vp9(_) => Box::new(Vp9 { dec: deckcraft_vp9::Decoder::new() }),
                CodecConfig::Av1(c) => Box::new(Av1 { dec: deckcraft_av1::Decoder::new(), config_obus: c.config_obus.clone(), primed: false }),
                other => return Err(MediaError::Unsupported(format!("{} video", codec_name(other)))),
            };
            let (w, h) = entry.video.as_ref().map(|v| (v.width as u32, v.height as u32)).unwrap_or((t.width, t.height));
            let samples = (0..t.samples.len()).map(|i| (t.presentation_pts(i).unwrap_or(0), t.samples[i].is_sync)).collect();
            let track = Track { samples, timebase: (1, t.timescale.max(1) as i64), width: w, height: h, codec: codec_name(&entry.codec) };
            Ok((Demux::Mp4(Box::new(f), ti), track, codec))
        }
        Container::Matroska => {
            let f = deckcraft_matroska::open(bytes).map_err(dec_err)?;
            let ti = f
                .tracks
                .iter()
                .position(|t| t.kind == deckcraft_matroska::TrackKind::Video && !t.samples.is_empty())
                .ok_or(MediaError::Missing("video track"))?;
            let t = &f.tracks[ti];
            let codec: Box<dyn Codec> = match &t.codec {
                MkvCodec::Avc { avcc } => Box::new(H264::new(avcc.clone())?),
                MkvCodec::Hevc { hvcc } => Box::new(Hevc::new(hvcc.clone())?),
                MkvCodec::Vp9 { .. } => Box::new(Vp9 { dec: deckcraft_vp9::Decoder::new() }),
                MkvCodec::Av1 { av1c } => Box::new(Av1 {
                    dec: deckcraft_av1::Decoder::new(),
                    config_obus: deckcraft_isobmff::Av1Config::parse(av1c).map(|c| c.config_obus).unwrap_or_default(),
                    primed: false,
                }),
                other => return Err(MediaError::Unsupported(format!("{} video", mkv_codec_name(other)))),
            };
            let (w, h) = t.video.as_ref().map(|v| (v.pixel_width, v.pixel_height)).unwrap_or((0, 0));
            let samples = t.samples.iter().map(|s| (s.pts, s.keyframe)).collect();
            let track = Track {
                samples,
                timebase: (t.timebase.0.max(1) as i64, t.timebase.1.max(1) as i64),
                width: w,
                height: h,
                codec: mkv_codec_name(&t.codec),
            };
            Ok((Demux::Mkv(Box::new(f), ti), track, codec))
        }
        Container::Asf => Err(MediaError::Unsupported("Windows Media Video (WMV)".into())),
        _ => Err(MediaError::Missing("video track")),
    }
}

pub(crate) fn codec_name(c: &CodecConfig) -> String {
    match c {
        CodecConfig::Avc(_) => "H.264".into(),
        CodecConfig::Hevc(_) => "HEVC".into(),
        CodecConfig::Av1(_) => "AV1".into(),
        CodecConfig::Vp9(_) => "VP9".into(),
        CodecConfig::Apv(_) => "APV".into(),
        CodecConfig::ProRes { .. } => "Apple ProRes".into(),
        CodecConfig::Jpeg { .. } => "Motion JPEG".into(),
        CodecConfig::Dnx { .. } => "DNxHD".into(),
        CodecConfig::Aac(_) => "AAC".into(),
        CodecConfig::Mp3 => "MP3".into(),
        CodecConfig::Pcm(_) => "PCM".into(),
        CodecConfig::Alac { .. } => "ALAC".into(),
        CodecConfig::Opus(_) => "Opus".into(),
        CodecConfig::Flac(_) => "FLAC".into(),
        CodecConfig::Ac3 { .. } => "AC-3".into(),
        CodecConfig::Eac3 { .. } => "E-AC-3".into(),
        CodecConfig::Timecode(_) => "timecode".into(),
        CodecConfig::Unknown { fourcc, .. } => format!("{fourcc:?}"),
    }
}

pub(crate) fn mkv_codec_name(c: &MkvCodec) -> String {
    match c {
        MkvCodec::Avc { .. } => "H.264".into(),
        MkvCodec::Hevc { .. } => "HEVC".into(),
        MkvCodec::Vp8 => "VP8".into(),
        MkvCodec::Vp9 { .. } => "VP9".into(),
        MkvCodec::Av1 { .. } => "AV1".into(),
        MkvCodec::Aac { .. } => "AAC".into(),
        MkvCodec::Opus { .. } => "Opus".into(),
        MkvCodec::Vorbis { .. } => "Vorbis".into(),
        MkvCodec::Flac { .. } => "FLAC".into(),
        MkvCodec::Mp3 => "MP3".into(),
        MkvCodec::Pcm { .. } => "PCM".into(),
        other => other.name(),
    }
}

/// Decodes the video track of a file, frame by frame in presentation order.
pub struct VideoDecoder {
    bytes: Bytes,
    demux: Demux,
    track: Track,
    codec: Box<dyn Codec>,
    /// Next sample to feed (decode order).
    next: usize,
    /// Decoded pictures waiting, in presentation order.
    ready: VecDeque<Pic>,
    /// Frames before this time (s) are dropped (after a seek).
    skip_before: f64,
    eos: bool,
    errors: u32,
}

impl VideoDecoder {
    pub fn open(bytes: Bytes) -> Result<VideoDecoder> {
        let (demux, track, codec) = open_track(&bytes)?;
        Ok(VideoDecoder { bytes, demux, track, codec, next: 0, ready: VecDeque::new(), skip_before: f64::NEG_INFINITY, eos: false, errors: 0 })
    }
    pub fn size(&self) -> (u32, u32) {
        (self.track.width, self.track.height)
    }
    pub fn codec(&self) -> &str {
        &self.track.codec
    }
    pub fn frame_count(&self) -> usize {
        self.track.samples.len()
    }
    fn secs(&self, pts: i64) -> f64 {
        pts as f64 * self.track.timebase.0 as f64 / self.track.timebase.1 as f64
    }

    /// The next frame in presentation order, `None` at the end.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        loop {
            // A picture is safe to emit once the decoder has returned it (they come out in
            // presentation order); keep the queue sorted for decoders that emit in batches.
            if let Some(p) = self.ready.pop_front() {
                let time = self.secs(p.pts);
                if time + 1e-6 < self.skip_before {
                    continue;
                }
                return Ok(Some(Frame { time, width: p.width, height: p.height, rgba: p.rgba }));
            }
            if self.eos {
                return Ok(None);
            }
            if self.next >= self.track.samples.len() {
                self.eos = true;
                let mut pics = self.codec.flush();
                pics.sort_by_key(|p| p.pts);
                self.ready.extend(pics);
                continue;
            }
            let i = self.next;
            self.next += 1;
            let pts = self.track.samples[i].0;
            let data = self.demux.read(&self.bytes, i)?;
            match self.codec.decode(&data, pts) {
                Ok(mut pics) => {
                    pics.sort_by_key(|p| p.pts);
                    self.ready.extend(pics);
                }
                Err(e) => {
                    // A corrupt sample: carry on (the picture is lost), give up after many.
                    self.errors += 1;
                    log::debug!("video sample {i}: {e}");
                    if self.errors > 50 {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Position so that the next frame is the one shown at `t` seconds (decoding resumes at the
    /// preceding sync sample).
    pub fn seek(&mut self, t: f64) {
        let target = (t * self.track.timebase.1 as f64 / self.track.timebase.0 as f64) as i64;
        // The last sync sample whose pts is at or before the target.
        let mut start = 0;
        for (i, (pts, sync)) in self.track.samples.iter().enumerate() {
            if *sync && *pts <= target {
                start = i;
            }
        }
        self.codec.reset();
        self.ready.clear();
        self.next = start;
        self.eos = false;
        // Keep the frame on screen at `t`: the one whose pts is the last ≤ t.
        let shown = self.track.samples.iter().map(|s| s.0).filter(|p| *p <= target).max();
        self.skip_before = shown.map(|p| self.secs(p)).unwrap_or(f64::NEG_INFINITY);
    }
}

/// The frame shown at `t` seconds (the first frame when `t` is 0).
pub fn poster_frame(bytes: &Bytes, t: f64) -> Result<Frame> {
    let mut d = VideoDecoder::open(bytes.clone())?;
    if t > 0.0 {
        d.seek(t);
    }
    let mut last = None;
    while let Some(f) = d.next_frame()? {
        if f.time > t + 1e-6 && last.is_some() {
            break;
        }
        let done = f.time + 1e-6 >= t;
        last = Some(f);
        if done {
            break;
        }
    }
    last.ok_or(MediaError::Corrupt("no picture could be decoded".into()))
}

/// [`poster_frame`] as PNG bytes.
pub fn poster_png(bytes: &Bytes, t: f64) -> Result<Vec<u8>> {
    let f = poster_frame(bytes, t)?;
    let img = image::RgbaImage::from_raw(f.width, f.height, f.rgba).ok_or(MediaError::Corrupt("bad picture size".into()))?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(dec_err)?;
    Ok(out)
}
