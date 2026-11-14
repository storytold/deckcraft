//! SVG pictures (`image/svg+xml`) read into shapes, text runs and markers: paths, basic shapes,
//! groups, transforms, `use`, solid and gradient paints, strokes, `text` with its `tspan`s, and
//! `marker-start`/`-mid`/`-end`.
//!
//! What a presentation cannot show is not read: filters (glow and shadow), masks, clip paths,
//! patterns, animation and scripting are skipped. A gradient keeps its stops and its own
//! coordinate space, and the renderer draws the ramp. Text is laid out by the renderer, which has
//! the fonts; this reader records positions and styles.
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
use kurbo::{Affine, BezPath, ParamCurve as _, ParamCurveDeriv as _, Point, Rect, Shape as _};

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
/// Most characters read from one text element.
const MAX_TEXT_CHARS: usize = 64 * 1024;

/// Elements that only define something and are drawn through `use`.
const DEFINITIONS: [&str; 15] = [
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
    "image",
];

/// An SVG document as the renderer draws it.
#[derive(Clone, Debug, Default)]
pub struct Svg {
    /// The size the picture asks for, in 96-dpi pixels.
    pub size: (f64, f64),
    /// Features this picture uses that nothing here draws. A caller that has a raster fallback
    /// should prefer it, because the drawing would be missing them.
    pub skipped: Skipped,
    /// `viewBox` as `[x, y, width, height]` in user units.
    pub view_box: Option<[f64; 4]>,
    /// Every shape, in painting order, already in user units.
    pub items: Vec<Item>,
}

/// The features a picture uses that this renderer does not draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub filters: bool,
    pub masks: bool,
    pub clip_paths: bool,
    pub patterns: bool,
    pub images: bool,
}

impl Skipped {
    /// Whether anything at all had to be left out.
    pub fn any(&self) -> bool {
        self.filters || self.masks || self.clip_paths || self.patterns || self.images
    }
}

/// One filled and/or stroked shape, or a text element.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: BezPath,
    pub even_odd: bool,
    pub fill: Option<Paint>,
    pub stroke: Option<(Paint, kurbo::Stroke)>,
    /// Set when the item is a text element: `path` is empty and the runs are laid out by the
    /// renderer, which has the fonts.
    pub text: Option<Text>,
}

/// A text element: runs of characters on one or more baselines, with the transform the element
/// sits under.
#[derive(Clone, Debug)]
pub struct Text {
    pub runs: Vec<TextRun>,
    pub transform: Affine,
}

/// Characters drawn with one style. A run without `at_x`/`at_y` continues the line where the run
/// before it ended; the renderer knows the advances, this reader does not.
#[derive(Clone, Debug)]
pub struct TextRun {
    /// The characters, with whitespace collapsed the way SVG lays text out.
    pub text: String,
    /// Absolute positions the run sets (`x`/`y` on a `text` or `tspan`).
    pub at_x: Option<f64>,
    pub at_y: Option<f64>,
    /// Relative shift of the pen (`dx`/`dy`).
    pub dx: f64,
    pub dy: f64,
    pub size: f64,
    /// Families asked for, in order; the renderer picks the first it has.
    pub families: Vec<String>,
    pub bold: bool,
    pub italic: bool,
    pub anchor: Anchor,
    /// Extra space after each character.
    pub spacing: f64,
    /// `xml:space="preserve"`: the whitespace at the element's ends stays.
    pub preserve: bool,
    pub fill: Option<Paint>,
    pub stroke: Option<(Paint, kurbo::Stroke)>,
}

/// How a line of text is aligned on the position it starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
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
    let viewport = view_box.map(|v| (v[2], v[3])).unwrap_or(size);
    let mut build = Build {
        items: Vec::new(),
        ids,
        viewport,
        uses: 0,
        visits: 0,
        segments: 0,
        marker_depth: 0,
        text_chars: 0,
        paths: HashMap::new(),
        skipped: Skipped::default(),
    };
    build.walk(&root, &Frame { transform: Affine::IDENTITY, style: Style::default() }, 0, true);
    Some(Svg { size, view_box, skipped: build.skipped, items: build.items })
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

// shortcut: an element with an id is deep-copied into the map, so a definitions-heavy picture
// copies more than it draws; resolve ids as node paths if that ever shows up in profiles.
fn collect_ids(node: &Node, out: &mut HashMap<String, Arc<Node>>) {
    if let Some(id) = node.attr("id").filter(|i| !i.is_empty()) {
        out.entry(id.to_string()).or_insert_with(|| Arc::new(node.clone()));
    }
    for k in &node.kids {
        collect_ids(k, out);
    }
}

/// Fill, stroke and the text properties inherited down the tree, plus the opacity of the groups
/// above.
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
    /// Text properties, inherited by `text` and `tspan`.
    font: Font,
    /// Marker definitions an element draws at its start, middle and end vertices.
    marker_start: Option<String>,
    marker_mid: Option<String>,
    marker_end: Option<String>,
}

/// The text properties a `text` element inherits, with SVG's initial values.
#[derive(Clone, Debug)]
struct Font {
    families: Vec<String>,
    /// In user units.
    size: f64,
    bold: bool,
    italic: bool,
    anchor: Anchor,
    spacing: f64,
    /// `xml:space="preserve"` keeps whitespace as written.
    preserve: bool,
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
            font: Font { families: Vec::new(), size: 16.0, bold: false, italic: false, anchor: Anchor::Start, spacing: 0.0, preserve: false },
            marker_start: None,
            marker_mid: None,
            marker_end: None,
        }
    }
}

impl Style {
    fn resolve(&self, node: &Node, ids: &HashMap<String, Arc<Node>>, viewport: (f64, f64)) -> Style {
        let mut s = self.clone();
        // `opacity` belongs to the element that sets it: what an ancestor set composites the
        // subtree instead of being replaced by a descendant's own value.
        s.group_opacity *= self.opacity;
        s.opacity = 1.0;
        // `em` in a font size measures against the size inherited, while `em` anywhere else
        // measures against the size this element computed, whatever order the declarations come
        // in. So the size is resolved first, then everything else.
        s.declare(node, ids, viewport, Some(s.font.size));
        s.declare(node, ids, viewport, None);
        s
    }

    /// Apply the element's own declarations: the attributes first, then the `style` attribute,
    /// which wins as CSS does. `only_size` resolves the font size and nothing else.
    fn declare(&mut self, node: &Node, ids: &HashMap<String, Arc<Node>>, viewport: (f64, f64), size_pass: Option<f64>) {
        let mut decls: Vec<(&str, &str)> = Vec::new();
        for (k, v) in &node.attrs {
            if k != "style" {
                decls.push((k.as_str(), v.as_str()));
            }
        }
        if let Some(d) = node.attr("style") {
            for decl in d.split(';') {
                if let Some((k, v)) = decl.split_once(':') {
                    decls.push((k.trim(), v.trim()));
                }
            }
        }
        for (k, v) in decls {
            match size_pass {
                Some(inherited) if k == "font-size" => self.set(k, v, ids, viewport, inherited),
                Some(_) => {}
                None if k == "font-size" => {}
                None => {
                    let em = self.font.size;
                    self.set(k, v, ids, viewport, em)
                }
            }
        }
    }

