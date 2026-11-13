//! SVG pictures (`image/svg+xml`) read into bezier geometry: shapes, paths, groups, transforms,
//! solid and gradient fills, strokes, and `use`.
//!
//! What a picture needs is drawn; what a presentation cannot show is not read: text, filters,
//! masks, clip paths, patterns, animation and scripting are skipped. A gradient keeps its stops and
//! its own coordinate space, and the renderer draws the ramp.
//!
//! `display: none` hides a subtree and `visibility: hidden` one shape. An ancestor's `opacity`
//! multiplies each shape it paints rather than compositing the group once, so overlapping shapes
//! in a translucent group show their overlap (a `shortcut:` for real group compositing).
//!
//! Geometry stays in the document's user units (the `viewBox` space); [`Svg::size`] is the size the
//! picture asks for in 96-dpi pixels, which is where the renderer maps the geometry.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod attr;
mod dom;

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_color::Rgba;
use dom::Node;
use kurbo::{Affine, BezPath, Point, Shape as _};

/// Most shapes read from one picture.
pub const MAX_ITEMS: usize = 100_000;
/// Largest document read.
const MAX_BYTES: usize = 64 << 20;
/// Most path segments kept per picture, across all shapes.
const MAX_SEGMENTS_TOTAL: usize = 2_000_000;
/// Most elements visited, which also bounds a reference graph that paints nothing.
const MAX_VISITS: usize = 500_000;
/// Most stops read from one gradient.
const MAX_STOPS: usize = 64;
/// Largest coordinate or radius taken from a shape (a bigger one would only tessellate absurdly).
const MAX_COORD: f64 = 1e5;
/// Most segments one path may hold.
const MAX_SEGMENTS: usize = 50_000;
/// Deepest `use` chain followed.
const MAX_USE_DEPTH: u32 = 8;
/// Elements that only define something and are drawn through `use`.
const DEFINITIONS: [&str; 16] = [
    "defs",
    "symbol",
    "clipPath",
    "mask",
    "marker",
    "pattern",
    "filter",
    "linearGradient",
    "radialGradient",
    "style",
    "script",
    "title",
    "desc",
    "metadata",
    "text",
    "image",
];

/// An SVG document as the renderer draws it.
#[derive(Clone, Debug, Default)]
pub struct Svg {
    /// The size the picture asks for, in 96-dpi pixels.
    pub size: (f64, f64),
    /// `viewBox` as `[x, y, width, height]` in user units.
    pub view_box: Option<[f64; 4]>,
    /// Every shape, in painting order, already in user units.
    pub items: Vec<Item>,
}

/// One filled and/or stroked shape.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: BezPath,
    pub even_odd: bool,
    pub fill: Option<Paint>,
    pub stroke: Option<(Paint, kurbo::Stroke)>,
}

/// What a fill or a stroke paints.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    Gradient(Gradient),
}

impl Paint {
    /// The paint with every colour's alpha multiplied by `k`.
    fn faded(self, k: f64) -> Paint {
        match self {
            Paint::Solid(c) => Paint::Solid(c.with_alpha(mul_alpha(c.a, k))),
            Paint::Gradient(mut g) => {
                for (_, c) in &mut g.stops {
                    *c = c.with_alpha(mul_alpha(c.a, k));
                }
                Paint::Gradient(g)
            }
        }
    }
}

/// A gradient with the stops and coordinate space it was declared in.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Stops by offset, ascending, each offset clamped to the one before it as SVG reads them.
    pub stops: Vec<(f64, Rgba)>,
    pub units: Units,
    /// `gradientTransform`, applied in the gradient's own space.
    pub transform: Affine,
    pub spread: Spread,
}

/// Where a gradient ramps, in the space [`Units`] names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    /// A circle at `(cx, cy)` of radius `r`, its first stop at `(fx, fy)`.
    Radial {
        cx: f64,
        cy: f64,
        r: f64,
        fx: f64,
        fy: f64,
    },
}

/// What a gradient's coordinates mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    /// Fractions of the painted shape's bounding box, SVG's default.
    ObjectBoundingBox,
    /// User units, as the geometry is.
    UserSpace,
}

/// What a gradient paints outside its last stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spread {
    Pad,
    Reflect,
    Repeat,
}

/// Does `bytes` look like an SVG picture? The root may carry a namespace prefix (`<s:svg>`).
pub fn is_svg(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = bytes.get(..bytes.len().min(4096)).unwrap_or(bytes);
    let first = head.iter().find(|b| !b.is_ascii_whitespace()).copied().unwrap_or(0);
    first == b'<' && root_is_svg(head)
}

