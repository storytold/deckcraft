//! OpenDocument Presentation (.odp) export: slides (size, background, hidden, transitions),
//! autoshapes and text boxes with their text, connectors, groups, pictures (with crops), tables,
//! the layout's and master's graphics, and speaker notes.
//!
//! Everything is written with explicit formatting (automatic styles resolved through the
//! placeholder and theme chain), so the file looks the same without our masters. Gradients keep
//! their first and last colours; dashes map to ODF stroke dashes; line patterns become hatches
//! and dotted patterns a blended colour; picture fills become bitmap fills. Charts (and objects
//! we only keep) are written as pictures drawn by the caller's hook; audio and video are embedded
//! as plugins; ink strokes become polylines; entrance, exit and motion-path animations become ODF
//! animation nodes (other effects fall back to a fade or are left out). Not written yet: shadows
//! and other effects, emphasis animations.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use deckcraft_color::Rgba;
use deckcraft_geom::{Rect, Xfrm};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::Fill;
use deckcraft_model::style::{Dash, GradientShape};
use deckcraft_model::table::Table;
use deckcraft_model::text::{Align, Anchor, Bullet, Caps, Paragraph, RunKind, Spacing, Strike, TextBody};
use deckcraft_model::{Geom, MediaId, Presentation, Shape, ShapeKind, Slide};

use crate::PptxError;
use crate::xml::{A, W};

pub const MIMETYPE: &str = "application/vnd.oasis.opendocument.presentation";

const NS: [(&str, &str); 13] = [
    ("xmlns:anim", "urn:oasis:names:tc:opendocument:xmlns:animation:1.0"),
    ("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
    ("xmlns:style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
    ("xmlns:text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
    ("xmlns:table", "urn:oasis:names:tc:opendocument:xmlns:table:1.0"),
    ("xmlns:draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    ("xmlns:fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
    ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
    ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
    ("xmlns:meta", "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"),
    ("xmlns:svg", "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"),
    ("xmlns:presentation", "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"),
    ("xmlns:smil", "urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0"),
];

fn root(name: &str) -> A {
    let mut a = A::new();
    for (k, v) in NS {
        a = a.a(k, v);
    }
    let _ = name;
    a.a("office:version", "1.3")
}

fn cm(pt: f64) -> String {
    format!("{:.3}cm", if pt.is_finite() { pt } else { 0.0 } / 72.0 * 2.54)
}

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Automatic styles, de-duplicated by their properties.
#[derive(Default)]
struct Styles {
    xml: String,
    seen: HashMap<String, String>,
    n: usize,
}

impl Styles {
    /// A style of `family` with `props` (the inner XML); returns its name.
    fn get(&mut self, prefix: &str, family: &str, props: String) -> String {
        let key = format!("{family}|{props}");
        if let Some(n) = self.seen.get(&key) {
            return n.clone();
        }
        self.n += 1;
        let name = format!("{prefix}{}", self.n);
        if family == "list" {
            let _ = write!(self.xml, "<text:list-style style:name=\"{name}\">{props}</text:list-style>");
        } else {
            let _ = write!(self.xml, "<style:style style:name=\"{name}\" style:family=\"{family}\">{props}</style:style>");
        }
        self.seen.insert(key, name.clone());
        name
    }
}

fn attrs(a: &[(&str, String)]) -> String {
    let mut s = String::new();
    for (k, v) in a {
        let _ = write!(s, " {k}=\"{}\"", crate::xml::esc_attr(v));
    }
    s
}

/// How a table cell looks under its table style (the renderer knows the built-in styles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellLook {
    pub fill: Option<Rgba>,
    pub text: Option<Rgba>,
    pub bold: bool,
    /// Left, right, top, bottom: colour and width in points.
    pub borders: [Option<(Rgba, f64)>; 4],
}

/// Supplies [`CellLook`]s: `(presentation, slide, table, row, column)`.
pub type CellLookFn<'a> = &'a dyn Fn(&Presentation, &Slide, &Table, usize, usize) -> Option<CellLook>;

/// Draws a shape ODF can't describe (charts, kept objects) as PNG bytes: `(presentation, slide,
/// shape)`.
pub type PictureFn<'a> = &'a dyn Fn(&Presentation, &Slide, &Shape) -> Option<Vec<u8>>;

/// What the caller can supply that this crate can't compute itself.
#[derive(Default, Clone, Copy)]
pub struct Hooks<'a> {
    pub cell_look: Option<CellLookFn<'a>>,
    pub picture: Option<PictureFn<'a>>,
}

struct Out<'a> {
    pres: &'a Presentation,
    styles: Styles,
    /// Named styles for `office:styles` in styles.xml (gradients, stroke dashes).
    named: String,
    named_seen: HashMap<String, String>,
    cell_look: Option<CellLookFn<'a>>,
    picture_hook: Option<PictureFn<'a>>,
    /// The slide being written, and its animated shapes (they get `draw:id`s).
    slide: Option<&'a Slide>,
    animated: Vec<deckcraft_model::ShapeId>,
    /// Generated pictures: (package path, PNG bytes).
    generated: Vec<(String, Vec<u8>)>,
    media: Vec<(MediaId, String)>,
    skipped: usize,
}

impl Out<'_> {
    /// A named `office:styles` element built by `f(name)`, de-duplicated by `key`.
    fn named(&mut self, prefix: &str, key: String, f: impl FnOnce(&str) -> String) -> String {
        if let Some(n) = self.named_seen.get(&key) {
            return n.clone();
        }
        let name = format!("{prefix}{}", self.named_seen.len() + 1);
        self.named.push_str(&f(&name));
        self.named_seen.insert(key, name.clone());
        name
    }
    fn picture(&mut self, id: MediaId) -> Option<String> {
        if let Some((_, n)) = self.media.iter().find(|m| m.0 == id) {
            return Some(n.clone());
        }
        let m = self.pres.media(id)?;
        let ext = match m.content_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/svg+xml" => "svg",
            "image/bmp" => "bmp",
            "image/tiff" => "tif",
            "image/webp" => "webp",
            "image/x-emf" | "image/emf" => "emf",
            "image/x-wmf" | "image/wmf" => "wmf",
            "video/mp4" => "mp4",
            "video/quicktime" => "mov",
            "video/webm" => "webm",
            "video/x-msvideo" => "avi",
            "video/x-ms-wmv" => "wmv",
            "audio/mpeg" => "mp3",
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/ogg" => "ogg",
            "audio/mp4" | "audio/x-m4a" => "m4a",
            _ => "bin",
        };
        let folder = if m.content_type.starts_with("image/") { "Pictures" } else { "Media" };
        let name = format!("{folder}/media{}.{ext}", id.0);
        self.media.push((id, name.clone()));
        Some(name)
    }
}

/// Position attributes: plain `svg:x/y` or, when rotated, `draw:transform` about the centre.
fn place(x: &Xfrm) -> Vec<(&'static str, String)> {
    let mut v = vec![("svg:width", cm(x.w)), ("svg:height", cm(x.h))];
    if x.rot.abs() < 1e-6 {
        v.push(("svg:x", cm(x.x)));
        v.push(("svg:y", cm(x.y)));
    } else {
        // ODF rotates counter-clockwise about the shape's origin, then translates.
        let a = -x.rot.to_radians();
        let (s, c) = a.sin_cos();
        let (hw, hh) = (x.w / 2.0, x.h / 2.0);
        let (rx, ry) = (hw * c + hh * s, -hw * s + hh * c);
        let (tx, ty) = (x.x + hw - rx, x.y + hh - ry);
        v.push(("draw:transform", format!("rotate ({a:.6}) translate ({} {})", cm(tx), cm(ty))));
    }
    v
}

/// A group child's box on the slide (group rotation and flips are not applied).
fn child_box(group: &Xfrm, ch: &Xfrm, c: &Xfrm) -> Xfrm {
    let a = deckcraft_geom::group_child_affine(
        &Xfrm { rot: 0.0, flip_h: false, flip_v: false, ..*group },
        Rect::new(ch.x, ch.y, ch.x + ch.w, ch.y + ch.h),
    );
    let sx = if ch.w.abs() > 1e-9 { group.w / ch.w } else { 1.0 };
    let sy = if ch.h.abs() > 1e-9 { group.h / ch.h } else { 1.0 };
    let p = a * deckcraft_geom::Point::new(c.x, c.y);
    Xfrm { x: p.x, y: p.y, w: c.w * sx, h: c.h * sy, ..*c }
}

