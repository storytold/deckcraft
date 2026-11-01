//! EquaSHionCraft fonts: the font database (bundled OFL families + optional user/system fonts),
//! vertical metrics, glyph outlines and OpenType shaping.
//!
//! Shaping here is style-agnostic: [`shape`] turns a string in one face into glyph ids, clusters
//! and advances in font units; `equashioncraft-layout` applies sizes on top. [`FontDb::bundled_only`]
//! is the offline database (no system fonts) used by tests and goldens.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod fontdb;

pub use fontdb::{FALLBACK_FAMILY, FaceId, FontDb, FontFace, base_style, bundled, system_font_dirs};
pub use harfrust::Feature;
use harfrust::{Direction, ShapeOptions, Tag, UnicodeBuffer};
pub use kurbo::BezPath;
use skrifa::MetadataProvider;
use skrifa::instance::Size;

/// The default equation family: bundled in Regular, Italic, Bold and Bold Italic, so the four
/// math variants never fall back to a different family.
pub const DEFAULT_FAMILY: &str = "Source Serif 4";

/// One shaped glyph, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub gid: u32,
    /// Byte offset (in the shaped string) of the cluster this glyph belongs to.
    pub cluster: usize,
    pub x_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

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

/// Shape `text` with `face`. `chars` lets callers substitute characters (e.g. uppercase for All
/// Caps) while keeping clusters pointing into the original string. Glyphs come in logical
/// order: right-to-left runs are shaped right to left, then their clusters put back in text
/// order (each cluster's glyphs keep the shaper's order); line layout reorders them visually.
pub fn shape(face: &FontFace, text: &str, features: &[Feature], map: impl Fn(char) -> char) -> Vec<ShapedGlyph> {
    if !text.chars().any(is_rtl) {
        return shape_dir(face, text, features, &map, false);
    }
    let mut out = Vec::with_capacity(text.len());
    for (r, rtl) in direction_runs(text) {
        let mut g = shape_dir(face, &text[r.clone()], features, &map, rtl);
        for x in &mut g {
            x.cluster += r.start;
        }
        if rtl {
            // Visual (clusters descending) → logical, cluster by cluster.
            let mut groups: Vec<Vec<ShapedGlyph>> = Vec::new();
            for x in g {
                match groups.last_mut() {
                    Some(last) if last[0].cluster == x.cluster => last.push(x),
                    _ => groups.push(vec![x]),
                }
            }
            groups.sort_by_key(|grp| grp[0].cluster);
            g = groups.into_iter().flatten().collect();
        }
        out.extend(g);
    }
    out
}

fn shape_dir(face: &FontFace, text: &str, features: &[Feature], map: &impl Fn(char) -> char, rtl: bool) -> Vec<ShapedGlyph> {
    let mut out = Vec::with_capacity(text.len());
    let shaped = face.hb().map(|hb| {
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        for (i, c) in text.char_indices() {
            buf.add(map(c), i as u32);
        }
        buf.guess_segment_properties();
        buf.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        let gb = shaper.shape(buf, ShapeOptions::new().features(features));
        for (info, pos) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            out.push(ShapedGlyph {
                gid: info.glyph_id,
                cluster: info.cluster as usize,
                x_advance: pos.x_advance,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
            });
        }
    });
    if shaped.is_none()
        && let Some(f) = face.skrifa()
    {
        let cmap = f.charmap();
        let gm = f.glyph_metrics(Size::unscaled(), face.location());
        for (i, c) in text.char_indices() {
            let g = cmap.map(map(c)).unwrap_or_default();
            let adv = gm.advance_width(g).unwrap_or(face.upem as f32 * 0.5);
            out.push(ShapedGlyph { gid: g.to_u32(), cluster: i, x_advance: adv.round() as i32, x_offset: 0, y_offset: 0 });
        }
    }
    out
}

