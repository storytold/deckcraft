//! MicroType Express (MTX) decompression, written from the public W3C member submission
//! "MicroType Express (MTX) Font Format" (2008). An MTX font is a TrueType font in the Compact Table
//! Format (CTF) split into three LZCOMP streams; [`decompress`] turns it back into a TrueType font.
//!
//! Two device-metric tables, `hdmx` and `VDMX`, are left out of the result: they only help hinted
//! rendering at fixed pixel sizes, and nothing here uses them.

/// Bytes the LZ window starts out holding, so copies can reach back before the first byte.
const PRELOAD: usize = 2 * 32 * 96 + 4 * 256;
/// The largest font or stream this reads or builds.
const MAX_OUT: usize = 64 << 20;

/// The TrueType font inside an MTX stream, or none when the data is not a valid MTX font.
pub fn decompress(mtx: &[u8]) -> Option<Vec<u8>> {
    let version = *mtx.first()?;
    if !(1..=3).contains(&version) {
        return None;
    }
    let off2 = u24(mtx.get(4..7)?);
    let off3 = u24(mtx.get(7..10)?);
    if !(10 <= off2 && off2 <= off3 && off3 <= mtx.len()) {
        return None;
    }
    let tables = unpack(mtx.get(10..off2)?, version)?;
    let push = unpack(mtx.get(off2..off3)?, version)?;
    let code = unpack(mtx.get(off3..)?, version)?;
    rebuild(&tables, &push, &code)
}

/// A big-endian unsigned integer of up to eight bytes.
fn u24(b: &[u8]) -> usize {
    b.iter().fold(0, |v, &x| v << 8 | x as usize)
}

/// A reader of big-endian values.
struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    /// A reader at the start of `d`.
    fn new(d: &'a [u8]) -> Self {
        Cur { d, p: 0 }
    }
    /// The next `n` bytes, or none if fewer are left.
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    /// The next byte.
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    /// The next unsigned 16-bit value.
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    /// The next signed 16-bit value.
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    /// A `255USHORT`: one byte, or a marker byte and more.
    fn ushort255(&mut self) -> Option<u16> {
        Some(match self.u8()? {
            253 => self.u16()?,
            255 => 253 + self.u8()? as u16,
            254 => 506 + self.u8()? as u16,
            v => v as u16,
        })
    }
    /// A `255SHORT`: like `255USHORT` with a sign marker.
    fn short255(&mut self) -> Option<i16> {
        let mut code = self.u8()?;
        if code == 253 {
            return self.i16();
        }
        let sign = if code == 250 {
            code = self.u8()?;
            -1
        } else {
            1
        };
        let v = match code {
            255 => 250 + self.u8()? as i16,
            254 => 500 + self.u8()? as i16,
            v => v as i16,
        };
        Some(v * sign)
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    /// The next bit, most significant first, or none at the end of the data.
    fn bit(&mut self) -> Option<usize> {
        let byte = *self.d.get(self.pos >> 3)?;
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Some(bit as usize)
    }
    /// The next `n` bits as a number.
    fn value(&mut self, n: u32) -> Option<usize> {
        (0..n).try_fold(0, |v, _| Some(v << 1 | self.bit()?))
    }
}

#[derive(Clone, Copy, Default)]
struct Node {
    up: usize,
    left: usize,
    right: usize,
    /// The symbol of a leaf, negative for an inner node.
    code: i32,
    weight: u32,
}

/// An adaptive Huffman code over the symbols `0..range`. Node 1 is the root and the children of
/// node `i` start out at `2i` and `2i + 1`; the leaves are `range..2 * range`.
struct Ahuff {
    tree: Vec<Node>,
    leaf: Vec<usize>,
}

impl Ahuff {
    /// The code for `range` symbols in the state the format starts every stream in.
    fn new(range: usize) -> Self {
        let mut tree = vec![Node { code: -1, ..Node::default() }; 2 * range];
        for (i, n) in tree.iter_mut().enumerate().skip(2) {
            n.up = i / 2;
            n.weight = 1;
        }
        for (i, n) in tree.iter_mut().enumerate().take(range).skip(1) {
            n.left = 2 * i;
            n.right = 2 * i + 1;
        }
        let mut leaf = vec![0; range];
        for (i, l) in leaf.iter_mut().enumerate() {
            tree[range + i].code = i as i32;
            *l = range + i;
        }
        for i in (1..range).rev() {
            tree[i].weight = tree[2 * i].weight + tree[2 * i + 1].weight;
        }
        let mut t = Ahuff { tree, leaf };
        if range > 256 && range < 512 {
            // The symbol alphabet: literals, then copy commands, then DUP2, DUP4 and DUP6. Start
            // with the first two bytes of a font and the short copies the likelier ones.
            for (sym, times) in [(256, 1), (257, 1), (range - 3, 12), (range - 2, 6)] {
                for _ in 0..times {
                    t.update(t.leaf[sym]);
                }
            }
        } else {
            for _ in 0..2 {
                for sym in 0..range {
                    t.update(t.leaf[sym]);
                }
            }
        }
        t
    }

    /// Exchanges two nodes of equal weight, with their subtrees.
    fn swap(&mut self, a: usize, b: usize) {
        let (upa, upb) = (self.tree[a].up, self.tree[b].up);
        self.tree.swap(a, b);
        self.tree[a].up = upa;
        self.tree[b].up = upb;
        for n in [a, b] {
            if self.tree[n].code < 0 {
                let (l, r) = (self.tree[n].left, self.tree[n].right);
                self.tree[l].up = n;
                self.tree[r].up = n;
            } else {
                self.leaf[self.tree[n].code as usize] = n;
            }
        }
    }

