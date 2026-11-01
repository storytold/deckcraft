//! Golden tests: every row of `corpus/equations.tsv` and every `corpus/omml/*.xml` sample is laid
//! out with the bundled fonts only and compared with `tests/golden/<id>.svg` (the SVG backend)
//! and `tests/golden/<id>.json` (metrics: width, ascent, descent, rows).
//!
//! Rewrite the files with `EQUASHIONCRAFT_BLESS=1 cargo test -p equashioncraft-render` (or
//! `cargo xtask bless`) after an intended change, and review the diff.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use equashioncraft_layout::{DEFAULT_FAMILY, FontDb, Rgb, layout_equation_wrapped, layout_math_wrapped};
use equashioncraft_math::{Math, from_linear, from_omml};
use equashioncraft_render::{fmt2, render_scene, scene_to_svg};

/// The family every golden is made with. Pinned here (not just inherited from
/// `DEFAULT_FAMILY`) so a change of default shows up as a test failure, not as new goldens.
const FAMILY: &str = "Source Serif 4";
/// Font size, points.
const SIZE: f64 = 24.0;
/// Padding around the equation, points.
const PAD: f64 = 4.0;
/// Line width, points: long equations wrap, unbreakable ones shrink.
const LINE: f64 = 400.0;

fn root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}

fn golden_dir() -> PathBuf {
    root().join("tests/golden")
}

/// (id, equation, display) for every corpus item.
fn corpus() -> Vec<(String, Math, bool)> {
    let text = std::fs::read_to_string(root().join("corpus/equations.tsv")).expect("corpus/equations.tsv");
    let mut v: Vec<(String, Math, bool)> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 3, "malformed row {l:?}");
            (f[0].to_string(), from_linear(f[1]), f[2].trim() == "1")
        })
        .collect();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("corpus/omml")).expect("corpus/omml").flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "xml")) {
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let m = from_omml(&std::fs::read_to_string(&p).unwrap());
        let display = m.para;
        v.push((format!("omml_{stem}"), m, display));
    }
    v
}

fn fonts() -> FontDb {
    FontDb::bundled_only()
}

fn metrics_json(db: &FontDb, m: &Math, display: bool, rows: usize) -> String {
    let b = layout_math_wrapped(db, m, FAMILY, SIZE, false, display, LINE);
    let mut s = String::from("{\n");
    let _ = writeln!(s, "  \"family\": \"{FAMILY}\",");
    let _ = writeln!(s, "  \"size\": {},", fmt2(SIZE));
    let _ = writeln!(s, "  \"line\": {},", fmt2(LINE));
    let _ = writeln!(s, "  \"display\": {display},");
    let _ = writeln!(s, "  \"width\": {},", fmt2(b.width()));
    let _ = writeln!(s, "  \"ascent\": {},", fmt2(b.ascent()));
    let _ = writeln!(s, "  \"descent\": {},", fmt2(b.descent()));
    let last = b.last_line_width().map_or("null".to_string(), fmt2);
    let _ = writeln!(s, "  \"last_line_width\": {last},");
    let _ = writeln!(s, "  \"rows\": {rows}");
    s.push_str("}\n");
    s
}

/// Compare `got` with the file at `path` (or write it when blessing). Returns a message on
/// mismatch.
fn compare(path: &Path, got: &str, bless: bool) -> Option<String> {
    if bless {
        if std::fs::read_to_string(path).ok().as_deref() != Some(got) {
            std::fs::write(path, got).unwrap();
        }
        return None;
    }
    let Ok(want) = std::fs::read_to_string(path) else {
        return Some(format!("{}: missing (run `cargo xtask bless`)", path.display()));
    };
    if want == got {
        return None;
    }
    let line = want.lines().zip(got.lines()).position(|(a, b)| a != b).unwrap_or_else(|| want.lines().count().min(got.lines().count()));
    let w = want.lines().nth(line).unwrap_or("<end>");
    let g = got.lines().nth(line).unwrap_or("<end>");
    let cut = |s: &str| s.chars().take(160).collect::<String>();
    Some(format!("{}: differs at line {}\n    want: {}\n    got:  {}", path.display(), line + 1, cut(w), cut(g)))
}

#[test]
fn goldens_match() {
    assert_eq!(FAMILY, DEFAULT_FAMILY, "the goldens pin the default family");
    let bless = std::env::var_os("EQUASHIONCRAFT_BLESS").is_some_and(|v| v != "0" && !v.is_empty());
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let db = fonts();
    let mut failures = vec![];
    let mut expected: BTreeSet<String> = BTreeSet::new();
    let items = corpus();
    assert!(items.len() >= 40, "{} corpus items", items.len());
    for (id, m, display) in &items {
        let l = layout_equation_wrapped(&db, m, FAMILY, SIZE, *display, Rgb::BLACK, PAD, LINE);
        // Every face the scene uses is a bundled one: the pinned family, Inter, or STIX Two Math
        // (the fallback for symbols the text faces lack), and no glyph is .notdef.
        for p in &l.scene.items {
            if let equashioncraft_layout::Prim::Glyphs { font, glyphs, .. } = p {
                let f = db.face_by_id(*font).unwrap();
                assert!(f.family == FAMILY || f.family == "Inter" || f.family == "STIX Two Math", "{id}: {f:?}");
                assert!(glyphs.iter().all(|g| g.0 != 0), "{id}: .notdef glyph in {f:?}");
            }
        }
        // The raster backend draws the same scene without trouble.
        let r = render_scene(&l.scene, &db, 1.0, Some(Rgb::WHITE));
        assert!(r.width >= 1 && r.height >= 1, "{id}");
        let svg = scene_to_svg(&l.scene, &db, FAMILY);
        let json = metrics_json(&db, m, *display, l.rows());
        for (ext, text) in [("svg", svg), ("json", json)] {
            let name = format!("{id}.{ext}");
            expected.insert(name.clone());
            failures.extend(compare(&dir.join(&name), &text, bless));
        }
    }
    // Golden files with no corpus item.
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if (name.ends_with(".svg") || name.ends_with(".json")) && !expected.contains(&name) {
            if bless {
                std::fs::remove_file(e.path()).unwrap();
            } else {
                failures.push(format!("{}: stale (no corpus item; `cargo xtask bless` removes it)", e.path().display()));
            }
        }
    }
    assert!(failures.is_empty(), "{} golden mismatches:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn goldens_are_deterministic() {
    // Two databases, two layouts: the same SVG (face ids differ between databases, the output
    // must not).
    let (a, b) = (fonts(), fonts());
    for (id, m, display) in corpus().iter().take(12) {
        let la = layout_equation_wrapped(&a, m, FAMILY, SIZE, *display, Rgb::BLACK, PAD, LINE);
        let lb = layout_equation_wrapped(&b, m, FAMILY, SIZE, *display, Rgb::BLACK, PAD, LINE);
        assert_eq!(scene_to_svg(&la.scene, &a, FAMILY), scene_to_svg(&lb.scene, &b, FAMILY), "{id}");
    }
}
