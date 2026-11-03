//! CTF control values and device metrics (MTX §§5.2–5.6).
use super::{
    MAX_FONT, Result,
    binary::{Bits, Reader, at_u16, put16},
};

pub(super) fn cvt(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let count = r.u16()?;
    let mut previous = 0i16;
    let mut out = Vec::with_capacity(count as usize * 2);
    for _ in 0..count {
        let b = r.byte()?;
        let delta = match b {
            238 => r.i16()?,
            239..=247 => -(238 * (b as i16 - 239) + r.byte()? as i16),
            248..=255 => 238 * (b as i16 - 247) + r.byte()? as i16,
            _ => b as i16,
        };
        previous = previous.wrapping_add(delta);
        out.extend(previous.to_be_bytes());
    }
    Ok(out)
}

fn plain(data: &[u8]) -> Result<Option<Vec<u8>>> {
    let version = at_u16(data, 0)?;
    if version & 0x8000 == 0 {
        return Ok(None);
    }
    let mut out = data.to_vec();
    put16(&mut out, 0, !version)?;
    Ok(Some(out))
}

pub(super) fn hdmx(data: &[u8], head: &[u8], hhea: &[u8], hmtx: &[u8], glyphs: usize) -> Result<Vec<u8>> {
    if let Some(out) = plain(data)? {
        return Ok(out);
    }
    let mut r = Reader::new(data);
    let version = r.u16()?;
    let records = r.u16()? as usize;
    let size = r.u32()? as usize;
    if size < glyphs + 2 || records.checked_mul(size).is_none_or(|n| n > MAX_FONT - 8) {
        return Err("invalid MTX device metric size");
    }
    let pairs = r.take(records * 2)?;
    let upem = at_u16(head, 18)? as i64;
    let metrics = at_u16(hhea, 34)? as usize;
    if upem == 0 || metrics == 0 || metrics > glyphs {
        return Err("invalid MTX advance metrics");
    }
    let mut advances = Vec::with_capacity(glyphs);
    for g in 0..glyphs {
        advances.push(at_u16(hmtx, g.min(metrics - 1) * 4)? as i64);
    }
    let mut bits = Bits::new(r.take(data.len() - r.pos)?, true);
    let mut out = Vec::with_capacity(8 + records * size);
    out.extend(version.to_be_bytes());
    out.extend((records as u16).to_be_bytes());
    out.extend((size as u32).to_be_bytes());
    for pair in pairs.chunks_exact(2) {
        let ppem = pair.first().copied().ok_or("missing MTX ppem")? as i64;
        out.extend(pair);
        for &advance in &advances {
            let prediction = (((64 * ppem * advance + upem / 2) / upem) + 32) / 64;
            out.push(u8::try_from(prediction + bits.magnitude()? as i64).map_err(|_| "MTX device width overflow")?);
        }
        out.resize(out.len() + size - glyphs - 2, 0);
    }
    Ok(out)
}

pub(super) fn vdmx(data: &[u8]) -> Result<Vec<u8>> {
    if let Some(out) = plain(data)? {
        return Ok(out);
    }
    let mut r = Reader::new(data);
    r.u16()?;
    let groups = r.u16()? as usize;
    let ratios = r.u16()? as usize;
    r.take(ratios * 4)?;
    let mut offsets = Vec::with_capacity(ratios);
    for _ in 0..ratios {
        offsets.push(r.u16()? as usize);
    }
    let header = r.pos;
    let start = *offsets.first().ok_or("missing MTX vertical metric group")?;
    if start < header || start > data.len() {
        return Err("invalid MTX vertical metric offset");
    }
    let mut out = data.get(..start).ok_or("short MTX vertical metrics")?.to_vec();
    r.pos = start;
    let mut group_offsets = Vec::with_capacity(groups);
    for _ in 0..groups {
        group_offsets.push(out.len());
        let n = r.u16()? as usize;
        let (max_multiplier, min_multiplier) = (r.i16()? as i64, r.i16()? as i64);
        if n == 0 || out.len() + 4 + n * 6 > MAX_FONT {
            return Err("invalid MTX vertical metric count");
        }
        let mut bits = Bits::new(data.get(r.pos..).ok_or("short MTX vertical metric bits")?, true);
        let (mut predicted, mut min, mut max) = (8i64, u8::MAX, 0u8);
        let mut entries = Vec::with_capacity(n * 6);
        for _ in 0..n {
            let ppem = predicted + bits.magnitude()? as i64;
            let ymax = (ppem * max_multiplier + 1024) / 2048 + bits.magnitude()? as i64;
            let ymin = -((ppem * min_multiplier + 1024) / 2048) + bits.magnitude()? as i64;
            let ppem = u8::try_from(ppem).map_err(|_| "MTX vertical ppem overflow")?;
            min = min.min(ppem);
            max = max.max(ppem);
            predicted = ppem as i64 + 1;
            entries.extend((ppem as u16).to_be_bytes());
            entries.extend(i16::try_from(ymax).map_err(|_| "MTX vertical maximum overflow")?.to_be_bytes());
            entries.extend(i16::try_from(ymin).map_err(|_| "MTX vertical minimum overflow")?.to_be_bytes());
        }
        r.pos += bits.consumed();
        out.extend((n as u16).to_be_bytes());
        out.extend([min, max]);
        out.extend(entries);
    }
    if offsets.iter().any(|offset| !group_offsets.contains(offset)) {
        return Err("invalid MTX vertical group reference");
    }
    Ok(out)
}