/// PowerPoint preset → ODF `draw:type` (LibreOffice also reads `ooxml-<preset>` for the rest).
fn draw_type(name: &str) -> String {
    match name {
        "rect" => "rectangle",
        "roundRect" => "round-rectangle",
        "ellipse" => "ellipse",
        "triangle" => "isosceles-triangle",
        "rtTriangle" => "right-triangle",
        "diamond" => "diamond",
        "parallelogram" => "parallelogram",
        "trapezoid" => "trapezoid",
        "hexagon" => "hexagon",
        "octagon" => "octagon",
        "pentagon" => "pentagon",
        "star4" => "star4",
        "star5" => "star5",
        "star8" => "star8",
        "star24" => "star24",
        "rightArrow" => "right-arrow",
        "leftArrow" => "left-arrow",
        "upArrow" => "up-arrow",
        "downArrow" => "down-arrow",
        "leftRightArrow" => "left-right-arrow",
        "upDownArrow" => "up-down-arrow",
        "heart" => "heart",
        "cloud" => "cloud",
        "can" => "can",
        "cube" => "cube",
        "smileyFace" => "smiley",
        "sun" => "sun",
        "moon" => "moon",
        "lightningBolt" => "lightning",
        "donut" => "ring",
        "plus" => "cross",
        "frame" => "frame",
        "flowChartProcess" => "flowchart-process",
        "flowChartDecision" => "flowchart-decision",
        "flowChartTerminator" => "flowchart-terminator",
        other => return format!("ooxml-{other}"),
    }
    .to_string()
}

fn geometry(g: &Geom) -> String {
    match g {
        Geom::Preset { name, .. } if name == "rect" => {
            "<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"/>".into()
        }
        Geom::Preset { name, .. } => format!("<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"{}\"/>", crate::xml::esc_attr(&draw_type(name))),
        Geom::Custom { paths } => {
            let (w, h) = paths.iter().fold((1.0f64, 1.0f64), |(w, h), p| (w.max(p.w), h.max(p.h)));
            let mut d = String::new();
            for p in paths {
                let k = (if p.w > 0.0 { w / p.w } else { 1.0 }, if p.h > 0.0 { h / p.h } else { 1.0 });
                let mut xy = 0;
                for tok in p.d.split_whitespace() {
                    match tok.parse::<f64>() {
                        Ok(v) => {
                            let s = if xy % 2 == 0 { k.0 } else { k.1 };
                            let _ = write!(d, "{} ", (v * s).round());
                            xy += 1;
                        }
                        Err(_) => {
                            d.push_str(tok);
                            d.push(' ');
                            xy = 0;
                        }
                    }
                }
                if matches!(p.fill, deckcraft_geom::preset::FillMode::None) {
                    d.push_str("F ");
                }
                if !p.stroke {
                    d.push_str("S ");
                }
                d.push_str("N ");
            }
            format!(
                "<draw:enhanced-geometry svg:viewBox=\"0 0 {} {}\" draw:type=\"non-primitive\" draw:enhanced-path=\"{}\"/>",
                w.round(),
                h.round(),
                crate::xml::esc_attr(d.trim())
            )
        }
    }
}

fn fill_props(o: &mut Out, ctx: &Ctx, f: Option<&Fill>, ph: Option<Rgba>) -> Vec<(&'static str, String)> {
    let solid = |c: Rgba| {
        let mut v = vec![("draw:fill", "solid".to_string()), ("draw:fill-color", hex(c))];
        if c.a < 255 {
            v.push(("draw:opacity", format!("{:.0}%", c.a as f64 / 2.55)));
        }
        v
    };
    match f {
        Some(Fill::Solid { color }) => solid(ctx.color(color, ph)),
        Some(Fill::Gradient(g)) if g.stops.len() >= 2 => {
            let mut stops = g.stops.clone();
            stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            let (c0, c1) = (ctx.color(&stops[0].color, ph), ctx.color(&stops[stops.len() - 1].color, ph));
            let (style, extra) = match &g.shape {
                // DrawingML: degrees clockwise from left→right; ODF: tenths counter-clockwise from top→bottom.
                GradientShape::Linear { angle, .. } => {
                    ("linear", format!(" draw:angle=\"{}\"", (((90.0 - angle) % 360.0 + 360.0) % 360.0 * 10.0).round() as i64))
                }
                GradientShape::Path { path, focus } => {
                    let cx = (focus[0] + (1.0 - focus[2])) / 2.0 * 100.0;
                    let cy = (focus[1] + (1.0 - focus[3])) / 2.0 * 100.0;
                    (if path == "circle" { "radial" } else { "rectangular" }, format!(" draw:cx=\"{cx:.0}%\" draw:cy=\"{cy:.0}%\""))
                }
            };
            // A path gradient runs from its centre (DrawingML's first stop) outwards; ODF's radial
            // runs from the border (start) to the centre (end).
            let (start, end) = if style == "linear" { (c0, c1) } else { (c1, c0) };
            let key = format!("{style}{extra}{}{}", hex(start), hex(end));
            let name = o.named("Gradient_", key, |n| {
                format!(
                    "<draw:gradient draw:name=\"{n}\" draw:style=\"{style}\"{extra} draw:start-color=\"{}\" draw:end-color=\"{}\" draw:start-intensity=\"100%\" draw:end-intensity=\"100%\" draw:border=\"0%\"/>",
                    hex(start),
                    hex(end)
                )
            });
            vec![("draw:fill", "gradient".into()), ("draw:fill-gradient-name", name)]
        }
        Some(Fill::Gradient(g)) => g.stops.first().map(|s| solid(ctx.color(&s.color, ph))).unwrap_or_else(|| vec![("draw:fill", "none".into())]),
        Some(Fill::Pattern(pf)) => {
            let (fg, bg) = (ctx.color(&pf.fg, ph), ctx.color(&pf.bg, ph));
            match hatch(&pf.preset) {
                Some((style, angle, dist)) => {
                    let key = format!("{style}/{angle}/{dist}/{}", hex(fg));
                    let name = o.named("Hatch_", key, |n| {
                        format!(
                            "<draw:hatch draw:name=\"{n}\" draw:style=\"{style}\" draw:color=\"{}\" draw:distance=\"{}\" draw:rotation=\"{}\"/>",
                            hex(fg),
                            cm(dist),
                            angle * 10
                        )
                    });
                    vec![
                        ("draw:fill", "hatch".into()),
                        ("draw:fill-hatch-name", name),
                        ("draw:fill-hatch-solid", "true".into()),
                        ("draw:fill-color", hex(bg)),
                    ]
                }
                // Dotted and textured patterns: their average colour.
                None => {
                    let k = pattern_cover(&pf.preset);
                    let mix = |a: u8, b: u8| (a as f64 * k + b as f64 * (1.0 - k)).round() as u8;
                    solid(Rgba { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b), a: 255 })
                }
            }
        }
        Some(Fill::Picture(pf)) => {
            match o.picture(pf.media) {
                Some(href) => {
                    let name = o.named("Bitmap_", format!("bmp:{href}"), |n| {
                    format!("<draw:fill-image draw:name=\"{n}\" xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>", crate::xml::esc_attr(&href))
                });
                    let repeat = if matches!(pf.mode, deckcraft_model::style::PictureMode::Tile { .. }) { "repeat" } else { "stretch" };
                    vec![("draw:fill", "bitmap".into()), ("draw:fill-image-name", name), ("style:repeat", repeat.into())]
                }
                None => vec![("draw:fill", "none".into())],
            }
        }
        _ => vec![("draw:fill", "none".into())],
    }
}

/// A line pattern as an ODF hatch: (style, rotation in degrees, line distance in points).
fn hatch(preset: &str) -> Option<(&'static str, i32, f64)> {
    Some(match preset {
        "horz" => ("single", 0, 6.0),
        "ltHorz" | "narHorz" => ("single", 0, 3.0),
        "dkHorz" | "wdHorz" => ("single", 0, 4.0),
        "dashHorz" => ("single", 0, 6.0),
        "vert" => ("single", 90, 6.0),
        "ltVert" | "narVert" => ("single", 90, 3.0),
        "dkVert" | "wdVert" | "dashVert" => ("single", 90, 4.0),
        "upDiag" | "wdUpDiag" | "dashUpDiag" => ("single", 45, 5.0),
        "ltUpDiag" | "dkUpDiag" => ("single", 45, 3.0),
        "dnDiag" | "wdDnDiag" | "dashDnDiag" => ("single", 135, 5.0),
        "ltDnDiag" | "dkDnDiag" => ("single", 135, 3.0),
        "cross" | "lgGrid" => ("double", 0, 6.0),
        "smGrid" => ("double", 0, 3.0),
        "diagCross" | "openDmnd" => ("double", 45, 5.0),
        _ => return None,
    })
}

