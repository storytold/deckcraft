//! Huffman coding of spectral values and scalefactors (ISO/IEC 14496-3 §4.6.3, Annex 4.A).

use std::sync::OnceLock;

use deckcraft_bitstream::{BitReader, BitWriter};

use crate::huffman_tables::{
    SCALEFACTOR, SPECTRUM_1, SPECTRUM_2, SPECTRUM_3, SPECTRUM_4, SPECTRUM_5, SPECTRUM_6, SPECTRUM_7, SPECTRUM_8, SPECTRUM_9, SPECTRUM_10, SPECTRUM_11,
};
use crate::{Error, Result};

/// `ZERO_HCB`: band quantised to zero, nothing transmitted.
pub const ZERO_HCB: u8 = 0;
/// `ESC_HCB`: codebook 11 with escape sequences.
pub const ESC_HCB: u8 = 11;
/// `NOISE_HCB`: perceptual noise substitution.
pub const NOISE_HCB: u8 = 13;
/// `INTENSITY_HCB2`: intensity stereo, out of phase.
pub const INTENSITY_HCB2: u8 = 14;
/// `INTENSITY_HCB`: intensity stereo, in phase.
pub const INTENSITY_HCB: u8 = 15;

/// Parameters of one spectrum codebook.
pub struct Book {
    /// Values per codeword (4 = quad, 2 = pair).
    pub dim: usize,
    /// Signed values in the codeword (no separate sign bits).
    pub signed: bool,
    /// Largest absolute value (16 = escape for codebook 11).
    pub lav: u32,
    /// Radix of the index (`2*lav+1` signed, `lav+1` unsigned).
    pub modulo: u32,
    pub codes: &'static [(u32, u8)],
}

static BOOKS: [Book; 11] = [
    Book { dim: 4, signed: true, lav: 1, modulo: 3, codes: &SPECTRUM_1 },
    Book { dim: 4, signed: true, lav: 1, modulo: 3, codes: &SPECTRUM_2 },
    Book { dim: 4, signed: false, lav: 2, modulo: 3, codes: &SPECTRUM_3 },
    Book { dim: 4, signed: false, lav: 2, modulo: 3, codes: &SPECTRUM_4 },
    Book { dim: 2, signed: true, lav: 4, modulo: 9, codes: &SPECTRUM_5 },
    Book { dim: 2, signed: true, lav: 4, modulo: 9, codes: &SPECTRUM_6 },
    Book { dim: 2, signed: false, lav: 7, modulo: 8, codes: &SPECTRUM_7 },
    Book { dim: 2, signed: false, lav: 7, modulo: 8, codes: &SPECTRUM_8 },
    Book { dim: 2, signed: false, lav: 12, modulo: 13, codes: &SPECTRUM_9 },
    Book { dim: 2, signed: false, lav: 12, modulo: 13, codes: &SPECTRUM_10 },
    Book { dim: 2, signed: false, lav: 16, modulo: 17, codes: &SPECTRUM_11 },
];

/// Spectrum codebook 1..=11.
#[inline]
pub fn book(cb: u8) -> &'static Book {
    &BOOKS[(cb as usize).clamp(1, 11) - 1]
}

const LEAF: u32 = 1 << 31;

/// Binary decoding tree for one codebook.
struct Tree {
    nodes: Vec<[u32; 2]>,
}

impl Tree {
    fn build(codes: &[(u32, u8)]) -> Tree {
        let mut nodes: Vec<[u32; 2]> = vec![[0, 0]];
        for (sym, &(code, len)) in codes.iter().enumerate() {
            let mut node = 0usize;
            for i in (0..len).rev() {
                let bit = ((code >> i) & 1) as usize;
                if i == 0 {
                    nodes[node][bit] = LEAF | sym as u32;
                } else {
                    if nodes[node][bit] == 0 {
                        nodes.push([0, 0]);
                        nodes[node][bit] = (nodes.len() - 1) as u32;
                    }
                    node = nodes[node][bit] as usize;
                }
            }
        }
        Tree { nodes }
    }

