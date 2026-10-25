//! Binary FBX records.
//!
//! After the 27-byte header (the magic, then a `u32` version), the file is a list of records:
//! end offset, property count and property-list length (`u32`, or `u64` from version 7500), a
//! name length byte and the name, the properties, then nested records up to the end offset. A
//! record of all zeros ends a list. Properties are a type byte then: `Y` i16, `C` bool, `I` i32,
//! `F` f32, `D` f64, `L` i64, `S`/`R` a `u32` length and bytes, or `f d l i b` arrays (count,
//! encoding — 0 raw, 1 zlib — and byte length, then the data).

use super::{Node, Prop};
use crate::{Error, Result, invalid};

const FORMAT: &str = "FBX";
/// Most array elements across the whole file.
const MAX_ELEMENTS: usize = 256 << 20;
/// Most records.
const MAX_NODES: usize = 4 << 20;

struct Reader<'a> {
    b: &'a [u8],
    wide: bool,
    elements: usize,
    nodes: usize,
}

fn short() -> Error {
    invalid(FORMAT, "a record runs past the end of the file")
}

impl<'a> Reader<'a> {
    fn bytes(&self, at: usize, n: usize) -> Result<&'a [u8]> {
        at.checked_add(n).and_then(|e| self.b.get(at..e)).ok_or_else(short)
    }

    fn arr<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        let s = self.bytes(at, N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    fn u32(&self, at: usize) -> Result<u32> {
        self.arr::<4>(at).map(u32::from_le_bytes)
    }

    /// An offset/count field: `u32`, or `u64` in 7500+ files.
    fn field(&self, at: &mut usize) -> Result<usize> {
        let v = if self.wide {
            let v = u64::from_le_bytes(self.arr::<8>(*at)?);
            *at += 8;
            usize::try_from(v).map_err(|_| short())?
        } else {
            let v = self.u32(*at)? as usize;
            *at += 4;
            v
        };
        Ok(v)
    }

    /// Records from `*at` up to `end`, stopping at a null record.
    fn list(&mut self, at: &mut usize, end: usize, depth: usize) -> Result<Vec<Node>> {
        if depth > crate::MAX_DEPTH {
            return Err(invalid(FORMAT, "records nest too deeply"));
        }
        let header = if self.wide { 25 } else { 13 };
        let mut out = Vec::new();
        while at.checked_add(header).is_some_and(|e| e <= end) {
            let start = *at;
            let rec_end = self.field(at)?;
            let num_props = self.field(at)?;
            let props_len = self.field(at)?;
            let name_len = *self.b.get(*at).ok_or_else(short)? as usize;
            *at += 1;
            if rec_end == 0 {
                // Null record: the list ends.
                break;
            }
            if rec_end <= start || rec_end > end {
                return Err(invalid(FORMAT, "a record's end offset is out of place"));
            }
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(Error::TooLarge("too many FBX records".into()));
            }
            let name = String::from_utf8_lossy(self.bytes(*at, name_len)?).into_owned();
            *at += name_len;
            let props_end = at.checked_add(props_len).filter(|&e| e <= rec_end).ok_or_else(short)?;
            let mut props = Vec::with_capacity(num_props.min(64));
            for _ in 0..num_props {
                if *at >= props_end {
                    break;
                }
                props.push(self.prop(at, props_end)?);
            }
            *at = props_end;
            let children = if *at < rec_end { self.list(at, rec_end, depth + 1)? } else { Vec::new() };
            *at = rec_end;
            out.push(Node { name, props, children });
        }
        Ok(out)
    }

    fn prop(&mut self, at: &mut usize, end: usize) -> Result<Prop> {
        let kind = *self.b.get(*at).ok_or_else(short)?;
        *at += 1;
        let mut take = |n: usize| -> Result<&'a [u8]> {
            let s = self.bytes(*at, n)?;
            if *at + n > end {
                return Err(short());
            }
            *at += n;
            Ok(s)
        };
        Ok(match kind {
            b'Y' => {
                let s = four(take(2)?);
                Prop::Int(i16::from_le_bytes([s[0], s[1]]) as i64)
            }
            b'C' => Prop::Int(take(1)?.first().copied().unwrap_or(0) as i64),
            b'I' => Prop::Int(i32::from_le_bytes(four(take(4)?)) as i64),
            b'F' => Prop::Float(f32::from_le_bytes(four(take(4)?)) as f64),
            b'D' => Prop::Float(f64::from_le_bytes(eight(take(8)?))),
            b'L' => Prop::Int(i64::from_le_bytes(eight(take(8)?))),
            b'S' | b'R' => {
                let n = u32::from_le_bytes(four(take(4)?)) as usize;
                let s = take(n)?;
                if kind == b'S' { Prop::Str(String::from_utf8_lossy(s).into_owned()) } else { Prop::Bytes(s.len()) }
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => {
                let count = u32::from_le_bytes(four(take(4)?)) as usize;
                let encoding = u32::from_le_bytes(four(take(4)?));
                let len = u32::from_le_bytes(four(take(4)?)) as usize;
                let data = take(len)?;
                self.array(kind, count, encoding, data)?
            }
            other => return Err(invalid(FORMAT, format!("unknown property type {:?}", other as char))),
        })
    }

    fn array(&mut self, kind: u8, count: usize, encoding: u32, data: &[u8]) -> Result<Prop> {
        self.elements = self.elements.checked_add(count).ok_or_else(|| Error::TooLarge("FBX arrays".into()))?;
        if self.elements > MAX_ELEMENTS {
            return Err(Error::TooLarge("FBX arrays hold too many values".into()));
        }
        let size = match kind {
            b'f' | b'i' => 4,
            b'd' | b'l' => 8,
            _ => 1,
        };
        let want = count.checked_mul(size).ok_or_else(|| Error::TooLarge("FBX array".into()))?;
        let raw;
        let bytes: &[u8] = match encoding {
            0 => data,
            1 => {
                raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, want)
                    .map_err(|_| invalid(FORMAT, "a compressed array can't be inflated"))?;
                &raw
            }
            e => return Err(invalid(FORMAT, format!("unknown array encoding {e}"))),
        };
        if bytes.len() != want {
            return Err(invalid(FORMAT, "an array's length doesn't match its count"));
        }
        Ok(match kind {
            b'f' => Prop::Floats(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c) as f64).collect()),
            b'd' => Prop::Floats(bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect()),
            b'i' => Prop::Ints(bytes.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c) as i64).collect()),
            b'l' => Prop::Ints(bytes.as_chunks::<8>().0.iter().map(|c| i64::from_le_bytes(*c)).collect()),
            _ => Prop::Ints(bytes.iter().map(|&b| b as i64).collect()),
        })
    }
}

fn four(s: &[u8]) -> [u8; 4] {
    let mut a = [0u8; 4];
    for (d, v) in a.iter_mut().zip(s) {
        *d = *v;
    }
    a
}

fn eight(s: &[u8]) -> [u8; 8] {
    let mut a = [0u8; 8];
    for (d, v) in a.iter_mut().zip(s) {
        *d = *v;
    }
    a
}

/// The top-level records of a binary FBX.
pub fn parse(b: &[u8]) -> Result<Vec<Node>> {
    if !b.starts_with(super::BINARY_MAGIC) {
        return Err(invalid(FORMAT, "missing the binary FBX signature"));
    }
    let mut r = Reader { b, wide: false, elements: 0, nodes: 0 };
    let version = r.u32(23)?;
    r.wide = version >= 7500;
    let mut at = 27usize;
    r.list(&mut at, b.len(), 0)
}
