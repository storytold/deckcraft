//! ODF geometry and affine transforms, mapped to model objects rather than rasterised.
use crate::{OdpError, Result, read::length, xml::El};
use deckcraft_geom::{Affine, BezPath, PathEl, Point, Vec2, preset};
use deckcraft_model::{CustomPath, Geom, Xfrm};

#[path = "read_path.rs"]
mod enhanced;
#[path = "read_formula.rs"]
mod formula;

pub fn number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|n| n.is_finite() && n.abs() < 1.0e9)
}

pub fn numbers(s: &str) -> Result<Vec<f64>> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| number(s).ok_or_else(|| OdpError("invalid geometry number".into())))
        .collect()
}

// ODF 1.3 §19.220: one area applies to every writing mode; a second
// supplies the vertical area. The current model has no custom relative text
// rectangle, so only explicitly full-frame rectangles can be omitted safely.
fn validate_text_areas(geometry: &El, formulas: &formula::Formulas<'_>, bounds: [f64; 4]) -> Result<()> {
    let Some(areas) = geometry.attr("draw:text-areas") else { return Ok(()) };
    let expanded = formulas.substitute(areas)?;
    let coordinates = expanded
        .split_whitespace()
        .take(9)
        .map(|value| number(value).ok_or_else(|| OdpError("invalid enhanced text area coordinate".into())))
        .collect::<Result<Vec<_>>>()?;
    let [x, y, width, height] = bounds;
    let expected = [x, y, x + width, y + height];
    if !matches!(coordinates.len(), 4 | 8) || coordinates.chunks_exact(4).any(|area| area != expected) {
        return Err(OdpError("custom enhanced text areas require source recovery".into()));
    }
    Ok(())
}