    /// Apply one declaration. `em` is the size an `em` or `ex` in it measures against.
    fn set(&mut self, key: &str, v: &str, ids: &HashMap<String, Arc<Node>>, viewport: (f64, f64), em: f64) {
        let opacity = |v: &str| {
            {
                let v = v.trim();
                if let Some(p) = v.strip_suffix('%') { p.parse::<f64>().map(|p| p / 100.0) } else { v.parse::<f64>() }
            }
            .ok()
            .map(|v: f64| v.clamp(0.0, 1.0))
        };
        match key {
            "fill" => self.fill = paint(v, ids, viewport),
            "stroke" => self.stroke = paint(v, ids, viewport),
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
                let mut d = attr::numbers(v);
                if d.iter().any(|v| *v < 0.0) || !d.iter().any(|v| *v > 0.0) {
                    d.clear();
                }
                // An odd number of values repeats to make the pattern even, as SVG says.
                if d.len() == 1 {
                    let v = d[0];
                    d.push(v);
                } else if d.len() % 2 == 1 {
                    let first = d[0];
                    d.push(first);
                }
                self.dash = (!d.is_empty()).then_some(d);
            }
            "fill-rule" => self.even_odd = v.trim() == "evenodd",
            "fill-opacity" => self.fill_opacity = opacity(v).unwrap_or(1.0),
            "stroke-opacity" => self.stroke_opacity = opacity(v).unwrap_or(1.0),
            "opacity" => self.opacity = opacity(v).unwrap_or(1.0),
            "display" => self.display = !v.trim().eq_ignore_ascii_case("none"),
            "visibility" => self.visibility = !matches!(v.trim(), "hidden" | "collapse"),
            "font-family" => {
                let families: Vec<String> =
                    v.split(',').map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_string()).filter(|s| !s.is_empty()).collect();
                if !families.is_empty() {
                    self.font.families = families;
                }
            }
            "font-size" => {
                let v = v.trim();
                let size = match v.strip_suffix('%') {
                    Some(p) => p.trim().parse::<f64>().ok().map(|p| em * p / 100.0),
                    None => attr::length_em(v, em),
                };
                if let Some(s) = size.filter(|s: &f64| s.is_finite() && *s > 0.0) {
                    self.font.size = s.clamp(0.1, MAX_COORD);
                }
            }
            "font-weight" => {
                let v = v.trim().to_ascii_lowercase();
                self.font.bold = match v.as_str() {
                    "bold" | "bolder" => true,
                    "normal" | "lighter" => false,
                    _ => v.parse::<f64>().map(|w| w >= 600.0).unwrap_or(self.font.bold),
                };
            }
            "font-style" => self.font.italic = matches!(v.trim(), "italic" | "oblique"),
            "text-anchor" => {
                self.font.anchor = match v.trim() {
                    "middle" => Anchor::Middle,
                    "end" => Anchor::End,
                    _ => Anchor::Start,
                }
            }
            "letter-spacing" => self.font.spacing = attr::length_em(v, em).filter(|v| v.is_finite()).map(|v| v.clamp(-1e3, 1e3)).unwrap_or(0.0),
            "xml:space" => self.font.preserve = v.trim() == "preserve",
            "marker-start" => self.marker_start = marker_ref(v),
            "marker-mid" => self.marker_mid = marker_ref(v),
            "marker-end" => self.marker_end = marker_ref(v),
            _ => {}
        }
    }
}

/// A paint value: solid, or a gradient whose stops the renderer ramps between. A reference to
/// anything else (a pattern, a filter) falls back to the average of its stops, and an unreadable
/// colour is black, as an unreadable colour name is in a browser.
fn paint(v: &str, ids: &HashMap<String, Arc<Node>>, viewport: (f64, f64)) -> Option<Paint> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(inner) = v.strip_prefix("url(").and_then(|r| r.strip_suffix(')')) {
        let id = inner.trim().trim_matches(|c| c == '\'' || c == '"').trim().strip_prefix('#').unwrap_or(inner.trim());
        let Some(node) = ids.get(id) else { return Some(Paint::Solid(Rgba::BLACK)) };
        // shortcut: a gradient is read again for every element that references it, rather than
        // cached by id; the parse is small and its result is cloned anyway.
        if let Some(g) = gradient_of(node, ids, viewport) {
            return Some(Paint::Gradient(g));
        }
        return Some(Paint::Solid(gradient_color(node, ids).unwrap_or(Rgba::BLACK)));
    }
    Some(Paint::Solid(attr::color(v).or_else(|| attr::named(v)).unwrap_or(Rgba::BLACK)))
}

/// The id a `marker-*` property names, or `None` for `none` and anything unreadable.
fn marker_ref(v: &str) -> Option<String> {
    let inner = v.trim().strip_prefix("url(")?.strip_suffix(')')?;
    let id = inner.trim().trim_matches(|c| c == '\'' || c == '"').trim().strip_prefix('#')?;
    (!id.is_empty()).then(|| id.to_string())
}

/// The gradient a `url(#…)` names, or `None` when the element is not a gradient this reader
/// understands (or has no usable stops).
fn gradient_of(node: &Node, ids: &HashMap<String, Arc<Node>>, viewport: (f64, f64)) -> Option<Gradient> {
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
    // SVG's own defaults: `0,0 → 100%,0` for a linear ramp, `50%,50%,50%` for the circle.
    let (full_x, half_x, half_y, half_r) = match units {
        Units::ObjectBoundingBox => (1.0, 0.5, 0.5, 0.5),
        Units::UserSpace => (viewport.0, viewport.0 / 2.0, viewport.1 / 2.0, viewport.0.hypot(viewport.1) / 2.0 / std::f64::consts::SQRT_2),
    };
    let kind = if linear {
        GradientKind::Linear {
            x1: coord(&chain, "x1", 0.0, units, viewport, Axis::X),
            y1: coord(&chain, "y1", 0.0, units, viewport, Axis::Y),
            x2: coord(&chain, "x2", full_x, units, viewport, Axis::X),
            y2: coord(&chain, "y2", 0.0, units, viewport, Axis::Y),
        }
    } else {
        let cx = coord(&chain, "cx", half_x, units, viewport, Axis::X);
        let cy = coord(&chain, "cy", half_y, units, viewport, Axis::Y);
        let r = coord(&chain, "r", half_r, units, viewport, Axis::Radius).clamp(0.0, MAX_COORD);
        let fx = coord(&chain, "fx", cx, units, viewport, Axis::X);
        let fy = coord(&chain, "fy", cy, units, viewport, Axis::Y);
        GradientKind::Radial { cx, cy, r, fx, fy }
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

/// A gradient coordinate: a percentage is a fraction of the bounding box (SVG's default units) or
/// of the viewport (`userSpaceOnUse`), and a bare number keeps the units it names.
fn coord(chain: &[&Node], key: &str, default: f64, units: Units, viewport: (f64, f64), axis: Axis) -> f64 {
    let Some(v) = chain_attr(chain, key).map(str::trim) else { return default };
    let base = match (units, axis) {
        (Units::ObjectBoundingBox, _) => 1.0,
        (Units::UserSpace, Axis::X) => viewport.0,
        (Units::UserSpace, Axis::Y) => viewport.1,
        // SVG 1.1: a percentage radius is a fraction of the diagonal over the square root of two.
        (Units::UserSpace, Axis::Radius) => viewport.0.hypot(viewport.1) / std::f64::consts::SQRT_2,
    };
    let v = match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|p| p / 100.0 * base),
        None => attr::length(v),
    };
    v.filter(|v: &f64| v.is_finite()).map(|v| v.clamp(-MAX_COORD, MAX_COORD)).unwrap_or(default)
}

