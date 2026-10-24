//! DeckCraft fonts: the font database (craft-fonts families, user and system fonts, and open
//! substitutes for the Office fonts presentations ask for), vertical metrics, glyph outlines and
//! OpenType shaping, script/slot classification and per-cluster fallback.
//!
//! Shaping here is style-agnostic: [`shape`] and [`shape_range`] turn text in one face into glyph
//! ids, clusters and advances in font units. `deckcraft-text` applies sizes, tracking, scaling and
//! justification on top.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod fallback;
mod fontdb;
pub mod script;
mod shape;

pub use fallback::{bundled_families, covers_cluster, is_emoji_cluster, ligature_carets};
pub use fontdb::{
    FALLBACK_FAMILY, FaceRef, FontDb, FontFace, LAST_RESORT_FAMILY, MISSING_FONT_FAMILY, base_style, bundled, declared_faces, substitutes,
    system_font_dirs,
};
pub use harfrust::Feature;
use harfrust::Tag;
pub use kurbo::BezPath;
pub use script::{FontSlot, ScriptTag};
pub use shape::{ShapeParams, ShapedGlyph, shape_range};

/// A font from the optional craft-fonts build input (https://github.com/storytold/craft-fonts;
/// empty unless built with `CRAFT_FONTS_DIR`, see `build.rs`).
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    /// ISO 15924 scripts the font is for, e.g. `"Jpan"`.
    pub scripts: &'static [&'static str],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// One face a host page offers through its `fonts/manifest.txt` — the mechanism PhotoCraft
/// documents in its HOSTING.md (#957). The web build otherwise only has the faces compiled into
/// the wasm, so a deck that needs a brand or corporate typeface is drawn with a substitute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFont {
    /// The family the file is expected to declare.
    pub family: String,
    /// The style the file is expected to declare.
    pub style: String,
    /// Where the font file is served, relative to the page that hosts the app.
    pub path: String,
}

/// Parse a host's `fonts/manifest.txt`: one face per line as `Family | Style | path/to/file`.
/// Only the first three fields are read; blank lines and lines starting with `#` are skipped, and
/// a line without three non-empty fields is left out.
pub fn parse_host_manifest(text: &str) -> Vec<HostFont> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut fields = line.split('|').map(str::trim);
            let (family, style, path) = (fields.next()?, fields.next()?, fields.next()?);
            if family.is_empty() || style.is_empty() || path.is_empty() {
                return None;
            }
            Some(HostFont { family: family.to_string(), style: style.to_string(), path: path.to_string() })
        })
        .collect()
}

/// The craft-fonts faces for Japanese (`"Jpan"`), in manifest order. Empty without craft-fonts.
pub fn japanese_fonts() -> impl Iterator<Item = &'static CraftFont> {
    CRAFT_FONTS.iter().filter(|f| f.scripts.contains(&"Jpan"))
}

/// The TrueType or OpenType font inside a PresentationML embedded-font part
/// (`application/x-fontdata`).
///
/// The part is the font itself, or an Embedded OpenType wrapper around it (the `0x504C` magic
/// at byte 34, from the W3C Embedded OpenType submission). A MicroType Express subset is neither,
/// so this returns none; the original part stays in the file for the round trip.
pub fn embedded_sfnt(part: &[u8]) -> Option<&[u8]> {
    eot_payload(part).or_else(|| sfnt_or_collection(part).then_some(part))
}

fn sfnt_or_collection(b: &[u8]) -> bool {
    b.len() >= 12
        && (b.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || b.starts_with(b"OTTO")
            || b.starts_with(b"true")
            || b.starts_with(b"typ1")
            || b.starts_with(b"ttcf"))
}

/// Font data at the end of an Embedded OpenType blob, when the header is one.
fn eot_payload(b: &[u8]) -> Option<&[u8]> {
    if b.len() < 82 || u16::from_le_bytes([b[34], b[35]]) != 0x504C {
        return None;
    }
    let version = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    if !matches!(version, 0x0001_0000 | 0x0002_0001 | 0x0002_0002) {
        return None;
    }
    let eot_size = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?) as usize;
    let font_size = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    if font_size < 12 || eot_size < font_size.saturating_add(82) || eot_size > b.len() {
        return None;
    }
    let font = b.get(eot_size - font_size..eot_size)?;
    sfnt_or_collection(font).then_some(font)
}

