//! A small H.264 encoder for synthetic pictures (slides): Constrained Baseline, CAVLC, 8-bit
//! 4:2:0, one slice per picture, deblocking off. Every coded macroblock is Intra 16x16 (vertical,
//! horizontal or DC prediction, chosen by SAD); in P pictures a macroblock whose source pixels are
//! unchanged since the previous picture is skipped, so a held slide costs a few bytes a frame.
//! Written from ITU-T Rec. H.264 (clauses 7.3, 8.3.3, 8.3.4, 8.5, 9.2) as the inverse of
//! [`crate::Decoder`], which the tests decode it with.

use crate::cavlc_tables::{COEFF_TOKEN, RUN_BEFORE, TOTAL_ZEROS_2X2, TOTAL_ZEROS_4X4};

/// Encoder settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    /// Picture size in pixels; both even, at most 8192×4320.
    pub width: u32,
    pub height: u32,
    /// Frame rate written to the VUI timing (1–240).
    pub fps: u32,
    /// Quantisation parameter, 10–51 (lower is better quality and larger).
    pub qp: u8,
}

/// One coded picture: its NAL units without start codes (SPS and PPS lead a key picture).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPicture {
    pub nals: Vec<Vec<u8>>,
    pub key: bool,
}

impl EncodedPicture {
    /// Annex B byte stream (start codes).
    pub fn annex_b(&self) -> Vec<u8> {
        let mut out = vec![];
        for n in &self.nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(n);
        }
        out
    }
    /// The picture's NAL units with 4-byte big-endian lengths (MP4 samples), parameter sets
    /// left out (they go in the `avcC` box).
    pub fn length_prefixed(&self) -> Vec<u8> {
        let mut out = vec![];
        for n in self.nals.iter().filter(|n| !matches!(n.first().map(|b| b & 0x1F), Some(7 | 8))) {
            out.extend_from_slice(&(n.len() as u32).to_be_bytes());
            out.extend_from_slice(n);
        }
        out
    }
}

/// Bit writer for RBSPs.
#[derive(Default)]
struct Bits {
    buf: Vec<u8>,
    cur: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, bits: u32) {
        if bits == 0 {
            return;
        }
        let bits = bits.min(32);
        let mask = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
        self.cur = (self.cur << bits) | u64::from(v & mask);
        self.n += bits;
        while self.n >= 8 {
            self.n -= 8;
            self.buf.push((self.cur >> self.n) as u8);
        }
        self.cur &= (1u64 << self.n) - 1;
    }
    fn flag(&mut self, b: bool) {
        self.put(u32::from(b), 1);
    }
    /// A code from the tables ("0101").
    fn code(&mut self, s: &str) {
        for c in s.bytes() {
            self.put(u32::from(c == b'1'), 1);
        }
    }
    fn ue(&mut self, v: u32) {
        let x = u64::from(v) + 1;
        let len = 64 - x.leading_zeros();
        self.put(0, len - 1);
        // len ≤ 33: write the high bit and the rest separately.
        self.put(1, 1);
        if len > 1 {
            self.put((x & ((1u64 << (len - 1)) - 1)) as u32, len - 1);
        }
    }
    fn se(&mut self, v: i32) {
        let k = if v > 0 { (v as u32) * 2 - 1 } else { v.unsigned_abs() * 2 };
        self.ue(k);
    }
    /// `rbsp_trailing_bits`, then the bytes.
    fn finish(mut self) -> Vec<u8> {
        self.put(1, 1);
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put(0, pad);
        }
        self.buf
    }
}

/// A NAL unit from its header byte and RBSP, with emulation prevention.
fn nal(header: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 64 + 2);
    out.push(header);
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    out
}

/// 4x4 zig-zag scan (frame): scan index → raster index.
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
/// Quantisation multipliers by `qp % 6` for positions (even, even), (odd, odd) and the rest.
const MF: [[i32; 3]; 6] = [[13107, 5243, 8066], [11916, 4660, 7490], [10082, 4194, 6554], [9362, 3647, 5825], [8192, 3355, 5243], [7282, 2893, 4559]];
/// Dequantisation scales (`normAdjust4x4`), same positions.
const V: [[i32; 3]; 6] = [[10, 16, 13], [11, 18, 14], [13, 20, 16], [14, 23, 18], [16, 25, 20], [18, 29, 23]];
/// Chroma QP for luma QP 30..=51 (Table 8-15); below 30 they are equal.
const QPC: [u8; 22] = [29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, 39, 39, 39, 39];

fn chroma_qp(qp: u8) -> u8 {
    if qp < 30 { qp } else { QPC.get(usize::from(qp - 30)).copied().unwrap_or(39) }
}

fn pos_class(raster: usize) -> usize {
    let (i, j) = (raster / 4, raster % 4);
    match (i % 2, j % 2) {
        (0, 0) => 0,
        (1, 1) => 1,
        _ => 2,
    }
}

