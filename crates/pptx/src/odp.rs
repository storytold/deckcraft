//! OpenDocument Presentation (`.odp`) import, written from the public OASIS ODF 1.3 standard.
//!
//! A first slice: slides, their size, text frames (paragraphs, lists, spans with font, size,
//! bold, italic, underline, colour, alignment), rectangles, ellipses and the common custom shapes
//! with their fill and outline, lines, pictures, groups (flattened), rotation and speaker notes.
//! Everything else is skipped. Sizes, counts and depths are bounded as in the PPTX reader.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::text::{Align, Anchor, Bullet, Paragraph, Run, RunProps, TextBody};
use deckcraft_model::{ColorRef, Fill, Geom, LayoutType, Line, Presentation, Shape, ShapeId, ShapeKind, defaults};

use crate::PptxError;
use crate::xml::{El, Node};

const MIME: &str = "application/vnd.oasis.opendocument.presentation";
const MAX_XML: u64 = 64 << 20;
const MAX_MEDIA: u64 = 200 << 20;
const MAX_PAGES: usize = 5_000;
const MAX_SHAPES: usize = 20_000;
const MAX_GROUP_DEPTH: usize = 16;

/// Is this an OpenDocument presentation (zip whose `mimetype` says so)?
pub fn sniff(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"PK") {
        return false;
    }
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return false };
    let Ok(f) = z.by_name("mimetype") else { return false };
    let mut s = String::new();
    f.take(200).read_to_string(&mut s).is_ok() && s.trim() == MIME
}

fn entry(z: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str, max: u64) -> Option<Vec<u8>> {
    let f = z.by_name(name).ok()?;
    if f.size() > max {
        log::warn!("odp: {name} is too large ({} bytes)", f.size());
        return None;
    }
    let mut v = Vec::new();
    f.take(max).read_to_end(&mut v).ok()?;
    Some(v)
}

/// A length (`2.5cm`, `10mm`, `1in`, `12pt`, `1pc`, `16px`) in points.
fn length(v: &str) -> Option<f64> {
    let v = v.trim();
    let split = v.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(v.len());
    let (n, unit) = v.split_at(split);
    let n: f64 = n.trim().parse().ok().filter(|x: &f64| x.is_finite())?;
    let k = match unit {
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "in" | "inch" => 72.0,
        "pt" | "" => 1.0,
        "pc" => 12.0,
        "px" => 0.75,
        _ => return None,
    };
    Some((n * k).clamp(-1.0e6, 1.0e6))
}

fn color(v: &str) -> Option<ColorRef> {
    Rgba::from_hex(v.trim().trim_start_matches('#')).map(ColorRef::rgb)
}

/// One style: family, parent and the properties of its `*-properties` children (local names).
#[derive(Clone, Debug, Default)]
struct Style {
    parent: Option<String>,
    props: HashMap<String, String>,
}

#[derive(Default)]
struct Styles {
    named: HashMap<(String, String), Style>,
    defaults: HashMap<String, Style>,
}

impl Styles {
    fn add_from(&mut self, container: &El) {
        for st in container.elements() {
            let family = st.attr("family").unwrap_or("").to_string();
            let mut s = Style { parent: st.attr("parent-style-name").map(str::to_string), props: HashMap::new() };
            for p in st.elements().filter(|e| e.local().ends_with("-properties")) {
                for (k, v) in &p.attrs {
                    let local = k.rsplit_once(':').map_or(k.as_str(), |x| x.1);
                    s.props.insert(local.to_string(), v.clone());
                }
            }
            if st.is("default-style") {
                self.defaults.insert(family, s);
            } else if st.is("style")
                && let Some(n) = st.attr("name")
            {
                self.named.insert((family, n.to_string()), s);
            }
        }
    }

    /// Property `key` of style `name` (family `family`), following parents, then the default.
    fn get(&self, family: &str, name: Option<&str>, key: &str) -> Option<String> {
        let mut cur = name.map(str::to_string);
        for _ in 0..32 {
            let Some(n) = cur else { break };
            let Some(s) = self.named.get(&(family.to_string(), n)) else { break };
            if let Some(v) = s.props.get(key) {
                return Some(v.clone());
            }
            cur = s.parent.clone();
        }
        self.defaults.get(family).and_then(|s| s.props.get(key).cloned())
    }
}

struct Ctx<'a> {
    styles: Styles,
    zip: zip::ZipArchive<std::io::Cursor<&'a [u8]>>,
    shapes: usize,
}

