//! Runtime-only synthetic fonts, derived from the existing contributor-original minimal sfnt.
//! No new font asset is stored in the repository. Glyph A's square also maps to Chinese 字.

use std::collections::BTreeMap;

pub fn localized(style: &str, weight: u16) -> Vec<u8> {
    font("DeckCraft Localized", style, weight, &["DeckCraft 测试字体", "DeckCraft Police locale"], (900, 100), false)
}

pub fn font(family: &str, style: &str, weight: u16, aliases: &[&str], win_metrics: (u16, u16), variable: bool) -> Vec<u8> {
    let base = include_bytes!("../fixtures/legacy-regular.ttf");
    let count = u16::from_be_bytes(base[4..6].try_into().unwrap()) as usize;
    let mut tables: BTreeMap<[u8; 4], Vec<u8>> = (0..count)
        .map(|i| {
            let r = &base[12 + i * 16..28 + i * 16];
            let start = u32::from_be_bytes(r[8..12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(r[12..16].try_into().unwrap()) as usize;
            (r[..4].try_into().unwrap(), base[start..start + len].to_vec())
        })
        .collect();
    let legacy = if style == "Semibold" { format!("{family} Semibold") } else { family.to_owned() };
    let legacy_style = if style == "Semibold" { "Regular" } else { style };
    let mut names = vec![(1, 0x0409, legacy), (2, 0x0409, legacy_style.into()), (16, 0x0409, family.into()), (17, 0x0409, style.into())];
    for (i, alias) in aliases.iter().enumerate() {
        let lang = if i == 0 { 0x0804 } else { 0x040c };
        let legacy = if style == "Semibold" { format!("{alias} Semibold") } else { (*alias).to_owned() };
        names.extend([(1, lang, legacy), (16, lang, (*alias).to_owned())]);
    }
    if variable {
        names.push((256, 0x0409, "Weight".into()));
        // One weight axis, no named instances: exercises FontDb's lazily cached user coordinates.
        let mut fvar = vec![];
        for v in [1_u16, 0, 16, 2, 1, 20, 0, 8] {
            fvar.extend(v.to_be_bytes());
        }
        fvar.extend(b"wght");
        for v in [100_i32, 400, 900] {
            fvar.extend((v << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]); // flags, axis name ID 256
        tables.insert(*b"fvar", fvar);
    }
    names.sort_by_key(|(id, lang, _)| (*lang, *id));
    let mut records = vec![];
    let mut strings = vec![];
    for (id, lang, text) in names {
        let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [3_u16, 1, lang, id, bytes.len() as u16, strings.len() as u16] {
            records.extend(v.to_be_bytes());
        }
        strings.extend(bytes);
    }
    let mut name = vec![];
    for v in [0_u16, (records.len() / 12) as u16, (6 + records.len()) as u16] {
        name.extend(v.to_be_bytes());
    }
    name.extend(records);
    name.extend(strings);
    tables.insert(*b"name", name);
    // A Windows UCS-4 cmap: A and 字 both draw the original square, rather than falling back.
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
    for v in [40_u32, 0, 2, 65, 65, 1, 0x5b57, 0x5b57, 1] {
        cmap.extend(v.to_be_bytes());
    }
    tables.insert(*b"cmap", cmap);
    let os2 = tables.get_mut(b"OS/2").unwrap();
    os2[4..6].copy_from_slice(&weight.to_be_bytes());
    os2[74..76].copy_from_slice(&win_metrics.0.to_be_bytes());
    os2[76..78].copy_from_slice(&win_metrics.1.to_be_bytes());
    // Rebuild the directory so replacements and the optional fvar have correct offsets/checksums.
    tables.get_mut(b"head").unwrap()[8..12].fill(0);
    let n = tables.len() as u16;
    let range = (1_u16 << n.ilog2()) * 16;
    let mut out = 0x0001_0000_u32.to_be_bytes().to_vec();
    for v in [n, range, n.ilog2() as u16, n * 16 - range] {
        out.extend(v.to_be_bytes());
    }
    out.resize(12 + tables.len() * 16, 0);
    let mut head = 0;
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        let offset = out.len();
        let record = &mut out[12 + i * 16..28 + i * 16];
        record[..4].copy_from_slice(&tag);
        record[4..8].copy_from_slice(&checksum(&data).to_be_bytes());
        record[8..12].copy_from_slice(&(offset as u32).to_be_bytes());
        record[12..16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == *b"head" {
            head = offset;
        }
        out.extend(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    let adjustment = 0xb1b0_afba_u32.wrapping_sub(checksum(&out));
    out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    out
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0_u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}