/// Forward core transform of a 4x4 residual (raster), `Cf · X · Cfᵀ`.
fn fdct(x: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for i in 0..4 {
        let r = &x[i * 4..i * 4 + 4];
        let (s03, d03, s12, d12) = (r[0] + r[3], r[0] - r[3], r[1] + r[2], r[1] - r[2]);
        t[i * 4] = s03 + s12;
        t[i * 4 + 1] = 2 * d03 + d12;
        t[i * 4 + 2] = s03 - s12;
        t[i * 4 + 3] = d03 - 2 * d12;
    }
    let mut out = [0i32; 16];
    for j in 0..4 {
        let (a, b, c, d) = (t[j], t[4 + j], t[8 + j], t[12 + j]);
        let (s03, d03, s12, d12) = (a + d, a - d, b + c, b - c);
        out[j] = s03 + s12;
        out[4 + j] = 2 * d03 + d12;
        out[8 + j] = s03 - s12;
        out[12 + j] = d03 - 2 * d12;
    }
    out
}

/// Inverse transform (8.5.12.2) of dequantised coefficients, giving the residual.
fn idct(d: &[i32; 16]) -> [i32; 16] {
    let mut f = [0i32; 16];
    for i in 0..4 {
        let r = &d[i * 4..i * 4 + 4];
        let e0 = r[0] + r[2];
        let e1 = r[0] - r[2];
        let e2 = (r[1] >> 1) - r[3];
        let e3 = r[1] + (r[3] >> 1);
        f[i * 4] = e0 + e3;
        f[i * 4 + 1] = e1 + e2;
        f[i * 4 + 2] = e1 - e2;
        f[i * 4 + 3] = e0 - e3;
    }
    let mut out = [0i32; 16];
    for j in 0..4 {
        let (a, b, c, dd) = (f[j], f[4 + j], f[8 + j], f[12 + j]);
        let g0 = a + c;
        let g1 = a - c;
        let g2 = (b >> 1) - dd;
        let g3 = b + (dd >> 1);
        out[j] = (g0 + g3 + 32) >> 6;
        out[4 + j] = (g1 + g2 + 32) >> 6;
        out[8 + j] = (g1 - g2 + 32) >> 6;
        out[12 + j] = (g0 - g3 + 32) >> 6;
    }
    out
}

/// Largest level magnitude written (keeps every level within the 12-bit escape).
const MAX_LEVEL: i32 = 2063;

fn quant(w: i32, mf: i32, qbits: u32, f: i32) -> i32 {
    let z = ((i64::from(w.abs()) * i64::from(mf) + i64::from(f)) >> qbits) as i32;
    z.min(MAX_LEVEL) * w.signum()
}

/// Dequantise an AC (or chroma AC) coefficient (8.5.12.1, flat scaling).
fn dequant(c: i32, qp: u8, class: usize) -> i32 {
    let ls = 16 * V[usize::from(qp % 6)][class];
    let q = u32::from(qp / 6);
    if qp >= 24 { (c * ls) << (q - 4) } else { (c * ls + (1 << (3 - q))) >> (4 - q) }
}

/// Write one CAVLC `residual_block` of `coeffs` (scan order) with context `nc` (−1: chroma DC).
/// Returns TotalCoeff.
fn residual_block(b: &mut Bits, coeffs: &[i32], nc: i32) -> u8 {
    let max = coeffs.len();
    let nz: Vec<usize> = (0..max).filter(|&i| coeffs[i] != 0).collect();
    let tc = nz.len();
    let mut t1 = 0;
    for &i in nz.iter().rev() {
        if coeffs[i].abs() == 1 && t1 < 3 {
            t1 += 1;
        } else {
            break;
        }
    }
    let col = match nc {
        -1 => 4,
        n if n < 2 => 0,
        n if n < 4 => 1,
        n if n < 8 => 2,
        _ => 3,
    };
    let token = COEFF_TOKEN.iter().find(|(o, t, _)| usize::from(*o) == t1 && usize::from(*t) == tc).map(|e| e.2[col]).unwrap_or("");
    b.code(token);
    if tc == 0 {
        return 0;
    }
    // Highest frequency first.
    let levels: Vec<i32> = nz.iter().rev().map(|&i| coeffs[i]).collect();
    for l in levels.iter().take(t1) {
        b.flag(*l < 0);
    }
    let mut sl: u32 = if tc > 10 && t1 < 3 { 1 } else { 0 };
    for (k, &l) in levels.iter().enumerate().skip(t1) {
        let mut code = if l > 0 { 2 * l - 2 } else { -2 * l - 1 };
        if k == t1 && t1 < 3 {
            code -= 2;
        }
        let code = code.max(0) as u32;
        if sl == 0 {
            if code < 14 {
                b.put(0, code);
                b.put(1, 1);
            } else if code < 30 {
                b.put(0, 14);
                b.put(1, 1);
                b.put(code - 14, 4);
            } else {
                b.put(0, 15);
                b.put(1, 1);
                b.put((code - 30).min(4095), 12);
            }
        } else if code < (15 << sl) {
            b.put(0, code >> sl);
            b.put(1, 1);
            b.put(code & ((1 << sl) - 1), sl);
        } else {
            b.put(0, 15);
            b.put(1, 1);
            b.put((code - (15 << sl)).min(4095), 12);
        }
        if sl == 0 {
            sl = 1;
        }
        if l.unsigned_abs() > (3 << (sl - 1)) && sl < 6 {
            sl += 1;
        }
    }
    let last = nz.last().copied().unwrap_or(0);
    let total_zeros = last + 1 - tc;
    if tc < max {
        let code = if max == 4 {
            TOTAL_ZEROS_2X2.get(tc - 1).and_then(|r| r.get(total_zeros))
        } else {
            TOTAL_ZEROS_4X4.get(tc - 1).and_then(|r| r.get(total_zeros))
        };
        b.code(code.copied().unwrap_or(""));
    }
    let mut zeros_left = total_zeros;
    for w in 0..tc.saturating_sub(1) {
        if zeros_left == 0 {
            break;
        }
        // nz descending: run of zeros below the w-th highest.
        let (hi, lo) = (nz[tc - 1 - w], nz[tc - 2 - w]);
        let run = hi - lo - 1;
        let row = zeros_left.min(7) - 1;
        b.code(RUN_BEFORE.get(row).and_then(|r| r.get(run)).copied().unwrap_or(""));
        zeros_left -= run;
    }
    tc as u8
}

