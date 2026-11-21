//! Font files for embedding in a presentation (`p:embeddedFont`): the face as a standalone
//! TrueType or OpenType font, optionally cut down to the glyphs a document uses, wrapped in an
//! uncompressed Embedded OpenType header (W3C Embedded OpenType submission, version 0x00020001)
//! the way PresentationML's `.fntdata` parts carry fonts.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use skrifa::MetadataProvider;

use crate::FontFace;

/// What the font's licence lets a document carry (OpenType `OS/2.fsType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// No restriction: the font may be embedded and installed.
    Installable,
    /// Embedded for viewing and editing.
    Editable,
    /// Embedded for viewing and printing only; PowerPoint opens such a file read-only.
    PreviewPrint,
    /// The font must not be embedded.
    Restricted,
    /// Only bitmaps may be embedded, never outlines.
    BitmapOnly,
}

impl Permission {
    pub fn allows_embedding(self) -> bool {
        !matches!(self, Permission::Restricted | Permission::BitmapOnly)
    }
}

/// Why a face was not embedded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `fsType` forbids embedding (restricted licence, or bitmaps only).
    NotPermitted(Permission),
    /// The face's file could not be read as a font.
    Unreadable,
}

/// An embeddable part: the Embedded OpenType bytes for a `.fntdata` part.
#[derive(Clone, Debug)]
pub struct EmbeddedPart {
    pub data: Vec<u8>,
    /// Holds only the glyphs the document's text needs.
    pub subset: bool,
    pub permission: Permission,
    /// The face's PANOSE classification (`OS/2`), when it gives one.
    pub panose: Option<[u8; 10]>,
}

/// TTEMBED_SUBSET in the Embedded OpenType `Flags`.
const EOT_SUBSET: u32 = 0x0000_0001;
const EOT_VERSION: u32 = 0x0002_0001;
const EOT_MAGIC: u16 = 0x504C;
const MAX_TABLES: usize = 512;

/// The embedding permission of a standalone font (no `OS/2` table: installable). When several
/// usage bits are set, the least restrictive applies.
pub fn permission(sfnt: &[u8]) -> Permission {
    let Some(fs) = table(sfnt, *b"OS/2").and_then(|t| be16(t, 8)) else { return Permission::Installable };
    if fs & 0x0200 != 0 {
        return Permission::BitmapOnly;
    }
    let usage = fs & 0x000F;
    if usage & 0x0008 != 0 {
        Permission::Editable
    } else if usage & 0x0004 != 0 {
        Permission::PreviewPrint
    } else if usage & 0x0002 != 0 {
        Permission::Restricted
    } else {
        Permission::Installable
    }
}

/// `fsType` bit 8: the font must be embedded whole.
fn no_subsetting(sfnt: &[u8]) -> bool {
    table(sfnt, *b"OS/2").and_then(|t| be16(t, 8)).is_some_and(|fs| fs & 0x0100 != 0)
}

/// The Embedded OpenType part for `face`: the whole font when `text` is `None` ("all
/// characters"), else only the glyphs `text` needs ("only the characters used"). Fonts that
/// can't be cut down (CFF outlines, variable fonts, `fsType` forbids subsetting, symbol
/// encodings) are embedded whole.
pub fn embed_face(face: &FontFace, text: Option<&[String]>) -> Result<EmbeddedPart, Refusal> {
    let sfnt = face_sfnt(face.data(), face.index()).ok_or(Refusal::Unreadable)?;
    let permission = permission(&sfnt);
    if !permission.allows_embedding() {
        return Err(Refusal::NotPermitted(permission));
    }
    let cut = match text {
        Some(text) if !no_subsetting(&sfnt) => subset_for_text(face, &sfnt, text),
        _ => None,
    };
    let subset = cut.is_some();
    let font = cut.map(Cow::Owned).unwrap_or(sfnt);
    let data = eot(&font, subset).ok_or(Refusal::Unreadable)?;
    let panose = table(&font, *b"OS/2").and_then(|t| t.get(32..42)).and_then(|b| <[u8; 10]>::try_from(b).ok()).filter(|b| b.iter().any(|x| *x != 0));
    Ok(EmbeddedPart { data, subset, permission, panose })
}