/// How much of a dotted pattern is foreground (`pct20` → 0.2).
fn pattern_cover(preset: &str) -> f64 {
    preset.strip_prefix("pct").and_then(|n| n.parse::<f64>().ok()).map(|v| v / 100.0).unwrap_or(0.5).clamp(0.0, 1.0)
}

/// A DrawingML dash as ODF `(dots1, dots1-length, dots2, dots2-length, distance)`, lengths as
/// multiples of the line width.
fn dash_parts(d: &Dash) -> Option<(u32, f64, u32, f64, f64)> {
    Some(match d {
        Dash::Preset { name } => match name.as_str() {
            "solid" => return None,
            "sysDot" => (1, 1.0, 0, 0.0, 1.0),
            "dot" => (1, 1.0, 0, 0.0, 3.0),
            "sysDash" => (1, 3.0, 0, 0.0, 1.0),
            "dash" => (1, 4.0, 0, 0.0, 3.0),
            "lgDash" => (1, 8.0, 0, 0.0, 3.0),
            "sysDashDot" => (1, 3.0, 1, 1.0, 1.0),
            "dashDot" => (1, 4.0, 1, 1.0, 3.0),
            "lgDashDot" => (1, 8.0, 1, 1.0, 3.0),
            "sysDashDotDot" => (1, 3.0, 2, 1.0, 1.0),
            "lgDashDotDot" => (1, 8.0, 2, 1.0, 3.0),
            _ => (1, 4.0, 0, 0.0, 3.0),
        },
        Dash::Custom { pattern } => {
            let (d, sp) = pattern.first().copied()?;
            (1, d.max(0.01), 0, 0.0, sp.max(0.01))
        }
    })
}

fn line_props(o: &mut Out, ctx: &Ctx, shape: &Shape) -> Vec<(&'static str, String)> {
    let (l, ph) = resolve::line(ctx, shape);
    let color = match &l.fill {
        Some(Fill::Solid { color }) => ctx.color(color, ph),
        Some(Fill::Gradient(g)) if !g.stops.is_empty() => ctx.color(&g.stops[0].color, ph),
        _ => return vec![("draw:stroke", "none".into())],
    };
    let mut v = vec![("svg:stroke-color", hex(color)), ("svg:stroke-width", cm(l.width.unwrap_or(0.75)))];
    if color.a < 255 {
        v.push(("svg:stroke-opacity", format!("{:.0}%", color.a as f64 / 2.55)));
    }
    match l.dash.as_ref().and_then(dash_parts) {
        Some((d1, l1, d2, l2, dist)) => {
            let pct = |x: f64| format!("{:.0}%", x * 100.0);
            let key = format!("{d1}/{l1}/{d2}/{l2}/{dist}");
            let name = o.named("Dash_", key, |n| {
                let two = if d2 > 0 { format!(" draw:dots2=\"{d2}\" draw:dots2-length=\"{}\"", pct(l2)) } else { String::new() };
                format!("<draw:stroke-dash draw:name=\"{n}\" draw:style=\"rect\" draw:dots1=\"{d1}\" draw:dots1-length=\"{}\"{two} draw:distance=\"{}\"/>", pct(l1), pct(dist))
            });
            v.insert(0, ("draw:stroke-dash", name));
            v.insert(0, ("draw:stroke", "dash".into()));
        }
        None => v.insert(0, ("draw:stroke", "solid".into())),
    }
    v
}

fn graphic_style(o: &mut Out, ctx: &Ctx, shape: &Shape, with_text: bool) -> String {
    let (f, ph) = resolve::fill(ctx, shape);
    let mut a = fill_props(o, ctx, f.as_ref(), ph);
    a.extend(line_props(o, ctx, shape));
    if with_text {
        let b = resolve::body(ctx, shape);
        let va = match b.anchor {
            Some(Anchor::Middle) => "middle",
            Some(Anchor::Bottom) => "bottom",
            Some(Anchor::Justified | Anchor::Distributed) => "justify",
            _ => "top",
        };
        a.push(("draw:textarea-vertical-align", va.into()));
        if b.anchor_ctr == Some(true) {
            a.push(("draw:textarea-horizontal-align", "center".into()));
        }
        a.push(("draw:auto-grow-height", "false".into()));
        a.push(("draw:auto-grow-width", "false".into()));
        a.push(("fo:wrap-option", if b.wrap == Some(false) { "no-wrap" } else { "wrap" }.into()));
        a.push(("fo:padding-left", cm(b.inset_l.unwrap_or(7.2))));
        a.push(("fo:padding-right", cm(b.inset_r.unwrap_or(7.2))));
        a.push(("fo:padding-top", cm(b.inset_t.unwrap_or(3.6))));
        a.push(("fo:padding-bottom", cm(b.inset_b.unwrap_or(3.6))));
    }
    o.styles.get("gr", "graphic", format!("<style:graphic-properties{}/>", attrs(&a)))
}

fn spacing(s: &Option<Spacing>) -> Option<String> {
    match s {
        Some(Spacing::Pts(p)) => Some(cm(*p)),
        _ => None,
    }
}

fn num_format(scheme: &str) -> (&'static str, &'static str, &'static str) {
    let fmt = if scheme.starts_with("roman") {
        if scheme.starts_with("romanUc") { "I" } else { "i" }
    } else if scheme.starts_with("alphaUc") {
        "A"
    } else if scheme.starts_with("alphaLc") {
        "a"
    } else {
        "1"
    };
    let (pre, suf) = if scheme.ends_with("ParenBoth") {
        ("(", ")")
    } else if scheme.ends_with("ParenR") {
        ("", ")")
    } else if scheme.ends_with("Plain") {
        ("", "")
    } else {
        ("", ".")
    };
    (fmt, pre, suf)
}

fn esc(s: &str) -> String {
    crate::xml::esc(s)
}

/// Text with tabs, line breaks and runs of spaces as ODF elements.
fn push_text(w: &mut String, t: &str) {
    let mut spaces = 0usize;
    let flush = |w: &mut String, n: &mut usize| {
        if *n > 0 {
            w.push(' ');
            if *n > 1 {
                let _ = write!(w, "<text:s text:c=\"{}\"/>", *n - 1);
            }
            *n = 0;
        }
    };
    let mut buf = String::new();
    for c in t.chars() {
        if c == ' ' {
            if !buf.is_empty() {
                w.push_str(&esc(&buf));
                buf.clear();
            }
            spaces += 1;
            continue;
        }
        flush(w, &mut spaces);
        match c {
            '\t' | '\u{b}' | '\n' => {
                w.push_str(&esc(&buf));
                buf.clear();
                w.push_str(if c == '\t' { "<text:tab/>" } else { "<text:line-break/>" });
            }
            c if c.is_control() => {}
            c => buf.push(c),
        }
    }
    w.push_str(&esc(&buf));
    flush(w, &mut spaces);
}

