//! Compact glyph records, triplets and split instruction streams (MTX §§5.7–7).
use super::{MAX_FONT, Result, binary::Reader};

pub(super) struct Decoded {
    pub glyphs: Vec<u8>,
    pub offsets: Vec<u32>,
    pub max_program: u16,
}

pub(super) fn decode(data: &[u8], pushes: &[u8], codes: &[u8], count: usize) -> Result<Decoded> {
    let (mut r, mut p, mut c) = (Reader::new(data), Reader::new(pushes), Reader::new(codes));
    let mut glyphs = Vec::new();
    let mut max_program = 0;
    let mut offsets = Vec::with_capacity(count + 1);
    for _ in 0..count {
        offsets.push(glyphs.len() as u32);
        let contours = r.i16()?;
        let glyph = match contours {
            0 => Vec::new(),
            -1 => composite(&mut r, &mut p, &mut c, count, &mut max_program)?,
            1..=32767 => simple(&mut r, &mut p, &mut c, contours, &mut max_program)?,
            _ => return Err("invalid MTX contour count"),
        };
        if glyphs.len() + glyph.len() + 3 > MAX_FONT {
            return Err("MTX glyph budget exceeded");
        }
        glyphs.extend(glyph);
        glyphs.resize(glyphs.len().next_multiple_of(4), 0);
    }
    offsets.push(glyphs.len() as u32);
    if p.pos != pushes.len() || c.pos != codes.len() {
        return Err("unused MTX instructions");
    }
    Ok(Decoded { glyphs, offsets, max_program })
}

fn program(r: &mut Reader<'_>, p: &mut Reader<'_>, c: &mut Reader<'_>, max_program: &mut u16) -> Result<Vec<u8>> {
    let count = r.u255()? as usize;
    let code_size = r.u255()? as usize;
    let mut values = Vec::<i16>::with_capacity(count);
    while values.len() < count {
        let code = *p.bytes.get(p.pos).ok_or("truncated MTX pushes")?;
        if matches!(code, 251 | 252) {
            p.byte()?;
            let a = *values.get(values.len().checked_sub(2).ok_or("invalid MTX hop")?).ok_or("invalid MTX hop")?;
            values.extend([a, p.s255()?, a]);
            if code == 252 {
                values.extend([p.s255()?, a]);
            }
            if values.len() > count {
                return Err("MTX hop exceeds push count");
            }
        } else {
            values.push(p.s255()?);
        }
    }
    let mut out = Vec::new();
    // PUSHW preserves stack values, even when the original chose a shorter PUSHB sequence.
    for chunk in values.chunks(255) {
        out.extend([0x41, chunk.len() as u8]);
        for value in chunk {
            out.extend(value.to_be_bytes());
        }
    }
    out.extend(c.take(code_size)?);
    if out.len() > u16::MAX as usize {
        return Err("oversized MTX glyph program");
    }
    *max_program = (*max_program).max(out.len() as u16);
    Ok(out)
}

fn composite(r: &mut Reader<'_>, p: &mut Reader<'_>, c: &mut Reader<'_>, glyph_count: usize, max_program: &mut u16) -> Result<Vec<u8>> {
    let mut out = (-1i16).to_be_bytes().to_vec();
    out.extend(r.take(8)?);
    let mut instructions = false;
    for _ in 0..65535 {
        let flags = r.u16()?;
        let id = r.u16()?;
        if id as usize >= glyph_count {
            return Err("invalid MTX component glyph");
        }
        out.extend(flags.to_be_bytes());
        out.extend(id.to_be_bytes());
        let args = if flags & 1 != 0 { 4 } else { 2 };
        let matrix = if flags & 0x80 != 0 {
            8
        } else if flags & 0x40 != 0 {
            4
        } else if flags & 8 != 0 {
            2
        } else {
            0
        };
        out.extend(r.take(args + matrix)?);
        instructions |= flags & 0x100 != 0;
        if flags & 0x20 == 0 {
            if instructions {
                let code = program(r, p, c, max_program)?;
                out.extend((code.len() as u16).to_be_bytes());
                out.extend(code);
            }
            return Ok(out);
        }
    }
    Err("too many MTX components")
}

