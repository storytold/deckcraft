//! Equation tests ported from WordCraft `crates/layout/src/tests.rs`. Upstream they put an
//! equation into a paragraph and inspected the paragraph layout; here they call
//! [`layout_equation`] and [`layout_math_wrapped`] directly. The paragraph-only checks
//! (centring a display equation on its line, justification of the line before it, line padding)
//! have no counterpart without a paragraph layout and were not ported (see PROVENANCE.md).

use std::sync::OnceLock;

use equashion_math::{Caret, Math, from_linear, from_omml};

use super::*;

fn fonts() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::bundled_only)
}

/// WordCraft's default body text size, points.
const BODY: f64 = 11.0;

fn eq(m: &Math, display: bool) -> EqLayout {
    layout_equation(fonts(), m, DEFAULT_FAMILY, BODY, display, Rgb::BLACK, 0.0)
}

#[test]
fn an_equation_is_taller_than_text_and_draws_glyphs_a_bar_and_a_radical() {
    // Upstream: inline_equation_is_one_char_cluster_taller_than_text.
    let plain = eq(&from_linear("ab"), false);
    let l = eq(&from_linear("x^2/sqrt(y)"), false);
    assert!(l.width() > 10.0, "{}", l.width());
    assert!(l.height() > plain.height(), "a fraction-with-root is taller than plain text");
    let items = &l.scene.items;
    assert!(items.iter().any(|i| matches!(i, Prim::Glyphs { .. })), "glyphs");
    assert!(items.iter().any(|i| matches!(i, Prim::Rule(..))), "fraction bar");
    assert!(items.iter().any(|i| matches!(i, Prim::Path(..))), "radical");
    // Carets before and after the equation are its width apart.
    let end = equashion_math::units(&from_linear("x^2/sqrt(y)").body);
    let (a, b) = (l.caret_rect(&Caret { path: vec![], pos: 0 }).unwrap(), l.caret_rect(&Caret { path: vec![], pos: end }).unwrap());
    assert!((b.x0 - a.x0 - l.width()).abs() < 0.01, "{a:?} {b:?} {}", l.width());
}

#[test]
fn equations_from_broken_omml_or_hostile_text_lay_out() {
    // Upstream: equations_from_omml_or_hostile_text_lay_out.
    let broken = from_omml("<m:oMath xmlns:m=\"x\"><m:f/></m:oMath>");
    for m in [broken, from_linear(""), from_linear("((((((((("), from_linear(&"x^".repeat(5000)), from_linear("\u{0}\u{1}\u{FFFE}")] {
        for display in [false, true] {
            let l = eq(&m, display);
            assert!(l.width().is_finite() && l.height().is_finite() && l.height() > 0.0);
            assert!(l.caret_rect(&Caret::default()).is_some());
        }
    }
}

#[test]
fn a_long_inline_equation_breaks_at_operators_within_the_line() {
    // Upstream: long_inline_equation_breaks_at_operators_within_the_line (a 6.5in line).
    let line = 468.0;
    let m = from_linear(&["abc"; 200].join("+"));
    let b = layout_math_wrapped(fonts(), &m, DEFAULT_FAMILY, BODY, false, false, line);
    assert!(b.width() <= line + 0.5, "{} wider than the line", b.width());
    assert!(b.ascent() + b.descent() > 4.0 * BODY, "several rows tall");
    assert!(b.last_line_width().is_some_and(|w| w > 0.0 && w <= b.width()));
    // Every glyph of the scene stays inside the line.
    let mut items = vec![];
    emit(&b, 0.0, b.ascent(), Rgb::BLACK, &mut items);
    for it in &items {
        if let Prim::Glyphs { glyphs, .. } = it {
            for g in glyphs {
                assert!(f64::from(g.1) < line, "glyph at {} beyond the line", g.1);
            }
        }
    }
    // An equation with no operator to break at still wraps between its items.
    let m2 = from_linear(&"x^2 ".repeat(300));
    let b2 = layout_math_wrapped(fonts(), &m2, DEFAULT_FAMILY, BODY, false, false, line);
    assert!(b2.width() < 500.0, "{}", b2.width());
}

#[test]
fn a_display_equation_is_laid_out_in_display_style() {
    // What remains of display_equation_line_has_room_above_and_below without a paragraph:
    // display style makes a fraction taller than inline style does.
    let m = from_linear("x/y");
    let (inline, display) = (eq(&m, false), eq(&m, true));
    assert!(display.height() > inline.height(), "{} vs {}", display.height(), inline.height());
}

#[test]
fn the_default_family_is_used_and_named() {
    let l = eq(&from_linear("x+1"), false);
    let faces: Vec<_> = l
        .scene
        .items
        .iter()
        .filter_map(|i| if let Prim::Glyphs { font, .. } = i { fonts().face_by_id(*font) } else { None })
        .map(|f| f.family.clone())
        .collect();
    assert!(!faces.is_empty());
    assert!(faces.iter().all(|f| f == DEFAULT_FAMILY), "{faces:?}");
}