fn paragraph(o: &mut Out, ctx: &Ctx, shape: &Shape, p: &Paragraph, w: &mut String) {
    let pp = resolve::para(ctx, shape, p);
    let mut pa: Vec<(&str, String)> = vec![];
    pa.push((
        "fo:text-align",
        match pp.align {
            Some(Align::Center) => "center",
            Some(Align::Right) => "end",
            Some(Align::Left) | None => "start",
            Some(_) => "justify",
        }
        .into(),
    ));
    if let Some(v) = spacing(&pp.space_before) {
        pa.push(("fo:margin-top", v));
    }
    if let Some(v) = spacing(&pp.space_after) {
        pa.push(("fo:margin-bottom", v));
    }
    match pp.line_spacing {
        Some(Spacing::Pct(v)) => pa.push(("fo:line-height", format!("{:.0}%", v * 100.0))),
        Some(Spacing::Pts(v)) => pa.push(("style:line-height-at-least", cm(v))),
        None => {}
    }
    let bulleted = matches!(pp.bullet, Some(Bullet::Char { .. } | Bullet::AutoNum { .. }));
    if !bulleted {
        pa.push(("fo:margin-left", cm(pp.margin_left.unwrap_or(0.0))));
        pa.push(("fo:text-indent", cm(pp.indent.unwrap_or(0.0))));
    }
    let first = p.runs.iter().find(|r| matches!(r.kind, RunKind::Text)).map(|r| r.props.clone()).unwrap_or_else(|| p.end_props.clone());
    // The paragraph's own size (bullets scale with it; empty paragraphs keep their height).
    let psize = resolve::run(ctx, shape, p, &first).size.map(|v| format!("<style:text-properties fo:font-size=\"{v}pt\"/>")).unwrap_or_default();
    let pstyle = o.styles.get("P", "paragraph", format!("<style:paragraph-properties{}/>{psize}", attrs(&pa)));
    let lvl = p.level.min(8) as usize;
    if bulleted {
        let r0 = resolve::run(ctx, shape, p, &first);
        let level = lvl + 1;
        let margin = pp.margin_left.unwrap_or(0.0);
        let indent = pp.indent.unwrap_or(0.0);
        // Impress lays bullets out in the older "position" mode: the label starts at
        // space-before and the text after min-label-width.
        let align = format!(
            "<style:list-level-properties text:space-before=\"{}\" text:min-label-width=\"{}\"/>",
            cm((margin + indent).max(0.0)),
            cm((-indent).max(0.0))
        );
        let color = pp.bullet_color.as_ref().map(|c| ctx.color(c, None)).unwrap_or_else(|| resolve::text_color(ctx, &r0));
        let rel = format!("{:.0}%", pp.bullet_size.unwrap_or(1.0) * 100.0);
        let props = match &pp.bullet {
            Some(Bullet::Char { char }) => format!(
                "<text:list-level-style-bullet text:level=\"{level}\" text:bullet-char=\"{}\" text:bullet-relative-size=\"{rel}\">{align}<style:text-properties fo:color=\"{}\"{}/></text:list-level-style-bullet>",
                crate::xml::esc_attr(char.chars().next().map(String::from).as_deref().unwrap_or("•")),
                hex(color),
                pp.bullet_font.as_ref().map(|f| format!(" fo:font-family=\"{}\"", crate::xml::esc_attr(f))).unwrap_or_default()
            ),
            Some(Bullet::AutoNum { scheme, start_at }) => {
                let (fmt, pre, suf) = num_format(scheme);
                format!(
                    "<text:list-level-style-number text:level=\"{level}\" style:num-format=\"{fmt}\" style:num-prefix=\"{pre}\" style:num-suffix=\"{suf}\" text:start-value=\"{start_at}\">{align}<style:text-properties fo:color=\"{}\" fo:font-size=\"{rel}\"/></text:list-level-style-number>",
                    hex(color)
                )
            }
            _ => String::new(),
        };
        let lstyle = o.styles.get("L", "list", props);
        for i in 0..=lvl {
            if i == 0 {
                let _ = write!(w, "<text:list text:style-name=\"{lstyle}\" text:continue-numbering=\"true\">");
            } else {
                w.push_str("<text:list>");
            }
            w.push_str("<text:list-item>");
        }
    }
    let _ = write!(w, "<text:p text:style-name=\"{pstyle}\">");
    for r in &p.runs {
        if matches!(r.kind, RunKind::Break) {
            w.push_str("<text:line-break/>");
            continue;
        }
        let rp = resolve::run(ctx, shape, p, &r.props);
        let mut ta: Vec<(&str, String)> = vec![("fo:font-family", resolve::font_family(ctx, &rp))];
        if let Some(sz) = rp.size {
            ta.push(("fo:font-size", format!("{sz}pt")));
        }
        ta.push(("fo:font-weight", if rp.bold == Some(true) { "bold" } else { "normal" }.into()));
        ta.push(("fo:font-style", if rp.italic == Some(true) { "italic" } else { "normal" }.into()));
        let c = resolve::text_color(ctx, &rp);
        ta.push(("fo:color", hex(c)));
        if let Some(u) = rp.underline.as_deref().filter(|u| *u != "none") {
            ta.push(("style:text-underline-style", "solid".into()));
            ta.push(("style:text-underline-width", "auto".into()));
            ta.push(("style:text-underline-color", "font-color".into()));
            if u == "dbl" {
                ta.push(("style:text-underline-type", "double".into()));
            }
        }
        match rp.strike {
            Some(Strike::Single) => ta.push(("style:text-line-through-style", "solid".into())),
            Some(Strike::Double) => {
                ta.push(("style:text-line-through-style", "solid".into()));
                ta.push(("style:text-line-through-type", "double".into()));
            }
            _ => {}
        }
        if let Some(b) = rp.baseline.filter(|b| b.abs() > 1e-6) {
            ta.push(("style:text-position", format!("{:.0}% 58%", b * 100.0)));
        }
        match rp.caps {
            Some(Caps::All) => ta.push(("fo:text-transform", "uppercase".into())),
            Some(Caps::Small) => ta.push(("fo:font-variant", "small-caps".into())),
            _ => {}
        }
        if let Some(sp) = rp.spacing.filter(|s| s.abs() > 1e-6) {
            ta.push(("fo:letter-spacing", cm(sp)));
        }
        let tstyle = o.styles.get("T", "text", format!("<style:text-properties{}/>", attrs(&ta)));
        let _ = write!(w, "<text:span text:style-name=\"{tstyle}\">");
        let link = match rp.link.as_ref().map(|l| &l.action) {
            Some(deckcraft_model::text::Action::Url { url }) if !url.is_empty() => Some(url.clone()),
            _ => None,
        };
        if let Some(u) = &link {
            let _ = write!(w, "<text:a xlink:type=\"simple\" xlink:href=\"{}\">", crate::xml::esc_attr(u));
        }
        push_text(w, &r.text);
        if link.is_some() {
            w.push_str("</text:a>");
        }
        w.push_str("</text:span>");
    }
    w.push_str("</text:p>");
    if bulleted {
        for _ in 0..=lvl {
            w.push_str("</text:list-item></text:list>");
        }
    }
}

fn text_body(o: &mut Out, ctx: &Ctx, shape: &Shape, t: &TextBody, w: &mut String) {
    for p in &t.paragraphs {
        paragraph(o, ctx, shape, p, w);
    }
}