/// Which axis of the viewport a percentage coordinate is measured on.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Radius,
}

/// The stops of the first gradient in `chain` that has any, in document order, each offset at
/// least the one before it.
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
            let declared = style_value(k, "stop-color").or_else(|| k.attr("stop-color").map(str::to_string));
            let c = declared.as_deref().and_then(|v| attr::color(v).or_else(|| attr::named(v))).unwrap_or(Rgba::BLACK);
            let a = style_value(k, "stop-opacity")
                .or_else(|| k.attr("stop-opacity").map(str::to_string))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            out.push((offset, c.with_alpha(mul_alpha(c.a, a))));
        }
        if !out.is_empty() {
            // SVG keeps the stops in document order and raises an offset that falls behind the
            // one before it; it does not reorder them.
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
            let a = style_value(k, "stop-opacity")
                .or_else(|| k.attr("stop-opacity").map(str::to_string))
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

/// The id a `url(#id)` reference names.
fn url_id(raw: &str) -> Option<&str> {
    let rest = raw.trim().strip_prefix("url(")?;
    let inner = rest.split(')').next()?.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    inner.strip_prefix('#')
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
    /// The picture's own box, which a percentage in `userSpaceOnUse` is a fraction of.
    viewport: (f64, f64),
    uses: u32,
    /// Marker instances being drawn, so a marker that references itself stops instead of
    /// recursing until the stack dies.
    marker_depth: u32,
    /// Characters kept across every text element, so repeated references to one label are bounded.
    text_chars: usize,
    /// Elements visited, so a reference graph that expands without painting anything stops too.
    visits: usize,
    /// Shapes already parsed, so a path referenced by many `use` elements is parsed once.
    paths: HashMap<usize, Option<kurbo::BezPath>>,
    /// What the parser had to leave out.
    skipped: Skipped,
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
        // An embedded image is not drawn, so the drawing is incomplete whether or not the element
        // is reached through a definition.
        if node.name == "image" {
            self.skipped.images = true;
        }
        if !via_use && DEFINITIONS.contains(&node.name.as_str()) {
            return;
        }
        let style = f.style.resolve(node, &self.ids, self.viewport);
        // A reference to an effect we do not draw means the drawing is incomplete, and a caller
        // holding the picture's raster copy should keep it.
        // A `pattern` fill falls back to nothing, which is a picture that would be blank.
        for attr in ["fill", "stroke"] {
            let raw = node.attr(attr).map(str::to_string).or_else(|| style_value(node, attr));
            let Some(id) = raw.as_deref().and_then(url_id) else { continue };
            if self.ids.get(id).map(|n| n.name == "pattern").unwrap_or(false) {
                self.skipped.patterns = true;
            }
        }
        for (attr, flag) in [("filter", 0), ("mask", 1), ("clip-path", 2)] {
            let set =
                node.attr(attr).map(|v| v.contains("url(")).unwrap_or(false) || style_value(node, attr).map(|v| v.contains("url(")).unwrap_or(false);
            if set {
                match flag {
                    0 => self.skipped.filters = true,
                    1 => self.skipped.masks = true,
                    _ => self.skipped.clip_paths = true,
                }
            }
        }
        // `display: none` hides the subtree; `visibility: hidden` leaves descendants able to show
        // themselves, so it is applied where a shape is painted.
        if !style.display {
            return;
        }
        let mut transform = f.transform;
        if node.name == "svg" {
            let at = |k: &str| node.attr(k).and_then(attr::length).unwrap_or(0.0);
            transform *= Affine::translate((at("x"), at("y")));
            // A nested `svg` establishes its own viewport: a view box maps into the width and
            // height it is given (the view box's own size when neither is written).
            if let Some(vb) = view_box_of(node) {
                let w = node.attr("width").and_then(attr::length).unwrap_or(vb[2]);
                let h = node.attr("height").and_then(attr::length).unwrap_or(vb[3]);
                if w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite() {
                    transform *= fit(vb, w, h);
                }
            }
        }
        transform *= attr::transform(node.attr("transform").unwrap_or(""));
        let frame = Frame { transform, style };
        match node.name.as_str() {
            "g" | "svg" | "symbol" | "a" | "switch" | "marker" => {
                for k in &node.kids {
                    self.walk(k, &frame, depth + 1, false);
                }
            }
            "use" => self.use_node(node, &frame, depth),
            "text" => self.text(node, &frame),
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
        let key = node as *const Node as usize;
        let cached = match self.paths.get(&key) {
            Some(hit) => hit.clone(),
            None => {
                let parsed = shape_path(node);
                self.paths.insert(key, parsed.clone());
                parsed
            }
        };
        let Some(mut path) = cached else { return };
        let segments = path.elements().len();
        if self.segments.saturating_add(segments) > MAX_SEGMENTS_TOTAL {
            return;
        }
        let bb = path.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite()) {
            return;
        }
        // Vertices are in user units, before the element transform, because a marker is placed
        // there and the transform applies to it as well.
        let verts = if f.style.marker_start.is_none() && f.style.marker_mid.is_none() && f.style.marker_end.is_none() {
            Vec::new()
        } else {
            marker_vertices(&path)
        };
        path.apply_affine(f.transform);
        // The pen is scaled with the shape; a non-uniform transform cannot scale it evenly, so the
        // mean of both axes stands in.
        let scale = f.transform.determinant().abs().sqrt();
        let opacity = f.style.group_opacity * f.style.opacity;
        let fill = f.style.fill.clone().and_then(|p| world(p.faded(f.style.fill_opacity * opacity), f.transform, bb));
        let stroke = f.style.stroke.clone().filter(|_| f.style.width > 0.0).and_then(|p| {
            let mut pen = kurbo::Stroke::new((f.style.width * scale).max(1e-4)).with_caps(f.style.cap).with_join(f.style.join);
            if let Some(d) = &f.style.dash {
                pen = pen.with_dashes(0.0, d.iter().map(|v| v * scale));
            }
            world(p.faded(f.style.stroke_opacity * opacity), f.transform, bb).map(|p| (p, pen))
        });
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.segments += segments;
        self.items.push(Item { path, even_odd: f.style.even_odd, fill, stroke, text: None });
        if !verts.is_empty() {
            self.markers(f, &verts);
        }
    }

    /// Draw the markers the shape's style names at its vertices, each turned to the path there.
    fn markers(&mut self, f: &Frame, verts: &[Vertex]) {
        for (at, id) in [(At::Start, &f.style.marker_start), (At::Mid, &f.style.marker_mid), (At::End, &f.style.marker_end)] {
            let Some(marker) = id.as_deref().and_then(|id| self.ids.get(id)).cloned() else { continue };
            if marker.name != "marker" {
                continue;
            }
            let pen = f.style.width;
            // The marker's own style, not the shape's fill and stroke: a definition inherits from
            // where it sits. The opacity of the shape it marks still fades the marker with it.
            let mut style = Style::default().resolve(&marker, &self.ids, self.viewport);
            style.group_opacity *= f.style.group_opacity * f.style.opacity;
            for (i, v) in verts.iter().enumerate() {
                if !at.matches(i, verts.len()) {
                    continue;
                }
                if self.marker_depth >= MAX_MARKER_DEPTH {
                    return;
                }
                let Some(m) = marker_frame(&marker, v, pen) else { continue };
                let frame = Frame { transform: f.transform * m, style: style.clone() };
                self.marker_depth += 1;
                self.walk(&marker, &frame, 0, true);
                self.marker_depth -= 1;
            }
        }
    }

    /// A `text` element: its own characters and every `tspan` under it, as runs the renderer lays
    /// out. The reader has no fonts, so it records where each run starts and the renderer
    /// advances the pen.
    fn text(&mut self, node: &Node, f: &Frame) {
        let mut runs = Vec::new();
        self.text_runs(node, &f.style, true, &mut runs, f.transform.determinant().abs().sqrt());
        // SVG drops the whitespace that starts and ends the element, unless it is preserved.
        if let Some(first) = runs.iter_mut().find(|r| !r.text.is_empty()).filter(|r| !r.preserve) {
            first.text = first.text.trim_start().to_string();
        }
        if let Some(last) = runs.iter_mut().rev().find(|r| !r.text.is_empty()).filter(|r| !r.preserve) {
            last.text = last.text.trim_end().to_string();
        }
        runs.retain(|r| !r.text.is_empty());
        // A run that only held whitespace is gone; the element's own position now belongs to the
        // first run that survived, unless a nearer `tspan` already set one.
        let at = |k: &str| node.attr(k).and_then(attr::length).filter(|v| v.is_finite());
        if let Some(first) = runs.first_mut() {
            if first.at_x.is_none() {
                first.at_x = Some(at("x").unwrap_or(0.0));
            }
            if first.at_y.is_none() {
                first.at_y = Some(at("y").unwrap_or(0.0));
            }
            first.dx += at("dx").unwrap_or(0.0);
            first.dy += at("dy").unwrap_or(0.0);
        }
        if runs.is_empty() {
            return;
        }
        self.items.push(Item { path: BezPath::new(), even_odd: false, fill: None, stroke: None, text: Some(Text { runs, transform: f.transform }) });
    }

    /// Append `node`'s characters and those of the `tspan`s under it. `parent` is the style in
    /// force above the node, `root` tells whether the node is the `text` element itself.
    fn text_runs(&mut self, node: &Node, parent: &Style, root: bool, out: &mut Vec<TextRun>, scale: f64) {
        self.visits += 1;
        if self.text_chars >= MAX_TEXT_CHARS || self.visits > MAX_VISITS {
            return;
        }
        let style = if root { parent.clone() } else { parent.resolve(node, &self.ids, self.viewport) };
        // `display: none` hides the subtree, but `visibility: hidden` leaves a child able to show
        // itself, so that one is applied where characters are kept.
        if !style.display {
            return;
        }
        let opacity = style.group_opacity * style.opacity;
        let at = |k: &str| node.attr(k).and_then(attr::length).filter(|v| v.is_finite());
        let rel = |k: &str| node.attr(k).and_then(attr::length).filter(|v| v.is_finite()).unwrap_or(0.0);
        let (at_x, at_y) = if root { (Some(at("x").unwrap_or(0.0)), Some(at("y").unwrap_or(0.0))) } else { (at("x"), at("y")) };
        let fill = style.fill.clone().map(|p| p.faded(style.fill_opacity * opacity));
        let stroke = style.stroke.clone().filter(|_| style.width > 0.0).map(|p| {
            let mut pen = kurbo::Stroke::new(style.width.max(1e-4)).with_caps(style.cap).with_join(style.join);
            if let Some(d) = &style.dash {
                pen = pen.with_dashes(0.0, d.iter().copied());
            }
            (p.faded(style.stroke_opacity * opacity), pen)
        });
        let start = out.len();
        for kid in &node.kids {
            match kid.name.as_str() {
                "#text" if style.visibility => {
                    let mut text = if style.font.preserve { kid.text.clone() } else { collapse(&kid.text) };
                    // Only the characters still inside the document's budget are kept, cut at a
                    // character boundary.
                    let room = MAX_TEXT_CHARS.saturating_sub(self.text_chars);
                    if text.len() > room {
                        // The cut has to land on a character boundary: `truncate` panics on a byte
                        // inside a multi-byte character.
                        let mut cut = room.min(text.len());
                        while cut > 0 && !text.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        text.truncate(cut);
                    }
                    if text.is_empty() || room == 0 {
                        continue;
                    }
                    self.text_chars += text.len();
                    // A space that the previous run already ended with would double it, unless the
                    // document asked for its whitespace to be preserved.
                    if !style.font.preserve && text.starts_with(' ') && out.last().is_some_and(|r| r.text.ends_with(' ')) {
                        text.remove(0);
                    }
                    out.push(TextRun {
                        text,
                        at_x: None,
                        at_y: None,
                        dx: 0.0,
                        dy: 0.0,
                        size: style.font.size,
                        families: style.font.families.clone(),
                        bold: style.font.bold,
                        italic: style.font.italic,
                        anchor: style.font.anchor,
                        spacing: style.font.spacing,
                        preserve: style.font.preserve,
                        fill: fill.clone(),
                        stroke: stroke.clone(),
                    });
                }
                "tspan" => self.text_runs(kid, &style, false, out, scale),
                _ => {}
            }
        }
        // `x`/`y`/`dx`/`dy` on the element place the first glyph it contributes, unless a nearer
        // `tspan` already placed it: the nearest positioning element wins. The `text` element
        // itself is placed once by [`Build::text`], after the whitespace trim.
        if !root && let Some(run) = out.get_mut(start) {
            if run.at_x.is_none() {
                run.at_x = at_x;
            }
            if run.at_y.is_none() {
                run.at_y = at_y;
            }
            run.dx += rel("dx");
            run.dy += rel("dy");
        }
    }
}

