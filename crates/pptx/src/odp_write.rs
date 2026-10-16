//! OpenDocument Presentation (.odp) export, first slice: slides (size, background, hidden),
//! autoshapes and text boxes with their text, connectors, groups, pictures, and speaker notes.
//!
//! Everything is written with explicit formatting (automatic styles resolved through the
//! placeholder and theme chain), so the file looks the same without our masters. Not written
//! yet: tables, charts, media, ink, master/layout graphics, gradients/patterns (first colour),
//! dashes (solid), shadows and other effects, picture crops, transitions and animations.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use deckcraft_color::Rgba;
use deckcraft_geom::{Rect, Xfrm};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::Fill;
use deckcraft_model::text::{Align, Anchor, Bullet, Caps, Paragraph, RunKind, Spacing, Strike, TextBody};
use deckcraft_model::{Geom, MediaId, Presentation, Shape, ShapeKind};

use crate::PptxError;
use crate::xml::{A, W};

pub const MIMETYPE: &str = "application/vnd.oasis.opendocument.presentation";

const NS: [(&str, &str); 12] = [
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

struct Out<'a> {
    pres: &'a Presentation,
    styles: Styles,
    media: Vec<(MediaId, String)>,
    skipped: usize,
}

impl Out<'_> {
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
            _ => "bin",
        };
        let name = format!("Pictures/media{}.{ext}", id.0);
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

fn fill_props(ctx: &Ctx, f: Option<&Fill>, ph: Option<Rgba>) -> Vec<(&'static str, String)> {
    let solid = |c: Rgba| {
        let mut v = vec![("draw:fill", "solid".to_string()), ("draw:fill-color", hex(c))];
        if c.a < 255 {
            v.push(("draw:opacity", format!("{:.0}%", c.a as f64 / 2.55)));
        }
        v
    };
    match f {
        Some(Fill::Solid { color }) => solid(ctx.color(color, ph)),
        Some(Fill::Gradient(g)) => g.stops.first().map(|s| solid(ctx.color(&s.color, ph))).unwrap_or_else(|| vec![("draw:fill", "none".into())]),
        _ => vec![("draw:fill", "none".into())],
    }
}

fn line_props(ctx: &Ctx, shape: &Shape) -> Vec<(&'static str, String)> {
    let (l, ph) = resolve::line(ctx, shape);
    match &l.fill {
        Some(Fill::Solid { color }) => {
            vec![("draw:stroke", "solid".into()), ("svg:stroke-color", hex(ctx.color(color, ph))), ("svg:stroke-width", cm(l.width.unwrap_or(0.75)))]
        }
        Some(Fill::Gradient(g)) if !g.stops.is_empty() => vec![
            ("draw:stroke", "solid".into()),
            ("svg:stroke-color", hex(ctx.color(&g.stops[0].color, ph))),
            ("svg:stroke-width", cm(l.width.unwrap_or(0.75))),
        ],
        _ => vec![("draw:stroke", "none".into())],
    }
}

fn graphic_style(o: &mut Out, ctx: &Ctx, shape: &Shape, with_text: bool) -> String {
    let (f, ph) = resolve::fill(ctx, shape);
    let mut a = fill_props(ctx, f.as_ref(), ph);
    a.extend(line_props(ctx, shape));
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
    let name = if sh.name.is_empty() { String::new() } else { format!(" draw:name=\"{}\"", crate::xml::esc_attr(&sh.name)) };
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
            a.extend(line_props(ctx, sh));
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
        _ => o.skipped += 1,
    }
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
    let mut o = Out { pres: p, styles: Styles::default(), media: vec![], skipped: 0 };
    let mut body = String::new();
    for (i, sl) in p.slides.iter().enumerate() {
        let Some(ctx) = Ctx::for_slide(p, sl) else { continue };
        let (bg, ph) = resolve::background(&ctx, Some(sl));
        let mut a = fill_props(&ctx, Some(&bg), ph);
        if sl.hidden {
            a.push(("presentation:visibility", "hidden".into()));
        }
        a.push(("presentation:background-visible", "true".into()));
        a.push(("presentation:background-objects-visible", "true".into()));
        let dp = o.styles.get("dp", "drawing-page", format!("<style:drawing-page-properties{}/>", attrs(&a)));
        let _ = write!(body, "<draw:page draw:name=\"page{}\" draw:style-name=\"{dp}\" draw:master-page-name=\"Default\">", i + 1);
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
}
