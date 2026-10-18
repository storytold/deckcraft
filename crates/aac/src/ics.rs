//! `ics_info` and helpers shared by the encoder and decoder (ISO/IEC 14496-3 §4.4.2.1, §4.5.2.3).

use deckcraft_bitstream::{BitReader, BitWriter};

use crate::mdct::WindowShape;
use crate::tables::{swb_offsets_long, swb_offsets_short};
use crate::{Error, Result};

/// `window_sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowSequence {
    #[default]
    OnlyLong = 0,
    LongStart = 1,
    EightShort = 2,
    LongStop = 3,
}

impl WindowSequence {
    pub fn from_bits(v: u32) -> Self {
        match v & 3 {
            0 => WindowSequence::OnlyLong,
            1 => WindowSequence::LongStart,
            2 => WindowSequence::EightShort,
            _ => WindowSequence::LongStop,
        }
    }
}

/// Decoded/encoded `ics_info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcsInfo {
    pub window_sequence: WindowSequence,
    pub window_shape: WindowShape,
    pub max_sfb: usize,
    pub num_groups: usize,
    /// Windows per group (only the first `num_groups` entries are used; long windows: `[1]`).
    pub group_len: [u8; 8],
}

impl Default for IcsInfo {
    fn default() -> Self {
        IcsInfo {
            window_sequence: WindowSequence::OnlyLong,
            window_shape: WindowShape::Sine,
            max_sfb: 0,
            num_groups: 1,
            group_len: [1, 0, 0, 0, 0, 0, 0, 0],
        }
    }
}

impl IcsInfo {
    #[inline]
    pub fn is_short(&self) -> bool {
        self.window_sequence == WindowSequence::EightShort
    }
    #[inline]
    pub fn num_windows(&self) -> usize {
        if self.is_short() { 8 } else { 1 }
    }
    /// Band offsets of one window.
    pub fn swb_offsets(&self, sf_index: u8) -> &'static [u16] {
        if self.is_short() { swb_offsets_short(sf_index) } else { swb_offsets_long(sf_index) }
    }
    /// Number of bands for this window length (`num_swb`).
    pub fn num_swb(&self, sf_index: u8) -> usize {
        self.swb_offsets(sf_index).len() - 1
    }
    /// First window of group `g`.
    pub fn group_start(&self, g: usize) -> usize {
        self.group_len[..g].iter().map(|&l| l as usize).sum()
    }

    pub fn parse(br: &mut BitReader, sf_index: u8) -> Result<IcsInfo> {
        if br.read_bits(1)? != 0 {
            // ics_reserved_bit; tolerated by most decoders
        }
        let window_sequence = WindowSequence::from_bits(br.read_bits(2)?);
        let window_shape = WindowShape::from_bit(br.read_bits(1)? == 1);
        let mut info = IcsInfo { window_sequence, window_shape, ..Default::default() };
        if info.is_short() {
            info.max_sfb = br.read_bits(4)? as usize;
            let grouping = br.read_bits(7)?;
            info.num_groups = 1;
            info.group_len = [1, 0, 0, 0, 0, 0, 0, 0];
            for w in 1..8 {
                if grouping & (1 << (6 - (w - 1))) != 0 {
                    info.group_len[info.num_groups - 1] += 1;
                } else {
                    info.num_groups += 1;
                    info.group_len[info.num_groups - 1] = 1;
                }
            }
        } else {
            info.max_sfb = br.read_bits(6)? as usize;
            if br.read_bits(1)? == 1 {
                return Err(Error::Unsupported("predictor data (AAC Main / LTP)"));
            }
        }
        if info.max_sfb > info.num_swb(sf_index) {
            return Err(Error::Bitstream("max_sfb exceeds the number of scalefactor bands"));
        }
        Ok(info)
    }

    pub fn write(&self, bw: &mut BitWriter) {
        bw.write_bits(0, 1);
        bw.write_bits(self.window_sequence as u32, 2);
        bw.write_bits(self.window_shape.bit(), 1);
        if self.is_short() {
            bw.write_bits(self.max_sfb as u32, 4);
            let mut grouping = 0u32;
            let mut w = 0;
            for g in 0..self.num_groups {
                for i in 0..self.group_len[g] as usize {
                    if w > 0 && i > 0 {
                        grouping |= 1 << (6 - (w - 1));
                    }
                    w += 1;
                }
            }
            bw.write_bits(grouping, 7);
        } else {
            bw.write_bits(self.max_sfb as u32, 6);
            bw.write_bits(0, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_roundtrip() {
        let info = IcsInfo {
            window_sequence: WindowSequence::EightShort,
            window_shape: WindowShape::Kbd,
            max_sfb: 12,
            num_groups: 3,
            group_len: [3, 1, 4, 0, 0, 0, 0, 0],
        };
        let mut bw = BitWriter::new();
        info.write(&mut bw);
        let d = bw.finish();
        let p = IcsInfo::parse(&mut BitReader::new(&d), 4).unwrap();
        assert_eq!(p, info);
        assert_eq!(p.group_start(2), 4);
    }
}