/// Read an `.odp` package.
pub fn import(bytes: &[u8]) -> Result<Presentation, PptxError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| PptxError::NotPptx(e.to_string()))?;
    let content = entry(&mut zip, "content.xml", MAX_XML).ok_or_else(|| PptxError::NotPptx("no content.xml".into()))?;
    let content = crate::xml::parse(&content)?;
    let styles_doc = entry(&mut zip, "styles.xml", MAX_XML).and_then(|b| crate::xml::parse(&b).ok());
    let mut styles = Styles::default();
    let mut size = defaults::WIDE;
    if let Some(sd) = &styles_doc {
        for c in sd.root.elements() {
            if c.is("styles") || c.is("automatic-styles") {
                styles.add_from(c);
            }
        }
        // The first page layout gives the slide size.
        if let Some(pl) = sd.root.find("page-layout-properties")
            && let (Some(w), Some(h)) = (pl.attr("page-width").and_then(length), pl.attr("page-height").and_then(length))
            && w > 1.0
            && h > 1.0
        {
            size = Size::new(w.min(4000.0), h.min(4000.0));
        }
    }
    if let Some(a) = content.root.child("automatic-styles") {
        styles.add_from(a);
    }
    let mut p = defaults::blank_presentation(size, Default::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).or_else(|| p.masters.first().and_then(|m| m.layouts.first().map(|l| l.id)));
    let Some(layout) = layout else { return Err(PptxError::Unsupported("no layout".into())) };
    let mut cx = Ctx { styles, zip, shapes: 0 };
    let pres = content.root.path(&["body", "presentation"]).ok_or_else(|| PptxError::NotPptx("no office:presentation".into()))?;
    for page in pres.children_named("page").take(MAX_PAGES) {
        let Some(mut slide) = defaults::new_slide(&mut p, layout) else { break };
        slide.shapes.clear();
        slide.name = page.attr("name").unwrap_or("").to_string();
        for el in page.elements() {
            if el.is("notes") {
                for f in el.children_named("frame") {
                    if f.attr("class") == Some("notes")
                        && let Some(tb) = f.child("text-box")
                    {
                        slide.notes = text_body(&cx, tb, false);
                    }
                }
            } else {
                shapes_of(&mut cx, &mut p, el, &mut slide.shapes, 0);
            }
        }
        p.slides.push(Arc::new(slide));
    }
    if p.slides.is_empty()
        && let Some(s) = defaults::new_slide(&mut p, layout)
    {
        p.slides.push(Arc::new(s));
    }
    Ok(p)
}

/// The box of a drawing element: `svg:x/y/width/height`, turned by `draw:transform`.
fn xfrm_of(el: &El) -> Option<Xfrm> {
    let w = el.attr("width").and_then(length).unwrap_or(0.0).max(0.0);
    let h = el.attr("height").and_then(length).unwrap_or(0.0).max(0.0);
    let mut x = Xfrm::new(el.attr("x").and_then(length).unwrap_or(0.0), el.attr("y").and_then(length).unwrap_or(0.0), w, h);
    if let Some(t) = el.attr("transform") {
        // `rotate (a) translate (x y)`: turned about the top-left corner, counter-clockwise.
        let mut a = 0.0;
        let mut tr = (x.x, x.y);
        let mut rest = t;
        while let Some(i) = rest.find('(') {
            let name = rest.get(..i).unwrap_or("").trim();
            let Some(j) = rest.find(')') else { break };
            let args: Vec<&str> = rest.get(i + 1..j).unwrap_or("").split([' ', ',']).filter(|s| !s.is_empty()).collect();
            match name {
                "rotate" => a = args.first().and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0),
                "translate" => tr = (args.first().and_then(|v| length(v)).unwrap_or(0.0), args.get(1).and_then(|v| length(v)).unwrap_or(0.0)),
                _ => {}
            }
            rest = rest.get(j + 1..).unwrap_or("");
        }
        let (c, s) = (a.cos(), a.sin());
        let cx = tr.0 + w / 2.0 * c + h / 2.0 * s;
        let cy = tr.1 - w / 2.0 * s + h / 2.0 * c;
        x.x = cx - w / 2.0;
        x.y = cy - h / 2.0;
        x.rot = (-a.to_degrees()).rem_euclid(360.0);
    }
    x.is_finite().then_some(x)
}

