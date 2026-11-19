use super::*;
use crate::FontDb;

const UBUNTU: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;
const LEGACY: &[u8] = include_bytes!("../../tests/fixtures/legacy-regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../tests/fixtures/legacy-semibold.ttf");

fn with_fs_type(font: &[u8], fs: u16) -> Vec<u8> {
    let (_, tables) = directory(font, 0).unwrap();
    let os2 = tables.iter().find(|(t, _)| t == b"OS/2").unwrap().1;
    let at = os2.as_ptr() as usize - font.as_ptr() as usize + 8;
    let mut out = font.to_vec();
    out[at..at + 2].copy_from_slice(&fs.to_be_bytes());
    out
}

fn glyph_len(sfnt: &[u8], gid: u32) -> usize {
    let long = be16(table(sfnt, *b"head").unwrap(), 50).unwrap() != 0;
    let loca = table(sfnt, *b"loca").unwrap();
    let at = |i: usize| if long { be32(loca, i * 4).unwrap() as usize } else { be16(loca, i * 2).unwrap() as usize * 2 };
    at(gid as usize + 1) - at(gid as usize)
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn utf16le(b: &[u8]) -> String {
    char::decode_utf16(b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c))).map(Result::unwrap).collect()
}

fn face(db: &FontDb, family: &str) -> std::sync::Arc<FontFace> {
    let f = db.face(family, "Regular");
    assert!(f.legacy.as_ref().is_some_and(|(l, _)| l == family) || f.family == family, "{f:?}");
    f
}

#[test]
fn the_eot_wrapper_reads_back_as_the_same_font() {
    let e = eot(UBUNTU, false).unwrap();
    assert_eq!(crate::embedded_sfnt(&e), Some(UBUNTU));
    assert_eq!(u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize, e.len());
    assert_eq!(u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize, UBUNTU.len());
    assert_eq!(u32::from_le_bytes(e[8..12].try_into().unwrap()), 0x0002_0001);
    assert_eq!(u32::from_le_bytes(e[12..16].try_into().unwrap()), 0, "not a subset");
    assert_eq!(le16(&e, 34), 0x504C);
    assert_eq!(u32::from_le_bytes(e[28..32].try_into().unwrap()), 300, "usWeightClass of Ubuntu Light");
    assert_eq!(e[16..26], *table(UBUNTU, *b"OS/2").unwrap().get(32..42).unwrap(), "PANOSE");
    assert_eq!(e[60..64], be32(table(UBUNTU, *b"head").unwrap(), 8).unwrap().to_le_bytes(), "checksum adjustment");
    let mut at = 80;
    let mut names = vec![];
    for _ in 0..4 {
        assert_eq!(le16(&e, at), 0, "padding");
        let len = le16(&e, at + 2) as usize;
        names.push(utf16le(&e[at + 4..at + 4 + len]));
        at += 4 + len;
    }
    assert_eq!(names, ["Ubuntu Light", "Regular", "0.83", "Ubuntu Light"]);
    assert_eq!((le16(&e, at), le16(&e, at + 2)), (0, 0), "padding and an empty root string");
    assert_eq!(&e[at + 4..], UBUNTU);
}

#[test]
fn permissions_follow_fs_type() {
    let p = |fs| permission(&with_fs_type(LEGACY, fs));
    assert_eq!(p(0x0000), Permission::Installable);
    assert_eq!(p(0x0002), Permission::Restricted);
    assert_eq!(p(0x0004), Permission::PreviewPrint);
    assert_eq!(p(0x0008), Permission::Editable);
    assert_eq!(p(0x0006), Permission::PreviewPrint, "the least restrictive bit wins");
    assert_eq!(p(0x000A), Permission::Editable);
    assert_eq!(p(0x0208), Permission::BitmapOnly);
    assert_eq!(p(0x0100), Permission::Installable);
    assert!(!Permission::Restricted.allows_embedding() && !Permission::BitmapOnly.allows_embedding());
    assert!(Permission::PreviewPrint.allows_embedding() && Permission::Editable.allows_embedding());
}

#[test]
fn a_restricted_face_is_refused() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert!(db.add_font(with_fs_type(LEGACY, 0x0002)) > 0);
    let f = face(&db, "DeckCraft Legacy");
    assert_eq!(embed_face(&f, None).unwrap_err(), Refusal::NotPermitted(Permission::Restricted));
    let db = FontDb::with_font_dirs(Vec::new());
    assert!(db.add_font(with_fs_type(LEGACY, 0x0004)) > 0);
    let part = embed_face(&face(&db, "DeckCraft Legacy"), None).unwrap();
    assert_eq!(part.permission, Permission::PreviewPrint);
    assert_eq!(le16(&part.data, 32), 0x0004, "fsType carried into the header");
}

#[test]
fn all_characters_embeds_the_font_file_unchanged() {
    let db = FontDb::with_font_dirs(Vec::new());
    let part = embed_face(&face(&db, "Ubuntu Light"), None).unwrap();
    assert!(!part.subset);
    assert_eq!(crate::embedded_sfnt(&part.data), Some(UBUNTU));
}