fn shape(o: &mut Out, ctx: &Ctx, sh: &Shape, xf: Xfrm, w: &mut String) {
    if sh.hidden {
        return;
    }
    let mut name = if sh.name.is_empty() { String::new() } else { format!(" draw:name=\"{}\"", crate::xml::esc_attr(&sh.name)) };
    if o.animated.contains(&sh.id) {
        let _ = write!(name, " draw:id=\"shape{0}\" xml:id=\"shape{0}\"", sh.id.0);
    }
    let title = |w: &mut String| {
        if !sh.title.is_empty() {
            let _ = write!(w, "<svg:title>{}</svg:title>", esc(&sh.title));
        }
        if !sh.descr.is_empty() {
            let _ = write!(w, "<svg:desc>{}</svg:desc>", esc(&sh.descr));
        }
    };
    match &sh.kind {
        ShapeKind::Shape => {
            let st = graphic_style(o, ctx, sh, true);
            let _ = write!(w, "<draw:custom-shape draw:style-name=\"{st}\"{name}{}>", attrs(&place(&xf)));
            title(w);
            if let Some(t) = &sh.text {
                text_body(o, ctx, sh, t, w);
            }
            w.push_str(&geometry(&sh.geom));
            w.push_str("</draw:custom-shape>");
        }
        ShapeKind::Picture { fill } => {
            let Some(href) = o.picture(fill.media) else {
                o.skipped += 1;
                return;
            };
            let mut a = vec![("draw:fill", "none".to_string())];
            a.extend(line_props(o, ctx, sh));
            if let Some(clip) = clip(o.pres, fill) {
                a.push(("fo:clip", clip));
            }
            let st = o.styles.get("gr", "graphic", format!("<style:graphic-properties{}/>", attrs(&a)));
            let _ = write!(w, "<draw:frame draw:style-name=\"{st}\"{name}{}>", attrs(&place(&xf)));
            let _ = write!(
                w,
                "<draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><text:p/></draw:image>",
                crate::xml::esc_attr(&href)
            );
            title(w);
            w.push_str("</draw:frame>");
        }
        ShapeKind::Connector { .. } => {
            let st = graphic_style(o, ctx, sh, false);
            let (mut x1, mut y1, mut x2, mut y2) = (xf.x, xf.y, xf.x + xf.w, xf.y + xf.h);
            if xf.flip_h {
                std::mem::swap(&mut x1, &mut x2);
            }
            if xf.flip_v {
                std::mem::swap(&mut y1, &mut y2);
            }
            let _ = write!(
                w,
                "<draw:line draw:style-name=\"{st}\"{name} svg:x1=\"{}\" svg:y1=\"{}\" svg:x2=\"{}\" svg:y2=\"{}\">",
                cm(x1),
                cm(y1),
                cm(x2),
                cm(y2)
            );
            title(w);
            w.push_str("<text:p/></draw:line>");
        }
        ShapeKind::Group { children, child } => {
            let _ = write!(w, "<draw:g{name}>");
            for c in children {
                let cx = c.xfrm.unwrap_or_else(|| resolve::xfrm(ctx, c));
                shape(o, ctx, c, child_box(&xf, child, &cx), w);
            }
            w.push_str("</draw:g>");
        }
        ShapeKind::Table(t) => table(o, ctx, sh, t, xf, &name, w),
        ShapeKind::Media(m) => {
            let Some(href) = o.picture(m.media) else {
                o.skipped += 1;
                return;
            };
            let mime = o.pres.media(m.media).map(|x| x.content_type.clone()).unwrap_or_default();
            let _ = write!(w, "<draw:frame{name}{}>", attrs(&place(&xf)));
            let _ = write!(
                w,
                "<draw:plugin xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" draw:mime-type=\"{}\"><draw:param draw:name=\"Loop\" draw:value=\"{}\"/><draw:param draw:name=\"Mute\" draw:value=\"{}\"/><draw:param draw:name=\"VolumeDB\" draw:value=\"{}\"/></draw:plugin>",
                crate::xml::esc_attr(&href),
                crate::xml::esc_attr(&mime),
                m.loop_play,
                m.volume <= 0.0,
                if m.volume > 0.0 { (20.0 * m.volume.log10()).round() as i64 } else { 0 }
            );
            title(w);
            w.push_str("</draw:frame>");
        }
        ShapeKind::Ink { strokes } => {
            let _ = write!(w, "<draw:g{name}>");
            for st in strokes.iter().filter(|s| s.points.len() >= 2) {
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for &(x, y, _) in &st.points {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
                let (bw, bh) = ((x1 - x0).max(0.01), (y1 - y0).max(0.01));
                let pts: Vec<String> =
                    st.points.iter().map(|&(x, y, _)| format!("{},{}", ((x - x0) * 100.0).round(), ((y - y0) * 100.0).round())).collect();
                let mut a = vec![
                    ("draw:fill", "none".to_string()),
                    ("draw:stroke", "solid".into()),
                    ("svg:stroke-color", hex(st.color)),
                    ("svg:stroke-width", cm(st.width)),
                    ("svg:stroke-linecap", "round".into()),
                    ("draw:stroke-linejoin", "round".into()),
                ];
                if st.highlighter || st.color.a < 255 {
                    let op = if st.highlighter { 50.0 } else { st.color.a as f64 / 2.55 };
                    a.push(("svg:stroke-opacity", format!("{op:.0}%")));
                }
                let gs = o.styles.get("gr", "graphic", format!("<style:graphic-properties{}/>", attrs(&a)));
                let _ = write!(
                    w,
                    "<draw:polyline draw:style-name=\"{gs}\" svg:x=\"{}\" svg:y=\"{}\" svg:width=\"{}\" svg:height=\"{}\" svg:viewBox=\"0 0 {} {}\" draw:points=\"{}\"><text:p/></draw:polyline>",
                    cm(x0),
                    cm(y0),
                    cm(bw),
                    cm(bh),
                    (bw * 100.0).round().max(1.0),
                    (bh * 100.0).round().max(1.0),
                    pts.join(" ")
                );
            }
            w.push_str("</draw:g>");
        }
        ShapeKind::Chart(_) | ShapeKind::Opaque { .. } | ShapeKind::Model3d { .. } => {
            let png = match (o.picture_hook, o.slide) {
                (Some(f), Some(sl)) => f(o.pres, sl, sh),
                _ => None,
            };
            let png = png.or_else(|| match &sh.kind {
                ShapeKind::Opaque { preview: Some(id), .. } | ShapeKind::Model3d { poster: Some(id), .. } => {
                    o.pres.media(*id).map(|m| m.data.as_ref().clone())
                }
                _ => None,
            });
            let Some(png) = png else {
                o.skipped += 1;
                return;
            };
            let href = format!("Pictures/object{}-{}.png", sh.id.0, o.generated.len() + 1);
            o.generated.push((href.clone(), png));
            let _ = write!(w, "<draw:frame{name}{}>", attrs(&place(&xf)));
            let _ = write!(
                w,
                "<draw:image xlink:href=\"{href}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"><text:p/></draw:image>"
            );
            title(w);
            w.push_str("</draw:frame>");
        }
    }
}

/// `fo:clip` for a cropped picture: insets measured on the picture's own size.
fn clip(p: &Presentation, f: &deckcraft_model::style::PictureFill) -> Option<String> {
    if f.crop.iter().all(|c| *c <= 1e-6) {
        return None;
    }
    let (w, h) = pixel_size(&p.media(f.media)?.data)?;
    let (w, h) = (w as f64 * 0.75, h as f64 * 0.75);
    let [l, t, r, b] = f.crop.map(|c| c.max(0.0));
    Some(format!("rect({}, {}, {}, {})", cm(t * h), cm(r * w), cm(b * h), cm(l * w)))
}

/// A PNG, JPEG or GIF's size in pixels, from its header.
fn pixel_size(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| b.get(i..i + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
    let be16 = |i: usize| b.get(i..i + 2).map(|x| u16::from_be_bytes([x[0], x[1]]) as u32);
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some((be32(16)?, be32(20)?)).filter(|s| s.0 > 0 && s.1 > 0);
    }
    if b.starts_with(b"GIF8") {
        let le16 = |i: usize| b.get(i..i + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as u32);
        return Some((le16(6)?, le16(8)?)).filter(|s| s.0 > 0 && s.1 > 0);
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                return Some((be16(i + 7)?, be16(i + 5)?)).filter(|s| s.0 > 0 && s.1 > 0);
            }
            i += 2 + be16(i + 2)? as usize;
        }
    }
    None
}

fn border(side: Option<(Rgba, f64)>) -> String {
    match side {
        Some((c, w)) if c.a > 0 && w > 0.0 => format!("{} solid {}", cm(w), hex(c)),
        _ => "none".into(),
    }
}

fn table(o: &mut Out, ctx: &Ctx, sh: &Shape, t: &Table, xf: Xfrm, name: &str, w: &mut String) {
    let _ = write!(w, "<draw:frame{name}{}><table:table>", attrs(&place(&xf)));
    for cw in &t.cols {
        let st = o.styles.get("co", "table-column", format!("<style:table-column-properties style:column-width=\"{}\"/>", cm(*cw)));
        let _ = write!(w, "<table:table-column table:style-name=\"{st}\"/>");
    }
    let slide = ctx.pres.slides.iter().find(|s| s.shapes.iter().any(|x| x.id == sh.id) || deckcraft_model::find_shape(&s.shapes, sh.id).is_some());
    for (r, row) in t.rows.iter().enumerate() {
        let rs = o.styles.get("ro", "table-row", format!("<style:table-row-properties style:row-height=\"{}\"/>", cm(row.height)));
        let _ = write!(w, "<table:table-row table:style-name=\"{rs}\">");
        for (c, cell) in row.cells.iter().enumerate() {
            if cell.h_merge || cell.v_merge {
                w.push_str("<table:covered-table-cell/>");
                continue;
            }
            let look = match (o.cell_look, slide) {
                (Some(f), Some(sl)) => f(ctx.pres, sl, t, r, c).unwrap_or_default(),
                _ => CellLook::default(),
            };
            let mut ga = match &cell.fill {
                Some(f) => fill_props(o, ctx, Some(f), None),
                None => match look.fill {
                    Some(c) if c.a > 0 => vec![("draw:fill", "solid".to_string()), ("draw:fill-color", hex(c))],
                    _ => vec![("draw:fill", "none".to_string())],
                },
            };
            let [ml, mr, mt, mb] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
            ga.push(("fo:padding-left", cm(ml)));
            ga.push(("fo:padding-right", cm(mr)));
            ga.push(("fo:padding-top", cm(mt)));
            ga.push(("fo:padding-bottom", cm(mb)));
            let va = match cell.anchor {
                Some(Anchor::Middle) => "middle",
                Some(Anchor::Bottom) => "bottom",
                _ => "top",
            };
            ga.push(("draw:textarea-vertical-align", va.into()));
            let mut sides = look.borders;
            for (k, b) in cell.borders.iter().enumerate() {
                if let Some(l) = b {
                    sides[k] = match &l.fill {
                        Some(Fill::Solid { color }) => Some((ctx.color(color, None), l.width.unwrap_or(0.75))),
                        _ => None,
                    };
                }
            }
            let pa = [
                ("fo:border-left", border(sides[0])),
                ("fo:border-right", border(sides[1])),
                ("fo:border-top", border(sides[2])),
                ("fo:border-bottom", border(sides[3])),
            ];
            let cs =
                o.styles.get("ce", "table-cell", format!("<style:graphic-properties{}/><style:paragraph-properties{}/>", attrs(&ga), attrs(&pa)));
            let span = if cell.grid_span > 1 || cell.row_span > 1 {
                format!(" table:number-columns-spanned=\"{}\" table:number-rows-spanned=\"{}\"", cell.grid_span.max(1), cell.row_span.max(1))
            } else {
                String::new()
            };
            let _ = write!(w, "<table:table-cell table:style-name=\"{cs}\"{span}>");
            // The table style's text colour and boldness apply where the runs don't say.
            let mut body = cell.text.clone();
            for para in &mut body.paragraphs {
                for run in &mut para.runs {
                    if run.props.fill.is_none()
                        && let Some(c) = look.text
                    {
                        run.props.fill = Some(Fill::solid(deckcraft_model::style::ColorRef::rgb(c)));
                    }
                    if run.props.bold.is_none() && look.bold {
                        run.props.bold = Some(true);
                    }
                }
            }
            if body.paragraphs.is_empty() {
                w.push_str("<text:p/>");
            }
            text_body(o, ctx, sh, &body, w);
            w.push_str("</table:table-cell>");
        }
        w.push_str("</table:table-row>");
    }
    w.push_str("</table:table></draw:frame>");
}