/// A picture's planes, padded to whole macroblocks.
#[derive(Clone)]
struct Planes {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    w: usize,
    h: usize,
}

impl Planes {
    fn new(w: usize, h: usize) -> Self {
        Planes { y: vec![16; w * h], u: vec![128; w * h / 4], v: vec![128; w * h / 4], w, h }
    }
}

/// The encoder; feed it pictures in display order.
pub struct Encoder {
    cfg: EncoderConfig,
    mbw: usize,
    mbh: usize,
    recon: Planes,
    prev: Option<Planes>,
    frame_num: u32,
    idr_id: u32,
    /// TotalCoeff per 4x4 block for this picture: luma (mbw*4 × mbh*4), Cb, Cr (mbw*2 × mbh*2).
    nc_y: Vec<u8>,
    nc_u: Vec<u8>,
    nc_v: Vec<u8>,
}

/// Why an encoder can't be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub &'static str);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Encoder {
    pub fn new(cfg: EncoderConfig) -> Result<Self, ConfigError> {
        if cfg.width < 16 || cfg.height < 16 || cfg.width > 8192 || cfg.height > 4320 || !cfg.width.is_multiple_of(2) || !cfg.height.is_multiple_of(2)
        {
            return Err(ConfigError("the picture must be 16×16 to 8192×4320 pixels, with even sides"));
        }
        let mbw = cfg.width.div_ceil(16) as usize;
        let mbh = cfg.height.div_ceil(16) as usize;
        let cfg = EncoderConfig { qp: cfg.qp.clamp(10, 51), fps: cfg.fps.clamp(1, 240), ..cfg };
        Ok(Encoder {
            cfg,
            mbw,
            mbh,
            recon: Planes::new(mbw * 16, mbh * 16),
            prev: None,
            frame_num: 0,
            idr_id: 0,
            nc_y: vec![0; mbw * mbh * 16],
            nc_u: vec![0; mbw * mbh * 4],
            nc_v: vec![0; mbw * mbh * 4],
        })
    }

    fn level_idc(&self) -> u8 {
        let mbs = (self.mbw * self.mbh) as u64;
        let rate = mbs * u64::from(self.cfg.fps);
        match (mbs, rate) {
            (m, r) if m <= 1620 && r <= 40_500 => 30,
            (m, r) if m <= 3600 && r <= 108_000 => 31,
            (m, r) if m <= 8192 && r <= 245_760 => 40,
            (m, r) if m <= 8704 && r <= 522_240 => 42,
            (m, r) if m <= 22_080 && r <= 589_824 => 50,
            _ => 51,
        }
    }

    /// The sequence parameter set NAL unit.
    pub fn sps(&self) -> Vec<u8> {
        let mut b = Bits::default();
        b.put(66, 8); // profile_idc: Baseline
        b.put(0xC0, 8); // constraint_set0 and set1: Constrained Baseline
        b.put(u32::from(self.level_idc()), 8);
        b.ue(0); // seq_parameter_set_id
        b.ue(12); // log2_max_frame_num_minus4: 16-bit frame_num
        b.ue(2); // pic_order_cnt_type 2: output order is decode order
        b.ue(1); // max_num_ref_frames
        b.flag(false); // gaps_in_frame_num_value_allowed_flag
        b.ue(self.mbw as u32 - 1);
        b.ue(self.mbh as u32 - 1);
        b.flag(true); // frame_mbs_only_flag
        b.flag(true); // direct_8x8_inference_flag
        let (cr, cb) = ((self.mbw as u32 * 16 - self.cfg.width) / 2, (self.mbh as u32 * 16 - self.cfg.height) / 2);
        b.flag(cr > 0 || cb > 0);
        if cr > 0 || cb > 0 {
            b.ue(0);
            b.ue(cr);
            b.ue(0);
            b.ue(cb);
        }
        b.flag(true); // vui_parameters_present_flag
        b.flag(false); // aspect_ratio_info_present_flag
        b.flag(false); // overscan_info_present_flag
        b.flag(true); // video_signal_type_present_flag
        b.put(5, 3); // video_format: unspecified
        b.flag(false); // video_full_range_flag
        b.flag(true); // colour_description_present_flag
        b.put(1, 8); // BT.709 primaries, transfer and matrix
        b.put(1, 8);
        b.put(1, 8);
        b.flag(false); // chroma_loc_info_present_flag
        b.flag(true); // timing_info_present_flag
        b.put(1, 32); // num_units_in_tick
        b.put(self.cfg.fps * 2, 32); // time_scale
        b.flag(true); // fixed_frame_rate_flag
        b.flag(false); // nal_hrd_parameters_present_flag
        b.flag(false); // vcl_hrd_parameters_present_flag
        b.flag(false); // pic_struct_present_flag
        b.flag(true); // bitstream_restriction_flag
        b.flag(true); // motion_vectors_over_pic_boundaries_flag
        b.ue(0); // max_bytes_per_pic_denom
        b.ue(0); // max_bits_per_mb_denom
        b.ue(0); // log2_max_mv_length_horizontal (every vector is zero)
        b.ue(0);
        b.ue(0); // max_num_reorder_frames
        b.ue(1); // max_dec_frame_buffering
        nal(0x67, &b.finish())
    }

    /// The picture parameter set NAL unit.
    pub fn pps(&self) -> Vec<u8> {
        let mut b = Bits::default();
        b.ue(0); // pic_parameter_set_id
        b.ue(0); // seq_parameter_set_id
        b.flag(false); // entropy_coding_mode_flag: CAVLC
        b.flag(false); // bottom_field_pic_order_in_frame_present_flag
        b.ue(0); // num_slice_groups_minus1
        b.ue(0); // num_ref_idx_l0_default_active_minus1
        b.ue(0);
        b.flag(false); // weighted_pred_flag
        b.put(0, 2); // weighted_bipred_idc
        b.se(0); // pic_init_qp_minus26
        b.se(0); // pic_init_qs_minus26
        b.se(0); // chroma_qp_index_offset
        b.flag(true); // deblocking_filter_control_present_flag
        b.flag(false); // constrained_intra_pred_flag
        b.flag(false); // redundant_pic_cnt_present_flag
        nal(0x68, &b.finish())
    }

    /// Encode one RGBA picture (`width`×`height`, row-major). A key picture (IDR, with SPS and
    /// PPS) is made when `key` is set and for the first picture.
    pub fn encode_rgba(&mut self, rgba: &[u8], key: bool) -> EncodedPicture {
        let src = self.to_yuv(rgba);
        let key = key || self.prev.is_none();
        if key {
            self.frame_num = 0;
            self.idr_id = (self.idr_id + 1) % 65_536;
        } else {
            self.frame_num = (self.frame_num + 1) % 65_536;
        }
        self.nc_y.iter_mut().for_each(|n| *n = 0);
        self.nc_u.iter_mut().for_each(|n| *n = 0);
        self.nc_v.iter_mut().for_each(|n| *n = 0);
        let qp = self.cfg.qp;
        let mut b = Bits::default();
        b.ue(0); // first_mb_in_slice
        b.ue(if key { 7 } else { 5 }); // slice_type: all I / all P
        b.ue(0); // pic_parameter_set_id
        b.put(self.frame_num, 16);
        if key {
            b.ue(self.idr_id);
        }
        if !key {
            b.flag(false); // num_ref_idx_active_override_flag
            b.flag(false); // ref_pic_list_modification_flag_l0
        }
        // dec_ref_pic_marking
        if key {
            b.flag(false); // no_output_of_prior_pics_flag
            b.flag(false); // long_term_reference_flag
        } else {
            b.flag(false); // adaptive_ref_pic_marking_mode_flag
        }
        b.se(i32::from(qp) - 26); // slice_qp_delta
        b.ue(1); // disable_deblocking_filter_idc: off
        let mut skip_run = 0u32;
        for my in 0..self.mbh {
            for mx in 0..self.mbw {
                let unchanged = !key && self.prev.as_ref().is_some_and(|p| same_mb(p, &src, mx, my));
                if unchanged {
                    skip_run += 1;
                    continue;
                }
                if !key {
                    b.ue(skip_run);
                    skip_run = 0;
                }
                self.intra_mb(&mut b, &src, mx, my, !key);
            }
        }
        if !key && skip_run > 0 {
            b.ue(skip_run);
        }
        let slice = nal(if key { 0x65 } else { 0x41 }, &b.finish());
        self.prev = Some(src);
        let nals = if key { vec![self.sps(), self.pps(), slice] } else { vec![slice] };
        EncodedPicture { nals, key }
    }

    /// RGBA to padded BT.709 limited-range 4:2:0 (edges replicated into the padding).
    fn to_yuv(&self, rgba: &[u8]) -> Planes {
        let (w, h) = (self.cfg.width as usize, self.cfg.height as usize);
        let mut p = Planes::new(self.mbw * 16, self.mbh * 16);
        let px = |x: usize, y: usize| -> (i32, i32, i32) {
            let i = (y.min(h - 1) * w + x.min(w - 1)) * 4;
            match rgba.get(i..i + 3) {
                Some(s) => (i32::from(s[0]), i32::from(s[1]), i32::from(s[2])),
                None => (0, 0, 0),
            }
        };
        for y in 0..p.h {
            for x in 0..p.w {
                let (r, g, bl) = px(x, y);
                p.y[y * p.w + x] = (((47 * r + 157 * g + 16 * bl + 128) >> 8) + 16).clamp(0, 255) as u8;
            }
        }
        let cw = p.w / 2;
        for y in 0..p.h / 2 {
            for x in 0..cw {
                let (mut su, mut sv) = (0, 0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (r, g, bl) = px(x * 2 + dx, y * 2 + dy);
                    su += (-26 * r - 87 * g + 112 * bl + 128) >> 8;
                    sv += (112 * r - 102 * g - 10 * bl + 128) >> 8;
                }
                p.u[y * cw + x] = ((su + 2).div_euclid(4) + 128).clamp(0, 255) as u8;
                p.v[y * cw + x] = ((sv + 2).div_euclid(4) + 128).clamp(0, 255) as u8;
            }
        }
        p
    }

    fn nc_luma(&self, bx: usize, by: usize) -> i32 {
        let w = self.mbw * 4;
        let a = (bx > 0).then(|| i32::from(self.nc_y[by * w + bx - 1]));
        let t = (by > 0).then(|| i32::from(self.nc_y[(by - 1) * w + bx]));
        combine(a, t)
    }

    fn nc_chroma(&self, plane: &[u8], bx: usize, by: usize) -> i32 {
        let w = self.mbw * 2;
        let a = (bx > 0).then(|| i32::from(plane[by * w + bx - 1]));
        let t = (by > 0).then(|| i32::from(plane[(by - 1) * w + bx]));
        combine(a, t)
    }

    fn intra_mb(&mut self, b: &mut Bits, src: &Planes, mx: usize, my: usize, in_p: bool) {
        let qp = self.cfg.qp;
        let stride = self.recon.w;
        let (x0, y0) = (mx * 16, my * 16);
        let (has_top, has_left) = (my > 0, mx > 0);
        let top: Vec<i32> = if has_top { (0..16).map(|i| i32::from(self.recon.y[(y0 - 1) * stride + x0 + i])).collect() } else { vec![] };
        let left: Vec<i32> = if has_left { (0..16).map(|i| i32::from(self.recon.y[(y0 + i) * stride + x0 - 1])).collect() } else { vec![] };
        let dc = match (has_top, has_left) {
            (true, true) => (top.iter().sum::<i32>() + left.iter().sum::<i32>() + 16) >> 5,
            (true, false) => (top.iter().sum::<i32>() + 8) >> 4,
            (false, true) => (left.iter().sum::<i32>() + 8) >> 4,
            _ => 128,
        };
        let s = |x: usize, y: usize| i32::from(src.y[(y0 + y) * stride + x0 + x]);
        let pred_of = |mode: u32, x: usize, y: usize| match mode {
            0 => top[x],
            1 => left[y],
            _ => dc,
        };
        let mut modes = vec![2u32];
        if has_top {
            modes.push(0);
        }
        if has_left {
            modes.push(1);
        }
        let mode =
            modes.iter().copied().min_by_key(|&m| (0..256).map(|i| (s(i % 16, i / 16) - pred_of(m, i % 16, i / 16)).abs()).sum::<i32>()).unwrap_or(2);
        // Luma: transform each 4x4, DC through the Hadamard.
        let qbits = 15 + u32::from(qp / 6);
        let f = (1 << qbits) / 3;
        let m6 = usize::from(qp % 6);
        let mut ac = [[0i32; 16]; 16]; // [block raster (by*4+bx)][raster coeff], quantised
        let mut wdc = [0i32; 16];
        for blk in 0..16 {
            let (bx, by) = (blk % 4, blk / 4);
            let mut r = [0i32; 16];
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                r[i] = s(x, y) - pred_of(mode, x, y);
            }
            let w = fdct(&r);
            wdc[blk] = w[0];
            for k in 1..16 {
                ac[blk][k] = quant(w[k], MF[m6][pos_class(k)], qbits, f);
            }
        }
        let ydc = hadamard4(&wdc);
        let mut zdc = [0i32; 16];
        for k in 0..16 {
            zdc[k] = quant(ydc[k] / 2, MF[m6][0], qbits + 1, 2 * f);
        }
        let cbp_luma = ac.iter().any(|blk| blk.iter().skip(1).any(|c| *c != 0));
        // Chroma.
        let qpc = chroma_qp(qp);
        let cq = qpc_planes(self, src, mx, my, qpc);
        let cbp_chroma = if cq.iter().any(|c| c.ac.iter().any(|blk| blk.iter().skip(1).any(|v| *v != 0))) {
            2
        } else if cq.iter().any(|c| c.dc.iter().any(|v| *v != 0)) {
            1
        } else {
            0
        };
        // Syntax.
        let mb_type = 1 + mode + 4 * cbp_chroma + if cbp_luma { 12 } else { 0 };
        b.ue(mb_type + if in_p { 5 } else { 0 });
        b.ue(0); // intra_chroma_pred_mode: DC
        b.se(0); // mb_qp_delta
        let (bx0, by0) = (mx * 4, my * 4);
        let dc_scan: Vec<i32> = ZIGZAG.iter().map(|&r| zdc[r]).collect();
        let nc0 = self.nc_luma(bx0, by0);
        residual_block(b, &dc_scan, nc0);
        let lw = self.mbw * 4;
        for idx in 0..16 {
            let (bx, by) = blk_xy(idx);
            let blk = by * 4 + bx;
            let total = if cbp_luma {
                let scan: Vec<i32> = ZIGZAG.iter().skip(1).map(|&r| ac[blk][r]).collect();
                let nc = self.nc_luma(bx0 + bx, by0 + by);
                residual_block(b, &scan, nc)
            } else {
                0
            };
            self.nc_y[(by0 + by) * lw + bx0 + bx] = total;
        }
        if cbp_chroma > 0 {
            for c in &cq {
                residual_block(b, &c.dc, -1);
            }
        }
        let cw = self.mbw * 2;
        for (ci, c) in cq.iter().enumerate() {
            for blk in 0..4 {
                let (bx, by) = (blk % 2, blk / 2);
                let total = if cbp_chroma == 2 {
                    let scan: Vec<i32> = ZIGZAG.iter().skip(1).map(|&r| c.ac[blk][r]).collect();
                    let plane = if ci == 0 { &self.nc_u } else { &self.nc_v };
                    let nc = self.nc_chroma(plane, mx * 2 + bx, my * 2 + by);
                    residual_block(b, &scan, nc)
                } else {
                    0
                };
                let plane = if ci == 0 { &mut self.nc_u } else { &mut self.nc_v };
                plane[(my * 2 + by) * cw + mx * 2 + bx] = total;
            }
        }
        // Reconstruction, as the decoder does it.
        let fdc = hadamard4(&zdc);
        let ls0 = 16 * V[m6][0];
        let q6 = u32::from(qp / 6);
        for blk in 0..16 {
            let (bx, by) = (blk % 4, blk / 4);
            let mut d = [0i32; 16];
            d[0] = if qp >= 36 { (fdc[blk] * ls0) << (q6 - 6) } else { (fdc[blk] * ls0 + (1 << (5 - q6))) >> (6 - q6) };
            for k in 1..16 {
                d[k] = dequant(ac[blk][k], qp, pos_class(k));
            }
            let r = idct(&d);
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                self.recon.y[(y0 + y) * stride + x0 + x] = (pred_of(mode, x, y) + r[i]).clamp(0, 255) as u8;
            }
        }
        for (ci, c) in cq.iter().enumerate() {
            let cs = self.recon.w / 2;
            let plane = if ci == 0 { &mut self.recon.u } else { &mut self.recon.v };
            // Chroma DC levels are in raster order; inverse 2x2 transform and scale.
            let f2 = [
                c.dc[0] + c.dc[1] + c.dc[2] + c.dc[3],
                c.dc[0] - c.dc[1] + c.dc[2] - c.dc[3],
                c.dc[0] + c.dc[1] - c.dc[2] - c.dc[3],
                c.dc[0] - c.dc[1] - c.dc[2] + c.dc[3],
            ];
            let lsc = 16 * V[usize::from(qpc % 6)][0];
            for blk in 0..4 {
                let (bx, by) = (blk % 2, blk / 2);
                let mut d = [0i32; 16];
                d[0] = ((f2[blk] * lsc) << u32::from(qpc / 6)) >> 5;
                for k in 1..16 {
                    d[k] = dequant(c.ac[blk][k], qpc, pos_class(k));
                }
                let r = idct(&d);
                for i in 0..16 {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    plane[(my * 8 + y) * cs + mx * 8 + x] = (c.pred[blk] + r[i]).clamp(0, 255) as u8;
                }
            }
        }
    }
}

