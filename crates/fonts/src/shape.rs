//! OpenType shaping with HarfRust.
//!
//! [`shape_range`] shapes one item of a paragraph (one face, one direction, one script) with the
//! rest of the paragraph as pre/post context, so Arabic letters at an item boundary (a colour
//! change inside a word, a bold letter) still take their joining forms. Glyphs come back in
//! visual order, exactly as the shaper produced them: right-to-left items are not reversed.

use std::ops::Range;

use harfrust::{Direction, Feature, Language, Script, ShapeOptions, Tag, UnicodeBuffer};
use skrifa::MetadataProvider;
use skrifa::instance::Size;

use crate::FontFace;
use crate::script::ScriptTag;

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

/// How to shape an item.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeParams<'a> {
    pub features: &'a [Feature],
    /// Right-to-left (odd bidi level). Mirrored characters (brackets) take their mirrored glyphs.
    pub rtl: bool,
    /// ISO 15924 script (`*b"Arab"`); `None` lets the shaper guess from the text.
    pub script: Option<ScriptTag>,
    /// BCP 47 language (`"ar"`, `"ar-MA"`); `None` = the script's default.
    pub language: Option<&'a str>,
}

/// Shape `para[range]` with `face`, the rest of `para` as context. Glyphs are in visual order
/// (left to right) and clusters are byte offsets into `para`. `map` substitutes characters (All
/// Caps) without moving clusters. An empty or out-of-bounds range shapes nothing.
pub fn shape_range(face: &FontFace, para: &str, range: Range<usize>, params: &ShapeParams, map: impl Fn(char) -> char) -> Vec<ShapedGlyph> {
    let (Some(pre), Some(text), Some(post)) = (para.get(..range.start), para.get(range.clone()), para.get(range.end..)) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(text.len());
    let shaped = face.hb().map(|hb| {
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        buf.set_pre_context(pre);
        buf.set_post_context(post);
        for (i, c) in text.char_indices() {
            buf.add(map(c), (range.start + i) as u32);
        }
        buf.guess_segment_properties();
        if let Some(s) = params.script.and_then(|t| Script::from_iso15924_tag(Tag::new(&t))) {
            buf.set_script(s);
        }
        if let Some(l) = params.language.or_else(|| params.script.and_then(crate::script::default_language)).and_then(|l| l.parse::<Language>().ok())
        {
            buf.set_language(l);
        }
        buf.set_direction(if params.rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        let gb = shaper.shape(buf, ShapeOptions::new().features(params.features));
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
        // No shaper for this face: one glyph per character from the cmap, in visual order.
        let cmap = f.charmap();
        let gm = f.glyph_metrics(Size::unscaled(), face.location());
        for (i, c) in text.char_indices() {
            let g = cmap.map(map(c)).unwrap_or_default();
            let adv = gm.advance_width(g).unwrap_or(face.upem as f32 * 0.5);
            out.push(ShapedGlyph { gid: g.to_u32(), cluster: range.start + i, x_advance: adv.round() as i32, x_offset: 0, y_offset: 0 });
        }
        if params.rtl {
            out.reverse();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_FAMILY, FontDb};

    #[test]
    fn rtl_items_come_in_visual_order_and_mirror() {
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let text = "(ab)";
        let p = ShapeParams { rtl: true, ..Default::default() };
        let g = shape_range(&face, text, 0..text.len(), &p, |c| c);
        // Visual order: last cluster first.
        assert_eq!(g.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![3, 2, 1, 0]);
        // L4: in a right-to-left run "(" is drawn with the ")" glyph and vice versa.
        if face.covers('(') && face.covers(')') {
            assert_eq!(g[3].gid, face.glyph_for(')'));
            assert_eq!(g[0].gid, face.glyph_for('('));
        }
    }

    #[test]
    fn clusters_point_into_the_whole_paragraph() {
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let para = "Hello world";
        let g = shape_range(&face, para, 6..11, &ShapeParams::default(), |c| c);
        assert_eq!(g.first().map(|g| g.cluster), Some(6));
        assert!(shape_range(&face, para, 6..99, &ShapeParams::default(), |c| c).is_empty());
        // Not a char boundary.
        assert!(shape_range(&face, "سنة", 1..2, &ShapeParams::default(), |c| c).is_empty());
    }
}