/// Any start tag in `head` whose local name is `svg`.
fn root_is_svg(head: &[u8]) -> bool {
    let name_at = |i: usize| -> &[u8] {
        let rest = head.get(i + 1..).unwrap_or(&[]);
        let end = rest.iter().position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))).unwrap_or(rest.len());
        rest.get(..end).unwrap_or(&[])
    };
    head.iter().enumerate().filter(|(_, b)| **b == b'<').any(|(i, _)| {
        let name = name_at(i);
        name.rsplit(|b| *b == b':').next().unwrap_or(name).eq_ignore_ascii_case(b"svg")
    })
}

/// Read a picture. `None` when the document is not XML, not an `svg` root, or too large.
pub fn parse(bytes: &[u8]) -> Option<Svg> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let root = dom::parse(bytes)?;
    if root.name != "svg" {
        return None;
    }
    let view_box = view_box_of(&root);
    let size = root_size(&root, view_box);
    let mut ids = HashMap::new();
    collect_ids(&root, &mut ids);
    let mut build = Build { items: Vec::new(), ids, uses: 0, visits: 0, segments: 0 };
    build.walk(&root, &Frame { transform: Affine::IDENTITY, style: Style::default() }, 0, true);
    Some(Svg { size, view_box, items: build.items })
}

/// The size in pixels a picture asks for: its `width`/`height`, else the `viewBox`, else CSS's
/// default 300 × 150.
fn root_size(root: &Node, view_box: Option<[f64; 4]>) -> (f64, f64) {
    let len = |k: &str| root.attr(k).and_then(attr::length).filter(|v| v.is_finite() && *v > 0.0 && *v < 1e6);
    let (w, h) = (len("width"), len("height"));
    let size = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        // One side given: the other follows the viewBox, else CSS's 4:3 picture default.
        (Some(w), None) => (w, view_box.map(|v| w * v[3] / v[2]).unwrap_or(w * 0.75)),
        (None, Some(h)) => (view_box.map(|v| h * v[2] / v[3]).unwrap_or(h / 0.75), h),
        (None, None) => view_box.map(|v| (v[2], v[3])).unwrap_or((300.0, 150.0)),
    };
    (size.0.clamp(0.01, 1e6), size.1.clamp(0.01, 1e6))
}

fn collect_ids(node: &Node, out: &mut HashMap<String, Arc<Node>>) {
    if let Some(id) = node.attr("id").filter(|i| !i.is_empty()) {
        out.entry(id.to_string()).or_insert_with(|| Arc::new(node.clone()));
    }
    for k in &node.kids {
        collect_ids(k, out);
    }
}

/// Fill and stroke inherited down the tree, plus the opacity of the groups above.
#[derive(Clone, Debug)]
struct Style {
    fill: Option<Paint>,
    stroke: Option<Paint>,
    width: f64,
    cap: kurbo::Cap,
    join: kurbo::Join,
    dash: Option<Vec<f64>>,
    even_odd: bool,
    fill_opacity: f64,
    stroke_opacity: f64,
    /// The element's own `opacity`, applied to what it paints.
    opacity: f64,
    /// The product of the `opacity` of every group above, which composites the subtree.
    group_opacity: f64,
    /// `display` is not inherited: `none` hides the whole subtree.
    display: bool,
    visibility: bool,
}

impl Default for Style {
    fn default() -> Self {
        // SVG's initial values: a black fill, no stroke, a one-unit miter-joined pen.
        Style {
            fill: Some(Paint::Solid(Rgba::BLACK)),
            stroke: None,
            width: 1.0,
            cap: kurbo::Cap::Butt,
            join: kurbo::Join::Miter,
            dash: None,
            even_odd: false,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            opacity: 1.0,
            group_opacity: 1.0,
            display: true,
            visibility: true,
        }
    }
}

impl Style {
    fn resolve(&self, node: &Node, ids: &HashMap<String, Arc<Node>>) -> Style {
        let mut s = self.clone();
        // `opacity` belongs to the element that sets it: what an ancestor set composites the
        // subtree instead of being replaced by a descendant's own value.
        s.group_opacity *= self.opacity;
        s.opacity = 1.0;
        for (k, v) in &node.attrs {
            if k != "style" {
                s.set(k, v, ids);
            }
        }
        // A `style` attribute wins over the presentation attributes, as CSS does.
        if let Some(d) = node.attr("style") {
            for decl in d.split(';') {
                if let Some((k, v)) = decl.split_once(':') {
                    s.set(k.trim(), v.trim(), ids);
                }
            }
        }
        s
    }