    #[inline]
    fn decode(&self, br: &mut BitReader) -> Result<u32> {
        let mut node = 0usize;
        loop {
            let bit = br.read_bits(1)? as usize;
            let next = self.nodes[node][bit];
            if next & LEAF != 0 {
                return Ok(next & !LEAF);
            }
            if next == 0 {
                return Err(Error::Bitstream("invalid Huffman codeword"));
            }
            node = next as usize;
        }
    }
}

fn trees() -> &'static [Tree; 12] {
    static TREES: OnceLock<[Tree; 12]> = OnceLock::new();
    TREES.get_or_init(|| std::array::from_fn(|i| if i == 0 { Tree::build(&SCALEFACTOR) } else { Tree::build(BOOKS[i - 1].codes) }))
}

/// Decode one scalefactor difference (`-60..=60`).
#[inline]
pub fn decode_sf(br: &mut BitReader) -> Result<i32> {
    Ok(trees()[0].decode(br)? as i32 - 60)
}

/// Write one scalefactor difference (`-60..=60`).
#[inline]
pub fn write_sf(bw: &mut BitWriter, diff: i32) {
    let (code, len) = SCALEFACTOR[(diff + 60).clamp(0, 120) as usize];
    bw.write_bits(code, len as u32);
}

/// Bits of one scalefactor difference.
#[inline]
pub fn sf_bits(diff: i32) -> u32 {
    SCALEFACTOR[(diff + 60).clamp(0, 120) as usize].1 as u32
}

/// Decode one codeword of codebook `cb` (1..=11) into `out[..dim]`, including sign bits and escapes.
pub fn decode_spectral(br: &mut BitReader, cb: u8, out: &mut [i32]) -> Result<()> {
    let b = book(cb);
    let mut idx = trees()[cb as usize].decode(br)?;
    for i in (0..b.dim).rev() {
        out[i] = (idx % b.modulo) as i32;
        idx /= b.modulo;
    }
    if b.signed {
        for v in out[..b.dim].iter_mut() {
            *v -= b.lav as i32;
        }
        return Ok(());
    }
    for v in out[..b.dim].iter_mut() {
        if *v != 0 && br.read_bits(1)? == 1 {
            *v = -*v;
        }
    }
    if cb == ESC_HCB {
        for v in out[..b.dim].iter_mut() {
            if v.abs() == 16 {
                let mut n = 0u32;
                while br.read_bits(1)? == 1 {
                    n += 1;
                    if n > 8 {
                        return Err(Error::Bitstream("escape sequence too long"));
                    }
                }
                let mag = (1i32 << (n + 4)) + br.read_bits(n + 4)? as i32;
                *v = if *v < 0 { -mag } else { mag };
            }
        }
    }
    Ok(())
}

/// Codebook index of the (sign-stripped for unsigned books) values.
#[inline]
fn index_of(b: &Book, vals: &[i32]) -> usize {
    let mut idx = 0u32;
    for &v in &vals[..b.dim] {
        let d = if b.signed { (v + b.lav as i32) as u32 } else { v.unsigned_abs().min(b.lav) };
        idx = idx * b.modulo + d;
    }
    idx as usize
}

#[inline]
fn escape_bits(mag: u32) -> u32 {
    // mag >= 16: N ones, a zero, N+4 bits where 2^(N+4) <= mag < 2^(N+5).
    let n = 31 - mag.leading_zeros() - 4;
    2 * n + 5
}