/// What an animation becomes in ODF: (preset class, preset id, smil transition type/subtype).
fn anim_kind(a: &deckcraft_model::anim::Animation) -> Option<(&'static str, String, Option<(&'static str, &'static str)>)> {
    use deckcraft_model::anim::AnimClass;
    let filter = match a.effect.as_str() {
        "appear" | "disappear" => None,
        "wipe" => Some(("barWipe", if matches!(a.option.as_str(), "u" | "d" | "fromTop" | "fromBottom") { "topToBottom" } else { "leftToRight" })),
        "split" => Some(("barnDoorWipe", "vertical")),
        "wheel" => Some(("pinWheelWipe", "oneBlade")),
        "circle" | "shape" => Some(("ellipseWipe", "circle")),
        "dissolve" | "dissolveIn" | "dissolveOut" => Some(("dissolve", "defaultSubtype")),
        "checkerboard" => Some(("checkerBoardWipe", "across")),
        "blinds" => Some(("blindsWipe", "vertical")),
        "randomBars" => Some(("randomBarWipe", "vertical")),
        _ => Some(("fade", "crossfade")),
    };
    match a.class {
        AnimClass::Entrance => Some(("entrance", format!("ooo-entrance-{}", if filter.is_none() { "appear" } else { "fade-in" }), filter)),
        AnimClass::Exit => Some(("exit", format!("ooo-exit-{}", if filter.is_none() { "disappear" } else { "fade-out" }), filter)),
        AnimClass::Path => a.path.as_ref().map(|_| ("motion-path", "ooo-motionpath-custom".to_string(), None)),
        _ => None,
    }
}

fn secs(ms: u32) -> String {
    format!("{}.{:03}s", ms / 1000, ms % 1000)
}

/// The slide's entrance, exit and motion-path effects as an ODF main sequence: one `par` per
/// click, then the effects starting with it (with previous) or after it.
fn animations(sl: &Slide, w: &mut String) {
    use deckcraft_model::anim::AnimStart;
    let list: Vec<_> = sl.animations.iter().filter(|a| a.trigger.is_none()).filter_map(|a| anim_kind(a).map(|k| (a, k))).collect();
    if list.is_empty() {
        return;
    }
    w.push_str("<anim:par presentation:node-type=\"timing-root\"><anim:seq presentation:node-type=\"main-sequence\">");
    let mut open_click = false;
    let mut open_group = false;
    let mut group_start = 0u32;
    let mut group_end = 0u32;
    for (i, (a, (class, preset, filter))) in list.iter().enumerate() {
        let new_click = i == 0 || a.start == AnimStart::OnClick;
        if new_click {
            if open_group {
                w.push_str("</anim:par>");
            }
            if open_click {
                w.push_str("</anim:par>");
            }
            let begin = if a.start == AnimStart::OnClick { "indefinite" } else { "0s" };
            let _ = write!(w, "<anim:par smil:begin=\"{begin}\" smil:fill=\"hold\">");
            open_click = true;
            open_group = false;
            group_end = 0;
        }
        if new_click || a.start == AnimStart::AfterPrevious {
            if open_group {
                w.push_str("</anim:par>");
            }
            group_start = if new_click { 0 } else { group_end };
            let _ = write!(w, "<anim:par smil:begin=\"{}\" smil:fill=\"hold\">", secs(group_start));
            open_group = true;
        }
        let node = match a.start {
            AnimStart::OnClick => "on-click",
            AnimStart::WithPrevious => "with-previous",
            AnimStart::AfterPrevious => "after-previous",
        };
        let dur = a.duration_ms.max(1);
        group_end = group_end.max(group_start + a.delay_ms + dur);
        let target = format!("shape{}", a.shape.0);
        let _ = write!(
            w,
            "<anim:par smil:begin=\"{}\" smil:fill=\"hold\" presentation:node-type=\"{node}\" presentation:preset-class=\"{class}\" presentation:preset-id=\"{preset}\">",
            secs(a.delay_ms)
        );
        match *class {
            "entrance" => {
                let _ = write!(
                    w,
                    "<anim:set smil:begin=\"0s\" smil:dur=\"0.001s\" smil:fill=\"hold\" smil:targetElement=\"{target}\" smil:attributeName=\"visibility\" smil:to=\"visible\"/>"
                );
                if let Some((ty, sub)) = filter {
                    let _ = write!(
                        w,
                        "<anim:transitionFilter smil:dur=\"{}\" smil:targetElement=\"{target}\" smil:type=\"{ty}\" smil:subtype=\"{sub}\"/>",
                        secs(dur)
                    );
                }
            }
            "exit" => {
                if let Some((ty, sub)) = filter {
                    let _ = write!(
                        w,
                        "<anim:transitionFilter smil:dur=\"{}\" smil:targetElement=\"{target}\" smil:type=\"{ty}\" smil:subtype=\"{sub}\" smil:mode=\"out\"/>",
                        secs(dur)
                    );
                }
                let end = if filter.is_some() { dur } else { 0 };
                let _ = write!(
                    w,
                    "<anim:set smil:begin=\"{}\" smil:dur=\"0.001s\" smil:fill=\"hold\" smil:targetElement=\"{target}\" smil:attributeName=\"visibility\" smil:to=\"hidden\"/>",
                    secs(end)
                );
            }
            _ => {
                let path: String = a.path.as_deref().unwrap_or("").split_whitespace().filter(|t| *t != "E").collect::<Vec<_>>().join(" ");
                let _ = write!(
                    w,
                    "<anim:animateMotion smil:dur=\"{}\" smil:fill=\"hold\" smil:targetElement=\"{target}\" svg:path=\"{}\"/>",
                    secs(dur),
                    crate::xml::esc_attr(&path)
                );
            }
        }
        w.push_str("</anim:par>");
    }
    if open_group {
        w.push_str("</anim:par>");
    }
    if open_click {
        w.push_str("</anim:par>");
    }
    w.push_str("</anim:seq></anim:par>");
}

fn package(files: Vec<(String, Vec<u8>, bool)>) -> Result<Vec<u8>, PptxError> {
    let err = |e: &dyn std::fmt::Display| PptxError::Write(e.to_string());
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // `mimetype` first and uncompressed, as ODF requires.
    zw.start_file("mimetype", stored).map_err(|e| err(&e))?;
    zw.write_all(MIMETYPE.as_bytes()).map_err(|e| err(&e))?;
    for (name, data, compress) in &files {
        zw.start_file(name.as_str(), if *compress { deflate } else { stored }).map_err(|e| err(&e))?;
        zw.write_all(data).map_err(|e| err(&e))?;
    }
    Ok(zw.finish().map_err(|e| err(&e))?.into_inner())
}