    fn set(&mut self, key: &str, v: &str, ids: &HashMap<String, Arc<Node>>) {
        let opacity = |v: &str| {
            {
                let v = v.trim();
                if let Some(p) = v.strip_suffix('%') { p.parse::<f64>().map(|p| p / 100.0) } else { v.parse::<f64>() }
            }
            .ok()
            .map(|v: f64| v.clamp(0.0, 1.0))
        };
        match key {
            "fill" => self.fill = paint(v, ids),
            "stroke" => self.stroke = paint(v, ids),
            "stroke-width" => {
                if let Some(w) = attr::length(v) {
                    self.width = w.clamp(0.0, 1e4);
                }
            }
            "stroke-linecap" => {
                self.cap = match v.trim() {
                    "round" => kurbo::Cap::Round,
                    "square" => kurbo::Cap::Square,
                    _ => kurbo::Cap::Butt,
                }
            }
            "stroke-linejoin" => {
                self.join = match v.trim() {
                    "round" => kurbo::Join::Round,
                    "bevel" => kurbo::Join::Bevel,
                    _ => kurbo::Join::Miter,
                }
            }
            "stroke-dasharray" => {
                let d = attr::numbers(v);
                self.dash = (d.len() >= 2 && d.iter().all(|v| *v > 0.0)).then_some(d);
            }
            "fill-rule" => self.even_odd = v.trim() == "evenodd",
            "fill-opacity" => self.fill_opacity = opacity(v).unwrap_or(1.0),
            "stroke-opacity" => self.stroke_opacity = opacity(v).unwrap_or(1.0),
            "opacity" => self.opacity = opacity(v).unwrap_or(1.0),
            "display" => self.display = !v.trim().eq_ignore_ascii_case("none"),
            "visibility" => self.visibility = !matches!(v.trim(), "hidden" | "collapse"),
            _ => {}
        }
    }
}

/// A paint value: solid, or a gradient whose stops the renderer ramps between. A reference to
/// anything else (a pattern, a filter) falls back to the average of its stops, and an unreadable
/// colour is black, as an unreadable colour name is in a browser.
fn paint(v: &str, ids: &HashMap<String, Arc<Node>>) -> Option<Paint> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(inner) = v.strip_prefix("url(").and_then(|r| r.strip_suffix(')')) {
        let id = inner.trim().trim_matches(|c| c == '\'' || c == '"').trim().strip_prefix('#').unwrap_or(inner.trim());
        let Some(node) = ids.get(id) else { return Some(Paint::Solid(Rgba::BLACK)) };
        if let Some(g) = gradient_of(node, ids) {
            return Some(Paint::Gradient(g));
        }
        return Some(Paint::Solid(gradient_color(node, ids).unwrap_or(Rgba::BLACK)));
    }
    Some(Paint::Solid(attr::color(v).or_else(|| attr::named(v)).unwrap_or(Rgba::BLACK)))
}

/// The gradient a `url(#…)` names, or `None` when the element is not a gradient this reader
/// understands (or has no usable stops).
fn gradient_of(node: &Node, ids: &HashMap<String, Arc<Node>>) -> Option<Gradient> {
    let linear = match node.name.as_str() {
        "linearGradient" => true,
        "radialGradient" => false,
        _ => return None,
    };
    // A gradient takes the attributes and stops it does not set itself from the one its `href`
    // names, which is how an exporter writes a family of gradients that differ in their stops.
    let chain = href_chain(node, ids);
    let stops = stops_of(&chain);
    if stops.is_empty() {
        return None;
    }
    let units = match chain_attr(&chain, "gradientUnits").map(str::trim) {
        Some("userSpaceOnUse") => Units::UserSpace,
        _ => Units::ObjectBoundingBox,
    };
    let spread = match chain_attr(&chain, "spreadMethod").map(str::trim) {
        Some("reflect") => Spread::Reflect,
        Some("repeat") => Spread::Repeat,
        _ => Spread::Pad,
    };
    let transform = attr::transform(chain_attr(&chain, "gradientTransform").unwrap_or(""));
    let kind = if linear {
        GradientKind::Linear {
            x1: coord(&chain, "x1", 0.0),
            y1: coord(&chain, "y1", 0.0),
            x2: coord(&chain, "x2", 1.0),
            y2: coord(&chain, "y2", 0.0),
        }
    } else {
        let (cx, cy) = (coord(&chain, "cx", 0.5), coord(&chain, "cy", 0.5));
        GradientKind::Radial { cx, cy, r: coord(&chain, "r", 0.5).clamp(0.0, MAX_COORD), fx: coord(&chain, "fx", cx), fy: coord(&chain, "fy", cy) }
    };
    Some(Gradient { kind, stops, units, transform, spread })
}