/// One chroma component of a macroblock: DC prediction per 4x4 block, quantised DC (raster
/// 2x2) and AC levels.
struct ChromaMb {
    pred: [i32; 4],
    dc: [i32; 4],
    ac: [[i32; 16]; 4],
}

fn qpc_planes(e: &Encoder, src: &Planes, mx: usize, my: usize, qpc: u8) -> [ChromaMb; 2] {
    let cs = e.recon.w / 2;
    let (x0, y0) = (mx * 8, my * 8);
    let qbits = 15 + u32::from(qpc / 6);
    let f = (1 << qbits) / 3;
    let m6 = usize::from(qpc % 6);
    let one = |rec: &[u8], s: &[u8]| -> ChromaMb {
        let top: Option<Vec<i32>> = (my > 0).then(|| (0..8).map(|i| i32::from(rec[(y0 - 1) * cs + x0 + i])).collect());
        let left: Option<Vec<i32>> = (mx > 0).then(|| (0..8).map(|i| i32::from(rec[(y0 + i) * cs + x0 - 1])).collect());
        let mut pred = [128; 4];
        for (blk, p) in pred.iter_mut().enumerate() {
            let (xo, yo) = ((blk % 2) * 4, (blk / 2) * 4);
            let st = top.as_ref().map(|t| t[xo..xo + 4].iter().sum::<i32>());
            let sl = left.as_ref().map(|l| l[yo..yo + 4].iter().sum::<i32>());
            *p = match ((xo, yo), st, sl) {
                ((0, 0), Some(a), Some(c)) | ((4, 4), Some(a), Some(c)) => (a + c + 4) >> 3,
                ((0, 0), _, Some(c)) | ((4, 4), _, Some(c)) => (c + 2) >> 2,
                ((0, 0), Some(a), _) | ((4, 4), Some(a), _) => (a + 2) >> 2,
                ((4, 0), Some(a), _) => (a + 2) >> 2,
                ((4, 0), None, Some(c)) => (c + 2) >> 2,
                ((0, 4), _, Some(c)) => (c + 2) >> 2,
                ((0, 4), Some(a), None) => (a + 2) >> 2,
                _ => 128,
            };
        }
        let mut wdc = [0i32; 4];
        let mut ac = [[0i32; 16]; 4];
        for blk in 0..4 {
            let (bx, by) = (blk % 2, blk / 2);
            let mut r = [0i32; 16];
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                r[i] = i32::from(s[(y0 + y) * cs + x0 + x]) - pred[blk];
            }
            let w = fdct(&r);
            wdc[blk] = w[0];
            for k in 1..16 {
                ac[blk][k] = quant(w[k], MF[m6][pos_class(k)], qbits, f);
            }
        }
        let y2 = [
            wdc[0] + wdc[1] + wdc[2] + wdc[3],
            wdc[0] - wdc[1] + wdc[2] - wdc[3],
            wdc[0] + wdc[1] - wdc[2] - wdc[3],
            wdc[0] - wdc[1] - wdc[2] + wdc[3],
        ];
        let dc = y2.map(|v| quant(v, MF[m6][0], qbits + 1, 2 * f));
        ChromaMb { pred, dc, ac }
    };
    [one(&e.recon.u, &src.u), one(&e.recon.v, &src.v)]
}

