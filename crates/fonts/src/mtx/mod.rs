//! Bounded MicroType Express → sfnt decoding, derived from the W3C MTX submission.
//! <https://www.w3.org/submissions/MTX/>. No executable font hinting is performed here.
mod binary;
mod glyph;
mod lz;
mod metrics;

use binary::{Reader, at_u16, put16, put32};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, &'static str>;
const MAX_FONT: usize = 40 << 20;
type Tables = BTreeMap<[u8; 4], Vec<u8>>;

pub(super) fn decode(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > MAX_FONT {
        return Err("oversized MTX input");
    }
    let mut r = Reader::new(data);
    let version = r.byte()?;
    if !(1..=3).contains(&version) {
        return Err("unsupported MTX version");
    }
    let limit = r.u24()?;
    let second = r.u24()?;
    let third = r.u24()?;
    if second < 10 || third < second || third > data.len() {
        return Err("invalid MTX block offsets");
    }
    let first = lz::decode(data.get(10..second).ok_or("missing MTX tables")?, version, limit)?;
    let pushes = lz::decode(data.get(second..third).ok_or("missing MTX pushes")?, version, limit)?;
    let codes = lz::decode(data.get(third..).ok_or("missing MTX codes")?, version, limit)?;
    if first.len() + pushes.len() + codes.len() > MAX_FONT {
        return Err("MTX block budget exceeded");
    }
    restore(&first, &pushes, &codes)
}

fn table<'a>(tables: &'a Tables, tag: &[u8; 4]) -> Result<&'a [u8]> {
    tables.get(tag).map(Vec::as_slice).ok_or("missing MTX font table")
}

fn restore(ctf: &[u8], pushes: &[u8], codes: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(ctf);
    let scaler = r.u32()?;
    if scaler != 0x10000 && scaler != u32::from_be_bytes(*b"true") {
        return Err("MTX needs TrueType outlines");
    }
    let count = r.u16()? as usize;
    if count == 0 || count > 4095 {
        return Err("invalid MTX table count");
    }
    r.take(6)?;
    let mut tables = Tables::new();
    let mut total = 0usize;
    for _ in 0..count {
        let tag: [u8; 4] = r.take(4)?.try_into().map_err(|_| "short MTX table tag")?;
        r.u32()?;
        let offset = r.u32()? as usize;
        let size = r.u32()? as usize;
        let end = offset.checked_add(size).ok_or("MTX table range overflow")?;
        total = total.checked_add(size).ok_or("MTX table size overflow")?;
        if total > MAX_FONT {
            return Err("MTX table budget exceeded");
        }
        let data = ctf.get(offset..end).ok_or("truncated MTX font table")?.to_vec();
        if tables.insert(tag, data).is_some() {
            return Err("duplicate MTX font table");
        }
    }
    let glyph_count = at_u16(table(&tables, b"maxp")?, 4)? as usize;
    let decoded = glyph::decode(table(&tables, b"glyf")?, pushes, codes, glyph_count)?;
    let head = tables.get_mut(b"head").ok_or("missing MTX head")?;
    // Long loca makes the expanded glyph encoding independent of its compact source length.
    put16(head, 50, 1)?;
    put32(head, 8, 0)?;
    if let Some(maxp) = tables.get_mut(b"maxp")
        && maxp.len() >= 32
    {
        put16(maxp, 26, decoded.max_program)?;
    }
    tables.insert(*b"glyf", decoded.glyphs);
    tables.insert(*b"loca", decoded.offsets.into_iter().flat_map(u32::to_be_bytes).collect());
    if let Some(cvt) = tables.remove(b"cvt ") {
        tables.insert(*b"cvt ", metrics::cvt(&cvt)?);
    }
    if let Some(hdmx) = tables.remove(b"hdmx") {
        let decoded = metrics::hdmx(&hdmx, table(&tables, b"head")?, table(&tables, b"hhea")?, table(&tables, b"hmtx")?, glyph_count)?;
        tables.insert(*b"hdmx", decoded);
    }
    if let Some(vdmx) = tables.remove(b"VDMX") {
        tables.insert(*b"VDMX", metrics::vdmx(&vdmx)?);
    }
    // A digital signature of the source bytes cannot sign a reconstructed sfnt serialization.
    tables.remove(b"DSIG");
    assemble(scaler, tables)
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        for (dest, src) in word.iter_mut().zip(chunk) {
            *dest = *src;
        }
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn assemble(scaler: u32, tables: Tables) -> Result<Vec<u8>> {
    let count = tables.len();
    let mut out = vec![0; 12 + count * 16];
    put32(&mut out, 0, scaler)?;
    put16(&mut out, 4, count as u16)?;
    let exponent = count.ilog2();
    put16(&mut out, 6, (16usize << exponent) as u16)?;
    put16(&mut out, 8, exponent as u16)?;
    put16(&mut out, 10, (count * 16 - (16usize << exponent)) as u16)?;
    let mut head_offset = None;
    for (i, (tag, bytes)) in tables.into_iter().enumerate() {
        let offset = out.len();
        if offset + bytes.len() + 3 > MAX_FONT {
            return Err("expanded MTX font too large");
        }
        if tag == *b"head" {
            head_offset = Some(offset);
        }
        let record = 12 + i * 16;
        put32(&mut out, record, u32::from_be_bytes(tag))?;
        put32(&mut out, record + 4, checksum(&bytes))?;
        put32(&mut out, record + 8, offset as u32)?;
        put32(&mut out, record + 12, bytes.len() as u32)?;
        out.extend(bytes);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    let adjustment = 0xb1b0afbau32.wrapping_sub(checksum(&out));
    put32(&mut out, head_offset.ok_or("missing MTX head")? + 8, adjustment)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