pub fn read_with_style(el: &El, w: f64, h: f64, has_fill: bool, has_stroke: bool) -> Result<Geom> {
    // Equation logical units precede draw:transform, which may already have been
    // decomposed into the model's physical shape dimensions by the caller.
    let w = el.attr("svg:width").and_then(length).unwrap_or(w);
    let h = el.attr("svg:height").and_then(length).unwrap_or(h);
    let name = match el.name.as_str() {
        "draw:ellipse" | "draw:circle" => "ellipse",
        "draw:line" | "draw:connector" => "line",
        "draw:custom-shape" => {
            let geometry = el.child("draw:enhanced-geometry").ok_or_else(|| OdpError("custom shape has no enhanced geometry".into()))?;
            let kind = geometry.attr("draw:type").unwrap_or("non-primitive");
            let kind = kind.strip_prefix("ooxml-").unwrap_or(kind);
            let mapped = match kind {
                "rectangle" => "rect",
                "round-rectangle" => "roundRect",
                "circle" => "ellipse",
                "isosceles-triangle" => "triangle",
                "right-triangle" => "rtTriangle",
                "parallelogram" => "parallelogram",
                "trapezoid" => "trapezoid",
                "diamond" => "diamond",
                "pentagon" => "pentagon",
                "hexagon" => "hexagon",
                "octagon" => "octagon",
                "cross" => "plus",
                "ring" => "donut",
                "star5" => "star5",
                "right-arrow" => "rightArrow",
                "left-arrow" => "leftArrow",
                "up-arrow" => "upArrow",
                "down-arrow" => "downArrow",
                "left-right-arrow" => "leftRightArrow",
                "up-down-arrow" => "upDownArrow",
                other => other,
            };
            if geometry.attr("draw:enhanced-path").is_some() || geometry.attr("draw:text-areas").is_some() {
                let bounds = numbers(geometry.attr("svg:viewBox").unwrap_or("0 0 21600 21600"))?;
                let [bx, by, bw, bh] = bounds.as_slice() else {
                    return Err(OdpError("invalid enhanced viewBox".into()));
                };
                // Zero viewBox extents select the shape's logical size in 1/100 mm.
                let bw = if *bw == 0.0 { w * 2540.0 / 72.0 } else { *bw };
                let bh = if *bh == 0.0 { h * 2540.0 / 72.0 } else { *bh };
                if bw <= 0.0 || bh <= 0.0 {
                    return Err(OdpError("invalid enhanced geometry dimensions".into()));
                }
                // ODF stretch points require cutting/moving fragments, not uniform scaling.
                // Keep such source geometry recoverable until the model can express it.
                if (geometry.attr("draw:path-stretchpoint-x").is_some() && w / h > bw / bh + 1e-10)
                    || (geometry.attr("draw:path-stretchpoint-y").is_some() && w / h < bw / bh - 1e-10)
                {
                    return Err(OdpError("enhanced geometry stretch points require source recovery".into()));
                }
                let formulas = formula::Formulas::new(geometry, w, h, [*bx, *by, bw, bh], has_fill, has_stroke)?;
                validate_text_areas(geometry, &formulas, [*bx, *by, bw, bh])?;
                if let Some(d) = geometry.attr("draw:enhanced-path") {
                    let d = formulas.substitute(d)?;
                    let paths = enhanced::read(&d, [*bx, *by, bw, bh])?;
                    return Ok(Geom::Custom { paths });
                }
            }
            if preset::info(mapped).is_none() {
                return Err(OdpError(format!("custom geometry '{kind}' has no path")));
            }
            let adj = geometry.attr("draw:modifiers").map(numbers).transpose()?.unwrap_or_default();
            if geometry.attr("draw:text-areas").is_some() {
                // A full-frame ODF override is not redundant for presets whose native
                // text box is inset (ellipse, diamond, rounded rectangles, ...).
                let native = preset::build(mapped, w, h, &adj).ok_or_else(|| OdpError("missing preset text geometry".into()))?;
                if native.text_rect != deckcraft_geom::Rect::new(0.0, 0.0, w, h) {
                    return Err(OdpError("explicit preset text area requires editable geometry ownership".into()));
                }
            }
            return Ok(Geom::Preset { name: mapped.into(), adj });
        }
        "draw:path" | "draw:polygon" | "draw:polyline" => {
            let bounds = el.attr("svg:viewBox").map(numbers).transpose()?.unwrap_or_else(|| vec![0.0, 0.0, w, h]);
            let [bx, by, bw, bh] = bounds.as_slice() else {
                return Err(OdpError("invalid viewBox".into()));
            };
            if *bw <= 0.0 || *bh <= 0.0 {
                return Err(OdpError("invalid path dimensions".into()));
            }
            let path = if el.name == "draw:path" {
                BezPath::from_svg(el.attr("svg:d").unwrap_or("")).map_err(|e| OdpError(format!("invalid SVG path: {e}")))?
            } else {
                let pts = numbers(el.attr("draw:points").unwrap_or(""))?;
                if pts.len() % 2 != 0 {
                    return Err(OdpError("invalid polygon points".into()));
                }
                let mut path = BezPath::new();
                for (i, pair) in pts.chunks_exact(2).enumerate() {
                    if let [x, y] = pair {
                        if i == 0 {
                            path.move_to((*x, *y));
                        } else {
                            path.line_to((*x, *y));
                        }
                    }
                }
                if el.name == "draw:polygon" {
                    path.close_path();
                }
                path
            };
            let path = Affine::translate(Vec2::new(-bx, -by)) * path;
            return Ok(Geom::Custom { paths: vec![CustomPath { w: *bw, h: *bh, d: model_path(&path)?, ..Default::default() }] });
        }
        "draw:rect" if el.attr("draw:corner-radius").and_then(length).is_some_and(|r| r > 0.0) => "roundRect",
        _ => "rect",
    };
    Ok(Geom::preset(name))
}

/// The model's path grammar requires whitespace between commands and their coordinates.
/// SVG's compact serialization is valid interchange XML but is not a model path.
fn model_path(path: &BezPath) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    for element in path.elements() {
        let finite = |p: &Point| p.x.is_finite() && p.y.is_finite();
        let valid = match element {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => finite(p),
            PathEl::QuadTo(a, b) => finite(a) && finite(b),
            PathEl::CurveTo(a, b, c) => finite(a) && finite(b) && finite(c),
            PathEl::ClosePath => true,
        };
        if !valid {
            return Err(OdpError("non-finite path coordinate".into()));
        }
        match element {
            PathEl::MoveTo(p) => write!(&mut out, "M {} {} ", p.x, p.y),
            PathEl::LineTo(p) => write!(&mut out, "L {} {} ", p.x, p.y),
            PathEl::QuadTo(a, b) => write!(&mut out, "Q {} {} {} {} ", a.x, a.y, b.x, b.y),
            PathEl::CurveTo(a, b, c) => write!(&mut out, "C {} {} {} {} {} {} ", a.x, a.y, b.x, b.y, c.x, c.y),
            PathEl::ClosePath => write!(&mut out, "Z "),
        }
        .map_err(|_| OdpError("cannot serialize model path".into()))?;
        if out.len() > 256 * 1024 {
            return Err(OdpError("expanded model path exceeds size limit".into()));
        }
    }
    Ok(out.trim_end().to_string())
}

