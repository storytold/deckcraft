//! H.264 in MP4: one [`H264Mp4Track`] per clip, and the frame fitting a camera recording uses.
//!
//! The encoder is ours ([`deckcraft_h264::Encoder`]) and the muxer is [`deckcraft_isobmff`], so
//! no external tool or codec library is involved and the module builds for wasm. Two users share
//! it: recorded Cameo clips (per slide, video only) and, later, video export.

use std::io::Cursor;

use deckcraft_h264::{Encoder, EncoderConfig};
use deckcraft_isobmff::{AvcConfig, Brand, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};

use crate::{MediaError, Result};

/// Largest picture side the encoder accepts.
pub const MAX_WIDTH: u32 = 8192;
/// Largest picture height the encoder accepts.
pub const MAX_HEIGHT: u32 = 4320;
/// Seconds between key pictures inside a clip; the first picture of every clip is one.
pub const KEY_EVERY_S: u32 = 2;

/// One video-only MP4 being written: H.264 Constrained Baseline, 4:2:0, no audio.
pub struct H264Mp4Track {
    enc: Encoder,
    mux: Mp4Writer<Cursor<Vec<u8>>>,
    track: usize,
    fps: u32,
    width: u32,
    height: u32,
    /// Pictures written since the last key picture.
    since_key: u32,
    frames: u64,
    /// Encoded sample bytes written so far (the bulk of the finished MP4).
    bytes: u64,
}