/// Bits needed to code `vals` (a multiple of the codebook dimension) with codebook `cb` (1..=11).
/// The values must fit the codebook (`|v| <= lav`, or `<= 8191` for codebook 11).
pub fn count_bits(cb: u8, vals: &[i32]) -> u32 {
    let b = book(cb);
    let mut bits = 0u32;
    for chunk in vals.chunks_exact(b.dim) {
        bits += b.codes[index_of(b, chunk)].1 as u32;
        if !b.signed {
            for &v in chunk {
                if v != 0 {
                    bits += 1;
                    if cb == ESC_HCB && v.unsigned_abs() >= 16 {
                        bits += escape_bits(v.unsigned_abs());
                    }
                }
            }
        }
    }
    bits
}

/// Write `vals` with codebook `cb` (1..=11).
pub fn write_spectral(bw: &mut BitWriter, cb: u8, vals: &[i32]) {
    let b = book(cb);
    for chunk in vals.chunks_exact(b.dim) {
        let (code, len) = b.codes[index_of(b, chunk)];
        bw.write_bits(code, len as u32);
        if b.signed {
            continue;
        }
        for &v in chunk {
            if v != 0 {
                bw.write_bits((v < 0) as u32, 1);
            }
        }
        if cb == ESC_HCB {
            for &v in chunk {
                let mag = v.unsigned_abs();
                if mag >= 16 {
                    let n = 31 - mag.leading_zeros() - 4;
                    for _ in 0..n {
                        bw.write_bits(1, 1);
                    }
                    bw.write_bits(0, 1);
                    bw.write_bits(mag - (1 << (n + 4)), n + 4);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_complete(codes: &[(u32, u8)]) {
        // Kraft sum exactly one and prefix-free.
        let max = codes.iter().map(|c| c.1).max().unwrap() as u32;
        let mut kraft = 0u64;
        for &(_, l) in codes {
            kraft += 1u64 << (max - l as u32);
        }
        assert_eq!(kraft, 1u64 << max);
        for (i, &(ci, li)) in codes.iter().enumerate() {
            for (j, &(cj, lj)) in codes.iter().enumerate() {
                if i != j && li <= lj {
                    assert_ne!(cj >> (lj - li), ci, "codeword {i} is a prefix of {j}");
                }
            }
        }
    }

    #[test]
    fn codebooks_are_complete_prefix_codes() {
        check_complete(&SCALEFACTOR);
        assert_eq!(SCALEFACTOR[60], (0, 1));
        for b in &BOOKS {
            assert_eq!(b.codes.len() as u32, b.modulo.pow(b.dim as u32));
            check_complete(b.codes);
        }
    }

    #[test]
    fn spectral_roundtrip_every_book() {
        for cb in 1..=11u8 {
            let b = book(cb);
            let lim = if cb == 11 { 8191 } else { b.lav as i32 };
            let mut vals = Vec::new();
            let mut seed = 12345u32;
            for _ in 0..400 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let r = (seed >> 8) as i32;
                let v = if cb == 11 && r % 3 == 0 { r % (lim + 1) } else { r % (b.lav.min(16) as i32 + 1) };
                let v = v.min(lim);
                vals.push(if b.signed || r & 1 == 0 { v } else { -v });
            }
            if b.signed {
                for v in vals.iter_mut() {
                    *v = (*v).clamp(-(b.lav as i32), b.lav as i32);
                }
            }
            let mut bw = BitWriter::new();
            write_spectral(&mut bw, cb, &vals);
            assert_eq!(bw.bit_len() as u32, count_bits(cb, &vals));
            let data = bw.finish();
            let mut br = BitReader::new(&data);
            let mut out = vec![0i32; vals.len()];
            for c in out.chunks_mut(b.dim) {
                decode_spectral(&mut br, cb, c).unwrap();
            }
            assert_eq!(out, vals, "codebook {cb}");
        }
    }

    #[test]
    fn scalefactor_roundtrip() {
        let mut bw = BitWriter::new();
        for d in -60..=60 {
            write_sf(&mut bw, d);
        }
        let data = bw.finish();
        let mut br = BitReader::new(&data);
        for d in -60..=60 {
            assert_eq!(decode_sf(&mut br).unwrap(), d);
        }
    }
}
