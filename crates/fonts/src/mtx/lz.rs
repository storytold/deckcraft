//! LZCOMP and its three adaptive prefix-code alphabets (W3C MTX §3 and Appendix C).
//! Implemented from the format description; no external decoder is linked.
use super::{MAX_FONT, Result, binary::Bits};

#[derive(Clone, Copy, Default)]
struct Node {
    parent: usize,
    children: Option<[usize; 2]>,
    symbol: usize,
    weight: u32,
}
struct Alphabet {
    nodes: Vec<Node>,
    leaves: Vec<usize>,
}
impl Alphabet {
    fn new(count: usize) -> Result<Self> {
        if !(2..512).contains(&count) {
            return Err("invalid MTX alphabet");
        }
        let mut nodes = vec![Node::default(); count * 2];
        let mut leaves = vec![0; count];
        for (i, n) in nodes.iter_mut().enumerate().skip(1) {
            n.parent = i / 2;
            n.weight = 1;
            if i < count {
                n.children = Some([i * 2, i * 2 + 1]);
            } else {
                n.symbol = i - count;
                leaves[i - count] = i;
            }
        }
        for i in (1..count).rev() {
            nodes[i].weight = nodes[i * 2].weight + nodes[i * 2 + 1].weight;
        }
        let mut a = Self { nodes, leaves };
        if count > 256 {
            a.bump(a.leaves[256]);
            a.bump(a.leaves[257]);
            for _ in 0..12 {
                a.bump(a.leaves[count - 3]);
            }
            for _ in 0..6 {
                a.bump(a.leaves[count - 2]);
            }
        } else {
            for _ in 0..2 {
                for symbol in 0..count {
                    a.bump(a.leaves[symbol]);
                }
            }
        }
        Ok(a)
    }
    fn bump(&mut self, mut i: usize) {
        // Only internally constructed indices enter this tree. The root is never swapped.
        while i > 1 {
            let w = self.nodes[i].weight;
            let mut peer = i;
            while peer > 1 && self.nodes[peer - 1].weight == w {
                peer -= 1;
            }
            if peer > 1 && peer != i {
                let (ip, pp) = (self.nodes[i].parent, self.nodes[peer].parent);
                self.nodes.swap(i, peer);
                self.nodes[i].parent = ip;
                self.nodes[peer].parent = pp;
                for n in [i, peer] {
                    if let Some(children) = self.nodes[n].children {
                        for child in children {
                            self.nodes[child].parent = n;
                        }
                    } else {
                        self.leaves[self.nodes[n].symbol] = n;
                    }
                }
                i = peer;
            }
            self.nodes[i].weight += 1;
            i = self.nodes[i].parent;
        }
        self.nodes[1].weight += 1;
    }
    fn read(&mut self, bits: &mut Bits<'_>) -> Result<usize> {
        let mut i = 1;
        while let Some(children) = self.nodes[i].children {
            i = children[bits.bit()?];
        }
        let value = self.nodes[i].symbol;
        self.bump(i);
        Ok(value)
    }
}

fn preload() -> Vec<u8> {
    let mut h = Vec::with_capacity(7168);
    for a in 0..32 {
        for b in 0..96 {
            h.extend([a, b]);
        }
    }
    for b in 0..=255 {
        h.extend([b; 4]);
    }
    h
}

pub(super) fn decode(data: &[u8], version: u8, limit: usize) -> Result<Vec<u8>> {
    let mut bits = Bits::new(data, false);
    let rle = version != 1 && bits.bit()? != 0;
    let len = bits.read(24)?;
    if len > MAX_FONT {
        return Err("MTX expansion budget exceeded");
    }
    let mut ranges = 1;
    while (1usize << (ranges * 3)) < len {
        ranges += 1;
    }
    let dup = 256 + ranges * 8;
    let mut symbols = Alphabet::new(dup + 3)?;
    let mut lengths = Alphabet::new(8)?;
    let mut distances = Alphabet::new(8)?;
    let mut history = preload();
    let base = history.len();
    history.try_reserve(len).map_err(|_| "MTX allocation failed")?;
    while history.len() - base < len {
        let symbol = symbols.read(&mut bits)?;
        if symbol < 256 {
            history.push(symbol as u8);
        } else if symbol >= dup {
            let offset = 2 * (symbol - dup + 1);
            let from = history.len().checked_sub(offset).ok_or("invalid MTX duplicate")?;
            let b = *history.get(from).ok_or("invalid MTX duplicate")?;
            history.push(b);
        } else {
            copy(&mut history, len + base, symbol, limit, &mut bits, &mut lengths, &mut distances)?;
        }
    }
    if rle { expand_runs(history.get(base..).ok_or("short MTX history")?) } else { Ok(history.split_off(base)) }
}

fn copy(
    history: &mut Vec<u8>,
    end: usize,
    symbol: usize,
    limit: usize,
    bits: &mut Bits<'_>,
    lengths: &mut Alphabet,
    distances: &mut Alphabet,
) -> Result<()> {
    let groups = (symbol - 256) / 8 + 1;
    let mut digit = (symbol - 256) % 8;
    let mut len = digit & 3;
    let mut digits = 1;
    while digit & 4 != 0 {
        digits += 1;
        if digits > 12 {
            return Err("oversized MTX copy length");
        }
        digit = lengths.read(bits)?;
        len = len * 4 + (digit & 3);
    }
    len += 2;
    let mut distance = 0;
    for _ in 0..groups {
        distance = distance * 8 + distances.read(bits)?;
    }
    distance += 1;
    if distance >= 512 {
        len += 1;
    }
    let back = distance.checked_add(len).and_then(|n| n.checked_sub(1)).ok_or("MTX distance overflow")?;
    if back > limit || len > end.saturating_sub(history.len()) {
        return Err("invalid MTX copy range");
    }
    let from = history.len().checked_sub(back).ok_or("MTX copy precedes history")?;
    for i in 0..len {
        let byte = *history.get(from + i).ok_or("invalid MTX history reference")?;
        history.push(byte);
    }
    Ok(())
}

fn expand_runs(data: &[u8]) -> Result<Vec<u8>> {
    let Some((&escape, rest)) = data.split_first() else { return Err("missing MTX RLE escape") };
    let mut reader = super::binary::Reader::new(rest);
    let mut out = Vec::new();
    while reader.pos < rest.len() {
        let b = reader.byte()?;
        let (value, count) = if b != escape {
            (b, 1)
        } else {
            match reader.byte()? {
                0 => (escape, 1),
                count => (reader.byte()?, count as usize),
            }
        };
        if out.len() + count > MAX_FONT {
            return Err("MTX RLE expansion budget exceeded");
        }
        out.resize(out.len() + count, value);
    }
    Ok(out)
}