/// Write `p` as an OpenDocument presentation.
pub fn export_odp(p: &Presentation) -> Result<Vec<u8>, PptxError> {
    export_odp_with(p, Hooks::default())
}

/// ODF `smil:type`, `smil:subtype` and whether it runs in reverse, for a transition.
fn smil(t: &deckcraft_model::anim::Transition) -> Option<(&'static str, &'static str, bool)> {
    let o = t.option.as_str();
    // DrawingML directions say where the slide moves to; ODF names where it comes from.
    let from = match o {
        "l" => "fromRight",
        "u" => "fromBottom",
        "d" => "fromTop",
        "lu" => "fromBottomRight",
        "ru" => "fromBottomLeft",
        "ld" => "fromTopRight",
        "rd" => "fromTopLeft",
        _ => "fromLeft",
    };
    let vert = matches!(o, "vert" | "u" | "d");
    Some(match t.kind.as_str() {
        "none" | "" | "cut" => return None,
        "fade" if o == "black" || o == "thruBlk" => ("fade", "fadeOverColor", false),
        "push" => ("pushWipe", from, false),
        "cover" => ("slideWipe", from, false),
        "uncover" | "pull" => ("slideWipe", from, true),
        "wipe" => ("barWipe", if vert { "topToBottom" } else { "leftToRight" }, matches!(o, "l" | "u")),
        "split" => ("barnDoorWipe", if o.starts_with("horz") || o == "horz" { "horizontal" } else { "vertical" }, o.ends_with("in") || o == "in"),
        "randomBar" => ("randomBarWipe", if vert { "horizontal" } else { "vertical" }, false),
        "dissolve" => ("dissolve", "defaultSubtype", false),
        "checker" => ("checkerBoardWipe", if vert { "down" } else { "across" }, false),
        "blinds" => ("blindsWipe", if vert { "horizontal" } else { "vertical" }, false),
        "clock" => ("clockWipe", "clockwiseTwelve", false),
        "wheel" => ("pinWheelWipe", "oneBlade", false),
        "circle" => ("ellipseWipe", "circle", o == "in"),
        "diamond" => ("irisWipe", "diamond", o == "in"),
        "zoom" | "shape" => ("irisWipe", "rectangle", o == "in"),
        "strips" => ("waterfallWipe", "horizontalLeft", false),
        _ => ("fade", "crossfade", false),
    })
}