fn subset_for_text(face: &FontFace, sfnt: &[u8], text: &[String]) -> Option<Vec<u8>> {
    let font = skrifa::FontRef::new(sfnt).ok()?;
    let num_glyphs = u32::from(be16(table(sfnt, *b"maxp")?, 4)?);
    let all: BTreeMap<u32, u32> = font.charmap().mappings().map(|(cp, g)| (cp, g.to_u32())).collect();
    let encoded: BTreeSet<u32> = all.values().copied().collect();
    let mut keep: BTreeSet<u32> = (0..num_glyphs).filter(|g| !encoded.contains(g)).collect();
    let mut cmap = BTreeMap::new();
    let mut add = |c: char, keep: &mut BTreeSet<u32>| {
        if let Some(g) = all.get(&u32::from(c)) {
            keep.insert(*g);
            cmap.insert(u32::from(c), *g);
        }
    };
    for c in [' ', '\u{A0}'] {
        add(c, &mut keep);
    }
    for seg in text {
        for c in seg.chars() {
            add(c, &mut keep);
        }
        keep.extend(crate::shape(face, seg, &[], |c| c).into_iter().map(|g| g.gid));
    }
    subset(sfnt, &keep, &cmap)
}

/// The face at `index` as a standalone font: the bytes themselves for a plain font, the face's
/// tables rebuilt into one font for a collection.
pub fn face_sfnt(data: &[u8], index: u32) -> Option<Cow<'_, [u8]>> {
    if data.starts_with(b"ttcf") {
        let count = be32(data, 8)?;
        if index >= count {
            return None;
        }
        let at = be32(data, 12usize.checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?)?;
        let (version, tables) = directory(data, usize::try_from(at).ok()?)?;
        return Some(Cow::Owned(assemble(version, tables.into_iter().map(|(t, b)| (t, Cow::Borrowed(b))).collect())));
    }
    (index == 0 && directory(data, 0).is_some()).then_some(Cow::Borrowed(data))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at.checked_add(1)?)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// The table directory at `at`: sfnt version and each table's tag and bytes.
fn directory(data: &[u8], at: usize) -> Option<(u32, Vec<([u8; 4], &[u8])>)> {
    let version = be32(data, at)?;
    if !matches!(version, 0x0001_0000 | 0x4F54_544F | 0x7472_7565) {
        return None;
    }
    let n = usize::from(be16(data, at.checked_add(4)?)?);
    if n == 0 || n > MAX_TABLES {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let rec = at.checked_add(12)?.checked_add(i.checked_mul(16)?)?;
        let tag = data.get(rec..rec.checked_add(4)?)?;
        let off = usize::try_from(be32(data, rec.checked_add(8)?)?).ok()?;
        let len = usize::try_from(be32(data, rec.checked_add(12)?)?).ok()?;
        let bytes = data.get(off..off.checked_add(len)?)?;
        out.push(([tag[0], tag[1], tag[2], tag[3]], bytes));
    }
    Some((version, out))
}

/// A table of a standalone font.
fn table(sfnt: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
    directory(sfnt, 0)?.1.into_iter().find(|(t, _)| *t == tag).map(|(_, b)| b)
}

