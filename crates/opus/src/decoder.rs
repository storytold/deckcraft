//! Single-stream Opus decoder: mode switching, hybrid summation, redundancy and transitions
//! (RFC 6716 §4, §4.5).

use crate::celt::CeltDecoder;
use crate::celt::tables::OVERLAP;
use crate::packet::{Bandwidth, Mode, Packet};
use crate::range::RangeDecoder;
use crate::silk::{LostFlag, SilkDecoder};
use crate::{Error, Result};

/// Output sample rates supported by the decoder.
pub const SAMPLE_RATES: [u32; 5] = [8000, 12000, 16000, 24000, 48000];

/// Decoder for one elementary Opus stream (mono or stereo), like `OpusDecoder`.
pub struct StreamDecoder {
    rate: u32,
    channels: usize,
    celt: CeltDecoder,
    silk: SilkDecoder,
    stream_channels: usize,
    bandwidth: Bandwidth,
    mode: Option<Mode>,
    prev_mode: Option<Mode>,
    frame_size: usize,
    prev_redundancy: bool,
    last_packet_duration: usize,
    range_final: u32,
    /// Output gain in Q8 dB (OpusHead output gain).
    gain_q8: i16,
}

impl StreamDecoder {
    /// Creates a decoder producing `channels` (1 or 2) at `rate` (8/12/16/24/48 kHz).
    pub fn new(rate: u32, channels: usize) -> Result<StreamDecoder> {
        if !SAMPLE_RATES.contains(&rate) {
            return Err(Error::InvalidArgument("sample rate must be 8000, 12000, 16000, 24000 or 48000"));
        }
        if !(1..=2).contains(&channels) {
            return Err(Error::InvalidArgument("channels must be 1 or 2"));
        }
        Ok(StreamDecoder {
            rate,
            channels,
            celt: CeltDecoder::new(rate, channels),
            silk: SilkDecoder::new(rate, channels),
            stream_channels: channels,
            bandwidth: Bandwidth::Fullband,
            mode: None,
            prev_mode: None,
            frame_size: rate as usize / 400,
            prev_redundancy: false,
            last_packet_duration: 0,
            range_final: 0,
            gain_q8: 0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Final range coder state of the last decoded frame (for conformance checks).
    pub fn final_range(&self) -> u32 {
        self.range_final
    }

    /// Duration (samples per channel) of the last decoded packet.
    pub fn last_packet_duration(&self) -> usize {
        self.last_packet_duration
    }

    /// Sets the output gain in Q8 dB (as in the OpusHead header).
    pub fn set_gain_q8(&mut self, gain: i16) {
        self.gain_q8 = gain;
    }

    /// Disables the 180-degree intensity-stereo phase inversion (RFC 8251 §10).
    pub fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        self.celt.disable_inv = disabled;
    }

    /// `OPUS_RESET_STATE`.
    pub fn reset(&mut self) {
        self.celt.reset();
        self.silk.reset();
        self.stream_channels = self.channels;
        self.bandwidth = Bandwidth::Fullband;
        self.mode = None;
        self.prev_mode = None;
        self.frame_size = self.rate as usize / 400;
        self.prev_redundancy = false;
        self.last_packet_duration = 0;
        self.range_final = 0;
    }

    /// Decodes a packet into interleaved samples appended to `out`. `None` conceals one lost
    /// packet of the last packet's duration. Returns samples per channel.
    pub fn decode_interleaved(&mut self, packet: Option<&[u8]>, out: &mut Vec<f32>) -> Result<usize> {
        match packet {
            Some(p) if !p.is_empty() => self.decode_packet(p, out),
            _ => {
                let n = if self.last_packet_duration > 0 { self.last_packet_duration } else { self.rate as usize / 50 };
                self.conceal(n, out)
            }
        }
    }

    /// Conceals `n` samples (a multiple of 2.5 ms).
    pub fn conceal(&mut self, n: usize, out: &mut Vec<f32>) -> Result<usize> {
        let f2_5 = self.rate as usize / 400;
        if n == 0 || !n.is_multiple_of(f2_5) {
            return Err(Error::InvalidArgument("concealment length must be a multiple of 2.5 ms"));
        }
        let start = out.len();
        out.resize(start + n * self.channels, 0.0);
        let mut done = 0;
        while done < n {
            let r = self.decode_frame(None, &mut out[start + done * self.channels..], n - done, false)?;
            done += r;
        }
        self.last_packet_duration = n;
        Ok(n)
    }

    /// Decodes the in-band FEC (LBRR) of `packet` for the previous (lost) packet of `n` samples.
    pub fn decode_fec(&mut self, packet: &[u8], n: usize, out: &mut Vec<f32>) -> Result<usize> {
        let p = Packet::parse(packet)?;
        let pfs = p.toc.frame_samples(self.rate);
        if n < pfs || p.toc.mode == Mode::CeltOnly || self.mode == Some(Mode::CeltOnly) {
            return self.conceal(n, out);
        }
        let start = out.len();
        if n > pfs {
            self.conceal(n - pfs, out)?;
        }
        self.mode = Some(p.toc.mode);
        self.bandwidth = p.toc.bandwidth;
        self.frame_size = pfs;
        self.stream_channels = p.toc.channels();
        let off = out.len();
        out.resize(off + pfs * self.channels, 0.0);
        self.decode_frame(Some(p.frames[0]), &mut out[off..], pfs, true)?;
        self.last_packet_duration = n;
        debug_assert_eq!(out.len() - start, n * self.channels);
        Ok(n)
    }

    fn decode_packet(&mut self, data: &[u8], out: &mut Vec<f32>) -> Result<usize> {
        let p = Packet::parse(data)?;
        let fs = p.toc.frame_samples(self.rate);
        self.mode = Some(p.toc.mode);
        self.bandwidth = p.toc.bandwidth;
        self.frame_size = fs;
        self.stream_channels = p.toc.channels();
        let start = out.len();
        out.resize(start + fs * p.frames.len() * self.channels, 0.0);
        let mut nb = 0;
        for f in &p.frames {
            let r = self.decode_frame(Some(f), &mut out[start + nb * self.channels..], fs, false)?;
            nb += r;
        }
        out.truncate(start + nb * self.channels);
        self.last_packet_duration = nb;
        Ok(nb)
    }

    /// `opus_decode_frame`: decodes one frame (or conceals when `data` is `None`/≤1 byte).
    fn decode_frame(&mut self, data: Option<&[u8]>, pcm: &mut [f32], frame_size: usize, fec: bool) -> Result<usize> {
        let ch = self.channels;
        let fs = self.rate as usize;
        let f20 = fs / 50;
        let f10 = f20 >> 1;
        let f5 = f10 >> 1;
        let f2_5 = f5 >> 1;
        let mut frame_size = frame_size.min(fs / 25 * 3);
        let data = match data {
            Some(d) if d.len() > 1 => Some(d),
            _ => {
                frame_size = frame_size.min(self.frame_size);
                None
            }
        };
        let (mut audiosize, mode, bandwidth);
        match data {
            Some(_) => {
                audiosize = self.frame_size;
                mode = self.mode.unwrap_or(Mode::CeltOnly);
                bandwidth = Some(self.bandwidth);
            }
            None => {
                audiosize = frame_size;
                bandwidth = None;
                mode = match self.prev_mode {
                    None => {
                        pcm[..audiosize * ch].fill(0.0);
                        return Ok(audiosize);
                    }
                    Some(m) => m,
                };
                if audiosize > f20 {
                    self.conceal_long(pcm, audiosize, f20)?;
                    return Ok(frame_size);
                }
                audiosize = shorten_concealment(audiosize, mode, fs);
            }
        }
        let data_len = data.map_or(0, <[_]>::len);
        let mut dec_storage;
        let mut dec: Option<&mut RangeDecoder> = match data {
            Some(d) => {
                dec_storage = RangeDecoder::new(d);
                Some(&mut dec_storage)
            }
            None => None,
        };
        let mut transition = false;
        if data.is_some()
            && self.prev_mode.is_some()
            && ((mode == Mode::CeltOnly && self.prev_mode != Some(Mode::CeltOnly) && !self.prev_redundancy)
                || (mode != Mode::CeltOnly && self.prev_mode == Some(Mode::CeltOnly)))
        {
            transition = true;
        }
        let mut pcm_transition = Vec::new();
        if transition && mode == Mode::CeltOnly {
            pcm_transition.resize(f5 * ch, 0.0);
            let n = f5.min(audiosize);
            self.decode_frame(None, &mut pcm_transition, n, false)?;
        }
        if audiosize > frame_size {
            return Err(Error::InvalidArgument("frame larger than output buffer"));
        }
        frame_size = audiosize;

        // SILK.
        let mut pcm_silk: Vec<f32> = Vec::new();
        if mode != Mode::CeltOnly {
            pcm_silk = self.decode_silk_layer(
                dec.as_deref_mut(),
                SilkLayer { mode, bandwidth, lost: lost_flag(data.is_some(), fec), frame_size, audiosize, f10 },
            )?;
        }

        let mut start_band = 0;
        let mut redundancy = Redundancy::none(data_len);
        if !fec
            && mode != Mode::CeltOnly
            && let Some(d) = dec.as_deref_mut()
        {
            redundancy = Redundancy::parse(d, mode, self.mode == Some(Mode::Hybrid), data_len);
        }
        let Redundancy { present: redundancy, celt_to_silk, bytes: redundancy_bytes, len } = redundancy;
        if mode != Mode::CeltOnly {
            start_band = 17;
        }
        if redundancy {
            transition = false;
        }
        if transition && mode != Mode::CeltOnly {
            pcm_transition.resize(f5 * ch, 0.0);
            let n = f5.min(audiosize);
            self.decode_frame(None, &mut pcm_transition, n, false)?;
        }
        if let Some(bw) = bandwidth {
            self.celt.end = bw.celt_end_band();
        }
        self.celt.stream_channels = self.stream_channels;
        let mut redundant_audio = vec![0f32; if redundancy { f5 * ch } else { 0 }];
        let mut redundant_rng = 0u32;
        let red_data = data.map(|d| &d[len..len + redundancy_bytes]);
        if redundancy && celt_to_silk {
            self.celt.start = 0;
            self.celt.decode(red_data, f5, &mut redundant_audio, false, None)?;
            redundant_rng = self.celt.rng;
        }
        self.celt.start = start_band;

        if mode != Mode::SilkOnly {
            let celt_frame = f20.min(frame_size);
            if self.prev_mode.is_some() && Some(mode) != self.prev_mode && !self.prev_redundancy {
                self.celt.reset();
            }
            let cdata = if fec { None } else { data.map(|d| &d[..len]) };
            let cdec = if fec { None } else { dec.as_deref_mut() };
            self.celt.decode(cdata, celt_frame, pcm, false, cdec)?;
        } else {
            pcm[..frame_size * ch].fill(0.0);
            if self.prev_mode == Some(Mode::Hybrid) && !(redundancy && celt_to_silk && self.prev_redundancy) {
                self.celt.start = 0;
                let silence = [0xFFu8, 0xFF];
                self.celt.decode(Some(&silence), f2_5, pcm, true, None)?;
            }
        }
        if mode != Mode::CeltOnly {
            for (o, s) in pcm[..frame_size * ch].iter_mut().zip(&pcm_silk) {
                *o += *s;
            }
        }
        let window = &crate::celt::mode().window;
        if redundancy && !celt_to_silk {
            self.celt.reset();
            self.celt.start = 0;
            self.celt.decode(red_data, f5, &mut redundant_audio, false, None)?;
            redundant_rng = self.celt.rng;
            let o = ch * (frame_size - f2_5);
            let tail: Vec<f32> = pcm[o..o + f2_5 * ch].to_vec();
            smooth_fade(&tail, &redundant_audio[ch * f2_5..], &mut pcm[o..], f2_5, ch, window, self.rate);
        }
        if redundancy && celt_to_silk {
            self.fade_in_from(pcm, &redundant_audio, f2_5);
        }
        if transition {
            if audiosize >= f5 {
                self.fade_in_from(pcm, &pcm_transition, f2_5);
            } else {
                let cur: Vec<f32> = pcm[..ch * f2_5].to_vec();
                smooth_fade(&pcm_transition, &cur, pcm, f2_5, ch, window, self.rate);
            }
        }
        if self.gain_q8 != 0 {
            let g = (6.488_141e-4f32 * self.gain_q8 as f32).exp2();
            for v in pcm[..frame_size * ch].iter_mut() {
                *v *= g;
            }
        }
        self.range_final = match dec.as_deref() {
            Some(d) if data_len > 1 => d.rng() ^ redundant_rng,
            _ => 0,
        };
        self.prev_mode = Some(mode);
        self.prev_redundancy = redundancy && !celt_to_silk;
        Ok(audiosize)
    }

    /// Conceals a lost span longer than 20 ms by decoding it in 20 ms pieces.
    fn conceal_long(&mut self, pcm: &mut [f32], audiosize: usize, f20: usize) -> Result<()> {
        let mut done = 0;
        while done < audiosize {
            done += self.decode_frame(None, &mut pcm[done * self.channels..], (audiosize - done).min(f20), false)?;
        }
        Ok(())
    }

    /// Replaces the first `f2_5` samples of `pcm` with `src` and cross-fades the following `f2_5` samples from `src` into `pcm`.
    fn fade_in_from(&self, pcm: &mut [f32], src: &[f32], f2_5: usize) {
        let ch = self.channels;
        let window = &crate::celt::mode().window;
        pcm[..f2_5 * ch].copy_from_slice(&src[..f2_5 * ch]);
        let cur: Vec<f32> = pcm[ch * f2_5..ch * 2 * f2_5].to_vec();
        smooth_fade(&src[ch * f2_5..], &cur, &mut pcm[ch * f2_5..], f2_5, ch, window, self.rate);
    }

    /// Runs the SILK layer over the frame, zero-filling spans it cannot decode when the packet was lost.
    fn decode_silk_layer(&mut self, mut dec: Option<&mut RangeDecoder>, l: SilkLayer) -> Result<Vec<f32>> {
        let ch = self.channels;
        let mut pcm_silk = vec![0.0; l.frame_size.max(l.f10) * ch];
        if self.prev_mode == Some(Mode::CeltOnly) {
            self.silk.reset();
        }
        let payload_ms = (1000 * l.audiosize / self.rate as usize).max(10);
        if l.lost != LostFlag::Lost {
            self.silk.set_stream(self.stream_channels, silk_internal_rate(l.mode, l.bandwidth));
        }
        let mut decoded = 0;
        while decoded < l.frame_size {
            let first = decoded == 0;
            let out = &mut pcm_silk[decoded * ch..];
            let n = match self.silk.decode(dec.as_deref_mut(), l.lost, first, payload_ms, out) {
                Ok(n) => n,
                Err(_) if l.lost != LostFlag::Normal => {
                    let n = l.frame_size - decoded;
                    pcm_silk[decoded * ch..(decoded + n) * ch].fill(0.0);
                    n
                }
                Err(e) => return Err(e),
            };
            if n == 0 {
                break;
            }
            decoded += n;
        }
        Ok(pcm_silk)
    }
}

/// Inputs for one SILK-layer decode.
struct SilkLayer {
    mode: Mode,
    bandwidth: Option<Bandwidth>,
    lost: LostFlag,
    frame_size: usize,
    audiosize: usize,
    f10: usize,
}

fn lost_flag(has_data: bool, fec: bool) -> LostFlag {
    if !has_data {
        LostFlag::Lost
    } else if fec {
        LostFlag::Lbrr
    } else {
        LostFlag::Normal
    }
}

fn silk_internal_rate(mode: Mode, bandwidth: Option<Bandwidth>) -> u32 {
    match (mode, bandwidth) {
        (Mode::SilkOnly, Some(Bandwidth::Narrowband)) => 8000,
        (Mode::SilkOnly, Some(Bandwidth::Mediumband)) => 12000,
        _ => 16000,
    }
}

/// Picks the shorter concealment length for spans of at most 20 ms (10 ms, or 5 ms outside SILK-only mode).
fn shorten_concealment(audiosize: usize, mode: Mode, rate: usize) -> usize {
    let f20 = rate / 50;
    let f10 = f20 >> 1;
    let f5 = f10 >> 1;
    if audiosize >= f20 {
        audiosize
    } else if audiosize > f10 {
        f10
    } else if mode != Mode::SilkOnly && audiosize > f5 && audiosize < f10 {
        f5
    } else {
        audiosize
    }
}

/// In-band redundancy header of a packet: whether a redundant CELT frame follows, its direction and size.
struct Redundancy {
    present: bool,
    celt_to_silk: bool,
    bytes: usize,
    /// Payload length left for the main frame.
    len: usize,
}

impl Redundancy {
    fn none(len: usize) -> Self {
        Self { present: false, celt_to_silk: false, bytes: 0, len }
    }

    fn parse(d: &mut RangeDecoder, mode: Mode, prev_hybrid: bool, len: usize) -> Self {
        let mut r = Self::none(len);
        if d.tell() + 17 + 20 * prev_hybrid as i32 > 8 * len as i32 {
            return r;
        }
        r.present = if mode == Mode::Hybrid { d.bit_logp(12) } else { true };
        if !r.present {
            return r;
        }
        r.celt_to_silk = d.bit_logp(1);
        r.bytes = if mode == Mode::Hybrid { d.uint(256) as usize + 2 } else { len - ((d.tell() as usize + 7) >> 3) };
        if r.bytes > len || (len - r.bytes) * 8 < d.tell() as usize {
            r = Self { celt_to_silk: r.celt_to_silk, ..Self::none(0) };
        } else {
            r.len -= r.bytes;
        }
        d.shrink_storage(r.bytes);
        r
    }
}

fn smooth_fade(in1: &[f32], in2: &[f32], out: &mut [f32], overlap: usize, ch: usize, window: &[f32; OVERLAP], rate: u32) {
    let inc = (48000 / rate) as usize;
    for c in 0..ch {
        for i in 0..overlap {
            let w = window[i * inc] * window[i * inc];
            out[i * ch + c] = w * in2[i * ch + c] + (1.0 - w) * in1[i * ch + c];
        }
    }
}