/// A drawing's fill from its style. `implicit`: a fill colour with no `draw:fill` counts (shapes
/// default to a solid fill, text frames to none).
fn graphic_fill(cx: &Ctx, style: Option<&str>, implicit: bool) -> Option<Fill> {
    let g = |k: &str| cx.styles.get("graphic", style, k).or_else(|| cx.styles.get("presentation", style, k));
    match g("fill").as_deref() {
        Some("none") => Some(Fill::None),
        None if !implicit => None,
        Some("solid") | None => g("fill-color").and_then(|c| color(&c)).map(Fill::solid),
        // Gradients, hatches and bitmaps: their first colour for now.
        Some(_) => g("fill-color").and_then(|c| color(&c)).map(Fill::solid),
    }
}

fn graphic_line(cx: &Ctx, style: Option<&str>) -> Option<Line> {
    let g = |k: &str| cx.styles.get("graphic", style, k).or_else(|| cx.styles.get("presentation", style, k));
    match g("stroke").as_deref() {
        Some("none") => Some(Line::none()),
        Some(_) => {
            let c = g("stroke-color").and_then(|c| color(&c)).unwrap_or(ColorRef::rgb(Rgba::rgb(0x34, 0x65, 0xa4)));
            let w = g("stroke-width").and_then(|v| length(&v)).unwrap_or(0.0).max(0.0);
            Some(Line::solid(c, if w > 0.0 { w } else { 0.75 }))
        }
        None => None,
    }
}

fn preset_for(kind: &str) -> &'static str {
    match kind {
        "ellipse" | "circle" => "ellipse",
        "round-rectangle" => "roundRect",
        "isosceles-triangle" => "triangle",
        "right-triangle" => "rtTriangle",
        "diamond" => "diamond",
        "pentagon" => "pentagon",
        "hexagon" => "hexagon",
        "octagon" => "octagon",
        "parallelogram" => "parallelogram",
        "trapezoid" => "trapezoid",
        "right-arrow" => "rightArrow",
        "left-arrow" => "leftArrow",
        "up-arrow" => "upArrow",
        "down-arrow" => "downArrow",
        "left-right-arrow" => "leftRightArrow",
        "star4" => "star4",
        "star5" => "star5",
        "star6" => "star6",
        "star8" => "star8",
        "heart" => "heart",
        "smiley" => "smileyFace",
        "cloud" => "cloud",
        "moon" => "moon",
        "sun" => "sun",
        "can" => "can",
        "cube" => "cube",
        "frame" => "frame",
        "ring" => "donut",
        "block-arc" => "blockArc",
        "flowchart-process" => "flowChartProcess",
        "flowchart-decision" => "flowChartDecision",
        "flowchart-terminator" => "flowChartTerminator",
        "flowchart-document" => "flowChartDocument",
        _ => "rect",
    }
}

/// A new shape with a fresh id, or `None` when the deck has no ids left.
fn new_shape(p: &mut Presentation, name: &str, x: Xfrm) -> Option<Shape> {
    let id = ShapeId(p.alloc_id()?);
    Some(Shape { id, name: if name.is_empty() { format!("Shape {}", id.0) } else { name.to_string() }, xfrm: Some(x), ..Default::default() })
}