    /// Counts one more use of the node `a` and its ancestors, keeping siblings ordered by weight.
    fn update(&mut self, mut a: usize) {
        while a > 1 {
            let w = self.tree[a].weight;
            if self.tree[a - 1].weight == w {
                let mut b = a - 1;
                while b > 1 && self.tree[b - 1].weight == w {
                    b -= 1;
                }
                if b > 1 {
                    self.swap(a, b);
                    a = b;
                }
            }
            self.tree[a].weight = w + 1;
            a = self.tree[a].up;
        }
        self.tree[1].weight += 1;
    }

    /// Decodes one symbol and counts it.
    fn read(&mut self, bits: &mut Bits) -> Option<usize> {
        let mut a = 1;
        // A tree over at most 512 symbols is never deeper than that.
        for _ in 0..512 {
            a = if bits.bit()? == 1 { self.tree[a].right } else { self.tree[a].left };
            let code = self.tree.get(a)?.code;
            if code >= 0 {
                self.update(a);
                return Some(code as usize);
            }
        }
        None
    }
}

/// One LZCOMP stream: an adaptive Huffman coded mix of literals and copies, optionally run-length
/// coded on top.
fn unpack(data: &[u8], version: u8) -> Option<Vec<u8>> {
    let mut bits = Bits { d: data, pos: 0 };
    let run_length = version != 1 && bits.bit()? == 1;
    let mut dist = Ahuff::new(8);
    let mut len = Ahuff::new(8);
    let out_len = bits.value(24)?;
    // Copy distances are written in groups of three bits; the group count a stream needs depends on
    // how long it is.
    let mut ranges = 1;
    while 1usize << (3 * ranges) < out_len {
        ranges += 1;
    }
    let dup2 = 256 + 8 * ranges;
    let mut sym = Ahuff::new(dup2 + 3);

    let mut buf = Vec::with_capacity(PRELOAD + out_len);
    for k in 0..32u8 {
        for j in 0..96u8 {
            buf.extend([k, j]);
        }
    }
    for j in 0..=255u8 {
        buf.extend([j; 4]);
    }
    buf.truncate(PRELOAD);
    while buf.len() < PRELOAD + out_len {
        let s = sym.read(&mut bits)?;
        if s < 256 {
            buf.push(s as u8);
        } else if s >= dup2 {
            // A byte copied from 2, 4 or 6 positions back.
            let v = *buf.get(buf.len().checked_sub(2 * (s - dup2 + 1))?)?;
            buf.push(v);
        } else {
            let first = s - 256;
            let groups = first / 8 + 1;
            let mut code = first % 8;
            let mut length = 0;
            loop {
                length = next_copy_length(length, code, out_len)?;
                if code & 4 == 0 {
                    break;
                }
                code = len.read(&mut bits)?;
            }
            length += 2;
            let mut distance = 0;
            for _ in 0..groups {
                distance = distance << 3 | dist.read(&mut bits)?;
            }
            distance += 1;
            if distance >= 512 {
                length += 1;
            }
            if buf.len() + length > PRELOAD + out_len {
                return None;
            }
            let from = buf.len().checked_sub(distance + length - 1)?;
            for at in from..from + length {
                buf.push(*buf.get(at)?);
            }
        }
    }
    let raw = &buf[PRELOAD..];
    if !run_length {
        return Some(raw.to_vec());
    }
    let (&escape, mut rest) = raw.split_first()?;
    let mut out = Vec::new();
    while let Some((&b, tail)) = rest.split_first() {
        rest = tail;
        if b != escape {
            out.push(b);
            continue;
        }
        let (&count, tail) = rest.split_first()?;
        rest = tail;
        if count == 0 {
            out.push(escape);
        } else {
            let (&v, tail) = rest.split_first()?;
            rest = tail;
            out.resize(out.len() + count as usize, v);
        }
        if out.len() > MAX_OUT {
            return None;
        }
    }
    Some(out)
}

fn next_copy_length(length: usize, code: usize, out_len: usize) -> Option<usize> {
    let length = length.checked_mul(4)?.checked_add(code & 3)?;
    (length <= out_len).then_some(length)
}

/// One row of the table that turns a glyph point's flag byte into a coordinate step.
struct Triplet {
    bytes: usize,
    x_bits: u32,
    y_bits: u32,
    dx: i32,
    dy: i32,
    x_neg: bool,
    y_neg: bool,
}

/// The coordinate step a flag byte's low seven bits stand for.
fn triplet(index: u8) -> Triplet {
    let i = index as i32;
    let sign = |n: i32| (n & 1 == 0, n & 2 == 0);
    let (bytes, x_bits, y_bits, dx, dy, (x_neg, y_neg)) = match i {
        0..=9 => (2, 0, 8, 0, 256 * (i / 2), (false, i % 2 == 0)),
        10..=19 => (2, 8, 0, 256 * ((i - 10) / 2), 0, ((i - 10) % 2 == 0, false)),
        20..=83 => {
            let n = i - 20;
            (2, 4, 4, 1 + 16 * (n / 16), 1 + 16 * (n / 4 % 4), sign(n))
        }
        84..=119 => {
            let n = i - 84;
            (3, 8, 8, 1 + 256 * (n / 12), 1 + 256 * (n / 4 % 3), sign(n))
        }
        120..=123 => (4, 12, 12, 0, 0, sign(i)),
        _ => (5, 16, 16, 0, 0, sign(i)),
    };
    Triplet { bytes, x_bits, y_bits, dx, dy, x_neg, y_neg }
}

/// Pushes `values` the way a glyph program starts: `NPUSHB` or `NPUSHW`, 255 values at a time.
fn push_instructions(values: &[i16], out: &mut Vec<u8>) {
    for chunk in values.chunks(255) {
        if chunk.iter().all(|v| (0..=255).contains(v)) {
            out.extend([0x40, chunk.len() as u8]);
            out.extend(chunk.iter().map(|&v| v as u8));
        } else {
            out.extend([0x41, chunk.len() as u8]);
            out.extend(chunk.iter().flat_map(|v| v.to_be_bytes()));
        }
    }
}

