//! Independent selection/provenance tests; glyphs come solely from the existing CC0
//! contributor-original square font. Familiar family labels below are synthetic names,
//! never vendor font data. No test reads an installed font or application directory.
use deckcraft_fonts::{CRAFT_FONTS, FontDb};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const ORIGINAL: &[u8] = include_bytes!("fixtures/legacy-regular.ttf");

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// Rename only our original square fixture, preserving a valid SFNT/name directory
/// and checksum adjustment. This creates no recognizable vendor glyph artwork.
fn synthetic(family: &str, style: &str) -> Vec<u8> {
    let mut bytes = ORIGINAL.to_vec();
    let entries: Vec<_> = (0..usize::from(u16_at(&bytes, 4))).map(|index| 12 + index * 16).collect();
    let name_entry = entries.iter().copied().find(|&at| &bytes[at..at + 4] == b"name").unwrap();
    let head_entry = entries.iter().copied().find(|&at| &bytes[at..at + 4] == b"head").unwrap();
    let names = [(1u16, family), (2, style), (16, family), (17, style)];
    let string_start = 6 + names.len() * 12;
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&(names.len() as u16).to_be_bytes());
    table.extend_from_slice(&(string_start as u16).to_be_bytes());
    let mut strings = Vec::new();
    for (id, value) in names {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for field in [3u16, 1, 0x0409, id, encoded.len() as u16, strings.len() as u16] {
            table.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend_from_slice(&encoded);
    }
    table.extend_from_slice(&strings);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let offset = bytes.len();
    bytes.extend_from_slice(&table);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes[name_entry + 4..name_entry + 8].copy_from_slice(&checksum(&table).to_be_bytes());
    bytes[name_entry + 8..name_entry + 12].copy_from_slice(&(offset as u32).to_be_bytes());
    bytes[name_entry + 12..name_entry + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
    let head = u32_at(&bytes, head_entry + 8) as usize;
    bytes[head + 8..head + 12].copy_from_slice(&0u32.to_be_bytes());
    let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    bytes
}

fn bundled_family(family: &str, bytes: &[u8]) -> bool {
    CRAFT_FONTS.iter().any(|font| font.family.eq_ignore_ascii_case(family) && font.bytes == bytes)
}

struct OriginalFontDirectory(PathBuf);

impl OriginalFontDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path =
            std::env::temp_dir().join(format!("deckcraft-original-font-review-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for OriginalFontDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independent_metric_aliases_ignore_user_style_label_spoofs_before_ranking() {
    for (requested, expected) in [
        ("Arial", "Liberation Sans"),
        ("Calibri", "Carlito"),
        ("Cambria", "Caladea"),
        ("Consolas", "Liberation Mono"),
        ("Times New Roman", "Liberation Serif"),
    ] {
        if !CRAFT_FONTS.iter().any(|font| font.family.eq_ignore_ascii_case(expected)) {
            continue; // Optional craft-fonts input is deliberately supported.
        }
        let db = FontDb::with_font_dirs(Vec::new());
        let spoof = synthetic(expected, "Review Untrusted");
        assert_eq!(db.add_font(spoof.clone()), 1, "our synthetic exact style must be a real candidate");
        assert_eq!(db.add_font(synthetic(requested, "Review Untrusted")), 1);
        for style in ["Review Untrusted", "Review Untrusted {wght:650}", "Regular", "Italic", "Bold Italic"] {
            for selected in [db.face(requested, style), db.document_face(requested, style)] {
                assert!(
                    selected.family.eq_ignore_ascii_case(expected),
                    "a spoof style cannot hide the preferred bundled metric family: {requested}/{style}"
                );
                assert!(bundled_family(expected, selected.data()), "selected bytes must come from that exact compiled attributed family");
                assert_ne!(selected.data(), spoof, "a trusted family label cannot establish source provenance");
            }
        }
    }
}

#[test]
fn independent_open_requested_family_keeps_precedence_over_inter_substitution() {
    let db = FontDb::with_font_dirs(Vec::new());
    for family in ["Open Sans", "Lato", "DeckCraft Original Square"] {
        let original = synthetic(family, "Regular");
        let _ = db.add_font(original.clone());
        for requested in [family.to_string(), family.to_ascii_uppercase()] {
            for selected in [db.face(&requested, "Regular"), db.document_face(&requested, "Regular")] {
                assert!(selected.family.eq_ignore_ascii_case(family), "exact open/user family must win over Inter fallback");
                if CRAFT_FONTS.iter().any(|font| font.family.eq_ignore_ascii_case(family)) {
                    assert!(bundled_family(family, selected.data()), "only an exact existing compiled family may precede our user regular face");
                } else {
                    assert_eq!(selected.data(), original, "without an exact compiled face, select the actual user face");
                }
            }
        }
    }
}

#[test]
fn independent_bundled_metric_lookup_does_not_start_installed_catalogue() {
    for (requested, expected) in [("Arial", "Liberation Sans"), ("Calibri", "Carlito")] {
        if !CRAFT_FONTS.iter().any(|font| font.family.eq_ignore_ascii_case(expected)) {
            continue;
        }
        let directory = OriginalFontDirectory::new();
        let sentinel = directory.0.join("sentinel.ttf");
        std::fs::write(&sentinel, synthetic("DeckCraft Catalogue Sentinel", "Regular")).unwrap();
        std::fs::write(directory.0.join("synthetic-vendor-label.ttf"), synthetic(requested, "Regular")).unwrap();
        let db = FontDb::with_font_dirs(vec![directory.0.clone()]);
        assert!(bundled_family(expected, db.face(requested, "Regular").data()));
        assert!(bundled_family(expected, db.document_face(requested, "Regular").data()));
        // An early catalogue would retain the sentinel's family even after its file
        // disappears. A first scan after this removal can only see our other file.
        std::fs::remove_file(&sentinel).unwrap();
        assert!(!db.has_family("DeckCraft Catalogue Sentinel"), "trusted metric lookup must finish before cataloguing installed directories");
    }
}

#[test]
fn independent_exact_user_font_from_synthetic_catalogue_still_loads() {
    let directory = OriginalFontDirectory::new();
    let original = synthetic("DeckCraft Catalogue Original", "Regular");
    std::fs::write(directory.0.join("original.ttf"), &original).unwrap();
    let db = FontDb::with_font_dirs(vec![directory.0.clone()]);
    for selected in [db.face("DeckCraft Catalogue Original", "Regular"), db.document_face("DeckCraft Catalogue Original", "Regular")] {
        assert_eq!(selected.family, "DeckCraft Catalogue Original");
        assert_eq!(selected.data(), original);
        assert!(selected.glyph_for('A') != 0, "the loaded original square fixture is a usable font");
    }
}
