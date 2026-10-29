//! Equations (`equashion_math::Math`) laid out into positioned shapes and rules.
//!
//! The layout is equaSHion's ([`equashion_layout`]), the same engine the equation editor draws
//! with, so what is typed in place is what the slide shows. It runs in equaSHion's bundled
//! equation fonts, not the slide's text face: an equation's math faces and the editor's are one.
//!
//! The result is a [`MathBox`]: a width, an ascent and a descent around a baseline, plus rules
//! and filled shapes in box-local coordinates (x right, y down, baseline at y = 0). The paragraph
//! shaper treats the box as one inline atom; [`emit`] turns it into `Deco`s. Glyphs are filled
//! outlines (the faces are not the deck's): they draw, export and animate like any shape.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use deckcraft_color::Rgba;
use deckcraft_geom::Rect;
use equashion_layout::{DEFAULT_FAMILY, FontDb, MathFonts, Prim, Rgb};
use equashion_math::{Math, from_omml};
use kurbo::{Affine, BezPath, Shape as _};

use crate::{Deco, DecoLine, GlyphRun};

/// Cached equation layouts.
const CACHE_MAX: usize = 256;
/// Slant of a synthetic italic, and stroke of a synthetic bold (em), as equaSHion's renderer.
const SYNTH_ITALIC_SKEW: f64 = -0.21;
const SYNTH_BOLD_STROKE: f64 = 0.03;

#[derive(Clone)]
pub(crate) enum Item {
    Rule(Rect),
    /// A filled shape: a glyph outline, radical sign, delimiter or big operator.
    Path(BezPath),
}

/// A laid-out equation (or part of one).
#[derive(Clone, Default)]
pub(crate) struct MathBox {
    pub w: f64,
    pub asc: f64,
    pub desc: f64,
    /// Laid out in display style (a displayed equation, or alone in its paragraph).
    pub display: bool,
    pub items: Vec<Item>,
}

impl MathBox {
    /// Number of rules (tests).
    #[cfg(test)]
    pub(crate) fn rules(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Rule(_))).count()
    }
    /// Number of filled shapes (tests).
    #[cfg(test)]
    pub(crate) fn paths(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Path(_))).count()
    }
}

/// equaSHion's bundled fonts: no system fonts, so an equation lays out the same everywhere.
fn fonts() -> &'static FontDb {
    static DB: LazyLock<FontDb> = LazyLock::new(FontDb::bundled_only);
    &DB
}