fn checksum(b: &[u8]) -> u32 {
    b.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// A font file from its tables (sorted by tag, each padded to four bytes), with table checksums
/// and `head.checkSumAdjustment` computed. A digital signature (`DSIG`) no longer matches a
/// rebuilt font and is left out.
pub fn assemble(version: u32, mut tables: Vec<([u8; 4], Cow<'_, [u8]>)>) -> Vec<u8> {
    tables.retain(|(t, _)| t != b"DSIG");
    tables.sort_by_key(|(t, _)| *t);
    tables.dedup_by_key(|(t, _)| *t);
    let n = tables.len().min(MAX_TABLES);
    tables.truncate(n);
    let mut pow = 1usize;
    let mut sel = 0u16;
    while pow * 2 <= n {
        pow *= 2;
        sel += 1;
    }
    let search = (pow * 16) as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&search.to_be_bytes());
    out.extend_from_slice(&sel.to_be_bytes());
    out.extend_from_slice(&((n * 16) as u16).wrapping_sub(search).to_be_bytes());
    let mut offset = 12 + 16 * n;
    let mut head_at = None;
    let mut body = Vec::new();
    for (tag, bytes) in &tables {
        let mut data = bytes.to_vec();
        if tag == b"head" && data.len() >= 12 {
            data[8..12].fill(0);
            head_at = Some(offset + 8);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(&data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let padded = data.len().div_ceil(4) * 4;
        data.resize(padded, 0);
        offset += padded;
        body.extend_from_slice(&data);
    }
    out.extend_from_slice(&body);
    let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    if let Some(slot) = head_at.and_then(|at| out.get_mut(at..at + 4)) {
        slot.copy_from_slice(&adj.to_be_bytes());
    }
    out
}

/// The components a composite glyph is built from (empty for a simple glyph).
fn components(g: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    if be16(g, 0).is_none_or(|n| (n as i16) >= 0) {
        return out;
    }
    let mut at = 10usize;
    while let (Some(flags), Some(gid)) = (be16(g, at), be16(g, at + 2)) {
        out.push(u32::from(gid));
        at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
        at += if flags & 0x0008 != 0 {
            2
        } else if flags & 0x0040 != 0 {
            4
        } else if flags & 0x0080 != 0 {
            8
        } else {
            0
        };
        if flags & 0x0020 == 0 || out.len() >= 256 {
            break;
        }
    }
    out
}

/// `sfnt` with every glyph outside `keep` (and the composites' components) emptied and the
/// character map cut down to `cmap`. Glyph ids don't change, so the layout tables, metrics and
/// hinting stay valid. `None` for fonts this can't do: CFF outlines, variable fonts, or no
/// Unicode character map.
pub fn subset(sfnt: &[u8], keep: &BTreeSet<u32>, cmap: &BTreeMap<u32, u32>) -> Option<Vec<u8>> {
    let (version, tables) = directory(sfnt, 0)?;
    let get = |tag: &[u8; 4]| tables.iter().find(|(t, _)| t == tag).map(|(_, b)| *b);
    if [b"fvar", b"gvar", b"CFF ", b"CFF2"].iter().any(|t| get(t).is_some()) || !has_unicode_cmap(get(b"cmap")?) {
        return None;
    }
    let head = get(b"head")?;
    let glyf = get(b"glyf")?;
    let loca = get(b"loca")?;
    let n = usize::from(be16(get(b"maxp")?, 4)?);
    let long = be16(head, 50)? != 0;
    let offsets: Vec<usize> = (0..=n)
        .map(|i| if long { be32(loca, i * 4).and_then(|v| usize::try_from(v).ok()) } else { be16(loca, i * 2).map(|v| usize::from(v) * 2) })
        .collect::<Option<_>>()?;
    let glyph = |g: usize| -> &[u8] {
        match (offsets.get(g), offsets.get(g + 1)) {
            (Some(a), Some(b)) if a < b => glyf.get(*a..*b).unwrap_or_default(),
            _ => &[],
        }
    };
    let mut kept = vec![false; n];
    let mut work: Vec<u32> = keep.iter().copied().chain([0]).collect();
    while let Some(g) = work.pop() {
        let Some(slot) = usize::try_from(g).ok().and_then(|g| kept.get_mut(g)) else { continue };
        if *slot {
            continue;
        }
        *slot = true;
        work.extend(components(glyph(g as usize)));
    }
    let mut new_glyf = Vec::new();
    let mut new_offsets = Vec::with_capacity(n + 1);
    for (g, k) in kept.iter().enumerate() {
        new_offsets.push(new_glyf.len());
        if *k {
            new_glyf.extend_from_slice(glyph(g));
            new_glyf.resize(new_glyf.len().div_ceil(4) * 4, 0);
        }
    }
    new_offsets.push(new_glyf.len());
    let short = new_glyf.len() / 2 <= usize::from(u16::MAX);
    let mut new_loca = Vec::with_capacity((n + 1) * if short { 2 } else { 4 });
    for o in &new_offsets {
        if short {
            new_loca.extend_from_slice(&((o / 2) as u16).to_be_bytes());
        } else {
            new_loca.extend_from_slice(&u32::try_from(*o).ok()?.to_be_bytes());
        }
    }
    let mut new_head = head.to_vec();
    new_head.get_mut(50..52)?.copy_from_slice(&u16::from(!short).to_be_bytes());
    let new_cmap = build_cmap(cmap)?;
    let out = tables
        .into_iter()
        .map(|(t, b)| {
            let data = match &t {
                b"glyf" => Cow::Owned(std::mem::take(&mut new_glyf)),
                b"loca" => Cow::Owned(std::mem::take(&mut new_loca)),
                b"head" => Cow::Owned(std::mem::take(&mut new_head)),
                b"cmap" => Cow::Owned(new_cmap.clone()),
                _ => Cow::Borrowed(b),
            };
            (t, data)
        })
        .collect();
    Some(assemble(version, out))
}

/// Does the `cmap` table have a Unicode subtable (Windows Unicode BMP or full, or the Unicode
/// platform)? Symbol-encoded fonts don't, and keep their map as it is.
fn has_unicode_cmap(cmap: &[u8]) -> bool {
    let n = usize::from(be16(cmap, 2).unwrap_or(0));
    (0..n).any(|i| matches!((be16(cmap, 4 + i * 8), be16(cmap, 6 + i * 8)), (Some(0), _) | (Some(3), Some(1 | 10))))
}

/// A `cmap` table mapping exactly `map`: a Windows Unicode BMP subtable (format 4) and, when a
/// character lies beyond the BMP, a Windows Unicode full subtable (format 12). `None` when the
/// BMP part doesn't fit a format 4 subtable.
fn build_cmap(map: &BTreeMap<u32, u32>) -> Option<Vec<u8>> {
    let mut segs: Vec<(u16, u16, u16)> = Vec::new();
    for (&cp, &g) in map.range(..0xFFFF) {
        let (cp, g) = (u16::try_from(cp).ok()?, u16::try_from(g).ok()?);
        match segs.last_mut() {
            Some((start, end, first)) if end.checked_add(1) == Some(cp) && first.wrapping_add(cp - *start) == g => *end = cp,
            _ => segs.push((cp, cp, g)),
        }
    }
    let segs: Vec<(u16, u16, u16)> = segs.into_iter().map(|(s, e, g)| (s, e, g.wrapping_sub(s))).chain([(0xFFFF, 0xFFFF, 1)]).collect();
    let seg_count = segs.len();
    let len4 = 16 + seg_count * 8;
    if len4 > usize::from(u16::MAX) {
        return None;
    }
    let mut pow = 1usize;
    let mut sel = 0u16;
    while pow * 2 <= seg_count {
        pow *= 2;
        sel += 1;
    }
    let mut f4 = Vec::with_capacity(len4);
    for v in [4u16, len4 as u16, 0, (seg_count * 2) as u16, (pow * 2) as u16, sel, ((seg_count - pow) * 2) as u16] {
        f4.extend_from_slice(&v.to_be_bytes());
    }
    segs.iter().for_each(|s| f4.extend_from_slice(&s.1.to_be_bytes()));
    f4.extend_from_slice(&0u16.to_be_bytes());
    segs.iter().for_each(|s| f4.extend_from_slice(&s.0.to_be_bytes()));
    segs.iter().for_each(|s| f4.extend_from_slice(&s.2.to_be_bytes()));
    segs.iter().for_each(|_| f4.extend_from_slice(&0u16.to_be_bytes()));
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    if map.keys().any(|cp| *cp > 0xFFFF) {
        for (&cp, &g) in map {
            match groups.last_mut() {
                Some((start, end, gid)) if end.checked_add(1) == Some(cp) && gid.wrapping_add(cp - *start) == g => *end = cp,
                _ => groups.push((cp, cp, g)),
            }
        }
    }
    let subtables = if groups.is_empty() { 1u16 } else { 2 };
    let mut out = Vec::new();
    for v in [0u16, subtables, 3, 1] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let first = 4 + 8 * usize::from(subtables);
    out.extend_from_slice(&u32::try_from(first).ok()?.to_be_bytes());
    if !groups.is_empty() {
        out.extend_from_slice(&[0, 3, 0, 10]);
        out.extend_from_slice(&u32::try_from(first + f4.len()).ok()?.to_be_bytes());
    }
    out.extend_from_slice(&f4);
    if !groups.is_empty() {
        let len12 = u32::try_from(16 + groups.len() * 12).ok()?;
        out.extend_from_slice(&12u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&len12.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(groups.len()).ok()?.to_be_bytes());
        for (s, e, g) in groups {
            for v in [s, e, g] {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
    }
    Some(out)
}

/// A name from the `name` table: the US English Windows entry, else any Windows or Unicode one,
/// else a Macintosh Roman one read as ASCII.
fn name(sfnt: &[u8], id: u16) -> String {
    let Some(t) = table(sfnt, *b"name") else { return String::new() };
    let count = usize::from(be16(t, 2).unwrap_or(0));
    let storage = usize::from(be16(t, 4).unwrap_or(0));
    let mut best: Option<(u8, &[u8], bool)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (Some(pid), Some(eid), Some(lang), Some(nid), Some(len), Some(off)) =
            (be16(t, r), be16(t, r + 2), be16(t, r + 4), be16(t, r + 6), be16(t, r + 8), be16(t, r + 10))
        else {
            break;
        };
        if nid != id {
            continue;
        }
        let score = match (pid, eid, lang) {
            (3, 1, 0x0409) => 4,
            (3, 1 | 10, _) => 3,
            (0, _, _) => 2,
            (1, 0, _) => 1,
            _ => continue,
        };
        let start = storage + usize::from(off);
        let Some(bytes) = t.get(start..start + usize::from(len)) else { continue };
        if best.is_none_or(|b| b.0 < score) {
            best = Some((score, bytes, pid != 1));
        }
    }
    match best {
        Some((_, b, true)) => char::decode_utf16(b.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)))
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
        Some((_, b, false)) => b.iter().map(|&c| if c.is_ascii() { char::from(c) } else { '?' }).collect(),
        None => String::new(),
    }
}

fn put_name(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().take(16_000).collect();
    out.extend_from_slice(&((units.len() * 2) as u16).to_le_bytes());
    units.iter().for_each(|u| out.extend_from_slice(&u.to_le_bytes()));
}

/// An uncompressed Embedded OpenType (version 0x00020001) around a standalone font: the
/// header's PANOSE, weight, italic, `fsType`, Unicode and code page ranges and checksum
/// adjustment come from the font's `OS/2` and `head` tables, its names from the `name` table
/// (family, style, version, full name), and the root string is empty. `None` when the font is
/// too large for the header's 32-bit sizes or has no `head` table.
pub fn eot(sfnt: &[u8], subset: bool) -> Option<Vec<u8>> {
    let head = table(sfnt, *b"head")?;
    let os2 = table(sfnt, *b"OS/2").unwrap_or_default();
    let font_len = u32::try_from(sfnt.len()).ok()?;
    let mut h = Vec::with_capacity(160);
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(&font_len.to_le_bytes());
    h.extend_from_slice(&EOT_VERSION.to_le_bytes());
    h.extend_from_slice(&(if subset { EOT_SUBSET } else { 0 }).to_le_bytes());
    h.extend_from_slice(os2.get(32..42).unwrap_or(&[0; 10]));
    h.push(1);
    h.push(u8::from(be16(os2, 62).is_some_and(|s| s & 1 != 0)));
    h.extend_from_slice(&u32::from(be16(os2, 4).unwrap_or(400)).to_le_bytes());
    h.extend_from_slice(&be16(os2, 8).unwrap_or(0).to_le_bytes());
    h.extend_from_slice(&EOT_MAGIC.to_le_bytes());
    for i in 0..4 {
        h.extend_from_slice(&be32(os2, 42 + i * 4).unwrap_or(0).to_le_bytes());
    }
    let ranges = be16(os2, 0).is_some_and(|v| v >= 1);
    for i in 0..2 {
        h.extend_from_slice(&be32(os2, 78 + i * 4).filter(|_| ranges).unwrap_or(0).to_le_bytes());
    }
    h.extend_from_slice(&be32(head, 8).unwrap_or(0).to_le_bytes());
    h.extend_from_slice(&[0; 16]);
    for id in [1, 2, 5, 4] {
        h.extend_from_slice(&0u16.to_le_bytes());
        put_name(&mut h, &name(sfnt, id));
    }
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    let total = u32::try_from(h.len()).ok()?.checked_add(font_len)?;
    h.get_mut(0..4)?.copy_from_slice(&total.to_le_bytes());
    h.extend_from_slice(sfnt);
    Some(h)
}

#[cfg(test)]
mod tests;