/// Japanese faces in document-fallback order: Mincho (serif) families first, matching the
/// serif default text font, then the others; Regular before other styles.
pub fn japanese_document_fonts() -> Vec<&'static CraftFont> {
    let mut v: Vec<_> = japanese_fonts().collect();
    v.sort_by_key(|f| (!f.family.contains("Mincho"), f.style != "Regular"));
    v
}

/// Japanese faces in UI order: BIZ UDPGothic first (the UI face), then the others; `bold` puts
/// bold styles before regular ones.
pub fn japanese_ui_fonts(bold: bool) -> Vec<&'static CraftFont> {
    let mut v: Vec<_> = japanese_fonts().collect();
    v.sort_by_key(|f| (f.family != "BIZ UDPGothic", (f.style == "Bold") != bold));
    v
}

/// The default theme's font.
pub const DEFAULT_FAMILY: &str = "Inter";

/// An OpenType feature setting: `"liga"`, `"-kern"`, `"ss01"`.
pub fn feature(tag: &str) -> Option<Feature> {
    let (on, t) = match tag.strip_prefix('-') {
        Some(r) => (false, r),
        None => (true, tag.strip_prefix('+').unwrap_or(tag)),
    };
    let b = t.as_bytes();
    if b.len() != 4 {
        return None;
    }
    Some(Feature::new(Tag::new(&[b[0], b[1], b[2], b[3]]), on as u32, ..))
}

/// A strong right-to-left character (Hebrew, Arabic, …)?
pub fn is_rtl(c: char) -> bool {
    use unicode_bidi::BidiClass::{AL, R};
    matches!(unicode_bidi::bidi_class(c), R | AL)
}

/// Shape `text` with `face`. `chars` lets callers substitute characters (e.g. uppercase for All
/// Caps) while keeping clusters pointing into the original string. Glyphs come in logical
/// order: each direction run is shaped with [`shape_range`] (right-to-left runs right to left,
/// with the whole string as context), then its clusters put back in text order (each cluster's
/// glyphs keep the shaper's order). Paragraph layout uses [`shape_range`] directly and keeps the
/// visual order.
pub fn shape(face: &FontFace, text: &str, features: &[Feature], map: impl Fn(char) -> char) -> Vec<ShapedGlyph> {
    let mut out = Vec::with_capacity(text.len());
    for (r, rtl) in direction_runs(text) {
        let mut g = shape_range(face, text, r, &ShapeParams { features, rtl, ..Default::default() }, &map);
        if rtl {
            // Visual (clusters descending) → logical; the sort is stable, so each cluster's
            // glyphs keep the shaper's order.
            g.sort_by_key(|x| x.cluster);
        }
        out.extend(g);
    }
    out
}

/// Maximal runs of one direction (neutrals join the run they're in): (byte range, right-to-left).
fn direction_runs(text: &str) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut runs: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    let mut cur: Option<bool> = None;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        let strong = if is_rtl(c) {
            Some(true)
        } else if unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::L {
            Some(false)
        } else {
            None
        };
        match (cur, strong) {
            (None, Some(d)) => cur = Some(d),
            (Some(a), Some(d)) if a != d => {
                runs.push((start..i, a));
                start = i;
                cur = Some(d);
            }
            _ => {}
        }
    }
    runs.push((start..text.len(), cur.unwrap_or(false)));
    runs
}