/// The scene primitives of an equation as shapes.
fn items_of(prims: Vec<Prim>, fonts: &dyn MathFonts) -> Vec<Item> {
    let mut out = Vec::with_capacity(prims.len());
    for p in prims {
        match p {
            Prim::Rule(r, _) => out.push(Item::Rule(Rect::new(r.x0, r.y0, r.x1, r.y1))),
            Prim::Path(p, _) => out.push(Item::Path(p)),
            Prim::Glyphs { font, size, glyphs, synth_bold, synth_italic, .. } => {
                let Some(face) = fonts.face_by_id(font) else { continue };
                let k = f64::from(size) / face.units_per_em().max(1.0);
                let skew = if synth_italic { Affine::new([1.0, 0.0, SYNTH_ITALIC_SKEW, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                for (gid, x, y) in glyphs {
                    let o = fonts.outline(&face, gid);
                    if o.elements().is_empty() {
                        continue;
                    }
                    let place = Affine::translate((f64::from(x), f64::from(y))) * skew * Affine::scale(k);
                    let path = (*o).clone();
                    let mut placed = place * &path;
                    if synth_bold {
                        // Fill and stroke are one shape: the stroke outline joins the fill.
                        let w = face.units_per_em() * SYNTH_BOLD_STROKE;
                        let stroked = kurbo::stroke(path.iter(), &kurbo::Stroke::new(w), &kurbo::StrokeOpts::default(), 0.1);
                        placed.extend((place * &stroked).iter());
                    }
                    out.push(Item::Path(placed));
                }
            }
        }
    }
    out
}

/// Lay out an equation at `size` points (bold when `bold`; `display` for a displayed formula).
pub(crate) fn layout_math(m: &Math, size: f64, bold: bool, display: bool) -> MathBox {
    let f = fonts();
    let b = equashion_layout::layout_math(f, m, DEFAULT_FAMILY, size, bold, display);
    let mut prims = Vec::new();
    equashion_layout::emit(&b, 0.0, 0.0, Rgb::BLACK, &mut prims);
    MathBox { w: b.width(), asc: b.ascent(), desc: b.descent(), display, items: items_of(prims, f) }
}

type Key = (String, u64, bool, bool);
static CACHE: Mutex<Option<HashMap<Key, Arc<MathBox>>>> = Mutex::new(None);

/// The laid-out box for an OMML equation, cached by (source, size, weight).
pub(crate) fn layout_math_cached(omml: &str, size: f64, bold: bool, solo: bool) -> Arc<MathBox> {
    let key: Key = (omml.to_string(), size.to_bits(), bold, solo);
    if let Ok(g) = CACHE.lock()
        && let Some(hit) = g.as_ref().and_then(|c| c.get(&key))
    {
        return hit.clone();
    }
    let m = from_omml(omml);
    let b = Arc::new(layout_math(&m, size, bold, m.para || solo));
    if let Ok(mut g) = CACHE.lock() {
        let c = g.get_or_insert_with(HashMap::new);
        if c.len() >= CACHE_MAX {
            c.clear();
        }
        c.insert(key, b.clone());
    }
    b
}

/// Number of cached equation layouts (tests).
#[cfg(test)]
pub(crate) fn cache_len() -> usize {
    CACHE.lock().ok().and_then(|g| g.as_ref().map(|c| c.len())).unwrap_or(0)
}

/// Emit a box with its origin at (x, baseline) as rule and shape decorations.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit(
    b: &MathBox,
    x: f64,
    baseline: f64,
    color: Rgba,
    _alpha: f64,
    para: usize,
    _chars: (usize, usize),
    _runs: &mut Vec<GlyphRun>,
    decos: &mut Vec<Deco>,
) {
    for it in &b.items {
        match it {
            Item::Rule(r) => decos.push(Deco {
                rect: Rect::new(x + r.x0, baseline + r.y0, x + r.x1, baseline + r.y1),
                color,
                behind: false,
                para,
                line: DecoLine::Solid,
                path: None,
            }),
            Item::Path(p) => {
                let mut q = p.clone();
                q.apply_affine(Affine::translate((x, baseline)));
                let bb = q.bounding_box();
                decos.push(Deco {
                    rect: Rect::new(bb.x0, bb.y0, bb.x1, bb.y1),
                    color,
                    behind: false,
                    para,
                    line: DecoLine::Solid,
                    path: Some(Arc::new(q)),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use equashion_math::from_linear;

    use super::*;

    fn lay(lin: &str, display: bool) -> MathBox {
        layout_math(&from_linear(lin), 20.0, false, display)
    }

    #[test]
    fn plain_text_has_width_and_height() {
        let b = lay("xy", false);
        assert!(b.w > 0.0 && b.asc > 0.0);
        assert!(b.paths() >= 1, "glyphs are shapes");
    }

    #[test]
    fn fraction_stacks_and_has_a_rule() {
        let f = lay("a/b", false);
        let flat = lay("ab", false);
        assert!(f.rules() >= 1, "fraction bar");
        assert!(f.asc + f.desc > flat.asc + flat.desc);
    }

    #[test]
    fn scripts_raise_and_lower() {
        let base = lay("x", false);
        let sup = lay("x^2", false);
        let sub = lay("x_2", false);
        assert!(sup.w > base.w && sup.asc > base.asc);
        assert!(sub.w > base.w && sub.desc > base.desc);
    }

    #[test]
    fn root_grows_with_content() {
        let r = lay("sqrt(x)", false);
        let tall = lay("sqrt(a/b)", false);
        assert!(r.paths() >= 2, "radical sign and the glyph");
        assert!(tall.asc + tall.desc > r.asc + r.desc);
    }

    #[test]
    fn display_sum_puts_limits_above_and_below() {
        let m = from_linear("sum_(i=1)^n i");
        let disp = layout_math(&m, 20.0, false, true);
        let text = layout_math(&m, 20.0, false, false);
        assert!(disp.asc + disp.desc > text.asc + text.desc, "display limits stack");
    }

    #[test]
    fn matrix_is_taller_than_one_row() {
        let m = lay("[a,b;c,d]", false);
        let one = lay("[a,b]", false);
        assert!(m.asc + m.desc > one.asc + one.desc);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let s = format!("{}x{}", "(".repeat(5000), ")".repeat(5000));
        let b = lay(&s, false);
        assert!(b.w.is_finite() && b.asc.is_finite());
        let b = lay(&"x^".repeat(3000), false);
        assert!(b.w.is_finite());
    }

    #[test]
    fn all_extents_are_finite_for_every_template() {
        for t in equashion_math::templates() {
            let b = layout_math(&t.math, 18.0, false, true);
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite(), "{}", t.id);
            assert!(b.w >= 0.0 && b.asc >= 0.0 && b.desc >= 0.0, "{}", t.id);
        }
        for t in equashion_math::galleries().templates() {
            let b = layout_math(&t.math, 18.0, false, true);
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite(), "{}", t.id);
        }
    }

    #[test]
    fn layout_cache_returns_the_same_box() {
        let o = equashion_math::to_omml(&from_linear("a^2+b^2"));
        let a = layout_math_cached(&o, 24.0, false, false);
        let b = layout_math_cached(&o, 24.0, false, false);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(cache_len() >= 1);
    }

    #[test]
    fn emit_places_shapes_relative_to_origin() {
        let b = lay("a/b", false);
        let (mut runs, mut decos) = (vec![], vec![]);
        emit(&b, 100.0, 50.0, Rgba::BLACK, 1.0, 0, (0, 3), &mut runs, &mut decos);
        assert!(runs.is_empty(), "the equation fonts are not the deck's faces: shapes only");
        assert!(!decos.is_empty());
        assert!(decos.iter().all(|d| d.rect.x0 >= 100.0 - 1e-9));
    }
}