impl H264Mp4Track {
    /// Start a track. The picture must be even-sided and 16×16 to 8192×4320; `fps` 1–240.
    /// `qp` is clamped by the encoder to 10–51.
    pub fn new(width: u32, height: u32, fps: u32, qp: u8) -> Result<Self> {
        if width < 16
            || height < 16
            || width > MAX_WIDTH
            || height > MAX_HEIGHT
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || fps == 0
            || fps > 240
        {
            return Err(MediaError::Unsupported(format!("a {width}×{height} picture at {fps} fps")));
        }
        let enc = Encoder::new(EncoderConfig { width, height, fps, qp }).map_err(|e| MediaError::Unsupported(e.to_string()))?;
        let avc = AvcConfig::new(vec![enc.sps()], vec![enc.pps()], 4);
        let entry = SampleEntry::avc(avc, width as u16, height as u16);
        let mut mux = Mp4Writer::new(Cursor::new(Vec::new()), WriterOptions::new(Brand::Mp4)).map_err(mp4)?;
        let track = mux.add_track(TrackConfig::new(entry, fps)).map_err(mp4)?;
        Ok(Self { enc, mux, track, fps, width, height, since_key: 0, frames: 0, bytes: 0 })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Pictures written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Encoded sample bytes written so far; a recording can stop before it holds too many.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Encode one packed RGBA picture, exactly `width × height × 4` bytes. Returns whether it is a
    /// key picture. The first picture and every [`KEY_EVERY_S`] seconds are keys.
    pub fn push_rgba(&mut self, rgba: &[u8]) -> Result<bool> {
        let need = self.width as usize * self.height as usize * 4;
        if rgba.len() != need {
            return Err(MediaError::Corrupt(format!("a {}×{} frame is {need} bytes, not {}", self.width, self.height, rgba.len())));
        }
        let key = self.frames == 0 || self.since_key + 1 >= self.fps * KEY_EVERY_S;
        let pic = self.enc.encode_rgba(rgba, key).map_err(|e| MediaError::Corrupt(e.to_string()))?;
        let sample = pic.length_prefixed();
        self.mux.write_sample(self.track, WriteSample { data: &sample, duration: 1, composition_offset: 0, is_sync: pic.key }).map_err(mp4)?;
        self.bytes += sample.len() as u64;
        self.frames += 1;
        self.since_key = if pic.key { 0 } else { self.since_key + 1 };
        Ok(pic.key)
    }

    /// Finish the movie (`moov` first, so a player can start without the whole file).
    pub fn finish(self) -> Result<Vec<u8>> {
        let out = self.mux.finish_faststart().map_err(mp4)?;
        Ok(out.into_inner())
    }
}

fn mp4(e: deckcraft_isobmff::Error) -> MediaError {
    MediaError::Corrupt(e.to_string())
}

/// A source frame fitted into `dst_w × dst_h`: centre-cropped to the destination's aspect, then
/// scaled with bilinear sampling. Unlike the export path this never adds black bars, so a square
/// Cameo bubble is filled by the middle of a widescreen camera frame.
pub fn fitted_rgba(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Result<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return Err(MediaError::Corrupt("an empty frame".into()));
    }
    if dst_w > MAX_WIDTH || dst_h > MAX_HEIGHT {
        return Err(MediaError::Unsupported(format!("a {dst_w}×{dst_h} destination")));
    }
    let pixels = |w: u32, h: u32, what: &str| -> Result<usize> {
        (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)).ok_or_else(|| MediaError::Corrupt(format!("a {w}×{h} {what} overflows")))
    };
    let need = pixels(src_w, src_h, "source frame")?;
    if src.len() < need {
        return Err(MediaError::Corrupt(format!("a {src_w}×{src_h} frame is {need} bytes, not {}", src.len())));
    }
    let out_len = pixels(dst_w, dst_h, "destination")?;
    let (src_wf, src_hf) = (f64::from(src_w), f64::from(src_h));
    let (dst_wf, dst_hf) = (f64::from(dst_w), f64::from(dst_h));
    let (cw, ch) = if src_wf * dst_hf > src_hf * dst_wf {
        // Source is wider: keep full height, crop the sides.
        (src_hf * dst_wf / dst_hf, src_hf)
    } else {
        (src_wf, src_wf * dst_hf / dst_wf)
    };
    let x0 = (src_wf - cw) / 2.0;
    let y0 = (src_hf - ch) / 2.0;
    let mut out = vec![0u8; out_len];
    for y in 0..dst_h {
        let sy = (y0 + (f64::from(y) + 0.5) * ch / dst_hf - 0.5).clamp(0.0, src_hf - 1.0);
        let y1 = sy as usize;
        let y2 = (y1 + 1).min(src_h as usize - 1);
        let fy = (sy - y1 as f64) as f32;
        for x in 0..dst_w {
            let sx = (x0 + (f64::from(x) + 0.5) * cw / dst_wf - 0.5).clamp(0.0, src_wf - 1.0);
            let x1 = sx as usize;
            let x2 = (x1 + 1).min(src_w as usize - 1);
            let fx = (sx - x1 as f64) as f32;
            let at = |xx: usize, yy: usize| -> [f32; 4] {
                let i = (yy * src_w as usize + xx) * 4;
                [f32::from(src[i]), f32::from(src[i + 1]), f32::from(src[i + 2]), f32::from(src[i + 3])]
            };
            let (p00, p10) = (at(x1, y1), at(x2, y1));
            let (p01, p11) = (at(x1, y2), at(x2, y2));
            let o = (y as usize * dst_w as usize + x as usize) * 4;
            for c in 0..4 {
                let top = p00[c] + (p10[c] - p00[c]) * fx;
                let bottom = p01[c] + (p11[c] - p01[c]) * fx;
                out[o + c] = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for p in v.chunks_mut(4) {
            p.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        v
    }

    #[test]
    fn track_encodes_an_mp4_our_decoder_reads_back() {
        let (w, h) = (64u32, 48u32);
        let mut track = H264Mp4Track::new(w, h, 15, 26).expect("track");
        assert!(track.push_rgba(&frame(w, h, [10, 20, 30])).expect("first"));
        for i in 1..10 {
            let key = track.push_rgba(&frame(w, h, [10 + i as u8, 20, 30])).expect("push");
            assert!(!key);
        }
        assert_eq!(track.frames(), 10);
        let mp4 = track.finish().expect("finish");
        assert_eq!(&mp4[4..8], b"ftyp");
        // Decode the samples out of the MP4 with the media layer's own demuxer (an independent
        // parser from the encoder): size and timing come back from the container.
        let info = crate::probe(&std::sync::Arc::new(mp4)).expect("probe");
        assert_eq!(info.video.as_ref().map(|v| (v.width, v.height)), Some((w, h)));
        let ms = info.duration_ms;
        assert!((666..=667).contains(&ms), "ten samples at 15 fps, got {ms} ms");
    }

    #[test]
    fn key_pictures_are_first_and_every_two_seconds() {
        let (w, h) = (32u32, 32u32);
        let mut track = H264Mp4Track::new(w, h, 10, 30).expect("track");
        let mut keys = vec![];
        for i in 0..45 {
            keys.push(track.push_rgba(&frame(w, h, [i as u8, 0, 0])).expect("push"));
        }
        assert!(keys[0]);
        assert!(!keys[19]);
        assert!(keys[20]);
        assert!(keys[40]);
    }

    #[test]
    fn a_short_input_is_an_error_not_black() {
        let mut track = H264Mp4Track::new(32, 32, 15, 30).expect("track");
        assert!(track.push_rgba(&[0; 16]).is_err());
    }

    #[test]
    fn odd_sizes_are_rejected() {
        assert!(H264Mp4Track::new(31, 32, 15, 30).is_err());
        assert!(H264Mp4Track::new(16, 16, 0, 30).is_err());
    }

    #[test]
    fn fitting_centre_crops_without_bars() {
        // A 4:2 source into a square: the sides go, every output pixel is the source's middle.
        let mut src = vec![0u8; 8 * 4 * 4];
        for y in 0..4 {
            for x in 0..8 {
                let i = (y * 8 + x) * 4;
                let (r, g, b) = if x < 2 {
                    (255, 0, 0)
                } else if x > 5 {
                    (0, 0, 255)
                } else {
                    (0, 255, 0)
                };
                src[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        let out = fitted_rgba(&src, 8, 4, 4, 4).expect("fit");
        assert_eq!(out.len(), 4 * 4 * 4);
        for p in out.chunks(4) {
            assert_eq!(&p[..3], &[0, 255, 0], "no red or blue bars survive the crop");
        }
    }
}