/// Glyph id of the first of `chars` the face has (0 = .notdef).
pub fn first_glyph(face: &FontFace, chars: &[char]) -> u32 {
    chars.iter().map(|c| face.glyph_for(*c)).find(|g| *g != 0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any_face() -> std::sync::Arc<FontFace> {
        FontDb::global().face(DEFAULT_FAMILY, "Regular")
    }

    #[test]
    fn a_host_manifest_reads_three_fields_and_skips_the_rest() {
        let text = "\
# Family | Style | path, relative to the page
Inter | Semi Bold | fonts/inter/Inter-SemiBold.otf | this fourth field is not read

DeckCraft Legacy  |  Semibold  |  legacy-semibold.ttf
| no family | x.ttf
Only Two Fields
Inter | Regular |
";
        assert_eq!(
            parse_host_manifest(text),
            vec![
                HostFont { family: "Inter".into(), style: "Semi Bold".into(), path: "fonts/inter/Inter-SemiBold.otf".into() },
                HostFont { family: "DeckCraft Legacy".into(), style: "Semibold".into(), path: "legacy-semibold.ttf".into() },
            ]
        );
    }

    #[test]
    fn a_host_face_is_filed_under_the_family_it_declares() {
        // The typographic family (name id 16) wins over the Windows GDI family (id 1), so a
        // manifest that asks for "DeckCraft Legacy Semibold" can be warned about and the file's
        // own name kept — the mistake that costs half an hour to see (#957).
        let semibold = include_bytes!("../tests/fixtures/legacy-semibold.ttf").to_vec();
        assert_eq!(declared_faces(&semibold), vec![("DeckCraft Legacy".to_string(), "Semibold".to_string())]);
        let regular = include_bytes!("../tests/fixtures/legacy-regular.ttf").to_vec();
        assert_eq!(declared_faces(&regular), vec![("DeckCraft Legacy".to_string(), "Regular".to_string())]);
        assert_eq!(declared_faces(b"not a font at all"), Vec::new());

        let db = FontDb::with_font_dirs(Vec::new());
        assert_eq!(db.add_font(semibold), 1);
        assert!(db.has_family("DeckCraft Legacy"));
        assert!(db.styles("DeckCraft Legacy").iter().any(|s| s == "Semibold"));
        // A file that isn't a font adds nothing and doesn't stop the ones after it.
        assert_eq!(db.add_font(b"not a font at all".to_vec()), 0);
        assert_eq!(db.add_font(regular), 1);
        assert!(db.styles("DeckCraft Legacy").iter().any(|s| s == "Regular"));
    }

    #[test]
    fn emoji_clusters_are_recognised() {
        let c = |s: &str| s.chars().collect::<Vec<char>>();
        // As PowerPoint draws them: emoji-presentation characters (⭐ ✅ ☔ ⌚ ⏩) in colour even
        // without a selector, text-presentation ones (♥ ✔ ☎ ⬆ ▶) as text unless U+FE0F asks, and
        // keycaps always as text.
        for yes in ["😀", "👍🏽", "❤\u{FE0F}", "👨\u{200D}👩\u{200D}👧", "🇯🇵", "🏳\u{FE0F}\u{200D}🌈", "⭐", "✅", "☔", "⌚", "⏩", "🀄"]
        {
            assert!(is_emoji_cluster(&c(yes)), "{yes:?}");
        }
        for no in ["a", "1", "❤", "☺\u{FE0E}", "😀\u{FE0E}", "字", "→", "♥", "✔", "☎", "⬆", "▶", "1\u{FE0F}\u{20E3}", "#\u{20E3}"]
        {
            assert!(!is_emoji_cluster(&c(no)), "{no:?}");
        }
    }

    #[test]
    fn colour_fonts_are_recognised() {
        let db = FontDb::with_font_dirs(Vec::new());
        assert!(db.add_font(include_bytes!("../../render/tests/fixtures/colr-squares.ttf").to_vec()) > 0);
        assert!(db.face("DeckCraft Colr Squares", "Regular").is_color());
        assert!(!fontdb::last_resort_face().is_color());
    }

    #[test]
    fn last_resort_face_parses() {
        let f = fontdb::last_resort_face();
        assert_eq!(f.family, fontdb::LAST_RESORT_FAMILY);
        assert!(f.upem > 0.0);
    }

    #[test]
    fn line_split_follows_the_windows_ascent_and_descent() {
        use skrifa::raw::TableProvider;
        let f = fontdb::last_resort_face();
        let font = skrifa::FontRef::new(f.data()).unwrap();
        let os2 = font.os2().unwrap();
        let (a, d) = (os2.us_win_ascent() as f64, os2.us_win_descent() as f64);
        assert!((f.line_split - a / (a + d)).abs() < 1e-9, "{} vs {}", f.line_split, a / (a + d));
        assert!(f.line_split > 0.5 && f.line_split < 1.0);
    }

    #[test]
    fn some_face_always_resolves() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        let f = db.face("No Such Font Anywhere", "Regular");
        assert!(!f.family.is_empty());
        assert!(!shape(&f, "Hello", &[], |c| c).is_empty());
    }

    #[test]
    fn filtered_fallback_continues_after_a_rejected_loaded_face() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        assert_eq!(db.add_font(include_bytes!("../tests/fixtures/legacy-regular.ttf").to_vec()), 1);
        let first = db.fallback_for('A', u32::MAX).unwrap();
        let accepted_family = if first.family == LAST_RESORT_FAMILY { "DeckCraft Legacy" } else { LAST_RESORT_FAMILY };
        let fallback = db.fallback_for_filtered('A', u32::MAX, |f| f.family == accepted_family).unwrap();
        assert_eq!(fallback.family, accepted_family);
        assert!(fallback.covers('A'));
        assert_ne!(fallback.id(), first.id());
        assert_eq!(db.fallback_for('A', u32::MAX).unwrap().id(), first.id(), "document fallback order is unchanged");
        assert!(db.fallback_for_filtered('A', fallback.id(), |f| f.family == accepted_family).is_none(), "the excluded face stays excluded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn filtered_fallback_loads_an_eligible_system_face_and_does_not_cache_policy_misses() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let db = FontDb::with_font_dirs(vec![dir]);
        let first = db.fallback_for('A', u32::MAX).unwrap();
        assert_ne!(first.family, "DeckCraft Legacy");
        assert!(db.fallback_for_filtered('A', u32::MAX, |_| false).is_none());
        assert!(db.fallback_for('A', u32::MAX).is_some(), "an interface policy miss does not hide document glyphs");
        let accepted = db.fallback_for_filtered('A', u32::MAX, |f| f.family == "DeckCraft Legacy").unwrap();
        assert_eq!(accepted.family, "DeckCraft Legacy");
        assert!(accepted.covers('A'));
    }

    #[test]
    fn concurrent_lookups_all_find_an_installed_family() {
        // Threads resolving an installed family nobody had loaded yet used to fall back to
        // another face when a different thread finished loading it first, so the same text was
        // laid out in different fonts depending on timing.
        let dir = std::env::temp_dir().join(format!("deckcraft-fonts-concurrent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Hack-Regular.ttf"), epaint_default_fonts::HACK_REGULAR).unwrap();
        for _ in 0..20 {
            let db = FontDb::with_font_dirs(vec![dir.clone()]);
            db.set_system_fallback(false);
            let start = std::sync::Barrier::new(8);
            let got: Vec<String> = std::thread::scope(|s| {
                let threads: Vec<_> = (0..8)
                    .map(|_| {
                        s.spawn(|| {
                            start.wait();
                            db.face("Hack", "Regular").family.clone()
                        })
                    })
                    .collect();
                threads.into_iter().map(|t| t.join().unwrap()).collect()
            });
            assert!(got.iter().all(|f| f == "Hack"), "{got:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_family_names_find_their_faces() {
        // Documents name fonts by their Windows family ("Segoe UI Semibold"); the font's
        // typographic family is "Segoe UI" with a "Semibold" style. The fixture is named the same way.
        let check = |db: &FontDb| {
            assert!(db.has_family("DeckCraft Legacy Semibold"));
            for style in ["Regular", "Bold"] {
                let f = db.face("DeckCraft Legacy Semibold", style);
                assert_eq!((f.family.as_str(), f.style.as_str()), ("DeckCraft Legacy", "Semibold"), "{style}");
            }
            assert_eq!(db.document_face("DeckCraft Legacy Semibold", "Regular").family, "DeckCraft Legacy", "installed, so not Calibri");
            // The typographic name still works.
            assert_eq!(db.face("DeckCraft Legacy", "Semibold").style, "Semibold");
        };
        // Loaded directly…
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        assert_eq!(db.add_font(include_bytes!("../tests/fixtures/legacy-semibold.ttf").to_vec()), 1);
        check(&db);
        // …and found by the system font scan under its Windows name, then loaded.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            let db = FontDb::with_font_dirs(vec![dir.clone()]);
            db.set_system_fallback(false);
            check(&db);
            // Looking up the Windows name loads the Semibold file only; a later lookup of the
            // family's Regular must still load and pick the Regular file, not settle for Semibold.
            let db = FontDb::with_font_dirs(vec![dir]);
            db.set_system_fallback(false);
            assert_eq!(db.face("DeckCraft Legacy Semibold", "Regular").style, "Semibold");
            let regular = db.face("DeckCraft Legacy", "Regular");
            assert_eq!((regular.style.as_str(), regular.weight), ("Regular", 400.0));
        }
    }

    #[test]
    fn faces_without_outlines_are_not_cataloged() {
        // Rename the Semibold's `glyf` table, as in fonts whose glyphs are bitmaps or are stored in
        // Apple's `hvgl` table. The face still maps "A", but its outline is empty. The new tag
        // keeps the table directory sorted.
        let mut semibold = include_bytes!("../tests/fixtures/legacy-semibold.ttf").to_vec();
        let tables = u16::from_be_bytes([semibold[4], semibold[5]]) as usize;
        let glyf = (0..tables).map(|i| 12 + 16 * i).find(|&r| &semibold[r..r + 4] == b"glyf").unwrap();
        semibold[glyf..glyf + 4].copy_from_slice(b"gxyz");
        let loaded = FontDb::with_font_dirs(Vec::new());
        assert_eq!(loaded.add_font(semibold.clone()), 1);
        let face = loaded.face("DeckCraft Legacy", "Semibold");
        assert!(face.covers('A'));
        assert!(loaded.outline(&face, face.glyph_for('A')).elements().is_empty());
        // The system font scan lists the Regular, which has outlines, and leaves out the Semibold
        // under both of its names.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = std::env::temp_dir().join(format!("deckcraft-fonts-outlines-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("legacy-semibold.ttf"), &semibold).unwrap();
            std::fs::write(dir.join("legacy-regular.ttf"), include_bytes!("../tests/fixtures/legacy-regular.ttf")).unwrap();
            let db = FontDb::with_font_dirs(vec![dir.clone()]);
            db.set_system_fallback(false);
            assert_eq!(db.styles("DeckCraft Legacy"), ["Regular"]);
            assert!(!db.has_family("DeckCraft Legacy Semibold"));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn missing_document_fonts_are_drawn_in_calibri() {
        // PowerPoint draws a missing family in Calibri, whatever its panose or the theme says.
        let db = FontDb::global();
        for missing in ["Qqq Nonexistent", "Avenir Next Qqq", "", "Arimo Qqq"] {
            let f = db.document_face(missing, "Bold");
            assert_eq!(f.family, db.face(MISSING_FONT_FAMILY, "Bold").family, "{missing:?}");
        }
        // PowerPoint's own twins and Windows' substitutes, when the font itself is missing.
        for (missing, twin) in
            [("Liberation Sans", "Arial"), ("Liberation Serif", "Times New Roman"), ("Arial CE", "Arial"), ("Times New Roman CYR", "Times New Roman")]
        {
            if !db.has_family(missing) {
                assert_eq!(db.document_face(missing, "Regular").family, db.face(twin, "Regular").family, "{missing}");
            }
        }
        // Metric-compatible stand-ins and DeckCraft's default keep their own fallbacks.
        // So do Office's cloud fonts, which PowerPoint downloads and draws as themselves.
        for kept in ["Arial", "Times New Roman", "Cambria", "Courier New", FALLBACK_FAMILY, "Aptos", "Aptos Display", "Grandview"] {
            assert_eq!(db.document_face(kept, "Regular").family, db.face(kept, "Regular").family, "{kept}");
        }
    }

    #[test]
    fn office_fonts_have_substitutes() {
        assert!(substitutes("Calibri").contains(&"Carlito"));
        assert!(substitutes("Cambria").contains(&"Caladea"));
        assert!(substitutes("Times New Roman").contains(&"Liberation Serif"));
        assert!(substitutes("Aptos").contains(&"Inter"));
        assert!(substitutes("zzz").is_empty());
    }

    #[test]
    fn shaping_produces_clusters_and_advances() {
        let face = any_face();
        let g = shape(&face, "Hello", &[], |c| c);
        assert_eq!(g.len(), 5);
        assert_eq!(g.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert!(g.iter().all(|g| g.x_advance > 0 && g.gid != 0));
    }

    #[test]
    fn mapping_keeps_clusters() {
        let face = any_face();
        let g = shape(&face, "ab", &[], |c| c.to_ascii_uppercase());
        assert_eq!(g[0].gid, face.glyph_for('A'));
        assert_eq!(g[1].cluster, 1);
    }

    #[test]
    fn outlines_and_metrics() {
        let db = FontDb::global();
        let face = any_face();
        let o = db.outline(&face, face.glyph_for('O'));
        assert!(!o.elements().is_empty());
        assert!(face.ascent > 0.0 && face.descent > 0.0);
        assert!(feature("abc").is_none());
        assert!(feature("liga").is_some());
    }

    #[test]
    fn craft_fonts_latin_when_present() {
        if !CRAFT_FONTS.iter().any(|f| f.family == "Inter") {
            eprintln!("skipped: built without craft-fonts' Latin manifest (set CRAFT_FONTS_DIR)");
            return;
        }
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        assert_eq!(db.face("Inter", "Regular").family, "Inter");
        assert_eq!(db.face("Aptos", "Regular").family, "Inter");
        if CRAFT_FONTS.iter().any(|f| f.family == "Carlito") {
            assert_eq!(db.face("Calibri", "Bold").family, "Carlito");
        }
    }

    /// An Embedded OpenType header of `version` around `font`, with the font at the end.
    fn wrap_eot(font: &[u8], version: u32) -> Vec<u8> {
        let mut h = vec![0u8; 82];
        let total = 82 + font.len();
        h[0..4].copy_from_slice(&(total as u32).to_le_bytes());
        h[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        h[8..12].copy_from_slice(&version.to_le_bytes());
        h[34..36].copy_from_slice(&0x504Cu16.to_le_bytes());
        h.extend_from_slice(font);
        h
    }

    #[test]
    fn embedded_open_type_yields_the_font() {
        let font = epaint_default_fonts::UBUNTU_LIGHT;
        assert_eq!(embedded_sfnt(font), Some(font));
        assert_eq!(embedded_sfnt(&wrap_eot(font, 0x0002_0001)), Some(font));
        assert_eq!(embedded_sfnt(&wrap_eot(font, 0x0001_0000)), Some(font));
        assert!(embedded_sfnt(b"not a font file").is_none());
        assert!(embedded_sfnt(&wrap_eot(b"hello world!!", 0x0002_0002)).is_none());
        let mut truncated = wrap_eot(font, 0x0002_0001);
        truncated.truncate(40);
        assert!(embedded_sfnt(&truncated).is_none());
    }

    /// A FIFO, a folder and a link loop dressed up as font files: the system scan skips them
    /// instead of blocking forever on the FIFO (start-up hung when one sat in `~/.fonts`), and
    /// still finds the real font beside them.
    #[cfg(unix)]
    #[test]
    fn the_font_scan_skips_fifos_folders_and_link_loops() {
        let dir = std::env::temp_dir().join(format!("deckcraft-fontscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-semibold.ttf"), dir.join("sub/real.ttf")).unwrap();
        let fifo = std::process::Command::new("mkfifo").arg(dir.join("trap.ttf")).status().is_ok_and(|s| s.success());
        std::fs::create_dir(dir.join("folder.ttf")).unwrap();
        std::fs::write(dir.join("empty.otf"), b"").unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("sub/loop")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let scan_dir = dir.clone();
        std::thread::spawn(move || {
            let db = FontDb::with_font_dirs(vec![scan_dir]);
            let _ = tx.send(db.load_system_fonts());
        });
        let n = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the font scan hung");
        assert!(n >= 1, "the real font was found");
        assert!(fifo || n >= 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