/// The values a glyph's initial pushes set up, with `A,x,A,x,A` runs stored as hop codes.
fn push_data(c: &mut Cur, count: usize) -> Option<Vec<i16>> {
    let mut v: Vec<i16> = Vec::with_capacity(count);
    while v.len() < count {
        let hops = match c.d.get(c.p)? {
            251 => 1,
            252 => 2,
            _ => 0,
        };
        if hops == 0 {
            v.push(c.short255()?);
            continue;
        }
        c.p += 1;
        let a = *v.get(v.len().checked_sub(2)?)?;
        for _ in 0..hops {
            v.extend([a, c.short255()?]);
        }
        v.push(a);
    }
    (v.len() == count).then_some(v)
}

/// A glyph's program: the pushes, then `code_size` bytes of instructions.
fn program(glyf: &mut Cur, push: &mut Cur, code: &mut Cur) -> Option<Vec<u8>> {
    let (push_count, code_size) = (glyf.ushort255()?, glyf.ushort255()?);
    let mut out = Vec::new();
    push_instructions(&push_data(push, push_count as usize)?, &mut out);
    out.extend(code.take(code_size as usize)?);
    Some(out)
}

/// One glyph in TrueType form. Simple glyphs are written with 16-bit coordinate steps.
fn glyph(glyf: &mut Cur, push: &mut Cur, code: &mut Cur) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut n = glyf.i16()?;
    if n == 0 {
        return Some(out);
    }
    if n == -1 {
        out.extend(n.to_be_bytes());
        out.extend(glyf.take(8)?);
        loop {
            let flags = glyf.u16()?;
            out.extend(flags.to_be_bytes());
            let args = if flags & 1 != 0 { 4 } else { 2 };
            let scale = if flags & 0x08 != 0 {
                2
            } else if flags & 0x40 != 0 {
                4
            } else if flags & 0x80 != 0 {
                8
            } else {
                0
            };
            out.extend(glyf.take(2 + args + scale)?);
            if flags & 0x20 == 0 {
                if flags & 0x100 != 0 {
                    let p = program(glyf, push, code)?;
                    out.extend((p.len() as u16).to_be_bytes());
                    out.extend(p);
                }
                return Some(out);
            }
        }
    }
    let mut bbox = None;
    if n == 0x7FFF {
        n = glyf.i16()?;
        bbox = Some(glyf.take(8)?);
    }
    if n < 0 {
        return None;
    }
    let mut ends: Vec<u16> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let v = glyf.ushort255()?;
        ends.push(ends.last().map_or(Some(v), |e| e.checked_add(v))?);
    }
    let points = *ends.last()? as usize + 1;
    let flags = glyf.take(points)?;
    let (mut x, mut y) = (0i32, 0i32);
    let mut steps = Vec::with_capacity(points);
    let mut min = (i32::MAX, i32::MAX);
    let mut max = (i32::MIN, i32::MIN);
    for &f in flags {
        let t = triplet(f & 0x7F);
        let mut data = 0u32;
        for &b in glyf.take(t.bytes - 1)? {
            data = data << 8 | b as u32;
        }
        let shift = (t.bytes as u32 - 1) * 8;
        let step = |bits: u32, at: u32, delta: i32, neg: bool| {
            let v = (data >> (shift - at - bits)) as i32 & ((1 << bits) - 1);
            if neg { -(v + delta) } else { v + delta }
        };
        let (dx, dy) = (step(t.x_bits, 0, t.dx, t.x_neg), step(t.y_bits, t.x_bits, t.dy, t.y_neg));
        x += dx;
        y += dy;
        (min, max) = ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y)));
        steps.push((dx as i16, dy as i16));
    }
    let program = program(glyf, push, code)?;
    out.extend(n.to_be_bytes());
    match bbox {
        Some(b) => out.extend(b),
        None => out.extend([min.0, min.1, max.0, max.1].iter().flat_map(|&v| (v as i16).to_be_bytes())),
    }
    out.extend(ends.iter().flat_map(|e| e.to_be_bytes()));
    out.extend((program.len() as u16).to_be_bytes());
    out.extend(program);
    out.extend(flags.iter().map(|&f| (f >> 7) ^ 1));
    out.extend(steps.iter().flat_map(|s| s.0.to_be_bytes()));
    out.extend(steps.iter().flat_map(|s| s.1.to_be_bytes()));
    Some(out)
}

/// The `cvt ` table: a count, then each value as the step from the one before.
fn cvt(d: &[u8]) -> Option<Vec<u8>> {
    let mut c = Cur::new(d);
    let mut v = 0i16;
    let mut out = Vec::new();
    for _ in 0..c.u16()? {
        let code = c.u8()?;
        let step = match code {
            0..=237 => code as i16,
            239..=247 => -(c.u8()? as i16 + (code as i16 - 239) * 238),
            248..=255 => c.u8()? as i16 + (code as i16 - 247) * 238,
            238 => c.i16()?,
        };
        v = v.wrapping_add(step);
        out.extend(v.to_be_bytes());
    }
    Some(out)
}

/// The sum of the table's big-endian 32-bit words, as the table directory holds it.
fn checksum(d: &[u8]) -> u32 {
    d.chunks(4).fold(0u32, |s, c| {
        let mut w = [0; 4];
        w[..c.len()].copy_from_slice(c);
        s.wrapping_add(u32::from_be_bytes(w))
    })
}

fn checked_table_size(current: usize, len: usize) -> Option<usize> {
    current.checked_add(len.checked_add(3)? & !3)
}