/// Glyph id of the first of `chars` the face has (0 = .notdef).
pub fn first_glyph(face: &FontFace, chars: &[char]) -> u32 {
    chars.iter().map(|c| face.glyph_for(*c)).find(|g| *g != 0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> FontDb {
        FontDb::bundled_only()
    }

    #[test]
    fn last_resort_face_parses() {
        let f = fontdb::last_resort_face();
        assert_eq!(f.family, fontdb::FALLBACK_FAMILY);
        assert!(f.upem > 0.0);
    }

    #[test]
    fn bundled_families_load_and_nothing_else() {
        let db = db();
        assert_eq!(db.families(), vec!["Inter".to_string(), DEFAULT_FAMILY.to_string(), "STIX Two Math".to_string()]);
        let styles = db.styles(DEFAULT_FAMILY);
        for s in ["Regular", "Italic", "Bold", "Bold Italic"] {
            assert!(styles.iter().any(|x| x == s), "{s} in {styles:?}");
        }
    }

    #[test]
    fn the_default_family_resolves_to_itself_in_every_math_style() {
        let db = db();
        for (style, italic, bold) in [("Regular", false, false), ("Italic", true, false), ("Bold", false, true), ("Bold Italic", true, true)] {
            let f = db.face(DEFAULT_FAMILY, style);
            assert_eq!(f.family, DEFAULT_FAMILY, "{style}");
            assert_eq!(f.italic, italic, "{style}: {f:?}");
            assert_eq!(f.weight >= 600.0, bold, "{style}: {f:?}");
        }
    }

    #[test]
    fn an_unknown_family_falls_back_to_the_default_family() {
        let db = db();
        assert!(!db.has_family("Cambria Math"));
        assert_eq!(db.face("Cambria Math", "Regular").family, DEFAULT_FAMILY);
        assert_eq!(FALLBACK_FAMILY, DEFAULT_FAMILY);
    }

    #[test]
    fn faces_are_found_by_id() {
        let db = db();
        let f = db.face("Inter", "Regular");
        let back = db.face_by_id(f.face_id()).unwrap();
        assert!(std::sync::Arc::ptr_eq(&f, &back));
        assert!(db.face_by_id(FaceId(u32::MAX)).is_none());
    }

    #[test]
    fn shaping_produces_clusters_and_advances() {
        let face = db().face(DEFAULT_FAMILY, "Regular");
        let g = shape(&face, "Hello", &[], |c| c);
        assert_eq!(g.len(), 5);
        assert_eq!(g.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert!(g.iter().all(|g| g.x_advance > 0 && g.gid != 0));
    }

    #[test]
    fn ligatures_can_be_disabled() {
        let face = db().face(DEFAULT_FAMILY, "Regular");
        let on = shape(&face, "office", &[], |c| c);
        let off = shape(&face, "office", &[feature("-liga").unwrap()], |c| c);
        assert!(on.len() <= off.len());
        assert_eq!(off.len(), 6);
    }

    #[test]
    fn mapping_keeps_clusters() {
        let face = db().face(DEFAULT_FAMILY, "Regular");
        let g = shape(&face, "ab", &[], |c| c.to_ascii_uppercase());
        assert_eq!(g[0].gid, face.glyph_for('A'));
        assert_eq!(g[1].cluster, 1);
    }

    #[test]
    fn right_to_left_runs_come_back_in_logical_order() {
        // Neither bundled face covers Hebrew, so this only checks cluster order.
        let face = db().face(DEFAULT_FAMILY, "Regular");
        let g = shape(&face, "a\u{5D0}\u{5D1}b", &[], |c| c);
        let clusters: Vec<usize> = g.iter().map(|g| g.cluster).collect();
        let mut sorted = clusters.clone();
        sorted.sort_unstable();
        assert_eq!(clusters, sorted);
    }

    #[test]
    fn outlines_and_metrics() {
        let db = db();
        let face = db.face(DEFAULT_FAMILY, "Bold");
        assert_eq!(face.style, "Bold");
        let o = db.outline(&face, face.glyph_for('O'));
        assert!(!o.elements().is_empty());
        assert!(face.ascent > 0.0 && face.descent > 0.0 && face.cap_height > face.x_height);
        assert!(feature("abc").is_none());
    }

    #[test]
    fn fallback_finds_a_bundled_face_for_a_symbol() {
        let db = db();
        let base = db.face(DEFAULT_FAMILY, "Regular");
        // Whatever covers it must be bundled; nothing is loaded from the system.
        if let Some(f) = db.fallback_for('\u{2211}', base.id()) {
            assert!(f.family == "Inter" || f.family == DEFAULT_FAMILY, "{f:?}");
        }
        assert!(db.fallback_for('\u{8A9E}', base.id()).is_none(), "no bundled CJK font, no system fallback");
    }
}
