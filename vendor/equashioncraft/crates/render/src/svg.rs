//! A [`Scene`] as an SVG document: every glyph, rule and shape as a filled path in absolute
//! coordinates (points), numbers printed to 2 decimals. Deterministic for a given scene and font
//! database, so it serves as the golden-file format.

use std::fmt::Write as _;

use equashioncraft_layout::{MathFonts, Prim, Rgb, Scene};
use kurbo::{BezPath, PathEl};

use crate::{SYNTH_BOLD_STROKE, glyph_transform};

/// `v` with 2 decimals; never `-0.00`, and `0.00` for a non-finite value.
pub fn fmt2(v: f64) -> String {
    if !v.is_finite() {
        return "0.00".into();
    }
    let s = format!("{v:.2}");
    if s == "-0.00" { "0.00".into() } else { s }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn path_d(p: &BezPath) -> String {
    let mut d = String::new();
    let pt = |d: &mut String, x: f64, y: f64| {
        let _ = write!(d, "{} {}", fmt2(x), fmt2(y));
    };
    for el in p.elements() {
        if !d.is_empty() {
            d.push(' ');
        }
        match *el {
            PathEl::MoveTo(a) => {
                d.push('M');
                pt(&mut d, a.x, a.y);
            }
            PathEl::LineTo(a) => {
                d.push('L');
                pt(&mut d, a.x, a.y);
            }
            PathEl::QuadTo(a, b) => {
                d.push('Q');
                pt(&mut d, a.x, a.y);
                d.push(' ');
                pt(&mut d, b.x, b.y);
            }
            PathEl::CurveTo(a, b, c) => {
                d.push('C');
                pt(&mut d, a.x, a.y);
                d.push(' ');
                pt(&mut d, b.x, b.y);
                d.push(' ');
                pt(&mut d, c.x, c.y);
            }
            PathEl::ClosePath => d.push('Z'),
        }
    }
    d
}

/// Escape text for an XML attribute value.
fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The scene as a standalone SVG document. `family` is recorded on the root element (as
/// `data-family`) so a golden file names the font it was made with; pass `""` to leave it out.
pub fn scene_to_svg(scene: &Scene, fonts: &dyn MathFonts, family: &str) -> String {
    let (w, h) = (fmt2(scene.width), fmt2(scene.height));
    let mut out = String::new();
    let fam = if family.is_empty() { String::new() } else { format!(" data-family=\"{}\"", attr(family)) };
    let _ = writeln!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"{fam}>");
    for p in &scene.items {
        match p {
            Prim::Rule(r, c) => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    fmt2(r.x0),
                    fmt2(r.y0),
                    fmt2(r.width()),
                    fmt2(r.height()),
                    hex(*c)
                );
            }
            Prim::Path(path, c) => {
                let _ = writeln!(out, "<path d=\"{}\" fill=\"{}\"/>", path_d(path), hex(*c));
            }
            Prim::Glyphs { font, size, glyphs, color, synth_bold, synth_italic } => {
                let Some(face) = fonts.face_by_id(*font) else { continue };
                let stroke = if *synth_bold {
                    format!(" stroke=\"{}\" stroke-width=\"{}\"", hex(*color), fmt2(*size as f64 * SYNTH_BOLD_STROKE))
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "<g fill=\"{}\"{stroke} data-face=\"{} {}\" data-size=\"{}\">",
                    hex(*color),
                    attr(&face.family),
                    attr(&face.style),
                    fmt2(*size as f64)
                );
                for (gid, x, y) in glyphs {
                    let o = fonts.outline(&face, *gid);
                    if o.elements().is_empty() {
                        continue;
                    }
                    let mut q = (*o).clone();
                    q.apply_affine(glyph_transform(*x as f64, *y as f64, *size as f64, face.upem, *synth_italic));
                    let _ = writeln!(out, "<path data-gid=\"{gid}\" d=\"{}\"/>", path_d(&q));
                }
                out.push_str("</g>\n");
            }
        }
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use equashioncraft_layout::{DEFAULT_FAMILY, FontDb, layout_equation};
    use equashioncraft_math::from_linear;

    #[test]
    fn numbers_have_two_decimals_and_no_negative_zero() {
        assert_eq!(fmt2(1.0), "1.00");
        assert_eq!(fmt2(-0.001), "0.00");
        assert_eq!(fmt2(2.345), "2.35");
        assert_eq!(fmt2(f64::NAN), "0.00");
    }

    #[test]
    fn an_equation_becomes_paths_and_rects() {
        let db = FontDb::bundled_only();
        let l = layout_equation(&db, &from_linear("a/b+sqrt(x)"), DEFAULT_FAMILY, 20.0, false, Rgb::BLACK, 2.0);
        let svg = scene_to_svg(&l.scene, &db, DEFAULT_FAMILY);
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("data-family=\"Source Serif 4\""));
        assert!(svg.contains("<rect "), "fraction bar");
        assert!(svg.contains("data-gid="), "glyphs");
        assert!(svg.trim_end().ends_with("</svg>"));
        // The same scene gives the same text.
        assert_eq!(svg, scene_to_svg(&l.scene, &db, DEFAULT_FAMILY));
    }
}