/// nC from the left and top neighbours' TotalCoeff (9.2.1).
fn combine(a: Option<i32>, t: Option<i32>) -> i32 {
    match (a, t) {
        (Some(a), Some(t)) => (a + t + 1) >> 1,
        (Some(a), None) => a,
        (None, Some(t)) => t,
        (None, None) => 0,
    }
}

/// `luma4x4BlkIdx` → 4x4 block position in the macroblock.
fn blk_xy(idx: usize) -> (usize, usize) {
    let (b8, b4) = (idx / 4, idx % 4);
    ((b8 % 2) * 2 + b4 % 2, (b8 / 2) * 2 + b4 / 2)
}

/// 4x4 Hadamard `H · X · H` (raster, symmetric H).
fn hadamard4(x: &[i32; 16]) -> [i32; 16] {
    let h = |a: i32, b: i32, c: i32, d: i32| [a + b + c + d, a + b - c - d, a - b - c + d, a - b + c - d];
    let mut t = [0i32; 16];
    for i in 0..4 {
        let r = h(x[i * 4], x[i * 4 + 1], x[i * 4 + 2], x[i * 4 + 3]);
        t[i * 4..i * 4 + 4].copy_from_slice(&r);
    }
    let mut out = [0i32; 16];
    for j in 0..4 {
        let c = h(t[j], t[4 + j], t[8 + j], t[12 + j]);
        for i in 0..4 {
            out[i * 4 + j] = c[i];
        }
    }
    out
}