/// SVG's whitespace rule: runs of whitespace become one space, line breaks with them. A leading
/// space survives here because it separates this run from the one before it; the element trims the
/// spaces at its own ends.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            space = true;
            continue;
        }
        if space {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    if space {
        out.push(' ');
    }
    out
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

/// Where a marker sits on a path: a vertex and the direction the path runs there, in degrees.
struct Vertex {
    point: Point,
    angle: f64,
    /// The first vertex of the path.
    first: bool,
    /// The last vertex of the path.
    last: bool,
}

/// Which vertices a `marker-*` property paints.
#[derive(Clone, Copy)]
enum At {
    Start,
    Mid,
    End,
}

impl At {
    fn matches(self, i: usize, n: usize) -> bool {
        match self {
            At::Start => i == 0,
            At::Mid => i > 0 && i + 1 < n,
            At::End => n > 0 && i + 1 == n,
        }
    }
}

/// Most markers drawn on one shape; a path of thousands of points (a map outline) would otherwise
/// fill the picture with arrowheads. The first and last vertex always stay.
const MAX_MARKERS: usize = 256;
/// Most marker instances that may sit inside each other before a self-referencing marker stops.
const MAX_MARKER_DEPTH: u32 = 4;

/// The direction a segment leaves (`at_end` false) or arrives at, in degrees. A curve is measured
/// on the tangent there, not on its chord: an arrowhead at the end of an arc must follow the arc.
fn tangent(seg: &kurbo::PathSeg, at_end: bool) -> f64 {
    let t = if at_end { 1.0 } else { 0.0 };
    let chord = seg.end() - seg.start();
    let d = match seg {
        kurbo::PathSeg::Line(l) => l.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let d = if d.hypot() > 1e-12 { d } else { chord };
    if d.hypot() > 1e-12 { d.y.atan2(d.x).to_degrees() } else { 0.0 }
}

/// The direction between two directions, taking the shorter way round.
fn bisect(a: f64, b: f64) -> f64 {
    let d = (b - a).to_radians();
    let d = (d + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;
    a + d.to_degrees() / 2.0
}

/// The vertices of a path with the direction the path runs through each one.
fn marker_vertices(path: &BezPath) -> Vec<Vertex> {
    // shortcut: every segment is collected (up to the path cap) even though only MAX_MARKERS
    // vertices are kept; iterate lazily if a path-heavy picture ever shows up in profiles.
    let segs: Vec<kurbo::PathSeg> = path.segments().collect();
    let mut out: Vec<Vertex> = Vec::with_capacity(segs.len().min(MAX_MARKERS) + 1);
    let Some(first) = segs.first() else { return out };
    out.push(Vertex { point: first.start(), angle: tangent(first, false), first: true, last: false });
    // An interior vertex points along the bisector of the segments it joins, up to the cap.
    let last = segs.len() - 1;
    for i in 0..last.min(MAX_MARKERS.saturating_sub(1)) {
        let angle = bisect(tangent(&segs[i], true), tangent(&segs[i + 1], false));
        out.push(Vertex { point: segs[i].end(), angle, first: false, last: false });
    }
    // The end vertex is always drawn, even when the interior was capped.
    let end = &segs[last];
    out.push(Vertex { point: end.end(), angle: tangent(end, true), first: false, last: true });
    out
}

/// The place a marker sits: at the vertex, turned by `orient`, sized by `markerUnits` and its
/// view box, with its reference point on the vertex.
fn marker_frame(marker: &Node, v: &Vertex, pen: f64) -> Option<Affine> {
    let orient = marker.attr("orient").unwrap_or("0").trim();
    let angle = match orient {
        "auto" => v.angle,
        "auto-start-reverse" if v.first => v.angle + 180.0,
        "auto-start-reverse" => v.angle,
        other => other.parse::<f64>().ok().filter(|a: &f64| a.is_finite()).unwrap_or(0.0),
    };
    let size = |k: &str| marker.attr(k).and_then(attr::length).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(3.0).min(MAX_COORD);
    let scale = match marker.attr("markerUnits").map(str::trim) {
        Some("userSpaceOnUse") => 1.0,
        _ => pen.clamp(1e-4, MAX_COORD),
    };
    let (w, h) = (size("markerWidth") * scale, size("markerHeight") * scale);
    let at = |k: &str, d: f64| marker.attr(k).and_then(attr::length).filter(|v| v.is_finite()).unwrap_or(d);
    let (rx, ry) = (at("refX", 0.0).clamp(-MAX_COORD, MAX_COORD), at("refY", 0.0).clamp(-MAX_COORD, MAX_COORD));
    // A marker with no viewport paints nothing, as SVG says.
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let mut m = Affine::translate(v.point.to_vec2()) * Affine::rotate(angle.to_radians());
    m *= match view_box_of(marker) {
        Some(vb) => fit(vb, w, h),
        None => Affine::scale(scale),
    };
    Some(m * Affine::translate((-rx, -ry)))
}

/// A paint in the picture's own user space: an element's transform applies to its paint as it does
/// to its geometry, and a bounding-box gradient runs over the element's box as it was declared. A
/// bounding-box gradient on a shape with no area paints nothing, as SVG says.
fn world(paint: Paint, transform: Affine, bbox: Rect) -> Option<Paint> {
    match paint {
        Paint::Solid(c) => Some(Paint::Solid(c)),
        Paint::Gradient(mut g) => {
            g.transform = match g.units {
                Units::ObjectBoundingBox => {
                    if !(bbox.width() > 0.0 && bbox.height() > 0.0) {
                        return None;
                    }
                    transform * Affine::translate(bbox.origin().to_vec2()) * Affine::scale_non_uniform(bbox.width(), bbox.height()) * g.transform
                }
                Units::UserSpace => transform * g.transform,
            };
            // The coordinates are in the picture's user space now, whatever they were measured on.
            g.units = Units::UserSpace;
            Some(Paint::Gradient(g))
        }
    }
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
    fn lime_is_full_green_and_green_is_half() {
        let s = svg("<rect width='4' height='4' fill='lime'/><rect width='4' height='4' fill='green'/>");
        assert_eq!(s.items[0].fill, Some(Paint::Solid(Rgba::rgb(0, 255, 0))));
        assert_eq!(s.items[1].fill, Some(Paint::Solid(Rgba::rgb(0, 128, 0))));
    }

    #[test]
    fn cdata_stays_literal() {
        let s = svg("<text x='0' y='0'><![CDATA[a &amp; b]]></text>");
        assert_eq!(s.items.last().and_then(|i| i.text.as_ref()).map(|t| t.runs[0].text.as_str()), Some("a &amp; b"));
    }

    #[test]
    fn one_dash_value_repeats_and_zero_dashes_are_allowed() {
        let s = svg("<path d='M0 0 L4 4' stroke='black' stroke-width='1' stroke-dasharray='2'/>");
        let (_, pen) = s.items[0].stroke.as_ref().expect("stroked");
        assert_eq!(pen.dash_pattern.as_slice(), &[2.0, 2.0][..], "a single value repeats");
        let s = svg("<path d='M0 0 L4 4' stroke='black' stroke-width='1' stroke-dasharray='4 0'/>");
        let (_, pen) = s.items[0].stroke.as_ref().expect("stroked");
        assert_eq!(pen.dash_pattern.as_slice(), &[4.0, 0.0][..], "a zero gap is a valid pattern");
    }

    #[test]
    fn a_percentage_font_size_measures_against_the_inherited_size() {
        let s = svg("<g font-size='16'><text x='0' y='0' font-size='10' style='font-size:200%'>A</text></g>");
        assert_eq!(s.items[0].text.as_ref().map(|t| t.runs[0].size), Some(32.0));
    }

    #[test]
    fn a_stop_style_wins_over_its_presentation_attributes() {
        let s = svg(
            "<defs><linearGradient id='g'><stop offset='0' stop-color='red' stop-opacity='0.2' style='stop-color:#00ff00;stop-opacity:1'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("gradient") };
        assert_eq!(g.stops[0], (0.0, Rgba::rgb(0, 255, 0)));
    }

    #[test]
    fn a_nested_svg_maps_its_view_box_into_its_viewport() {
        // A 24-unit box into 12 units halves the drawing, then `x` moves it right.
        let s = svg("<svg x='6' y='0' width='12' height='12' viewBox='0 0 24 24'><rect width='24' height='24'/></svg>");
        let path = &s.items[0].path;
        let b = path.bounding_box();
        let (x0, y0, x1, y1) = (b.x0, b.y0, b.x1, b.y1);
        assert!((x0 - 6.0).abs() < 1e-9 && (x1 - 18.0).abs() < 1e-9, "{x0}..{x1}");
        assert!((y0 - 0.0).abs() < 1e-9 && (y1 - 12.0).abs() < 1e-9, "{y0}..{y1}");
    }

    #[test]
    fn a_filter_a_mask_a_clip_path_or_an_image_is_reported() {
        let clean = svg("<rect width='4' height='4' fill='red'/>");
        assert!(!clean.skipped.any(), "a plain drawing is whole");
        let filtered = svg("<defs><filter id='f'><feGaussianBlur stdDeviation='2'/></filter></defs><rect width='4' height='4' filter='url(#f)'/>");
        assert!(filtered.skipped.filters);
        let masked = svg("<defs><mask id='m'><rect width='4' height='4' fill='white'/></mask></defs><rect width='4' height='4' mask='url(#m)'/>");
        assert!(masked.skipped.masks);
        let clipped = svg("<defs><clipPath id='c'><rect width='2' height='2'/></clipPath></defs><rect width='4' height='4' clip-path='url(#c)'/>");
        assert!(clipped.skipped.clip_paths);
        let patterned = svg(
            "<defs><pattern id='p' width='4' height='4'><rect width='4' height='4'/></pattern></defs><rect width='4' height='4' fill='url(#p)'/>",
        );
        assert!(patterned.skipped.patterns);
        let picture = svg("<image x='0' y='0' width='4' height='4' href='a.png'/>");
        assert!(picture.skipped.images);
    }

    #[test]
    fn a_path_named_by_many_uses_is_parsed_once() {
        // The same 2,000-element path under 400 references stays inside the segment budget only
        // when each node is parsed and charged once.
        let mut d = String::from("M0 0");
        for i in 0..2000 {
            d.push_str(&format!(" L{} {}", i % 24, i / 24));
        }
        let mut body = format!("<defs><path id='p' d='{d}' stroke='black'/></defs>");
        for i in 0..400 {
            body.push_str(&format!("<use href='#p' x='{}'/>", i % 24));
        }
        let s = svg(&body);
        assert_eq!(s.items.len(), 400, "every reference draws");
    }

    #[test]
    fn a_text_element_counts_against_the_visit_budget() {
        // 3,000 empty tspan sequences would run forever without the budget.
        let mut body = String::new();
        for _ in 0..3000 {
            body.push_str("<text x='0' y='0'></text>");
        }
        let s = svg(&body);
        assert!(s.items.len() <= 3000);
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
        assert_eq!(g.spread, Spread::Pad);
        assert_eq!(g.kind, GradientKind::Linear { x1: 0.0, y1: 0.0, x2: 0.0, y2: 1.0 });
        assert_eq!(g.stops, vec![(0.0, Rgba::rgb(0, 0, 0)), (1.0, Rgba::rgb(255, 255, 255))]);
        // The bounding box is folded into the transform, so the paint is in user space by now: the
        // unit square of a 4 x 4 rectangle.
        assert_eq!(g.units, Units::UserSpace);
        assert!((g.transform.determinant() - 16.0).abs() < 1e-9, "{:?}", g.transform);
    }

    #[test]
    fn a_gradient_moves_with_the_element_it_fills() {
        let s = svg(
            "<defs><linearGradient id='g' gradientUnits='userSpaceOnUse' x1='0' y1='0' x2='10' y2='0'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient></defs>\
             <rect width='10' height='10' transform='translate(20 5)' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.units, Units::UserSpace);
        let moved = g.transform * Point::new(0.0, 0.0);
        assert!((moved.x - 20.0).abs() < 1e-9 && (moved.y - 5.0).abs() < 1e-9, "{moved:?}");
    }

    #[test]
    fn a_percentage_in_user_space_is_measured_on_the_viewport() {
        let s = parse(
            "<svg viewBox='0 0 200 100' width='200' height='100'><defs><linearGradient id='g' gradientUnits='userSpaceOnUse' x1='0%' x2='100%'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient></defs><rect width='200' height='100' fill='url(#g)'/></svg>"
                .as_bytes(),
        )
        .expect("reads");
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.kind, GradientKind::Linear { x1: 0.0, y1: 0.0, x2: 200.0, y2: 0.0 });
    }

    #[test]
    fn a_bounding_box_gradient_on_a_line_with_no_area_paints_nothing() {
        // SVG: a bounding-box gradient on a shape with no area is not rendered.
        let s = svg(
            "<defs><linearGradient id='g'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient></defs><line x1='0' y1='0' x2='10' y2='0' stroke='url(#g)' stroke-width='2'/>",
        );
        assert!(s.items[0].stroke.is_none());
    }

    #[test]
    fn stops_keep_their_order_and_only_a_falling_offset_is_raised() {
        let s = svg(
            "<defs><linearGradient id='g'><stop offset='0' stop-color='#ff0000'/><stop offset='1' stop-color='#00ff00'/><stop offset='0.5' stop-color='#0000ff'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = &s.items[0].fill else { panic!("a gradient") };
        assert_eq!(g.stops.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0.0, 1.0, 1.0]);
        assert_eq!(g.stops[2].1, Rgba::rgb(0, 0, 255));
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
        // A text element inside a definition draws only where a `use` names it.
        let s = svg("<defs><text id='t' x='1' y='2'>hi</text></defs>");
        assert!(s.items.is_empty());
    }

    fn text_of(s: &Svg) -> &Text {
        s.items.iter().find_map(|i| i.text.as_ref()).expect("a text item")
    }

    #[test]
    fn a_text_element_becomes_a_run_with_its_position_and_style() {
        let s =
            svg("<text x='5' y='10' font-family='Arial, sans-serif' font-size='20' font-weight='700' text-anchor='middle' fill='#ff0000'>Hi</text>");
        let t = text_of(&s);
        assert_eq!(t.runs.len(), 1);
        let r = &t.runs[0];
        assert_eq!(r.text, "Hi");
        assert_eq!((r.at_x, r.at_y), (Some(5.0), Some(10.0)));
        assert_eq!(r.size, 20.0);
        assert_eq!(r.families, vec!["Arial", "sans-serif"]);
        assert!(r.bold && !r.italic);
        assert_eq!(r.anchor, Anchor::Middle);
        assert_eq!(r.fill, Some(Paint::Solid(Rgba::rgb(255, 0, 0))));
    }

    #[test]
    fn tspans_keep_their_own_style_and_continue_the_line() {
        let s = svg("<text x='0' y='10' font-size='20'>A<tspan font-size='8' fill='red' dx='2'>B</tspan>C</text>");
        let t = text_of(&s);
        assert_eq!(t.runs.len(), 3);
        assert_eq!(t.runs[0].text, "A");
        assert_eq!((t.runs[1].text.as_str(), t.runs[1].size, t.runs[1].dx), ("B", 8.0, 2.0));
        assert_eq!(t.runs[2].text, "C");
        // Only the first run is positioned; the others continue where the pen is.
        assert!(t.runs[1].at_x.is_none() && t.runs[1].at_y.is_none());
        assert!(t.runs[2].at_x.is_none());
        assert_eq!(t.runs[2].size, 20.0);
    }

    #[test]
    fn a_tspans_own_position_wins_over_its_parent() {
        let s = svg("<text x='5' y='9'><tspan x='50' dx='3'>A</tspan></text>");
        let r = &text_of(&s).runs[0];
        assert_eq!((r.at_x, r.at_y), (Some(50.0), Some(9.0)));
        assert_eq!(r.dx, 3.0);
    }

    #[test]
    fn the_element_position_goes_to_the_first_run_that_survives_the_trim() {
        let s = svg("<text x='5' y='9'> <tspan>B</tspan></text>");
        let t = text_of(&s);
        assert_eq!(t.runs.len(), 1, "the whitespace-only run is gone");
        assert_eq!((t.runs[0].at_x, t.runs[0].at_y), (Some(5.0), Some(9.0)));
    }

    #[test]
    fn character_references_keep_their_place_in_a_label() {
        let s = svg("<text x='0' y='10'>A&amp;B&#x2192;</text>");
        let joined: String = text_of(&s).runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "A&B\u{2192}");
    }

    #[test]
    fn a_space_at_a_run_boundary_is_not_doubled() {
        let s = svg("<text x='0' y='10'>A <tspan> B</tspan></text>");
        let t = text_of(&s);
        let joined: String = t.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "A B");
    }

    #[test]
    fn an_em_is_measured_against_the_font_size_in_force() {
        let s = svg("<g font-size='20'><text x='0' y='10' font-size='2em' letter-spacing='0.5em'>A</text></g>");
        let r = &text_of(&s).runs[0];
        assert_eq!(r.size, 40.0);
        assert_eq!(r.spacing, 20.0);
    }

    #[test]
    fn a_relative_font_size_measures_against_the_inherited_size() {
        // 2em of the inherited 16, not of the 20 the presentation attribute set first.
        let s = svg("<text x='0' y='10' font-size='20' style='font-size:2em'>A</text>");
        assert_eq!(text_of(&s).runs[0].size, 32.0);
    }

    #[test]
    fn an_em_letter_spacing_uses_the_size_the_element_computed() {
        for body in
            ["<text x='0' y='10' letter-spacing='0.5em' font-size='20'>A</text>", "<text x='0' y='10' font-size='20' letter-spacing='0.5em'>A</text>"]
        {
            let s = svg(body);
            assert_eq!(text_of(&s).runs[0].spacing, 10.0, "{body}");
        }
    }

    #[test]
    fn the_elements_own_displacement_is_applied_once() {
        let s = svg("<text x='10' y='5' dx='2' dy='3'>A</text>");
        let r = &text_of(&s).runs[0];
        assert_eq!((r.at_x, r.at_y, r.dx, r.dy), (Some(10.0), Some(5.0), 2.0, 3.0));
    }

    #[test]
    fn a_truncated_label_cut_inside_a_character_does_not_panic() {
        // Three bytes per character, so the byte budget lands inside one.
        let long = "\u{4e2d}".repeat(MAX_TEXT_CHARS / 3 + 10);
        let s = svg(&format!("<text x='0' y='10'>{long}</text>"));
        let runs = &text_of(&s).runs;
        let kept: usize = runs.iter().map(|r| r.text.len()).sum();
        assert!(kept <= MAX_TEXT_CHARS, "{kept}");
        assert!(runs.iter().all(|r| std::str::from_utf8(r.text.as_bytes()).is_ok()));
    }

    #[test]
    fn preserved_whitespace_keeps_its_spaces_across_runs() {
        let s = svg("<text x='0' y='10' xml:space='preserve'>A <tspan> B</tspan></text>");
        let joined: String = text_of(&s).runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "A  B");
    }

    #[test]
    fn a_visible_tspan_shows_inside_a_hidden_text() {
        let s = svg("<text x='0' y='10' visibility='hidden'><tspan visibility='visible'>A</tspan></text>");
        assert_eq!(text_of(&s).runs.len(), 1);
    }

    #[test]
    fn a_text_node_beyond_the_budget_is_cut_instead_of_kept() {
        let long = "a".repeat(MAX_TEXT_CHARS + 5000);
        let s = svg(&format!("<text x='0' y='10'>{long}</text>"));
        let kept: usize = text_of(&s).runs.iter().map(|r| r.text.len()).sum();
        assert!(kept <= MAX_TEXT_CHARS, "{kept}");
    }

    #[test]
    fn a_tspan_position_starts_a_new_chunk() {
        let s = svg("<text x='0' y='10'>A<tspan x='30'>B</tspan></text>");
        let t = text_of(&s);
        assert_eq!((t.runs[1].at_x, t.runs[1].at_y), (Some(30.0), None));
    }

    #[test]
    fn whitespace_collapses_and_the_ends_are_trimmed() {
        let s = svg("<text x='0' y='10'>  Metric\n   One  </text>");
        assert_eq!(text_of(&s).runs[0].text, "Metric One");
        let s = svg("<text x='0' y='10' xml:space='preserve'> A  B </text>");
        assert_eq!(text_of(&s).runs[0].text, " A  B ");
    }

    #[test]
    fn a_space_between_two_tspans_survives() {
        let s = svg("<text x='0' y='10'><tspan>A</tspan> <tspan>B</tspan></text>");
        let t = text_of(&s);
        let joined: String = t.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "A B");
    }

    #[test]
    fn a_marker_is_placed_at_the_end_of_its_path() {
        let s = svg(
            "<defs><marker id='ag' refX='8' refY='5' markerWidth='5' markerHeight='5' markerUnits='strokeWidth' orient='auto' viewBox='0 0 10 10'><path d='M0 0 10 5 0 10Z' fill='#ff0000'/></marker></defs><line x1='0' y1='0' x2='20' y2='0' stroke='black' stroke-width='2' marker-end='url(#ag)'/>",
        );
        assert_eq!(s.items.len(), 2, "the line and its arrowhead");
        let b = s.items[1].path.bounding_box();
        assert!((b.x0 - 12.0).abs() < 1e-6 && (b.x1 - 22.0).abs() < 1e-6, "{b:?}");
        assert!((b.y0 + 5.0).abs() < 1e-6 && (b.y1 - 5.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn a_marker_turns_with_the_path_it_ends() {
        let s = svg(
            "<defs><marker id='m' markerWidth='2' markerHeight='2' markerUnits='userSpaceOnUse' orient='auto' viewBox='0 0 10 10'><path d='M0 0 10 0 10 10Z' fill='black'/></marker></defs><line x1='0' y1='0' x2='0' y2='20' stroke='black' marker-end='url(#m)'/>",
        );
        let b = s.items[1].path.bounding_box();
        assert!((b.x0 + 2.0).abs() < 1e-6 && b.x1.abs() < 1e-6, "{b:?}");
        assert!((b.y0 - 20.0).abs() < 1e-6 && (b.y1 - 22.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn a_marker_with_no_viewport_or_no_reference_paints_nothing() {
        let s = svg(
            "<defs><marker id='m' markerWidth='0' markerHeight='2'><path d='M0 0 10 0 10 10Z' fill='black'/></marker></defs><line x1='0' y1='0' x2='9' y2='0' stroke='black' marker-end='url(#m)'/>",
        );
        assert_eq!(s.items.len(), 1, "only the line");
        let s = svg("<line x1='0' y1='0' x2='9' y2='0' stroke='black' marker-end='none'/>");
        assert_eq!(s.items.len(), 1);
        let s = svg("<line x1='0' y1='0' x2='9' y2='0' stroke='black' marker-end='url(#missing)'/>");
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn a_marker_that_references_itself_stops_instead_of_recursing() {
        let s = svg(
            "<defs><marker id='m' markerWidth='2' markerHeight='2' viewBox='0 0 2 2'><path d='M0 0 2 0 2 2Z' fill='black' marker-start='url(#m)'/></marker></defs><line x1='0' y1='0' x2='9' y2='0' stroke='black' marker-start='url(#m)'/>",
        );
        // Nesting stops at the depth cap; the picture still draws what the first levels paint.
        assert!(s.items.len() <= 16, "{}", s.items.len());
    }

    #[test]
    fn a_mid_marker_points_into_a_clockwise_turn() {
        let s = svg(
            "<defs><marker id='m' markerWidth='10' markerHeight='10' markerUnits='userSpaceOnUse' orient='auto' refX='0' refY='0' viewBox='0 0 10 10'><line x1='0' y1='0' x2='6' y2='0' stroke='black'/></marker></defs><path d='M0 0 L10 0 L10 -10' fill='none' stroke='black' marker-mid='url(#m)'/>",
        );
        let b = s.items.last().expect("a marker").path.bounding_box();
        // The corner turns upwards: the bisector is 45 degrees above the x axis, not below it.
        assert!((b.x0 - 10.0).abs() < 1e-6, "{b:?}");
        assert!((b.y0 + 4.2426).abs() < 0.01 && b.y1.abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn an_end_marker_follows_the_tangent_of_a_curve() {
        let s = svg(
            "<defs><marker id='m' markerWidth='10' markerHeight='10' markerUnits='userSpaceOnUse' orient='auto' refX='0' refY='0' viewBox='0 0 10 10'><line x1='0' y1='0' x2='6' y2='0' stroke='black'/></marker></defs><path d='M0 0 C0 10 10 10 10 0' fill='none' stroke='black' marker-end='url(#m)'/>",
        );
        let b = s.items.last().expect("a marker").path.bounding_box();
        // The curve arrives at its end running straight up, so the marker's arm points up too.
        assert!((b.y0 + 6.0).abs() < 0.6 && b.y1.abs() < 0.6, "{b:?}");
        assert!((b.x0 - 10.0).abs() < 0.6 && (b.x1 - 10.0).abs() < 0.6, "{b:?}");
    }

    #[test]
    fn a_marker_fades_with_the_shape_it_marks() {
        let s = svg(
            "<defs><marker id='m' markerWidth='2' markerHeight='2' markerUnits='userSpaceOnUse' viewBox='0 0 2 2'><path d='M0 0 2 0 2 2Z' fill='black'/></marker></defs><line x1='0' y1='0' x2='9' y2='0' stroke='black' opacity='0.5' marker-end='url(#m)'/>",
        );
        let Some(Paint::Solid(c)) = &s.items[1].fill else { panic!("a filled marker") };
        assert_eq!(c.a, 128, "the shape's opacity reaches the marker");
    }

    #[test]
    fn the_last_vertex_keeps_its_marker_on_a_long_path() {
        let mut d = String::from("M0 0");
        for i in 1..=300 {
            d.push_str(&format!(" L{i} 0"));
        }
        let s = svg(&format!(
            "<defs><marker id='m' markerWidth='1' markerHeight='1' markerUnits='userSpaceOnUse' viewBox='0 0 1 1'><path d='M0 0 1 1Z' fill='black'/></marker></defs><path d='{d}' fill='none' stroke='black' marker-end='url(#m)'/>"
        ));
        let b = s.items.last().expect("a marker").path.bounding_box();
        assert!(b.x1 > 299.0, "{b:?}");
    }

    #[test]
    fn markers_draw_at_the_start_middle_and_end_they_are_named_for() {
        let marker = "<defs><marker id='m' markerWidth='1' markerHeight='1' markerUnits='userSpaceOnUse' viewBox='0 0 1 1'><path d='M0 0 1 0 1 1Z' fill='black'/></marker></defs>";
        let s = svg(&format!("{marker}<path d='M0 0 L10 0 L10 10' fill='none' stroke='black' marker-mid='url(#m)'/>"));
        assert_eq!(s.items.len(), 2, "one interior vertex");
        let s = svg(&format!("{marker}<path d='M0 0 L10 0 L10 10' fill='none' stroke='black' marker-start='url(#m)' marker-end='url(#m)'/>"));
        assert_eq!(s.items.len(), 3);
    }

    #[test]
    fn text_properties_are_inherited_from_the_group_above() {
        let s = svg("<g font-family='Georgia' font-size='12'><text x='0' y='10'>A</text></g>");
        let r = &text_of(&s).runs[0];
        assert_eq!(r.families, vec!["Georgia"]);
        assert_eq!(r.size, 12.0);
    }

    #[test]
    fn a_huge_font_size_is_clamped_instead_of_drawn() {
        let s = svg("<text x='0' y='10' font-size='1e300'>A</text>");
        assert!(text_of(&s).runs[0].size <= MAX_COORD);
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