/// ODF transform definitions are applied in their listed order.
pub fn transform(s: &str) -> Result<Affine> {
    transform_list(s, false)
}

/// SVG 1.1 §7.6: angles are degrees and list matrices postmultiply.
/// ODF draw:transform has its own listed-order convention, so keep these separate.
pub fn svg_transform(s: &str) -> Result<Affine> {
    transform_list(s, true)
}

fn transform_list(s: &str, svg: bool) -> Result<Affine> {
    let mut rest = s.trim();
    let mut out = Affine::IDENTITY;
    let mut count = 0;
    while !rest.is_empty() {
        count += 1;
        if count > 64 {
            return Err(OdpError("too many transforms".into()));
        }
        let (name, after) = rest.split_once('(').ok_or_else(|| OdpError("invalid transform".into()))?;
        let (args, after) = after.split_once(')').ok_or_else(|| OdpError("unclosed transform".into()))?;
        let values: Vec<_> = args.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        let n = |i: usize| values.get(i).copied().and_then(number).ok_or_else(|| OdpError("invalid transform number".into()));
        let pt = |i: usize| {
            values.get(i).copied().and_then(|s| length(s).or_else(|| number(s))).ok_or_else(|| OdpError("invalid transform length".into()))
        };
        let affine = match name.trim() {
            "matrix" if values.len() == 6 => Affine::new([n(0)?, n(1)?, n(2)?, n(3)?, pt(4)?, pt(5)?]),
            "translate" if (1..=2).contains(&values.len()) => Affine::translate((pt(0)?, if values.len() == 2 { pt(1)? } else { 0.0 })),
            "scale" if (1..=2).contains(&values.len()) => Affine::scale_non_uniform(n(0)?, if values.len() == 2 { n(1)? } else { n(0)? }),
            "rotate" if values.len() == 1 => Affine::rotate(if svg { n(0)?.to_radians() } else { n(0)? }),
            "rotate" if svg && values.len() == 3 => {
                let center = Vec2::new(n(1)?, n(2)?);
                Affine::translate(center) * Affine::rotate(n(0)?.to_radians()) * Affine::translate(-center)
            }
            "skewX" if svg && values.len() == 1 => Affine::new([1.0, 0.0, n(0)?.to_radians().tan(), 1.0, 0.0, 0.0]),
            "skewY" if svg && values.len() == 1 => Affine::new([1.0, n(0)?.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            _ => return Err(OdpError(format!("transform '{}' is not representable yet", name.trim()))),
        };
        out = if svg { out * affine } else { affine * out };
        rest = after.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    }
    if !out.as_coeffs().iter().all(|n| n.is_finite() && n.abs() <= 1e12) {
        return Err(OdpError("non-finite transform".into()));
    }
    Ok(out)
}

pub fn decompose(matrix: Affine, w: f64, h: f64) -> Result<Xfrm> {
    let [a, b, c, d, _, _] = matrix.as_coeffs();
    let sx = a.hypot(b);
    let sy = c.hypot(d);
    if sx < 1.0e-12 || sy < 1.0e-12 || (a * c + b * d).abs() > 1.0e-7 * sx * sy {
        return Err(OdpError("singular or skewed geometry cannot be represented yet".into()));
    }
    let center = matrix * Point::new(w / 2.0, h / 2.0);
    let mut x = Xfrm::new(center.x - w * sx / 2.0, center.y - h * sy / 2.0, w * sx, h * sy);
    x.rot = b.atan2(a).to_degrees();
    x.flip_v = a * d - b * c < 0.0;
    if !x.is_finite() || x.w > 1.0e7 || x.h > 1.0e7 {
        return Err(OdpError("transformed geometry exceeds limits".into()));
    }
    Ok(x)
}

pub fn matrix_attr(a: Affine) -> String {
    let [a, b, c, d, e, f] = a.as_coeffs();
    format!("matrix({a} {b} {c} {d} {e}pt {f}pt)")
}