/// The TrueType font from the three decompressed streams: the tables (CTF), the glyphs' push data
/// and their instructions.
fn rebuild(tables: &[u8], push: &[u8], code: &[u8]) -> Option<Vec<u8>> {
    let mut c = Cur::new(tables);
    let version = c.take(4)?;
    let count = c.u16()? as usize;
    c.take(6)?;
    let mut dir = Vec::with_capacity(count);
    for _ in 0..count {
        let tag: [u8; 4] = c.take(4)?.try_into().ok()?;
        c.take(4)?;
        let (off, len) = (u32::from_be_bytes(c.take(4)?.try_into().ok()?) as usize, u32::from_be_bytes(c.take(4)?.try_into().ok()?) as usize);
        dir.push((tag, tables.get(off..off.checked_add(len)?)?));
    }
    let table = |t: &[u8; 4]| dir.iter().find(|d| &d.0 == t).map(|d| d.1);
    let num_glyphs = u16::from_be_bytes(table(b"maxp")?.get(4..6)?.try_into().ok()?) as usize;

    let (mut g, mut p, mut k) = (Cur::new(table(b"glyf")?), Cur::new(push), Cur::new(code));
    let (mut glyf, mut loca) = (Vec::new(), Vec::with_capacity(4 * (num_glyphs + 1)));
    for _ in 0..num_glyphs {
        loca.extend((glyf.len() as u32).to_be_bytes());
        glyf.extend(glyph(&mut g, &mut p, &mut k)?);
        glyf.resize(glyf.len().next_multiple_of(4), 0);
        if glyf.len() > MAX_OUT {
            return None;
        }
    }
    loca.extend((glyf.len() as u32).to_be_bytes());

    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(count);
    // Directory entries may share bytes, so the font is bounded as a whole, not table by table.
    let mut size = 12 + 16 * count;
    let cvt_table = table(b"cvt ").map(cvt).unwrap_or_else(|| Some(vec![]))?;
    for (tag, data) in &dir {
        let len = match tag {
            b"loca" => loca.len(),
            b"glyf" => glyf.len(),
            b"cvt " => cvt_table.len(),
            _ => data.len(),
        };
        size = checked_table_size(size, len)?;
        if size > MAX_OUT {
            return None;
        }
        let data = match tag {
            b"loca" => loca.clone(),
            b"glyf" => glyf.clone(),
            b"cvt " => cvt_table.clone(),
            // shortcut: the device-metric tables are dropped, nothing here reads them.
            b"hdmx" | b"VDMX" => continue,
            b"head" => {
                let mut h = data.to_vec();
                // The offsets above are 32-bit.
                h.get_mut(50..52)?.copy_from_slice(&1u16.to_be_bytes());
                h
            }
            _ => data.to_vec(),
        };
        out_tables.push((*tag, data));
    }
    let n = out_tables.len();
    let entry_selector = n.max(1).ilog2() as usize;
    let search_range = 16 << entry_selector;
    let mut out = Vec::new();
    out.extend(version);
    out.extend((n as u16).to_be_bytes());
    out.extend((search_range as u16).to_be_bytes());
    out.extend((entry_selector as u16).to_be_bytes());
    out.extend(((16 * n).saturating_sub(search_range) as u16).to_be_bytes());
    let mut at = 12 + 16 * n;
    for (tag, data) in &out_tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend((at as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        at += data.len().next_multiple_of(4);
    }
    for (_, data) in &out_tables {
        out.extend(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    use skrifa::outline::{DrawSettings, OutlinePen};
    use skrifa::raw::TableProvider;

    #[test]
    fn rebuilt_table_size_checks_padding_and_addition() {
        assert_eq!(checked_table_size(12, 5), Some(20));
        assert_eq!(checked_table_size(usize::MAX - 3, 4), None);
        assert_eq!(checked_table_size(12, usize::MAX), None);
    }

    // ---- an MTX writer, enough to make test fonts from a TrueType one ----

    struct BitOut {
        bytes: Vec<u8>,
        n: usize,
    }

    impl BitOut {
        /// Appends one bit.
        fn bit(&mut self, b: usize) {
            if self.n.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let last = self.bytes.len() - 1;
            self.bytes[last] |= (b as u8) << (7 - self.n % 8);
            self.n += 1;
        }
        /// Appends the low `bits` bits of `v`.
        fn value(&mut self, v: usize, bits: u32) {
            (0..bits).rev().for_each(|i| self.bit(v >> i & 1));
        }
    }

    impl Ahuff {
        /// Appends the code for `sym` and counts it.
        fn write(&mut self, sym: usize, out: &mut BitOut) {
            let leaf = self.leaf[sym];
            let (mut a, mut path) = (leaf, vec![]);
            while a != 1 {
                let up = self.tree[a].up;
                path.push((self.tree[up].right == a) as usize);
                a = up;
            }
            path.iter().rev().for_each(|&b| out.bit(b));
            self.update(leaf);
        }
    }

    #[test]
    fn copy_length_rejects_overflow_before_accumulating() {
        assert_eq!(next_copy_length(10, 1, 41), Some(41));
        assert_eq!(next_copy_length(10, 1, 40), None);
        assert_eq!(next_copy_length(usize::MAX / 4 + 1, 0, usize::MAX), None);
        assert_eq!(next_copy_length(usize::MAX / 4, 3, usize::MAX), Some(usize::MAX));
    }

    /// The window an LZCOMP stream starts out holding.
    fn preload() -> Vec<u8> {
        let mut w = Vec::new();
        for k in 0..32u8 {
            for j in 0..96u8 {
                w.extend([k, j]);
            }
        }
        for j in 0..=255u8 {
            w.extend([j; 4]);
        }
        w
    }

    /// LZCOMP with literals, DUP2 and copies found by a search of the last kilobyte, run-length
    /// coded when `escape` is given.
    fn pack(data: &[u8], escape: Option<u8>) -> Vec<u8> {
        let data = match escape {
            None => data.to_vec(),
            Some(e) => {
                let mut v = vec![e];
                let mut i = 0;
                while i < data.len() {
                    let run = data[i..].iter().take(255).take_while(|&&b| b == data[i]).count();
                    if run > 3 {
                        v.extend([e, run as u8, data[i]]);
                        i += run;
                    } else {
                        v.extend(if data[i] == e { vec![e, 0] } else { vec![data[i]] });
                        i += 1;
                    }
                }
                v
            }
        };
        let mut out = BitOut { bytes: vec![], n: 0 };
        out.bit(escape.is_some() as usize);
        let (mut dist, mut len) = (Ahuff::new(8), Ahuff::new(8));
        out.value(data.len(), 24);
        let mut ranges = 1;
        while 1usize << (3 * ranges) < data.len() {
            ranges += 1;
        }
        let dup2 = 256 + 8 * ranges;
        let mut sym = Ahuff::new(dup2 + 3);
        let mut hist = preload();
        let mut i = 0;
        while i < data.len() {
            let at = hist.len();
            let mut best = (0, 0);
            for s in at.saturating_sub(1200)..at {
                let l = (0..(at - s).min(data.len() - i)).take_while(|&k| hist[s + k] == data[i + k]).count();
                if l > best.0 {
                    best = (l, s);
                }
            }
            let (l, s) = best;
            let distance = (at - s + 1).saturating_sub(l);
            let v = l.saturating_sub(if distance >= 512 { 3 } else { 2 });
            let n = (1..=ranges).find(|n| distance.saturating_sub(1) >> (3 * n) == 0);
            if let (true, Some(n)) = (l >= 3, n) {
                let mut groups = vec![v & 3];
                let mut r = v >> 2;
                while r > 0 {
                    groups.push(r & 3);
                    r >>= 2;
                }
                groups.reverse();
                let code = |k: usize| groups[k] | if k + 1 < groups.len() { 4 } else { 0 };
                sym.write(256 + (n - 1) * 8 + code(0), &mut out);
                for k in 1..groups.len() {
                    len.write(code(k), &mut out);
                }
                for g in (0..n).rev() {
                    dist.write((distance - 1) >> (3 * g) & 7, &mut out);
                }
                hist.extend(&data[i..i + l]);
                i += l;
                continue;
            }
            if hist.len() >= 2 && data[i] == hist[hist.len() - 2] {
                sym.write(dup2, &mut out);
            } else {
                sym.write(data[i] as usize, &mut out);
            }
            hist.push(data[i]);
            i += 1;
        }
        out.bytes
    }

    // ---- TrueType to CTF ----

    /// A big-endian 16-bit value at `at`.
    fn be16(d: &[u8], at: usize) -> usize {
        u16::from_be_bytes([d[at], d[at + 1]]) as usize
    }
    /// A big-endian 32-bit value at `at`.
    fn be32(d: &[u8], at: usize) -> usize {
        u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize
    }

    /// The font's tables by tag.
    fn tables(font: &[u8]) -> Vec<([u8; 4], &[u8])> {
        (0..be16(font, 4))
            .map(|i| {
                let e = 12 + 16 * i;
                (font[e..e + 4].try_into().unwrap(), &font[be32(font, e + 8)..be32(font, e + 8) + be32(font, e + 12)])
            })
            .collect()
    }

    /// Appends `v` as a `255USHORT`.
    fn put255ushort(out: &mut Vec<u8>, v: usize) {
        match v {
            0..=252 => out.push(v as u8),
            253..=505 => out.extend([255, (v - 253) as u8]),
            506..=761 => out.extend([254, (v - 506) as u8]),
            _ => out.extend([253, (v >> 8) as u8, v as u8]),
        }
    }

    /// Appends `v` as a `255SHORT`.
    fn put255short(out: &mut Vec<u8>, v: i16) {
        let m = v.unsigned_abs() as usize;
        if m >= 756 {
            out.extend([253, (v >> 8) as u8, v as u8]);
            return;
        }
        if v < 0 {
            out.push(250);
        }
        match m {
            0..=249 => out.push(m as u8),
            250..=499 => out.extend([255, (m - 250) as u8]),
            _ => out.extend([254, (m - 500) as u8]),
        }
    }

    /// The values the initial pushes of `program` leave on the stack, and what follows them.
    fn split_pushes(program: &[u8]) -> (Vec<i16>, &[u8]) {
        let (mut values, mut i) = (vec![], 0);
        while let Some(&op) = program.get(i) {
            let (n, words) = match op {
                0x40 | 0x41 => (program[i + 1] as usize, op == 0x41),
                0xB0..=0xB7 => ((op - 0xB0) as usize + 1, false),
                0xB8..=0xBF => ((op - 0xB8) as usize + 1, true),
                _ => break,
            };
            i += if op < 0x42 { 2 } else { 1 };
            for _ in 0..n {
                if words {
                    values.push(i16::from_be_bytes([program[i], program[i + 1]]));
                    i += 2;
                } else {
                    values.push(program[i] as i16);
                    i += 1;
                }
            }
        }
        (values, &program[i..])
    }

    /// Writes a glyph's `pushCount` and `codeSize` to `glyf`, its pushes (with hop codes where
    /// they fit) to `push` and its remaining instructions to `code`.
    fn put_program(program: &[u8], glyf: &mut Vec<u8>, push: &mut Vec<u8>, code: &mut Vec<u8>) {
        let (v, rest) = split_pushes(program);
        put255ushort(glyf, v.len());
        put255ushort(glyf, rest.len());
        let mut i = 0;
        while i < v.len() {
            let a = if i >= 2 { v[i - 2] } else { i16::MIN };
            if i >= 2 && i + 4 < v.len() && v[i] == a && v[i + 2] == a && v[i + 4] == a {
                push.push(252);
                put255short(push, v[i + 1]);
                put255short(push, v[i + 3]);
                i += 5;
            } else if i >= 2 && i + 2 < v.len() && v[i] == a && v[i + 2] == a {
                push.push(251);
                put255short(push, v[i + 1]);
                i += 3;
            } else {
                put255short(push, v[i]);
                i += 1;
            }
        }
        code.extend(rest);
    }

    /// The step from the previous point as the flag byte and the bytes that follow it.
    fn put_triplet(flag: &mut Vec<u8>, data: &mut Vec<u8>, dx: i32, dy: i32, on: bool) {
        let best = (0..128u8)
            .filter_map(|i| {
                let t = triplet(i);
                let fits = |d: i32, bits: u32, base: i32, neg: bool| {
                    if bits == 0 {
                        return d == 0;
                    }
                    let m = d.abs() - base;
                    (d == 0 || (d < 0) == neg) && (0..1 << bits).contains(&m)
                };
                (fits(dx, t.x_bits, t.dx, t.x_neg) && fits(dy, t.y_bits, t.dy, t.y_neg)).then_some((t.bytes, i))
            })
            .min()
            .unwrap();
        let t = triplet(best.1);
        let shift = (t.bytes as u32 - 1) * 8;
        let mut v = 0u32;
        if t.x_bits > 0 {
            v |= ((dx.abs() - t.dx) as u32) << (shift - t.x_bits);
        }
        if t.y_bits > 0 {
            v |= ((dy.abs() - t.dy) as u32) << (shift - t.x_bits - t.y_bits);
        }
        flag.push(best.1 | if on { 0 } else { 0x80 });
        data.extend(&v.to_be_bytes()[4 - (t.bytes - 1)..]);
    }

    /// A simple glyph's points: absolute coordinates and whether each is on the curve.
    fn points(g: &[u8], n: usize) -> (Vec<usize>, Vec<(i32, i32, bool)>, &[u8]) {
        let ends: Vec<usize> = (0..n).map(|i| be16(g, 10 + 2 * i)).collect();
        let count = ends.last().map_or(0, |e| e + 1);
        let ilen = be16(g, 10 + 2 * n);
        let instr = &g[12 + 2 * n..12 + 2 * n + ilen];
        let mut at = 12 + 2 * n + ilen;
        let mut flags = vec![];
        while flags.len() < count {
            let f = g[at];
            at += 1;
            let reps = if f & 8 != 0 {
                at += 1;
                g[at - 1] as usize
            } else {
                0
            };
            flags.extend(std::iter::repeat_n(f, 1 + reps));
        }
        let mut coord = |short: u8, same: u8| {
            let mut v = 0i32;
            flags
                .iter()
                .map(|&f| {
                    if f & short != 0 {
                        let d = g[at] as i32;
                        at += 1;
                        v += if f & same != 0 { d } else { -d };
                    } else if f & same == 0 {
                        v += i16::from_be_bytes([g[at], g[at + 1]]) as i32;
                        at += 2;
                    }
                    v
                })
                .collect::<Vec<_>>()
        };
        let xs = coord(2, 0x10);
        let ys = coord(4, 0x20);
        (ends, (0..count).map(|i| (xs[i], ys[i], flags[i] & 1 != 0)).collect(), instr)
    }

    /// The font as an MTX stream: TrueType glyphs in CTF form, `cvt ` compressed, the rest as is.
    fn to_mtx(font: &[u8], escape: Option<u8>) -> Vec<u8> {
        let t = tables(font);
        let get = |tag: &[u8; 4]| t.iter().find(|x| &x.0 == tag).unwrap().1;
        let loca_long = be16(get(b"head"), 50) == 1;
        let glyphs = be16(get(b"maxp"), 4);
        let at = |i: usize| if loca_long { be32(get(b"loca"), 4 * i) } else { 2 * be16(get(b"loca"), 2 * i) };
        let (mut glyf, mut push, mut code) = (vec![], vec![], vec![]);
        for i in 0..glyphs {
            let g = &get(b"glyf")[at(i)..at(i + 1)];
            if g.is_empty() {
                glyf.extend([0, 0]);
                continue;
            }
            let n = be16(g, 0) as i16;
            if n < 0 {
                glyf.extend(&g[..10]);
                let mut p = 10;
                loop {
                    let f = be16(g, p);
                    let len = 4
                        + if f & 1 != 0 { 4 } else { 2 }
                        + if f & 8 != 0 {
                            2
                        } else if f & 0x40 != 0 {
                            4
                        } else if f & 0x80 != 0 {
                            8
                        } else {
                            0
                        };
                    glyf.extend(&g[p..p + len]);
                    p += len;
                    if f & 0x20 == 0 {
                        if f & 0x100 != 0 {
                            let l = be16(g, p);
                            put_program(&g[p + 2..p + 2 + l], &mut glyf, &mut push, &mut code);
                        }
                        break;
                    }
                }
                continue;
            }
            let (ends, pts, instr) = points(g, n as usize);
            // Keep the box when it is not the points' own, as a font may.
            let (xs, ys): (Vec<i32>, Vec<i32>) = pts.iter().map(|p| (p.0, p.1)).unzip();
            let own = [xs.iter().min(), ys.iter().min(), xs.iter().max(), ys.iter().max()].map(|v| *v.unwrap() as i16 as usize);
            let stored = [be16(g, 2), be16(g, 4), be16(g, 6), be16(g, 8)].map(|v| v & 0xFFFF);
            if own.map(|v| v & 0xFFFF) != stored {
                glyf.extend(0x7FFFu16.to_be_bytes());
                glyf.extend(&g[..10]);
            } else {
                glyf.extend(&g[..2]);
            }
            for (k, e) in ends.iter().enumerate() {
                put255ushort(&mut glyf, if k == 0 { *e } else { e - ends[k - 1] });
            }
            let (mut flags, mut data) = (vec![], vec![]);
            let (mut px, mut py) = (0, 0);
            for &(x, y, on) in &pts {
                put_triplet(&mut flags, &mut data, x - px, y - py, on);
                (px, py) = (x, y);
            }
            glyf.extend(flags);
            glyf.extend(data);
            put_program(instr, &mut glyf, &mut push, &mut code);
        }
        // cvt: each value as a step from the last.
        let mut cvt = vec![];
        let raw = get(b"cvt ");
        cvt.extend(((raw.len() / 2) as u16).to_be_bytes());
        let mut last = 0i16;
        for c in raw.chunks(2) {
            let v = i16::from_be_bytes([c[0], c[1]]);
            let d = v.wrapping_sub(last);
            last = v;
            let m = d.unsigned_abs() as usize;
            if d == i16::MIN || m / 238 > 8 {
                cvt.extend([238, (d >> 8) as u8, d as u8]);
            } else if d < 0 {
                cvt.extend([239 + (m / 238) as u8, (m % 238) as u8]);
            } else if m >= 238 {
                cvt.extend([247 + (m / 238) as u8, (m % 238) as u8]);
            } else {
                cvt.push(m as u8);
            }
        }
        let kept: Vec<_> = t.iter().filter(|x| !matches!(&x.0, b"hdmx" | b"VDMX")).collect();
        let mut ctf = vec![];
        ctf.extend(&font[..4]);
        ctf.extend((kept.len() as u16).to_be_bytes());
        ctf.extend([0; 6]);
        let mut off = 12 + 16 * kept.len();
        let mut body: Vec<u8> = vec![];
        for (tag, data) in &kept {
            let data: &[u8] = match tag {
                b"glyf" => &glyf,
                b"cvt " => &cvt,
                b"loca" => &[],
                _ => data,
            };
            ctf.extend(tag);
            ctf.extend([0; 4]);
            ctf.extend(((if data.is_empty() { 0 } else { off }) as u32).to_be_bytes());
            ctf.extend((data.len() as u32).to_be_bytes());
            body.extend(data);
            off += data.len();
        }
        ctf.extend(body);
        let (a, b, c) = (pack(&ctf, escape), pack(&push, escape), pack(&code, escape));
        let mut out = vec![3, 0, 0, 0];
        out.extend(&((10 + a.len()) as u32).to_be_bytes()[1..]);
        out.extend(&((10 + a.len() + b.len()) as u32).to_be_bytes()[1..]);
        out.extend([a, b, c].concat());
        out
    }

    /// An Embedded OpenType header around `font`, with the given flags.
    fn eot(font: &[u8], flags: u32) -> Vec<u8> {
        let mut h = vec![0u8; 82];
        h[0..4].copy_from_slice(&((82 + font.len()) as u32).to_le_bytes());
        h[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        h[8..12].copy_from_slice(&0x0002_0002u32.to_le_bytes());
        h[12..16].copy_from_slice(&flags.to_le_bytes());
        h[34..36].copy_from_slice(&0x504Cu16.to_le_bytes());
        [h, font.to_vec()].concat()
    }

    #[derive(Default)]
    struct Path(String);

    impl OutlinePen for Path {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0 += &format!("M{x},{y}");
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0 += &format!("L{x},{y}");
        }
        fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
            self.0 += &format!("Q{a},{b},{x},{y}");
        }
        fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
            self.0 += &format!("C{a},{b},{c},{d},{x},{y}");
        }
        fn close(&mut self) {
            self.0 += "Z";
        }
    }

    /// Every glyph's outline as a string, in glyph order.
    fn outlines(font: &[u8]) -> Vec<String> {
        let f = skrifa::FontRef::new(font).unwrap();
        let glyphs = f.outline_glyphs();
        (0..f.maxp().unwrap().num_glyphs())
            .map(|g| {
                let mut p = Path::default();
                if let Some(o) = glyphs.get(skrifa::GlyphId::new(g as u32)) {
                    o.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut p).unwrap();
                }
                p.0
            })
            .collect()
    }

    /// Each glyph's program as its pushed values and the instructions after them.
    fn programs(font: &[u8]) -> Vec<(Vec<i16>, Vec<u8>)> {
        let t = tables(font);
        let get = |tag: &[u8; 4]| t.iter().find(|x| &x.0 == tag).unwrap().1;
        let long = be16(get(b"head"), 50) == 1;
        let at = |i: usize| if long { be32(get(b"loca"), 4 * i) } else { 2 * be16(get(b"loca"), 2 * i) };
        (0..be16(get(b"maxp"), 4))
            .filter_map(|i| {
                let g = &get(b"glyf")[at(i)..at(i + 1)];
                (!g.is_empty() && (be16(g, 0) as i16) > 0).then(|| {
                    let (_, _, instr) = points(g, be16(g, 0));
                    let (v, rest) = split_pushes(instr);
                    (v, rest.to_vec())
                })
            })
            .collect()
    }

    const FONT: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;

    /// A font through MTX and back keeps its outlines and glyph programs.
    #[test]
    fn a_compressed_embedded_font_decodes_to_the_same_glyphs() {
        let original = outlines(FONT);
        assert!(original.iter().filter(|o| !o.is_empty()).count() > 100);
        for (escape, flags) in [(None, 0x4), (Some(0), 0x4), (Some(0x41), 0x1000_0004)] {
            let mtx = to_mtx(FONT, escape);
            let part = eot(&if flags & 0x1000_0000 != 0 { mtx.iter().map(|b| b ^ 0x50).collect() } else { mtx }, flags);
            let font = crate::embedded_sfnt(&part).expect("decoded");
            assert!(outlines(&font) == original, "outlines differ (run-length escape {escape:?})");
            let (a, b) = (programs(&font), programs(FONT));
            if let Some(i) = (0..a.len()).find(|&i| a[i] != b[i]) {
                panic!("glyph program {i} differs: {:?} vs {:?}", a[i], b[i]);
            }
        }
    }

    /// Tables other than the translated ones come through unchanged.
    #[test]
    fn the_other_tables_survive() {
        let font = decompress(&to_mtx(FONT, None)).unwrap();
        let (a, b) = (tables(FONT), tables(&font));
        for (tag, data) in &a {
            let kept = b.iter().find(|x| &x.0 == tag);
            match tag {
                b"loca" | b"glyf" | b"head" => assert!(kept.is_some()),
                b"hdmx" | b"VDMX" => assert!(kept.is_none()),
                _ => assert!(kept.is_some_and(|k| k.1 == *data), "{}", String::from_utf8_lossy(tag)),
            }
        }
    }

    /// Hop codes repeat the value two back.
    #[test]
    fn push_data_expands_hop_codes() {
        // A,x1,A,x2,A and A,x1,A,x2,A,x3,A, after a lone 7.
        let mut c = Cur::new(&[7, 9, 251, 3, 8, 252, 3, 4, 250, 5]);
        assert_eq!(push_data(&mut c, 12).unwrap(), [7, 9, 7, 3, 7, 8, 7, 3, 7, 4, 7, -5]);
        assert!(Cur::new(&[251]).short255().is_some());
        // A hop code with nothing before it to repeat is invalid.
        assert!(push_data(&mut Cur::new(&[251, 1]), 3).is_none());
    }

    /// `255SHORT` and `255USHORT` read every form they are written in.
    #[test]
    fn short_numbers_use_every_form() {
        let mut out = vec![];
        let shorts = [0, 249, -249, 250, 499, 500, 755, -250, -755, 756, -756, 32767, -32768];
        shorts.iter().for_each(|&v| put255short(&mut out, v));
        let mut c = Cur::new(&out);
        assert!(shorts.iter().all(|&v| c.short255() == Some(v)));
        let mut out = vec![];
        let longs = [0, 252, 253, 505, 506, 761, 762, 65535];
        longs.iter().for_each(|&v| put255ushort(&mut out, v));
        let mut c = Cur::new(&out);
        assert!(longs.iter().all(|&v| c.ushort255() == Some(v as u16)));
    }

    /// `cvt` steps add up to the table's values.
    #[test]
    fn cvt_values_decode_as_running_sums() {
        // 5; then +1000 (two bytes); then -3000 (a word); then -238 (two bytes).
        let enc = [0, 4, 5, 251, 48, 238, 0xF4, 0x48, 240, 0];
        let v: Vec<i16> = cvt(&enc).unwrap().chunks(2).map(|c| i16::from_be_bytes([c[0], c[1]])).collect();
        assert_eq!(v, [5, 1005, -1995, -2233]);
    }

    /// Cut-off and corrupted data is refused.
    #[test]
    fn broken_input_is_refused_without_panicking() {
        let mtx = to_mtx(FONT, None);
        let part = eot(&mtx, 0x4);
        for len in (0..part.len()).step_by(997) {
            assert!(crate::embedded_sfnt(&part[..len]).is_none());
        }
        let mut bad = mtx.clone();
        for i in (10..bad.len()).step_by(211) {
            bad[i] ^= 0x5A;
            let _ = decompress(&bad);
        }
        assert!(decompress(&[]).is_none());
        assert!(decompress(&[9; 40]).is_none());
        assert!(decompress(&[3; 10]).is_none());
    }

    /// An MTX font whose directory lists one `len`-byte range `copies` times.
    fn shared_ranges(copies: usize, len: usize) -> Vec<u8> {
        let mut entries: Vec<([u8; 4], usize)> = vec![(*b"glyf", 0), (*b"head", 54), (*b"loca", 0), (*b"maxp", 32)];
        entries.extend((0..copies).map(|_| (*b"zz\0\0", len)));
        let mut ctf = vec![0, 1, 0, 0];
        ctf.extend((entries.len() as u16).to_be_bytes());
        ctf.extend([0; 6]);
        let body = 12 + 16 * entries.len();
        for (tag, l) in &entries {
            ctf.extend(tag);
            ctf.extend([0; 4]);
            ctf.extend(((if *tag == *b"zz\0\0" { body + 100 } else { body }) as u32).to_be_bytes());
            ctf.extend((*l as u32).to_be_bytes());
        }
        ctf.resize(body + 100 + len, 0);
        let (a, b) = (pack(&ctf, Some(7)), pack(&[], None));
        let mut out = vec![3, 0, 0, 0];
        out.extend(&((10 + a.len()) as u32).to_be_bytes()[1..]);
        out.extend(&((10 + a.len() + b.len()) as u32).to_be_bytes()[1..]);
        out.extend([a, b.clone(), b].concat());
        out
    }

    /// A `cvt ` table of 65535 one-byte values, which decodes to twice that.
    fn cvt_directory(copies: usize) -> Vec<u8> {
        let mut cvt = 65535u16.to_be_bytes().to_vec();
        cvt.resize(2 + 65535, 0);
        let mut entries = vec![(*b"glyf", 0, 0), (*b"head", 0, 54), (*b"loca", 0, 0), (*b"maxp", 0, 32)];
        entries.extend((0..copies).map(|_| (*b"cvt ", 1, cvt.len())));
        let mut ctf = vec![0, 1, 0, 0];
        ctf.extend((entries.len() as u16).to_be_bytes());
        ctf.extend([0; 6]);
        let body = 12 + 16 * entries.len();
        for (tag, at, l) in &entries {
            ctf.extend(tag);
            ctf.extend([0; 4]);
            ctf.extend(((body + at * 100) as u32).to_be_bytes());
            ctf.extend((*l as u32).to_be_bytes());
        }
        ctf.resize(body + 100, 0);
        ctf.extend(&cvt);
        let (a, b) = (pack(&ctf, Some(7)), pack(&[], None));
        let mut out = vec![3, 0, 0, 0];
        out.extend(&((10 + a.len()) as u32).to_be_bytes()[1..]);
        out.extend(&((10 + a.len() + b.len()) as u32).to_be_bytes()[1..]);
        out.extend([a, b.clone(), b].concat());
        out
    }

    /// Directory entries that share one range cannot add up to more than the size limit.
    #[test]
    fn the_rebuilt_font_is_bounded_as_a_whole() {
        assert!(decompress(&shared_ranges(3, 1 << 20)).is_some());
        assert!(decompress(&shared_ranges(70, 1 << 20)).is_none());
        // A table that grows when decoded is charged at its decoded size.
        assert!(decompress(&cvt_directory(3)).is_some());
        assert!(decompress(&cvt_directory(1000)).is_none());
    }
}