fn shapes_of(cx: &mut Ctx, p: &mut Presentation, el: &El, out: &mut Vec<Shape>, depth: usize) {
    if cx.shapes >= MAX_SHAPES || depth > MAX_GROUP_DEPTH {
        return;
    }
    let style = el.attr("style-name");
    let name = el.attr("name").unwrap_or("");
    match el.local() {
        "g" => {
            for c in el.elements() {
                shapes_of(cx, p, c, out, depth + 1);
            }
        }
        "frame" => {
            let Some(x) = xfrm_of(el) else { return };
            if let Some(img) = el.child("image")
                && let Some(href) = img.attr("href")
                && !href.contains("://")
                && let Some(data) = entry(&mut cx.zip, href.trim_start_matches("./"), MAX_MEDIA)
            {
                let file = href.rsplit('/').next().unwrap_or(href);
                let Some(media) = p.add_media(file, crate::mime_for(file), data) else { return };
                let Some(mut sh) = new_shape(p, name, x) else { return };
                sh.kind = ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, ..Default::default() } };
                sh.descr = el.child("desc").map(|d| d.text()).unwrap_or_default();
                cx.shapes += 1;
                out.push(sh);
                return;
            }
            if let Some(tb) = el.child("text-box") {
                let class = el.attr("class").unwrap_or("");
                let pstyle = el.attr("presentation:style-name").or(style);
                let Some(mut sh) = new_shape(p, name, x) else { return };
                sh.text_box = true;
                sh.fill = graphic_fill(cx, pstyle, false).or(Some(Fill::None));
                sh.line = graphic_line(cx, pstyle).or(Some(Line::none()));
                let mut body = text_body(cx, tb, class == "outline");
                body_props(cx, pstyle, &mut body);
                default_sizes(&mut body, class);
                sh.text = Some(body);
                cx.shapes += 1;
                out.push(sh);
            }
        }
        "rect" | "ellipse" | "circle" | "custom-shape" => {
            let Some(x) = xfrm_of(el) else { return };
            let kind = match el.local() {
                "custom-shape" => el.child("enhanced-geometry").and_then(|g| g.attr("type")).unwrap_or("rectangle"),
                k => k,
            };
            let Some(mut sh) = new_shape(p, name, x) else { return };
            sh.geom = Geom::preset(preset_for(kind));
            sh.fill = graphic_fill(cx, style, true).or(Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0x72, 0x9f, 0xcf)))));
            sh.line = graphic_line(cx, style).or(Some(Line::solid(ColorRef::rgb(Rgba::rgb(0x34, 0x65, 0xa4)), 0.75)));
            if el.children_named("p").next().is_some() || el.children_named("list").next().is_some() {
                let mut body = text_body(cx, el, false);
                body.body.anchor = Some(Anchor::Middle);
                body_props(cx, style, &mut body);
                for para in &mut body.paragraphs {
                    para.props.align.get_or_insert(Align::Center);
                }
                sh.text = Some(body);
            }
            cx.shapes += 1;
            out.push(sh);
        }
        "line" => {
            let g = |k: &str| el.attr(k).and_then(length).unwrap_or(0.0);
            let (x1, y1, x2, y2) = (g("x1"), g("y1"), g("x2"), g("y2"));
            let mut x = Xfrm::new(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
            x.flip_h = x2 < x1;
            x.flip_v = y2 < y1;
            if !x.is_finite() {
                return;
            }
            let Some(mut sh) = new_shape(p, name, x) else { return };
            sh.geom = Geom::preset("line");
            sh.line = graphic_line(cx, style).or(Some(Line::solid(ColorRef::rgb(Rgba::rgb(0x34, 0x65, 0xa4)), 0.75)));
            cx.shapes += 1;
            out.push(sh);
        }
        _ => {}
    }
}

/// Insets and vertical alignment from a frame's style.
fn body_props(cx: &Ctx, style: Option<&str>, body: &mut TextBody) {
    let g = |k: &str| cx.styles.get("graphic", style, k).or_else(|| cx.styles.get("presentation", style, k));
    let pad = |k: &str| g(k).or_else(|| g("padding")).and_then(|v| length(&v));
    body.body.inset_l = pad("padding-left").or(body.body.inset_l);
    body.body.inset_r = pad("padding-right").or(body.body.inset_r);
    body.body.inset_t = pad("padding-top").or(body.body.inset_t);
    body.body.inset_b = pad("padding-bottom").or(body.body.inset_b);
    match g("textarea-vertical-align").as_deref() {
        Some("middle") => body.body.anchor = Some(Anchor::Middle),
        Some("bottom") => body.body.anchor = Some(Anchor::Bottom),
        Some("top") => body.body.anchor = Some(Anchor::Top),
        _ => {}
    }
}

/// LibreOffice's sizes for title and outline text that names none.
fn default_sizes(body: &mut TextBody, class: &str) {
    let size = |level: u8| match class {
        "title" => Some(44.0),
        "subtitle" => Some(32.0),
        "outline" => Some([32.0, 28.0, 24.0, 20.0, 20.0][level.min(4) as usize]),
        _ => None,
    };
    for para in &mut body.paragraphs {
        if let Some(s) = size(para.level) {
            for r in &mut para.runs {
                r.props.size.get_or_insert(s);
            }
            para.end_props.size.get_or_insert(s);
        }
    }
}

fn run_props(cx: &Ctx, family: &str, style: Option<&str>, base: &RunProps) -> RunProps {
    let g = |k: &str| style.and_then(|_| cx.styles.get(family, style, k));
    let mut r = base.clone();
    if let Some(s) = g("font-size").and_then(|v| length(&v)).filter(|s| *s > 0.0) {
        r.size = Some(s.min(4000.0));
    }
    if let Some(w) = g("font-weight") {
        r.bold = Some(w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600));
    }
    if let Some(i) = g("font-style") {
        r.italic = Some(i == "italic" || i == "oblique");
    }
    if let Some(u) = g("text-underline-style") {
        r.underline = Some(if u == "none" { "none".into() } else { "sng".into() });
    }
    if let Some(c) = g("color").and_then(|c| color(&c)) {
        r.fill = Some(Fill::solid(c));
    }
    if let Some(f) = g("font-name").or_else(|| g("font-family")) {
        let f = f.trim_matches(['\'', '"']).to_string();
        if !f.is_empty() {
            r.font = Some(f);
        }
    }
    r
}