/// The macroblock's source pixels are the same in both pictures.
fn same_mb(a: &Planes, b: &Planes, mx: usize, my: usize) -> bool {
    let w = a.w;
    let rows = |pa: &[u8], pb: &[u8], stride: usize, x0: usize, y0: usize, n: usize| {
        (0..n).all(|y| pa[(y0 + y) * stride + x0..(y0 + y) * stride + x0 + n] == pb[(y0 + y) * stride + x0..(y0 + y) * stride + x0 + n])
    };
    rows(&a.y, &b.y, w, mx * 16, my * 16, 16) && rows(&a.u, &b.u, w / 2, mx * 8, my * 8, 8) && rows(&a.v, &b.v, w / 2, mx * 8, my * 8, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test card: gradients, a hard-edged box and text-like stripes.
    fn card(w: u32, h: u32, shift: u32) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let inbox = (x + shift) % w > w / 4 && (x + shift) % w < w / 2 && y > h / 3 && y < h * 2 / 3;
                let stripe = y > h * 3 / 4 && (x / 3) % 2 == 0;
                let (r, g, b) = if inbox {
                    (230, 80, 30)
                } else if stripe {
                    (20, 20, 20)
                } else {
                    ((x * 255 / w) as u8, (y * 255 / h) as u8, 200)
                };
                out[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        out
    }

    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse: f64 = a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum::<f64>() / a.len().max(1) as f64;
        if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
    }

    #[test]
    fn decodes_with_our_decoder_at_good_quality() {
        let (w, h) = (200, 120); // not a multiple of 16: cropping
        let mut enc = Encoder::new(EncoderConfig { width: w, height: h, fps: 30, qp: 22 }).unwrap();
        let mut dec = crate::Decoder::new();
        let frames = [card(w, h, 0), card(w, h, 0), card(w, h, 17), card(w, h, 17)];
        let mut stream = vec![];
        let mut sizes = vec![];
        let mut recons = vec![];
        for (i, f) in frames.iter().enumerate() {
            let p = enc.encode_rgba(f, i == 0);
            sizes.push(p.length_prefixed().len());
            stream.extend(p.annex_b());
            recons.push(enc.recon.y.clone());
        }
        let mut pics = dec.decode(&stream, 0).unwrap();
        pics.extend(dec.flush());
        assert_eq!(pics.len(), 4);
        let src = enc.to_yuv(&frames[3]);
        for (k, pic) in pics.iter().enumerate() {
            assert_eq!((pic.width, pic.height), (w, h));
            // The decoder reconstructs exactly what the encoder predicted from.
            let stride = enc.recon.w;
            for y in 0..h as usize {
                let d = &pic.y[y * pic.y_stride..y * pic.y_stride + w as usize];
                let e = &recons[k][y * stride..y * stride + w as usize];
                assert_eq!(d, e, "picture {k} row {y}");
            }
        }
        let last = &pics[3];
        let dy: Vec<u8> = (0..h as usize).flat_map(|y| last.y[y * last.y_stride..y * last.y_stride + w as usize].to_vec()).collect();
        let sy: Vec<u8> = (0..h as usize).flat_map(|y| src.y[y * src.w..y * src.w + w as usize].to_vec()).collect();
        assert!(psnr(&dy, &sy) > 38.0, "psnr {}", psnr(&dy, &sy));
        // An unchanged picture is all skipped: a handful of bytes.
        assert!(sizes[1] < 16, "{sizes:?}");
        assert!(sizes[3] < 16 && sizes[2] < sizes[0], "{sizes:?}");
    }

    #[test]
    fn rejects_bad_sizes_and_writes_levels_for_big_pictures() {
        assert!(Encoder::new(EncoderConfig { width: 15, height: 16, fps: 30, qp: 26 }).is_err());
        assert!(Encoder::new(EncoderConfig { width: 101, height: 16, fps: 30, qp: 26 }).is_err());
        let e = Encoder::new(EncoderConfig { width: 1920, height: 1080, fps: 30, qp: 26 }).unwrap();
        assert_eq!(e.level_idc(), 40);
        let e = Encoder::new(EncoderConfig { width: 3840, height: 2160, fps: 30, qp: 26 }).unwrap();
        assert_eq!(e.level_idc(), 51);
    }

    #[test]
    fn big_levels_and_flat_pictures_round_trip() {
        // Black then white at a low QP: large DC levels and escapes.
        let (w, h) = (64, 32);
        let mut enc = Encoder::new(EncoderConfig { width: w, height: h, fps: 30, qp: 10 }).unwrap();
        let mut dec = crate::Decoder::new();
        let mut stream = vec![];
        for (i, v) in [0u8, 255, 128].iter().enumerate() {
            let mut img = vec![*v; (w * h * 4) as usize];
            // Noise for many coefficients per block.
            for (k, p) in img.iter_mut().enumerate() {
                if k % 4 != 3 && (k * 7919) % 13 < 6 {
                    *p = p.wrapping_add(((k * 31) % 97) as u8);
                }
            }
            stream.extend(enc.encode_rgba(&img, i == 0).annex_b());
        }
        let mut pics = dec.decode(&stream, 0).unwrap();
        pics.extend(dec.flush());
        assert_eq!(pics.len(), 3);
        let last = &pics[2];
        let stride = enc.recon.w;
        for y in 0..h as usize {
            assert_eq!(&last.y[y * last.y_stride..y * last.y_stride + w as usize], &enc.recon.y[y * stride..y * stride + w as usize]);
        }
    }
}
