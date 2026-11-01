//! Property test over the shared corpus: every caret position of every corpus equation (linear
//! rows and OMML samples) has a finite caret rectangle inside the canvas, and clicking it comes
//! back to a caret at the same place. The same property over every template is the unit test
//! `every_caret_position_of_every_template_has_a_rectangle` in `src/math.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use equashioncraft_layout::{DEFAULT_FAMILY, EqLayout, FontDb, Rgb, layout_equation};
use equashioncraft_math::{Caret, Math, Node, children, from_linear, from_omml, seq_at, units};

fn fonts() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::bundled_only)
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn corpus() -> Vec<(String, Math, bool)> {
    let text = std::fs::read_to_string(corpus_dir().join("equations.tsv")).unwrap();
    let mut v: Vec<(String, Math, bool)> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0].to_string(), from_linear(f[1]), f[2].trim() == "1")
        })
        .collect();
    let mut files: Vec<_> = std::fs::read_dir(corpus_dir().join("omml")).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files {
        let m = from_omml(&std::fs::read_to_string(&p).unwrap());
        let display = m.para;
        v.push((p.file_name().unwrap().to_string_lossy().to_string(), m, display));
    }
    v
}

/// Every row path of a tree (the root included).
fn all_rows(seq: &[Node], path: &mut Vec<(usize, usize)>, out: &mut Vec<Vec<(usize, usize)>>) {
    out.push(path.clone());
    for (i, n) in seq.iter().enumerate() {
        for (c, k) in children(n).into_iter().enumerate() {
            path.push((i, c));
            all_rows(k, path, out);
            path.pop();
        }
    }
}

fn check(id: &str, m: &Math, l: &EqLayout) -> usize {
    let mut rows = vec![];
    all_rows(&m.body, &mut vec![], &mut rows);
    let mut n = 0;
    for path in rows {
        let Some(seq) = seq_at(&m.body, &path) else { continue };
        for pos in 0..=units(seq) {
            let c = Caret { path: path.clone(), pos };
            let r = l.caret_rect(&c).unwrap_or_else(|| panic!("{id}: no rect for {c:?}"));
            assert!(r.x0.is_finite() && r.y0.is_finite() && r.y1.is_finite() && r.y1 > r.y0, "{id}: {c:?} {r:?}");
            assert!(
                r.x0 >= -0.5 && r.x0 <= l.width() + 0.5 && r.y0 >= -0.5 && r.y1 <= l.height() + 0.5,
                "{id}: {c:?} {r:?} outside {}x{}",
                l.width(),
                l.height()
            );
            let got = l.hit(r.x0, (r.y0 + r.y1) / 2.0);
            let gr = l.caret_rect(&got).unwrap_or_else(|| panic!("{id}: hit gave {got:?} with no rect"));
            assert!((gr.x0 - r.x0).abs() < 2.0, "{id}: {c:?} -> {got:?}");
            n += 1;
        }
    }
    n
}

#[test]
fn every_caret_position_of_every_corpus_equation_has_a_finite_rect() {
    let mut total = 0;
    for (id, m, display) in corpus() {
        for size in [11.0, 24.0] {
            let l = layout_equation(fonts(), &m, DEFAULT_FAMILY, size, display, Rgb::BLACK, 4.0);
            total += check(&id, &m, &l);
        }
    }
    assert!(total > 500, "{total} caret positions checked");
}

/// Source Serif 4 and Inter lack these math symbols; the bundled STIX Two Math covers them,
/// so `bundled_only` (no system fallback) draws them from it. This used to be a pin of the gap
/// (.notdef boxes in the goldens) before STIX Two Math was bundled.
#[test]
fn symbols_the_text_faces_lack_resolve_to_stix_two_math() {
    let db = fonts();
    let regular = db.face(DEFAULT_FAMILY, "Regular");
    for c in ['\u{2207}', '\u{222E}', '\u{2308}', '\u{2309}', '\u{230A}', '\u{230B}', '\u{27E8}', '\u{27E9}'] {
        assert!(!regular.covers(c), "{c} is now in Source Serif 4: simplify this test");
        let f = db.fallback_for(c, regular.id()).unwrap_or_else(|| panic!("no face for {c}"));
        assert_eq!(f.family, "STIX Two Math", "{c}");
        assert_ne!(f.glyph_for(c), 0, "{c}");
    }
}

/// Every character of every corpus equation has a face, and no laid-out corpus equation draws a
/// .notdef glyph (glyph id 0).
#[test]
fn no_corpus_equation_draws_a_missing_glyph() {
    let db = fonts();
    let regular = db.face(DEFAULT_FAMILY, "Regular");
    let mut missing: Vec<char> = corpus()
        .iter()
        .flat_map(|(_, m, _)| equashioncraft_math::to_linear(m).chars().collect::<Vec<_>>())
        .filter(|c| !c.is_ascii() && !c.is_whitespace() && !regular.covers(*c) && db.fallback_for(*c, regular.id()).is_none())
        .collect();
    missing.sort_unstable();
    missing.dedup();
    assert_eq!(missing, Vec::<char>::new(), "characters no bundled font covers");
    for (id, m, display) in corpus() {
        let l = layout_equation(db, &m, DEFAULT_FAMILY, 24.0, display, Rgb::BLACK, 4.0);
        let notdef: usize = l
            .scene
            .items
            .iter()
            .map(|p| if let equashioncraft_layout::Prim::Glyphs { glyphs, .. } = p { glyphs.iter().filter(|g| g.0 == 0).count() } else { 0 })
            .sum();
        assert_eq!(notdef, 0, "{id}: .notdef glyphs drawn");
    }
}