/// Paragraphs of a text container (`draw:text-box`, or a shape holding `text:p` directly).
fn text_body(cx: &Ctx, el: &El, bullets: bool) -> TextBody {
    let mut paras = Vec::new();
    collect_paras(cx, el, 0, false, bullets, &mut paras, 0);
    if paras.is_empty() {
        paras.push(Paragraph::default());
    }
    TextBody { paragraphs: paras, ..Default::default() }
}

fn collect_paras(cx: &Ctx, el: &El, level: u8, in_list: bool, bullets: bool, out: &mut Vec<Paragraph>, depth: usize) {
    if depth > 24 || out.len() > 100_000 {
        return;
    }
    for c in el.elements() {
        match c.local() {
            "p" | "h" => out.push(paragraph(cx, c, level, in_list && bullets)),
            "list" => {
                let next = if in_list { level.saturating_add(1).min(8) } else { level };
                for item in c.elements().filter(|e| e.is("list-item") || e.is("list-header")) {
                    collect_paras(cx, item, next, true, bullets, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn paragraph(cx: &Ctx, el: &El, level: u8, bullet: bool) -> Paragraph {
    let pstyle = el.attr("style-name");
    let base = run_props(cx, "paragraph", pstyle, &RunProps::default());
    let mut para = Paragraph { level, ..Default::default() };
    match cx.styles.get("paragraph", pstyle, "text-align").as_deref() {
        Some("center") => para.props.align = Some(Align::Center),
        Some("end" | "right") => para.props.align = Some(Align::Right),
        Some("justify") => para.props.align = Some(Align::Justify),
        Some("start" | "left") => para.props.align = Some(Align::Left),
        _ => {}
    }
    if bullet {
        para.props.bullet = Some(Bullet::Char { char: "•".into() });
    }
    spans(cx, el, &base, &mut para.runs, 0);
    para.end_props = base;
    para.runs.retain(|r| !r.text.is_empty());
    para
}

fn spans(cx: &Ctx, el: &El, props: &RunProps, runs: &mut Vec<Run>, depth: usize) {
    if depth > 24 || runs.len() > 100_000 {
        return;
    }
    let push = |runs: &mut Vec<Run>, t: &str, props: &RunProps| {
        if let Some(last) = runs.last_mut()
            && last.props == *props
        {
            last.text.push_str(t);
        } else {
            runs.push(Run { text: t.to_string(), props: props.clone(), ..Run::new("") });
        }
    };
    for n in &el.children {
        match n {
            Node::Text(t) => {
                // ODF collapses white space in text (spaces are `text:s`).
                let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
                let lead = n_text_starts_ws(n);
                let trail = n_text_ends_ws(n);
                let mut s = String::new();
                if lead && !t.is_empty() {
                    s.push(' ');
                }
                s.push_str(&t);
                if trail && !t.is_empty() {
                    s.push(' ');
                }
                if t.is_empty() && (lead || trail) {
                    s.push(' ');
                }
                if !s.is_empty() {
                    push(runs, &s, props);
                }
            }
            Node::El(c) => match c.local() {
                "span" | "a" => {
                    let p2 = run_props(cx, "text", c.attr("style-name"), props);
                    spans(cx, c, &p2, runs, depth + 1);
                }
                "s" => {
                    let n = c.u32("c").unwrap_or(1).clamp(1, 1000) as usize;
                    push(runs, &" ".repeat(n), props);
                }
                "tab" => push(runs, "\t", props),
                "line-break" => push(runs, "\u{b}", props),
                _ => spans(cx, c, props, runs, depth + 1),
            },
        }
    }
}

fn n_text_starts_ws(n: &Node) -> bool {
    matches!(n, Node::Text(t) if t.starts_with(char::is_whitespace))
}
fn n_text_ends_ws(n: &Node) -> bool {
    matches!(n, Node::Text(t) if t.ends_with(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(length("1in"), Some(72.0));
        assert!((length("2.54cm").unwrap() - 72.0).abs() < 1e-9);
        assert_eq!(length("12pt"), Some(12.0));
        assert_eq!(length("x"), None);
        assert_eq!(length("3furlongs"), None);
    }
}