fn simple(r: &mut Reader<'_>, p: &mut Reader<'_>, c: &mut Reader<'_>, mut contours: i16, max_program: &mut u16) -> Result<Vec<u8>> {
    let explicit = if contours == 32767 {
        contours = r.i16()?;
        if contours < 0 {
            return Err("invalid MTX explicit contour count");
        }
        Some([r.i16()?, r.i16()?, r.i16()?, r.i16()?])
    } else {
        None
    };
    let mut ends = Vec::with_capacity(contours as usize);
    let mut end = 0usize;
    for i in 0..contours {
        let n = r.u255()? as usize;
        if i != 0 && n == 0 {
            return Err("empty MTX contour");
        }
        end += n;
        if end > u16::MAX as usize {
            return Err("oversized MTX contour");
        }
        ends.push(end as u16);
    }
    let count = if contours == 0 { 0 } else { end + 1 };
    let flags = r.take(count)?;
    let mut xs = Vec::with_capacity(count * 2);
    let mut ys = Vec::with_capacity(count * 2);
    let (mut x, mut y) = (0i32, 0i32);
    let mut bbox = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
    for &flag in flags {
        let (dx, dy) = triplet(r, flag & 127)?;
        x += dx;
        y += dy;
        let (px, py) = (i16::try_from(x).map_err(|_| "MTX x coordinate overflow")?, i16::try_from(y).map_err(|_| "MTX y coordinate overflow")?);
        bbox = [bbox[0].min(px), bbox[1].min(py), bbox[2].max(px), bbox[3].max(py)];
        // TrueType deltas are signed words with modulo-65536 coordinate accumulation.
        xs.extend((dx as i16).to_be_bytes());
        ys.extend((dy as i16).to_be_bytes());
    }
    let bbox = explicit.unwrap_or(if count == 0 { [0; 4] } else { bbox });
    let code = if contours != 0 { program(r, p, c, max_program)? } else { Vec::new() };
    let mut out = contours.to_be_bytes().to_vec();
    for value in bbox {
        out.extend(value.to_be_bytes());
    }
    for value in ends {
        out.extend(value.to_be_bytes());
    }
    out.extend((code.len() as u16).to_be_bytes());
    out.extend(code);
    out.extend(flags.iter().map(|f| u8::from(f & 128 == 0)));
    out.extend(xs);
    out.extend(ys);
    Ok(out)
}

fn triplet(r: &mut Reader<'_>, f: u8) -> Result<(i32, i32)> {
    let f = f as i32;
    let sign = |flag: i32, n: i32| if flag & 1 != 0 { n } else { -n };
    let (dx, dy) = match f {
        0..=9 => (0, sign(f, ((f & 14) << 7) + r.byte()? as i32)),
        10..=19 => (sign(f, (((f - 10) & 14) << 7) + r.byte()? as i32), 0),
        20..=83 => {
            let b = f - 20;
            let v = r.byte()? as i32;
            (sign(f, 1 + (b & 0x30) + (v >> 4)), sign(f >> 1, 1 + ((b & 12) << 2) + (v & 15)))
        }
        84..=119 => {
            let b = f - 84;
            (sign(f, 1 + ((b / 12) << 8) + r.byte()? as i32), sign(f >> 1, 1 + (((b % 12) >> 2) << 8) + r.byte()? as i32))
        }
        120..=123 => {
            let (a, b, c) = (r.byte()? as i32, r.byte()? as i32, r.byte()? as i32);
            (sign(f, (a << 4) + (b >> 4)), sign(f >> 1, ((b & 15) << 8) + c))
        }
        _ => (sign(f, r.u16()? as i32), sign(f >> 1, r.u16()? as i32)),
    };
    Ok((dx, dy))
}