/// Write `p` as an OpenDocument presentation. `hooks.cell_look` gives table cells their table
/// style's look and `hooks.picture` draws charts (the engine passes the renderer's); without
/// them only explicit cell formatting is written and charts are left out.
pub fn export_odp_with(p: &Presentation, hooks: Hooks) -> Result<Vec<u8>, PptxError> {
    let mut o = Out {
        pres: p,
        styles: Styles::default(),
        named: String::new(),
        named_seen: HashMap::new(),
        cell_look: hooks.cell_look,
        picture_hook: hooks.picture,
        slide: None,
        animated: vec![],
        generated: vec![],
        media: vec![],
        skipped: 0,
    };
    let mut body = String::new();
    for (i, sl) in p.slides.iter().enumerate() {
        let Some(ctx) = Ctx::for_slide(p, sl) else { continue };
        o.slide = Some(sl);
        o.animated = sl.animations.iter().filter(|a| anim_kind(a).is_some()).map(|a| a.shape).collect();
        let (bg, ph) = resolve::background(&ctx, Some(sl));
        let mut a = fill_props(&mut o, &ctx, Some(&bg), ph);
        if let Some(t) = &sl.transition {
            if let Some((ty, sub, rev)) = smil(t) {
                a.push(("smil:type", ty.into()));
                a.push(("smil:subtype", sub.into()));
                if rev {
                    a.push(("smil:direction", "reverse".into()));
                }
                if sub == "fadeOverColor" {
                    a.push(("smil:fadeColor", "#000000".into()));
                }
                a.push((
                    "presentation:transition-speed",
                    if t.duration_ms <= 500 {
                        "fast"
                    } else if t.duration_ms <= 1000 {
                        "medium"
                    } else {
                        "slow"
                    }
                    .into(),
                ));
            }
            if let Some(ms) = t.advance_after_ms {
                a.push(("presentation:transition-type", "automatic".into()));
                a.push(("presentation:duration", format!("PT{}.{:03}S", ms / 1000, ms % 1000)));
            }
        }
        if sl.hidden {
            a.push(("presentation:visibility", "hidden".into()));
        }
        a.push(("presentation:background-visible", "true".into()));
        a.push(("presentation:background-objects-visible", "true".into()));
        let dp = o.styles.get("dp", "drawing-page", format!("<style:drawing-page-properties{}/>", attrs(&a)));
        let _ = write!(body, "<draw:page draw:name=\"page{}\" draw:style-name=\"{dp}\" draw:master-page-name=\"Default\">", i + 1);
        // The master's and layout's own graphics (not their placeholders), behind the slide's.
        let (show_layout, show_master) = resolve::show_master_shapes(sl, ctx.layout);
        let behind =
            show_master.then_some(ctx.master.shapes.as_slice()).into_iter().chain(ctx.layout.filter(|_| show_layout).map(|l| l.shapes.as_slice()));
        for list in behind {
            for sh in list.iter().filter(|x| x.ph.is_none()) {
                let xf = sh.xfrm.unwrap_or_default();
                shape(&mut o, &ctx, sh, xf, &mut body);
            }
        }
        for sh in &sl.shapes {
            let xf = resolve::xfrm(&ctx, sh);
            shape(&mut o, &ctx, sh, xf, &mut body);
        }
        if sl.notes.paragraphs.iter().any(|q| !q.is_empty()) {
            let ns = p.notes_size;
            let (nw, nh) = if ns.width > 0.0 && ns.height > 0.0 { (ns.width, ns.height) } else { (540.0, 720.0) };
            let _ = write!(
                body,
                "<presentation:notes><draw:page-thumbnail presentation:class=\"page\" draw:page-number=\"{}\" svg:x=\"{}\" svg:y=\"{}\" svg:width=\"{}\" svg:height=\"{}\"/>",
                i + 1,
                cm(nw * 0.125),
                cm(nh * 0.075),
                cm(nw * 0.75),
                cm(nh * 0.375)
            );
            let _ = write!(
                body,
                "<draw:frame presentation:class=\"notes\" svg:x=\"{}\" svg:y=\"{}\" svg:width=\"{}\" svg:height=\"{}\"><draw:text-box>",
                cm(nw * 0.1),
                cm(nh * 0.48),
                cm(nw * 0.8),
                cm(nh * 0.42)
            );
            for q in &sl.notes.paragraphs {
                body.push_str("<text:p>");
                for r in &q.runs {
                    if matches!(r.kind, RunKind::Break) {
                        body.push_str("<text:line-break/>");
                    } else {
                        push_text(&mut body, &r.text);
                    }
                }
                body.push_str("</text:p>");
            }
            body.push_str("</draw:text-box></draw:frame></presentation:notes>");
        }
        animations(sl, &mut body);
        body.push_str("</draw:page>");
    }

    let mut content = W::new();
    content.open("office:document-content", root("content"));
    content.raw("<office:automatic-styles>");
    content.raw(&o.styles.xml);
    content.raw("</office:automatic-styles><office:body><office:presentation>");
    content.raw(&body);
    content.raw("</office:presentation></office:body>");
    content.close("office:document-content");

    let size = p.slide_size;
    let mut styles = W::new();
    styles.open("office:document-styles", root("styles"));
    styles.raw(&format!(
        "<office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"0cm\" fo:margin-bottom=\"0cm\" fo:margin-left=\"0cm\" fo:margin-right=\"0cm\" fo:page-width=\"{}\" fo:page-height=\"{}\" style:print-orientation=\"{}\"/></style:page-layout></office:automatic-styles>",
        cm(size.width),
        cm(size.height),
        if size.width >= size.height { "landscape" } else { "portrait" }
    ));
    if !o.named.is_empty() {
        styles.raw("<office:styles>");
        styles.raw(&o.named);
        styles.raw("</office:styles>");
    }
    styles.raw("<office:master-styles><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles>");
    styles.close("office:document-styles");

    let mut meta = W::new();
    meta.open("office:document-meta", root("meta"));
    meta.raw("<office:meta><meta:generator>DeckCraft</meta:generator>");
    if !p.props.title.is_empty() {
        meta.raw(&format!("<dc:title>{}</dc:title>", esc(&p.props.title)));
    }
    meta.raw("</office:meta>");
    meta.close("office:document-meta");

    let mut files: Vec<(String, Vec<u8>, bool)> =
        vec![("content.xml".into(), content.finish(), true), ("styles.xml".into(), styles.finish(), true), ("meta.xml".into(), meta.finish(), true)];
    let mut manifest = W::new();
    manifest
        .open("manifest:manifest", A::new().a("xmlns:manifest", "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0").a("manifest:version", "1.3"));
    manifest.empty("manifest:file-entry", A::new().a("manifest:full-path", "/").a("manifest:version", "1.3").a("manifest:media-type", MIMETYPE));
    for f in ["content.xml", "styles.xml", "meta.xml"] {
        manifest.empty("manifest:file-entry", A::new().a("manifest:full-path", f).a("manifest:media-type", "text/xml"));
    }
    for (id, name) in &o.media {
        let Some(m) = p.media(*id) else { continue };
        manifest.empty("manifest:file-entry", A::new().a("manifest:full-path", name).a("manifest:media-type", &m.content_type));
        files.push((name.clone(), m.data.as_ref().clone(), false));
    }
    for (name, data) in &o.generated {
        manifest.empty("manifest:file-entry", A::new().a("manifest:full-path", name).a("manifest:media-type", "image/png"));
        files.push((name.clone(), data.clone(), false));
    }
    manifest.close("manifest:manifest");
    files.push(("META-INF/manifest.xml".into(), manifest.finish(), true));
    package(files)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Read;

    fn read(zip: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn rotation_keeps_the_centre() {
        let x = Xfrm { x: 100.0, y: 50.0, w: 200.0, h: 100.0, rot: 90.0, ..Default::default() };
        let t = place(&x).into_iter().find(|a| a.0 == "draw:transform").unwrap().1;
        // 90° clockwise about the centre (200, 100): the origin corner lands at (250, 0).
        assert!(t.starts_with("rotate (-1.570796) translate (8.819cm 0.000cm)"), "{t}");
    }

    #[test]
    fn writes_a_valid_package() {
        let mut p = deckcraft_model::defaults::new_presentation(None);
        let png = crate::png::transparent_1x1();
        let media = p.add_media("dot.png", "image/png", png).unwrap();
        let sl = std::sync::Arc::make_mut(&mut p.slides[0]);
        if let Some(t) = sl.shapes.iter_mut().find(|s| s.ph_type().is_some_and(|k| k.is_title())) {
            t.text = Some(TextBody::from_text("Hello & <ODF>"));
        }
        sl.shapes.push(Shape {
            id: deckcraft_model::ShapeId(500),
            xfrm: Some(Xfrm::new(10.0, 10.0, 50.0, 50.0)),
            kind: ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, ..Default::default() } },
            ..Default::default()
        });
        sl.notes = TextBody::from_text("Say  hi");
        let bytes = export_odp(&p).unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        {
            let first = z.by_index(0).unwrap();
            assert_eq!(first.name(), "mimetype");
            assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        }
        let content = read(&bytes, "content.xml");
        crate::xml::parse(content.as_bytes()).unwrap();
        crate::xml::parse(read(&bytes, "styles.xml").as_bytes()).unwrap();
        crate::xml::parse(read(&bytes, "META-INF/manifest.xml").as_bytes()).unwrap();
        assert!(content.contains("Hello &amp; &lt;ODF&gt;"), "{content}");
        assert!(content.contains("<draw:image xlink:href=\"Pictures/media"));
        assert!(content.contains("<presentation:notes>") && content.contains("Say <text:s text:c=\"1\"/>hi"));
        assert!(read(&bytes, "META-INF/manifest.xml").contains("image/png"));
        assert!(read(&bytes, "styles.xml").contains("fo:page-width=\"33.867cm\""));
    }

    #[test]
    fn master_graphics_transitions_and_crops() {
        let mut p = deckcraft_model::defaults::new_presentation(None);
        let master = std::sync::Arc::make_mut(&mut p.masters[0]);
        master.shapes.push(Shape {
            id: deckcraft_model::ShapeId(900),
            name: "Logo bar".into(),
            xfrm: Some(Xfrm::new(0.0, 500.0, 960.0, 40.0)),
            ..Default::default()
        });
        let png = crate::png::transparent_1x1();
        assert_eq!(pixel_size(&png), Some((1, 1)));
        let media = p.add_media("dot.png", "image/png", png).unwrap();
        let sl = std::sync::Arc::make_mut(&mut p.slides[0]);
        sl.transition = Some(deckcraft_model::anim::Transition {
            kind: "wipe".into(),
            option: "d".into(),
            duration_ms: 2000,
            advance_after_ms: Some(3500),
            ..Default::default()
        });
        sl.shapes.push(Shape {
            id: deckcraft_model::ShapeId(501),
            xfrm: Some(Xfrm::new(10.0, 10.0, 50.0, 50.0)),
            kind: ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, crop: [0.5, 0.0, 0.0, 0.0], ..Default::default() } },
            ..Default::default()
        });
        let bytes = export_odp(&p).unwrap();
        let content = read(&bytes, "content.xml");
        assert!(content.contains("draw:name=\"Logo bar\""), "the master's graphic is drawn on the slide");
        assert!(content.contains("smil:type=\"barWipe\" smil:subtype=\"topToBottom\""), "{content}");
        assert!(content.contains("presentation:transition-speed=\"slow\"") && content.contains("presentation:duration=\"PT3.500S\""));
        assert!(content.contains("fo:clip=\"rect(0.000cm, 0.000cm, 0.000cm, 0.013cm)\""), "half of a 0.75 pt picture");
        let sl = std::sync::Arc::make_mut(&mut p.slides[0]);
        sl.show_master_shapes = false;
        assert!(!read(&export_odp(&p).unwrap(), "content.xml").contains("Logo bar"));
    }

    #[test]
    fn ink_media_and_picture_fills() {
        use deckcraft_model::{InkStroke, MediaClip, ShapeId};
        let mut p = deckcraft_model::defaults::new_presentation(None);
        let png = p.add_media("dot.png", "image/png", crate::png::transparent_1x1()).unwrap();
        let clip = p.add_media("clip.mp4", "video/mp4", vec![0; 8]).unwrap();
        let sl = std::sync::Arc::make_mut(&mut p.slides[0]);
        sl.shapes.push(Shape {
            id: ShapeId(601),
            xfrm: Some(Xfrm::new(10.0, 10.0, 100.0, 100.0)),
            kind: ShapeKind::Ink {
                strokes: vec![InkStroke { points: vec![(10.0, 10.0, 1.0), (60.0, 80.0, 1.0)], width: 3.0, highlighter: true, ..Default::default() }],
            },
            ..Default::default()
        });
        sl.shapes.push(Shape {
            id: ShapeId(602),
            xfrm: Some(Xfrm::new(200.0, 10.0, 160.0, 90.0)),
            kind: ShapeKind::Media(MediaClip { media: clip, video: true, loop_play: true, volume: 0.5, ..Default::default() }),
            ..Default::default()
        });
        sl.shapes.push(Shape {
            id: ShapeId(603),
            xfrm: Some(Xfrm::new(400.0, 10.0, 100.0, 100.0)),
            fill: Some(deckcraft_model::style::Fill::Picture(deckcraft_model::style::PictureFill { media: png, ..Default::default() })),
            ..Default::default()
        });
        let bytes = export_odp(&p).unwrap();
        let content = read(&bytes, "content.xml");
        let styles = read(&bytes, "styles.xml");
        assert!(content.contains("<draw:polyline ") && content.contains("svg:stroke-opacity=\"50%\""), "{content}");
        assert!(
            content.contains("<draw:plugin ") && content.contains("Media/") && content.contains("draw:name=\"Loop\" draw:value=\"true\""),
            "{content}"
        );
        assert!(styles.contains("<draw:fill-image ") && content.contains("draw:fill=\"bitmap\""), "{styles}");
        assert!(read(&bytes, "META-INF/manifest.xml").contains("video/mp4"));
    }

    #[test]
    fn dashes_map_to_odf_parts() {
        assert_eq!(dash_parts(&Dash::Preset { name: "solid".into() }), None);
        assert_eq!(dash_parts(&Dash::Preset { name: "lgDashDotDot".into() }), Some((1, 8.0, 2, 1.0, 3.0)));
    }
}