/// `node` and the gradients its `href` names, outermost first.
fn href_chain<'a>(node: &'a Node, ids: &'a HashMap<String, Arc<Node>>) -> Vec<&'a Node> {
    let mut out = vec![node];
    let mut cur = node;
    // Four links is what a generator writes at most; a longer chain would only repeat itself.
    for _ in 0..4 {
        let Some(next) = k_href(cur).and_then(|h| ids.get(h)).map(Arc::as_ref) else { break };
        if out.iter().any(|n| std::ptr::eq(*n, next)) {
            break;
        }
        out.push(next);
        cur = next;
    }
    out
}

fn chain_attr<'a>(chain: &[&'a Node], key: &str) -> Option<&'a str> {
    chain.iter().find_map(|n| n.attr(key))
}

/// A gradient coordinate: `50%` is half the box, a bare number keeps the units it names.
fn coord(chain: &[&Node], key: &str, default: f64) -> f64 {
    let Some(v) = chain_attr(chain, key).map(str::trim) else { return default };
    let v = match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|p| p / 100.0),
        None => attr::length(v),
    };
    v.filter(|v: &f64| v.is_finite()).map(|v| v.clamp(-MAX_COORD, MAX_COORD)).unwrap_or(default)
}

/// The stops of the first gradient in `chain` that has any, by offset, each offset at least the
/// one before it.
fn stops_of(chain: &[&Node]) -> Vec<(f64, Rgba)> {
    for n in chain {
        let mut out: Vec<(f64, Rgba)> = Vec::new();
        for k in n.kids.iter().filter(|k| k.name == "stop").take(MAX_STOPS) {
            let offset = k.attr("offset").map(str::to_string).or_else(|| style_value(k, "offset"));
            let offset = offset.as_deref().map(str::trim).and_then(|v| match v.strip_suffix('%') {
                Some(p) => p.trim().parse::<f64>().ok().map(|p| p / 100.0),
                None => v.parse::<f64>().ok(),
            });
            let offset = offset.filter(|o: &f64| o.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
            let declared = k.attr("stop-color").map(str::to_string).or_else(|| style_value(k, "stop-color"));
            let c = declared.as_deref().and_then(|v| attr::color(v).or_else(|| attr::named(v))).unwrap_or(Rgba::BLACK);
            let a = k
                .attr("stop-opacity")
                .map(str::to_string)
                .or_else(|| style_value(k, "stop-opacity"))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            out.push((offset, c.with_alpha(mul_alpha(c.a, a))));
        }
        if !out.is_empty() {
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut last = 0.0;
            for (o, _) in &mut out {
                last = o.max(last);
                *o = last;
            }
            return out;
        }
    }
    Vec::new()
}

/// The average colour of a gradient's stops.
fn gradient_color(node: &Node, ids: &HashMap<String, Arc<Node>>) -> Option<Rgba> {
    let mut stops: Vec<Rgba> = Vec::new();
    fn collect(n: &Node, out: &mut Vec<Rgba>, ids: &HashMap<String, Arc<Node>>, depth: usize) {
        if depth > 4 || out.len() >= 64 {
            return;
        }
        for k in &n.kids {
            if out.len() >= MAX_STOPS {
                break;
            }
            if k.name != "stop" {
                continue;
            }
            let declared = k.attr("stop-color").map(str::to_string).or_else(|| style_value(k, "stop-color"));
            let c = declared.as_deref().and_then(|v| attr::color(v).or_else(|| attr::named(v))).unwrap_or(Rgba::BLACK);
            let a = k
                .attr("stop-opacity")
                .map(str::to_string)
                .or_else(|| style_value(k, "stop-opacity"))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            out.push(c.with_alpha((f64::from(c.a) * a).round() as u8));
        }
        // `href` chains (a gradient that only adds stops to another).
        if out.is_empty()
            && depth < 4
            && let Some(id) = k_href(n).and_then(|h| ids.get(h))
        {
            collect(id, out, ids, depth + 1);
        }
    }
    collect(node, &mut stops, ids, 0);
    if stops.is_empty() {
        return None;
    }
    let n = stops.len() as u32;
    let sum = |f: fn(&Rgba) -> u8| stops.iter().map(|c| u32::from(f(c))).sum::<u32>() / n;
    Some(Rgba::rgba(sum(|c| c.r) as u8, sum(|c| c.g) as u8, sum(|c| c.b) as u8, sum(|c| c.a) as u8))
}

fn k_href(n: &Node) -> Option<&str> {
    n.attr("href").or_else(|| n.attr("xlink:href")).and_then(|h| h.trim().strip_prefix('#'))
}

fn style_value(n: &Node, key: &str) -> Option<String> {
    n.attr("style")?.split(';').find_map(|d| {
        let (k, v) = d.split_once(':')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// A transform and the style in force where an element sits.
#[derive(Clone)]
struct Frame {
    transform: Affine,
    style: Style,
}

struct Build {
    items: Vec<Item>,
    ids: HashMap<String, Arc<Node>>,
    uses: u32,
    /// Elements visited, so a reference graph that expands without painting anything stops too.
    visits: usize,
    /// Path elements kept, so many references cannot pile up behind [`MAX_ITEMS`].
    segments: usize,
}

impl Build {
    /// `via_use`: the element was named by `use`, so a definition such as a `symbol` draws here.
    fn walk(&mut self, node: &Node, f: &Frame, depth: usize, via_use: bool) {
        self.visits += 1;
        if depth > dom::MAX_DEPTH || self.visits > MAX_VISITS || self.items.len() >= MAX_ITEMS || self.segments >= MAX_SEGMENTS_TOTAL {
            return;
        }
        if !via_use && DEFINITIONS.contains(&node.name.as_str()) {
            return;
        }
        let style = f.style.resolve(node, &self.ids);
        // `display: none` hides the subtree; `visibility: hidden` leaves descendants able to show
        // themselves, so it is applied where a shape is painted.
        if !style.display {
            return;
        }
        let mut transform = f.transform;
        if node.name == "svg" {
            let at = |k: &str| node.attr(k).and_then(attr::length).unwrap_or(0.0);
            transform *= Affine::translate((at("x"), at("y")));
        }
        transform *= attr::transform(node.attr("transform").unwrap_or(""));
        let frame = Frame { transform, style };
        match node.name.as_str() {
            "g" | "svg" | "symbol" | "a" | "switch" => {
                for k in &node.kids {
                    self.walk(k, &frame, depth + 1, false);
                }
            }
            "use" => self.use_node(node, &frame, depth),
            _ => self.shape(node, &frame),
        }
    }

    fn use_node(&mut self, node: &Node, f: &Frame, depth: usize) {
        if self.uses >= MAX_USE_DEPTH {
            return;
        }
        let Some(id) = k_href(node) else { return };
        let Some(target) = self.ids.get(id).cloned() else { return };
        let at = |k: &str| node.attr(k).and_then(attr::length).unwrap_or(0.0);
        let mut transform = f.transform * Affine::translate((at("x"), at("y")));
        // A `symbol` (or nested `svg`) maps its own `viewBox` into the `use`'s width and height;
        // a zero-sized viewport paints nothing.
        if let Some(vb) = view_box_of(&target) {
            let w = node.attr("width").and_then(attr::length).unwrap_or(vb[2]);
            let h = node.attr("height").and_then(attr::length).unwrap_or(vb[3]);
            if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
                return;
            }
            transform *= fit(vb, w, h);
        }
        let frame = Frame { transform, style: f.style.clone() };
        self.uses += 1;
        self.walk(&target, &frame, depth + 1, true);
        self.uses -= 1;
    }

    fn shape(&mut self, node: &Node, f: &Frame) {
        if !f.style.visibility {
            return;
        }
        let Some(mut path) = shape_path(node) else { return };
        let segments = path.elements().len();
        if self.segments.saturating_add(segments) > MAX_SEGMENTS_TOTAL {
            return;
        }
        let bb = path.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite()) {
            return;
        }
        path.apply_affine(f.transform);
        // The pen is scaled with the shape; a non-uniform transform cannot scale it evenly, so the
        // mean of both axes stands in.
        let scale = f.transform.determinant().abs().sqrt();
        let opacity = f.style.group_opacity * f.style.opacity;
        let fill = f.style.fill.clone().map(|p| p.faded(f.style.fill_opacity * opacity));
        let stroke = f.style.stroke.clone().filter(|_| f.style.width > 0.0).map(|p| {
            let mut pen = kurbo::Stroke::new((f.style.width * scale).max(1e-4)).with_caps(f.style.cap).with_join(f.style.join);
            if let Some(d) = &f.style.dash {
                pen = pen.with_dashes(0.0, d.iter().map(|v| v * scale));
            }
            (p.faded(f.style.stroke_opacity * opacity), pen)
        });
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.segments += segments;
        self.items.push(Item { path, even_odd: f.style.even_odd, fill, stroke });
    }
}

/// The `viewBox` of a `symbol` or nested `svg`, in user units.
fn view_box_of(node: &Node) -> Option<[f64; 4]> {
    node.attr("viewBox")
        .or_else(|| node.attr("viewbox"))
        .map(attr::numbers)
        .filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0)
        .map(|v| [v[0], v[1], v[2], v[3]])
}

/// Fits a `viewBox` into a `w` × `h` viewport, both sides visible and centred (SVG's default
/// `preserveAspectRatio`).
fn fit(vb: [f64; 4], w: f64, h: f64) -> Affine {
    let s = (w / vb[2]).min(h / vb[3]);
    if !(s.is_finite() && s > 0.0) {
        return Affine::IDENTITY;
    }
    Affine::translate(((w - vb[2] * s) / 2.0, (h - vb[3] * s) / 2.0)) * Affine::scale(s) * Affine::translate((-vb[0], -vb[1]))
}

fn mul_alpha(a: u8, k: f64) -> u8 {
    (f64::from(a) * k.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
}

/// The geometry of one shape element, in user units.
fn shape_path(n: &Node) -> Option<BezPath> {
    let num = |k: &str, d: f64| n.attr(k).and_then(attr::length).unwrap_or(d);
    match n.name.as_str() {
        "path" => {
            let p = BezPath::from_svg(n.attr("d")?).ok()?;
            let bb = p.bounding_box();
            (p.elements().len() <= MAX_SEGMENTS && bb.width() <= 1e6 && bb.height() <= 1e6).then_some(p)
        }
        "rect" => {
            let (x, y, w, h) = (num("x", 0.0), num("y", 0.0), num("width", 0.0), num("height", 0.0));
            if !(w > 0.0 && h > 0.0 && w <= MAX_COORD && h <= MAX_COORD) {
                return None;
            }
            let rx = n.attr("rx").and_then(attr::length);
            let ry = n.attr("ry").and_then(attr::length);
            let (rx, ry) = match (rx, ry) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => (a, a),
                (None, Some(b)) => (b, b),
                (None, None) => return Some(kurbo::Rect::new(x, y, x + w, y + h).to_path(0.1)),
            };
            let r = ((rx + ry) / 2.0).clamp(0.0, w.min(h) / 2.0);
            Some(kurbo::RoundedRect::new(x, y, x + w, y + h, r).to_path(0.1))
        }
        "circle" => {
            let r = num("r", 0.0);
            (r > 0.0 && r <= MAX_COORD).then(|| kurbo::Ellipse::new((num("cx", 0.0), num("cy", 0.0)), (r, r), 0.0).to_path(0.1))
        }
        "ellipse" => {
            let (rx, ry) = (num("rx", 0.0), num("ry", 0.0));
            (rx > 0.0 && ry > 0.0 && rx <= MAX_COORD && ry <= MAX_COORD)
                .then(|| kurbo::Ellipse::new((num("cx", 0.0), num("cy", 0.0)), (rx, ry), 0.0).to_path(0.1))
        }
        "line" => {
            let mut p = BezPath::new();
            p.move_to((num("x1", 0.0), num("y1", 0.0)));
            p.line_to((num("x2", 0.0), num("y2", 0.0)));
            Some(p)
        }
        "polyline" | "polygon" => {
            let pts = attr::numbers(n.attr("points").unwrap_or(""));
            let mut p = BezPath::new();
            for (i, c) in pts.as_chunks::<2>().0.iter().enumerate() {
                let pt = Point::new(c[0], c[1]);
                if i == 0 { p.move_to(pt) } else { p.line_to(pt) }
            }
            if n.name == "polygon" {
                p.close_path();
            }
            (!p.elements().is_empty()).then_some(p)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(body: &str) -> Svg {
        parse(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'>{body}</svg>").as_bytes()).expect("reads")
    }

    #[test]
    fn reads_a_path_with_its_fill_and_stroke() {
        let s = svg("<path d='M0 0 L10 0 L10 10 Z' fill='#ff0000' stroke='#0000ff' stroke-width='2'/>");
        assert_eq!(s.items.len(), 1);
        let it = &s.items[0];
        assert_eq!(it.fill, Some(Paint::Solid(Rgba::rgb(255, 0, 0))));
        let (c, pen) = it.stroke.as_ref().expect("stroked");
        assert_eq!(*c, Paint::Solid(Rgba::rgb(0, 0, 255)));
        assert_eq!(pen.width, 2.0);
        assert!(!it.even_odd);
    }

    #[test]
    fn stroke_width_follows_the_transform() {
        let s = svg("<g transform='scale(3)'><path d='M0 0 L5 5' stroke='black' stroke-width='1'/></g>");
        let (_, pen) = s.items[0].stroke.as_ref().expect("stroked");
        assert!((pen.width - 3.0).abs() < 1e-9, "{}", pen.width);
    }

    #[test]
    fn a_style_attribute_wins_over_presentation_attributes() {
        let s = svg("<rect width='4' height='4' fill='red' style='fill:#00ff00;stroke:none'/>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::rgb(0, 255, 0))));
        assert!(s.items[0].stroke.is_none());
    }

    #[test]
    fn groups_pass_their_fill_and_opacity_down() {
        let s = svg("<g fill='blue' fill-opacity='0.5'><rect width='4' height='4'/></g>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::rgba(0, 0, 255, 128))));
    }

    #[test]
    fn a_child_opacity_does_not_replace_its_groups() {
        let s = svg("<g opacity='0.5'><rect width='4' height='4' opacity='0.5' fill='black'/></g>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::rgba(0, 0, 0, 64))));
        let s = svg("<g opacity='0.5'><rect width='4' height='4' fill='black'/></g>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::rgba(0, 0, 0, 128))));
    }

    #[test]
    fn display_none_hides_the_subtree_and_visibility_hides_one_shape() {
        let s = svg("<g display='none'><rect width='4' height='4'/></g>");
        assert!(s.items.is_empty());
        let s = svg("<g visibility='hidden'><rect width='4' height='4'/><rect width='4' height='4' visibility='visible'/></g>");
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn a_symbol_is_scaled_into_the_use_viewport() {
        let s = svg(
            "<defs><symbol id='s' viewBox='0 0 10 10'><rect width='10' height='10' fill='black'/></symbol></defs><use href='#s' width='100' height='100'/>",
        );
        let b = s.items[0].path.bounding_box();
        assert!((b.width() - 100.0).abs() < 1e-6 && (b.height() - 100.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn a_zero_sized_use_viewport_paints_nothing() {
        let s = svg(
            "<defs><symbol id='s' viewBox='0 0 10 10'><rect width='10' height='10' fill='black'/></symbol></defs><use href='#s' width='0' height='10'/>",
        );
        assert!(s.items.is_empty());
    }

    #[test]
    fn absurd_shapes_are_refused_instead_of_tessellated() {
        assert!(svg("<circle r='1e60'/>").items.is_empty());
        assert!(svg("<ellipse rx='1e60' ry='1'/>").items.is_empty());
        assert!(svg("<rect width='1e60' height='1'/>").items.is_empty());
    }

    #[test]
    fn use_draws_the_element_it_names() {
        let s = svg("<defs><rect id='r' width='4' height='4' fill='red'/></defs><use href='#r' x='10' y='5'/>");
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].path.bounding_box().x0, 10.0);
        assert_eq!(s.items[0].path.bounding_box().y0, 5.0);
    }

    #[test]
    fn a_self_referencing_use_stops() {
        let s = svg("<use id='u' href='#u'/>");
        assert!(s.items.is_empty());
    }

    #[test]
    fn a_gradient_fill_keeps_its_stops_and_its_box() {
        let s = svg(
            "<defs><linearGradient id='g' x1='0' y1='0' x2='0' y2='1'><stop offset='0' stop-color='#000000'/><stop offset='100%' stop-color='#ffffff'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.units, Units::ObjectBoundingBox);
        assert_eq!(g.spread, Spread::Pad);
        assert_eq!(g.kind, GradientKind::Linear { x1: 0.0, y1: 0.0, x2: 0.0, y2: 1.0 });
        assert_eq!(g.stops, vec![(0.0, Rgba::rgb(0, 0, 0)), (1.0, Rgba::rgb(255, 255, 255))]);
    }

    #[test]
    fn a_radial_gradient_defaults_to_the_middle_of_the_box() {
        let s = svg(
            "<defs><radialGradient id='g'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></radialGradient></defs><circle r='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.kind, GradientKind::Radial { cx: 0.5, cy: 0.5, r: 0.5, fx: 0.5, fy: 0.5 });
        // An offset the document leaves out is 0, as SVG reads it.
        assert_eq!(g.stops[0].0, 0.0);
    }

    #[test]
    fn a_gradient_takes_missing_stops_from_the_gradient_its_href_names() {
        let s = svg("<defs><linearGradient id='base'><stop stop-color='#ff0000'/><stop offset='1' stop-color='#0000ff'/></linearGradient>\
             <linearGradient id='g' href='#base' x1='0' x2='1'/></defs><rect width='4' height='4' fill='url(#g)'/>");
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.stops, vec![(0.0, Rgba::rgb(255, 0, 0)), (1.0, Rgba::rgb(0, 0, 255))]);
        assert_eq!(g.kind, GradientKind::Linear { x1: 0.0, y1: 0.0, x2: 1.0, y2: 0.0 });
    }

    #[test]
    fn a_gradient_reads_its_units_space_and_transform() {
        let s = svg(
            "<defs><linearGradient id='g' gradientUnits='userSpaceOnUse' spreadMethod='reflect' gradientTransform='rotate(90)'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.units, Units::UserSpace);
        assert_eq!(g.spread, Spread::Reflect);
        assert!((g.transform.as_coeffs()[1] - 1.0).abs() < 1e-9, "{:?}", g.transform);
    }

    #[test]
    fn a_gradient_stop_opacity_and_the_shape_opacity_reach_the_stops() {
        let s = svg(
            "<defs><linearGradient id='g'><stop stop-color='#ffffff' stop-opacity='0.5'/><stop offset='1' stop-color='#000000'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)' fill-opacity='0.5'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.stops[0].1, Rgba::rgba(255, 255, 255, 64));
        assert_eq!(g.stops[1].1, Rgba::rgba(0, 0, 0, 128));
    }

    #[test]
    fn a_reference_to_something_that_is_not_a_gradient_still_paints() {
        let s = svg("<defs><pattern id='p'><rect width='2' height='2'/></pattern></defs><rect width='4' height='4' fill='url(#p)'/>");
        assert!(matches!(s.items[0].fill, Some(Paint::Solid(_))));
        let s = svg("<rect width='4' height='4' fill='url(#missing)'/>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::BLACK)));
    }

    #[test]
    fn size_comes_from_width_and_height_else_the_view_box() {
        let s = parse(b"<svg width='48px' height='24px' viewBox='0 0 2 1'/>").expect("reads");
        assert_eq!(s.size, (48.0, 24.0));
        let s = parse(b"<svg viewBox='0 0 12 6'/>").expect("reads");
        assert_eq!(s.size, (12.0, 6.0));
        let s = parse(b"<svg width='10'/>").expect("reads");
        assert_eq!(s.size.0, 10.0);
    }

    #[test]
    fn circles_and_rounded_rectangles_become_paths() {
        let s = svg("<circle cx='5' cy='5' r='2' fill='black'/><rect width='10' height='4' rx='2' fill='black'/>");
        assert_eq!(s.items.len(), 2);
        assert!(s.items[0].path.elements().len() > 3);
        assert!(s.items[1].path.elements().len() > 4);
    }

    #[test]
    fn definitions_and_text_are_not_drawn() {
        let s = svg("<defs><rect width='4' height='4'/></defs><title>t</title>");
        assert!(s.items.is_empty());
    }

    #[test]
    fn refuses_documents_over_the_size_cap() {
        let mut big = b"<svg viewBox='0 0 1 1'>".to_vec();
        big.resize(MAX_BYTES + 1, b' ');
        assert!(parse(&big).is_none());
    }

    #[test]
    fn sniffing_accepts_svg_and_refuses_other_data() {
        assert!(is_svg(b"\xEF\xBB\xBF<?xml version='1.0'?><svg xmlns='http://www.w3.org/2000/svg'/>"));
        assert!(is_svg(b"  <SVG viewBox='0 0 1 1'/>"));
        assert!(is_svg(b"<s:svg xmlns:s='http://www.w3.org/2000/svg'><s:rect width='1' height='1'/></s:svg>"));
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"text before <svg"));
        assert!(!is_svg(b"<svga viewBox='0 0 1 1'/>"));
        assert!(!parse(b"<html><body/></html>").is_some());
    }
}