#[test]
fn only_the_characters_used_keeps_their_glyphs() {
    let db = FontDb::with_font_dirs(Vec::new());
    let f = face(&db, "Ubuntu Light");
    let part = embed_face(&f, Some(&["Hé".to_string()])).unwrap();
    assert!(part.subset);
    assert_eq!(u32::from_le_bytes(part.data[12..16].try_into().unwrap()), EOT_SUBSET);
    let sub = crate::embedded_sfnt(&part.data).unwrap();
    assert!(sub.len() * 2 < UBUNTU.len(), "{} of {}", sub.len(), UBUNTU.len());
    assert_eq!(checksum(sub), 0xB1B0_AFBA, "head.checkSumAdjustment");
    let font = skrifa::FontRef::new(sub).unwrap();
    let cmap = font.charmap();
    let orig = skrifa::FontRef::new(UBUNTU).unwrap();
    let g = |c: char| orig.charmap().map(c).unwrap().to_u32();
    for c in ['H', 'é', ' '] {
        assert_eq!(cmap.map(c).map(|x| x.to_u32()), Some(g(c)), "{c:?} keeps its glyph id");
        assert_eq!(glyph_len(sub, g(c)).div_ceil(4), glyph_len(UBUNTU, g(c)).div_ceil(4), "{c:?} keeps its outline");
    }
    assert!(glyph_len(UBUNTU, g('e')) > 0);
    assert_eq!(glyph_len(sub, g('e')).div_ceil(4), glyph_len(UBUNTU, g('e')).div_ceil(4), "the component of é is kept");
    for c in ['Z', 'q', '7'] {
        assert_eq!(cmap.map(c), None, "{c:?} left out of the character map");
        assert!(glyph_len(UBUNTU, g(c)) > 0);
        assert_eq!(glyph_len(sub, g(c)), 0, "{c:?} has no outline");
    }
    assert_eq!(be16(table(sub, *b"maxp").unwrap(), 4), Some(1261), "glyph ids stay as they were");
}

#[test]
fn a_font_that_forbids_subsetting_is_embedded_whole() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert!(db.add_font(with_fs_type(LEGACY, 0x0108)) > 0);
    let part = embed_face(&face(&db, "DeckCraft Legacy"), Some(&["A".to_string()])).unwrap();
    assert!(!part.subset);
    assert_eq!(crate::embedded_sfnt(&part.data), Some(with_fs_type(LEGACY, 0x0108).as_slice()));
}

#[test]
fn the_character_map_reaches_beyond_the_bmp() {
    let map: BTreeMap<u32, u32> = [(0x41, 5), (0x42, 6), (0x44, 7), (0x4E00, 9), (0x1F600, 3)].into_iter().collect();
    let cmap = build_cmap(&map).unwrap();
    let (version, tables) = directory(UBUNTU, 0).unwrap();
    let font =
        assemble(version, tables.into_iter().map(|(t, b)| (t, if &t == b"cmap" { Cow::Owned(cmap.clone()) } else { Cow::Borrowed(b) })).collect());
    let f = skrifa::FontRef::new(&font).unwrap();
    let got: Vec<(u32, u32)> = f.charmap().mappings().map(|(c, g)| (c, g.to_u32())).collect();
    assert_eq!(got, map.into_iter().collect::<Vec<_>>());
}

#[test]
fn a_face_of_a_collection_becomes_a_font_of_its_own() {
    let fonts = [LEGACY, SEMIBOLD];
    let mut ttc = b"ttcf".to_vec();
    ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    ttc.extend_from_slice(&2u32.to_be_bytes());
    let header = 12 + 8;
    let mut at = header;
    let mut body = Vec::new();
    for f in fonts {
        ttc.extend_from_slice(&(at as u32).to_be_bytes());
        let n = be16(f, 4).unwrap() as usize;
        let mut copy = f.to_vec();
        for i in 0..n {
            let rec = 12 + i * 16 + 8;
            let off = be32(f, rec).unwrap() + at as u32;
            copy[rec..rec + 4].copy_from_slice(&off.to_be_bytes());
        }
        at += copy.len();
        body.extend_from_slice(&copy);
    }
    ttc.extend_from_slice(&body);
    assert_eq!(ttc.len(), at);
    let one = face_sfnt(&ttc, 1).unwrap();
    assert!(matches!(one, Cow::Owned(_)));
    for (tag, bytes) in directory(SEMIBOLD, 0).unwrap().1 {
        if &tag != b"head" {
            assert_eq!(table(&one, tag), Some(bytes), "{}", String::from_utf8_lossy(&tag));
        }
    }
    assert_eq!(name(&one, 1), "DeckCraft Legacy Semibold");
    assert_eq!(checksum(&one), 0xB1B0_AFBA);
    assert!(face_sfnt(&ttc, 2).is_none());
    assert!(matches!(face_sfnt(LEGACY, 0), Some(Cow::Borrowed(b)) if b == LEGACY));
    assert!(face_sfnt(LEGACY, 1).is_none());
    assert!(face_sfnt(b"not a font at all", 0).is_none());
}

#[test]
fn hostile_bytes_do_not_panic() {
    for len in [0, 4, 12, 40, 200, 2000] {
        let junk: Vec<u8> = UBUNTU.iter().take(len).copied().collect();
        let _ = face_sfnt(&junk, 0);
        let _ = eot(&junk, false);
        let _ = permission(&junk);
        let _ = subset(&junk, &BTreeSet::new(), &BTreeMap::new());
    }
    let mut broken = UBUNTU.to_vec();
    broken[12 + 8..12 + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(face_sfnt(&broken, 0).is_none());
}
