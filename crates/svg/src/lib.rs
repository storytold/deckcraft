//! SVG pictures (`image/svg+xml`): read by the shared [`craft_svg`] reader, which runs usvg behind
//! its bounded preflight (SVGZ, entity expansion, nesting, attributes, embedded images), and mapped
//! onto the geometry the renderer draws: absolute bezier paths with a fill and a stroke.
//!
//! usvg resolves what a document means before this crate sees it: transforms, `use`, styles,
//! `display` and `visibility`, markers, and text as glyph outlines (with the fonts this app ships).
//! What remains here is the mapping: usvg paints, gradient coordinate spaces, paths and the opacity
//! an element sits under.
//!
//! Painting that the renderer has no model for is reported through [`Skipped`] rather than drawn
//! wrong, so a caller holding a raster fallback can prefer it: filters, masks, clip paths, patterns
//! and embedded images. A group's opacity multiplies each shape it paints rather than compositing
//! the group once. That is exact for a single fill or stroke; a translucent group that paints more
//! than that (overlapping shapes, or a fill under its own stroke) is reported in
//! [`Skipped::group_opacity`] instead, since its overlaps would show.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::{Arc, OnceLock};

use craft_svg::tree::{self, Limits, usvg};
use deckcraft_color::Rgba;
use kurbo::{Affine, BezPath, Shape as _};

/// Most shapes read from one picture.
pub const MAX_ITEMS: usize = 100_000;
/// Largest document read, unpacked.
const MAX_BYTES: usize = 64 << 20;
/// Most path segments kept per picture, across all shapes.
const MAX_SEGMENTS_TOTAL: usize = 2_000_000;
/// Element nesting followed. usvg uses about 15 kB of stack per level, so this fits a thread with
/// a 1 MB stack (the wasm build's).
const DEPTH: usize = 24;
/// Largest coordinate taken from a shape; a bigger one would only tessellate absurdly.
const MAX_COORD: f64 = 1e5;
/// Most cells one pattern may cover; a finer repeat is left to the shared rasterizer.
const MAX_PATTERN_CELLS: usize = 4096;

/// An SVG document as the renderer draws it.
#[derive(Clone, Debug, Default)]
pub struct Svg {
    /// The size the picture asks for, in 96-dpi pixels. The geometry below is in that space: usvg
    /// has already applied the document's `viewBox`.
    pub size: (f64, f64),
    /// Features this picture uses that nothing here draws. A caller that has a raster fallback
    /// should prefer it, because the drawing would be missing them.
    pub skipped: Skipped,
    /// Every shape, in painting order.
    pub items: Vec<Item>,
}

/// The features a picture uses that the vector mapping does not draw, so a caller can fall back to
/// the shared rasterizer for the whole picture. Clip paths and plain patterns are drawn here and
/// are not listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub filters: bool,
    pub masks: bool,
    /// A pattern this mapping could not turn into tiles (nested, or too many cells).
    pub patterns: bool,
    pub images: bool,
    /// A translucent group (`opacity` below 1) whose shapes overlap one another: SVG composites
    /// the group once and then fades it, which the per-shape alpha here cannot reproduce.
    pub group_opacity: bool,
}

impl Skipped {
    pub fn any(&self) -> bool {
        self.filters || self.masks || self.patterns || self.images || self.group_opacity
    }
}

/// One filled and/or stroked shape.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: BezPath,
    pub even_odd: bool,
    pub fill: Option<Paint>,
    pub stroke: Option<(Paint, kurbo::Stroke)>,
    /// Clip paths the shape is drawn inside, in user units; they intersect.
    pub clips: Vec<Clip>,
}

/// One clip path: geometry the picture is clipped to.
#[derive(Clone, Debug)]
pub struct Clip {
    pub path: BezPath,
    pub even_odd: bool,
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

/// A gradient with the stops and coordinate space it was declared in. usvg resolves the space, so
/// every gradient here is in user units (the picture's own coordinates) with its transform.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Stops by offset, ascending.
    pub stops: Vec<(f64, Rgba)>,
    /// `gradientTransform`, applied in the gradient's own space.
    pub transform: Affine,
    pub spread: Spread,
}

/// Where a gradient ramps.
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

/// What a gradient paints outside its last stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spread {
    Pad,
    Reflect,
    Repeat,
}

/// Does `bytes` look like an SVG picture, plain or gzip-compressed?
pub fn is_svg(bytes: &[u8]) -> bool {
    root_is_svg(bytes) || craft_svg::sniff::is_svgz(bytes)
}

/// Any start tag in `head` whose local name is `svg`. `craft_svg`'s sniffer wants the literal
/// `<svg`, so a root written with a namespace prefix (`<s:svg>` with `xmlns:s`) would not be
/// recognised there.
fn root_is_svg(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = bytes.get(..bytes.len().min(4096)).unwrap_or(bytes);
    let first = head.iter().find(|b| !b.is_ascii_whitespace()).copied().unwrap_or(0);
    if first != b'<' {
        return false;
    }
    head.iter().enumerate().filter(|(_, b)| **b == b'<').any(|(i, _)| {
        let rest = head.get(i + 1..).unwrap_or(&[]);
        let end = rest.iter().position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))).unwrap_or(rest.len());
        let name = rest.get(..end).unwrap_or(&[]);
        name.rsplit(|b| *b == b':').next().unwrap_or(name).eq_ignore_ascii_case(b"svg")
    })
}

/// Memory the shared rasterizer may spend on one picture, including its intermediate layers.
const DRAW_BUDGET: u64 = 512 << 20;

fn limits() -> Limits {
    Limits { bytes: MAX_BYTES, depth: DEPTH, ..Limits::default() }
}

fn options(limits: Limits) -> usvg::Options<'static> {
    usvg::Options {
        font_family: deckcraft_fonts::FALLBACK_FAMILY.to_string(),
        fontdb: fonts().clone(),
        font_resolver: resolver(),
        image_href_resolver: tree::image_resolver(limits),
        ..Default::default()
    }
}

/// The picture drawn by the shared, memory-checked resvg draw as straight RGBA, fitted into
/// `width` × `height`. For pictures that need what the vector mapping does not apply — filters,
/// masks, clip paths, patterns, embedded images: those are reported in [`Svg::skipped`], and this
/// is the complete drawing. `None` when the bytes don't parse or the drawing would exceed the
/// budget.
pub fn rasterize(bytes: &[u8], width: u32, height: u32) -> Option<image::RgbaImage> {
    let limits = limits();
    let tree = tree::parse(bytes, &limits, &options(limits)).ok()?;
    let size = tree.size();
    let (w, h) = (f64::from(size.width()), f64::from(size.height()));
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) || width == 0 || height == 0 {
        return None;
    }
    let k = (f64::from(width) / w).min(f64::from(height) / h);
    let (tw, th) = ((w * k).round().clamp(1.0, f64::from(width.max(1))) as u32, (h * k).round().clamp(1.0, f64::from(height.max(1))) as u32);
    let mut pixmap = tree::draw::render(&tree, usvg::Transform::from_scale(k as f32, k as f32), tw, th, 0, DRAW_BUDGET).ok()?;
    // tiny-skia keeps premultiplied RGBA; every other layer of this app works in straight alpha.
    for p in pixmap.data_mut().as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in p.iter_mut().take(3) {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take())
}

/// Read a picture. `None` when the bytes are not a readable SVG within the reader's limits.
pub fn parse(bytes: &[u8]) -> Option<Svg> {
    let limits = limits();
    let parsed = tree::parse(bytes, &limits, &options(limits)).ok()?;
    let size = parsed.size();
    let (w, h) = (f64::from(size.width()), f64::from(size.height()));
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 && w <= MAX_COORD * 100.0 && h <= MAX_COORD * 100.0) {
        return None;
    }
    let mut build = Build { items: Vec::new(), segments: 0, skipped: Skipped::default(), clips: Vec::new(), pattern_depth: 0 };
    build.group(parsed.root(), 1.0);
    Some(Svg { size: (w, h), skipped: build.skipped, items: build.items })
}

/// The fonts text is laid out with: the faces this app ships, once.
fn fonts() -> &'static Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for bytes in deckcraft_fonts::bundled() {
            db.load_font_data(bytes.to_vec());
        }
        Arc::new(db)
    })
}

/// Font selection for text: the families the picture asks for, then the substitutes this app maps
/// missing families to, then the faces it ships. Without that, a picture naming Arial would lose
/// its labels where the app has no Arial.
fn resolver() -> usvg::FontResolver<'static> {
    usvg::FontResolver {
        select_font: Box::new(|font, db| {
            let asked: Vec<String> = font
                .families()
                .iter()
                .map(|f| match f {
                    usvg::FontFamily::Serif => "serif".to_string(),
                    usvg::FontFamily::SansSerif => "sans-serif".to_string(),
                    usvg::FontFamily::Cursive => "cursive".to_string(),
                    usvg::FontFamily::Fantasy => "fantasy".to_string(),
                    usvg::FontFamily::Monospace => "monospace".to_string(),
                    usvg::FontFamily::Named(n) => n.clone(),
                })
                .collect();
            let mut names: Vec<String> = Vec::new();
            for name in &asked {
                names.push(name.clone());
                names.extend(deckcraft_fonts::substitutes(name).iter().map(|s| (*s).to_string()));
            }
            names.push(deckcraft_fonts::FALLBACK_FAMILY.to_string());
            names.push(deckcraft_fonts::LAST_RESORT_FAMILY.to_string());
            let families: Vec<usvg::fontdb::Family> = names
                .iter()
                .map(|n| match n.as_str() {
                    "serif" => usvg::fontdb::Family::Serif,
                    "sans-serif" => usvg::fontdb::Family::SansSerif,
                    "cursive" => usvg::fontdb::Family::Cursive,
                    "fantasy" => usvg::fontdb::Family::Fantasy,
                    "monospace" => usvg::fontdb::Family::Monospace,
                    other => usvg::fontdb::Family::Name(other),
                })
                .collect();
            let query = usvg::fontdb::Query {
                families: &families,
                weight: usvg::fontdb::Weight(font.weight()),
                stretch: stretch(font.stretch()),
                style: style(font.style()),
            };
            db.query(&query).or_else(|| db.faces().next().map(|f| f.id))
        }),
        select_fallback: usvg::FontResolver::default_fallback_selector(),
    }
}

fn stretch(s: usvg::FontStretch) -> usvg::fontdb::Stretch {
    match s {
        usvg::FontStretch::UltraCondensed => usvg::fontdb::Stretch::UltraCondensed,
        usvg::FontStretch::ExtraCondensed => usvg::fontdb::Stretch::ExtraCondensed,
        usvg::FontStretch::Condensed => usvg::fontdb::Stretch::Condensed,
        usvg::FontStretch::SemiCondensed => usvg::fontdb::Stretch::SemiCondensed,
        usvg::FontStretch::Normal => usvg::fontdb::Stretch::Normal,
        usvg::FontStretch::SemiExpanded => usvg::fontdb::Stretch::SemiExpanded,
        usvg::FontStretch::Expanded => usvg::fontdb::Stretch::Expanded,
        usvg::FontStretch::ExtraExpanded => usvg::fontdb::Stretch::ExtraExpanded,
        usvg::FontStretch::UltraExpanded => usvg::fontdb::Stretch::UltraExpanded,
    }
}

fn style(s: usvg::FontStyle) -> usvg::fontdb::Style {
    match s {
        usvg::FontStyle::Normal => usvg::fontdb::Style::Normal,
        usvg::FontStyle::Italic => usvg::fontdb::Style::Italic,
        usvg::FontStyle::Oblique => usvg::fontdb::Style::Oblique,
    }
}

struct Build {
    items: Vec<Item>,
    /// Path elements kept, so a document cannot pile up behind [`MAX_ITEMS`].
    segments: usize,
    skipped: Skipped,
    /// Clip paths in force, in user units.
    clips: Vec<Clip>,
    /// How deep inside a pattern tile the walk is; a pattern within a pattern is not tiled.
    pattern_depth: u32,
}

impl Build {
    fn group(&mut self, group: &usvg::Group, parent_opacity: f64) {
        if group.mask().is_some() {
            self.skipped.masks = true;
        }
        if !group.filters().is_empty() {
            self.skipped.filters = true;
        }
        let opacity = parent_opacity * f64::from(group.opacity().get());
        let first = self.items.len();
        let saved = self.clips.len();
        if let Some(clip) = group.clip_path() {
            self.push_clip(clip);
        }
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => self.group(g, opacity),
                // Text arrives as the glyph outlines usvg laid out.
                usvg::Node::Text(t) => self.group(t.flattened(), opacity),
                usvg::Node::Path(p) => self.path(p, opacity),
                usvg::Node::Image(_) => self.skipped.images = true,
            }
        }
        if group.opacity().get() < 1.0 {
            let painted = self.items.get(first..).unwrap_or(&[]);
            if painted.len() > 1 || painted.iter().any(|i| i.fill.is_some() && i.stroke.is_some()) {
                self.skipped.group_opacity = true;
            }
        }
        self.clips.truncate(saved);
    }

    /// Add a clip path, with the clip it nests inside, to the clips in force.
    fn push_clip(&mut self, clip: &usvg::ClipPath) {
        if let Some(inner) = clip.clip_path() {
            self.push_clip(inner);
        }
        for node in clip.root().children() {
            self.clip_geometry(node);
        }
    }

    fn clip_geometry(&mut self, node: &usvg::Node) {
        match node {
            usvg::Node::Group(g) => {
                for child in g.children() {
                    self.clip_geometry(child);
                }
            }
            usvg::Node::Path(p) => {
                if let Some(mut path) = to_bez(p.data()) {
                    path.apply_affine(affine(p.abs_transform()));
                    let even_odd = p.fill().is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd);
                    self.clips.push(Clip { path, even_odd });
                }
            }
            usvg::Node::Text(t) => {
                for child in t.flattened().children() {
                    self.clip_geometry(child);
                }
            }
            usvg::Node::Image(_) => {}
        }
    }

    fn path(&mut self, path: &usvg::Path, opacity: f64) {
        if !path.is_visible() || self.items.len() >= MAX_ITEMS {
            return;
        }
        let Some(mut bez) = to_bez(path.data()) else { return };
        let segments = bez.elements().len();
        if self.segments.saturating_add(segments) > MAX_SEGMENTS_TOTAL {
            return;
        }
        let bb = bez.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite() && bb.width() <= MAX_COORD && bb.height() <= MAX_COORD)
        {
            return;
        }
        let m = affine(path.abs_transform());
        bez.apply_affine(m);
        // The pen is scaled with the shape; a non-uniform transform cannot scale it evenly, so the
        // mean of both axes stands in.
        let scale = m.determinant().abs().sqrt();
        let even_odd = path.fill().is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd);
        let fill = match path.fill().map(|f| f.paint()) {
            Some(usvg::Paint::Pattern(p)) => {
                let k = opacity * path.fill().map(|f| f64::from(f.opacity().get())).unwrap_or(1.0);
                if self.tile_pattern(p, &bez, even_odd, k) {
                    // The tiles are the fill; only the outline is left to draw.
                    self.stroke(path, opacity, scale, &bez, segments);
                    return;
                }
                // A pattern that cannot be tiled here is reported, so the picture can be composed
                // by the shared rasterizer instead.
                self.skipped.patterns = true;
                Some(Paint::Solid(Rgba::BLACK.with_alpha(mul_alpha(255, k))))
            }
            Some(other) => Some(paint(other, opacity * path.fill().map(|f| f64::from(f.opacity().get())).unwrap_or(1.0))),
            None => None,
        };
        let stroke = self.pen(path, opacity, scale);
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.segments += segments;
        self.items.push(Item { path: bez, even_odd, fill, stroke, clips: self.clips.clone() });
    }

    /// Emit `path`'s outline, if it has one, over a fill that was drawn some other way (a pattern).
    fn stroke(&mut self, path: &usvg::Path, opacity: f64, scale: f64, bez: &BezPath, segments: usize) {
        let Some(stroke) = self.pen(path, opacity, scale) else { return };
        if self.items.len() >= MAX_ITEMS {
            return;
        }
        self.segments += segments;
        self.items.push(Item { path: bez.clone(), even_odd: false, fill: None, stroke: Some(stroke), clips: self.clips.clone() });
    }

    fn pen(&self, path: &usvg::Path, opacity: f64, scale: f64) -> Option<(Paint, kurbo::Stroke)> {
        let s = path.stroke().filter(|s| s.width().get() > 0.0)?;
        let color = match s.paint() {
            usvg::Paint::Pattern(_) => {
                // A patterned outline is rare enough to draw in its average colour rather than to
                // tile a pen.
                Paint::Solid(Rgba::BLACK.with_alpha(mul_alpha(255, opacity * f64::from(s.opacity().get()))))
            }
            other => paint(other, opacity * f64::from(s.opacity().get())),
        };
        let mut pen = kurbo::Stroke::new(f64::from(s.width().get()) * scale).with_caps(cap(s.linecap())).with_join(join(s.linejoin()));
        if let Some(dash) = s.dasharray().filter(|d| d.len() >= 2 && d.iter().all(|v| *v > 0.0)) {
            let pattern: Vec<f64> = dash.iter().map(|v| f64::from(*v) * scale).collect();
            pen = pen.with_dashes(f64::from(s.dashoffset()), pattern);
        }
        Some((color, pen))
    }

    /// Draw a pattern fill as geometry: the tile's own items, repeated over the shape and clipped
    /// to it. `false` when the pattern is nested or would need too many cells, which leaves the
    /// caller to report it (and the rasterizer draws the picture whole).
    fn tile_pattern(&mut self, pattern: &usvg::Pattern, shape: &BezPath, even_odd: bool, opacity: f64) -> bool {
        if self.pattern_depth > 0 {
            return false;
        }
        let to_user = affine(pattern.transform());
        let from_user = to_user.inverse();
        let rect = pattern.rect();
        let (rx, ry, rw, rh) = (f64::from(rect.x()), f64::from(rect.y()), f64::from(rect.width()), f64::from(rect.height()));
        if !(rw > 1e-9 && rh > 1e-9) {
            return false;
        }
        // The cells that cover the shape, in the pattern's own space.
        let bbox = shape.bounding_box();
        let corners = [bbox.origin(), kurbo::Point::new(bbox.x1, bbox.y0), kurbo::Point::new(bbox.x0, bbox.y1), kurbo::Point::new(bbox.x1, bbox.y1)];
        let mapped: Vec<kurbo::Point> = corners.iter().map(|p| from_user * *p).collect();
        let rect_of = |a: kurbo::Point, b: kurbo::Point| kurbo::Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
        let bb = rect_of(mapped[0], mapped[2]).union(rect_of(mapped[1], mapped[3]));
        let bbp = bbox;
        let (x0, y0) = (f64::min(bb.x0, bbp.x0), f64::min(bb.y0, bbp.y0));
        let (x1, y1) = (f64::max(bb.x1, bbp.x1), f64::max(bb.y1, bbp.y1));
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return false;
        }
        let i0 = ((x0 - rx) / rw).floor() as i64;
        let j0 = ((y0 - ry) / rh).floor() as i64;
        let i1 = ((x1 - rx) / rw).ceil() as i64;
        let j1 = ((y1 - ry) / rh).ceil() as i64;
        let cells = (i1 - i0).max(0).saturating_mul((j1 - j0).max(0)) as usize;
        if cells == 0 || cells > MAX_PATTERN_CELLS {
            return false;
        }
        // The tile's contents, mapped once in the pattern's own space.
        let items = {
            let saved_items = std::mem::take(&mut self.items);
            let saved_clips = std::mem::take(&mut self.clips);
            let saved_segments = self.segments;
            self.pattern_depth += 1;
            for node in pattern.root().children() {
                match node {
                    usvg::Node::Group(g) => self.group(g, 1.0),
                    usvg::Node::Path(p) => self.path(p, 1.0),
                    usvg::Node::Text(t) => self.group(t.flattened(), 1.0),
                    usvg::Node::Image(_) => self.skipped.images = true,
                }
            }
            self.pattern_depth -= 1;
            let items = std::mem::replace(&mut self.items, saved_items);
            self.clips = saved_clips;
            self.segments = saved_segments;
            items
        };
        if items.is_empty() {
            return false;
        }
        if items.len().saturating_mul(cells) > MAX_ITEMS {
            return false;
        }
        let mut outer = self.clips.clone();
        outer.push(Clip { path: shape.clone(), even_odd });
        for i in i0..i1 {
            for j in j0..j1 {
                let offset = to_user * kurbo::Point::new(i as f64 * rw, j as f64 * rh) - to_user * kurbo::Point::ORIGIN;
                let offset = kurbo::Vec2::new(offset.x, offset.y);
                for item in &items {
                    if self.items.len() >= MAX_ITEMS {
                        return true;
                    }
                    let mut clone = item.clone();
                    clone.path.apply_affine(Affine::translate(offset));
                    clone.fill = clone.fill.map(|p| p.faded(opacity));
                    clone.stroke = clone.stroke.map(|(p, pen)| (p.faded(opacity), pen));
                    clone.clips = outer.clone();
                    self.items.push(clone);
                }
            }
        }
        true
    }
}

/// A usvg paint as this renderer draws it, `opacity` multiplied into every colour's alpha.
fn paint(p: &usvg::Paint, opacity: f64) -> Paint {
    match p {
        usvg::Paint::Color(c) => Paint::Solid(Rgba::rgb(c.red, c.green, c.blue).with_alpha(mul_alpha(255, opacity))),
        usvg::Paint::LinearGradient(g) => Paint::Gradient(gradient(
            GradientKind::Linear { x1: f64::from(g.x1()), y1: f64::from(g.y1()), x2: f64::from(g.x2()), y2: f64::from(g.y2()) },
            g,
        ))
        .faded(opacity),
        usvg::Paint::RadialGradient(g) => Paint::Gradient(gradient(
            GradientKind::Radial {
                cx: f64::from(g.cx()),
                cy: f64::from(g.cy()),
                r: f64::from(g.r().get()),
                fx: f64::from(g.fx()),
                fy: f64::from(g.fy()),
            },
            g,
        ))
        .faded(opacity),
        // A pattern is a painting model of its own; the picture is reported as partly unreadable.
        usvg::Paint::Pattern(_) => Paint::Solid(Rgba::BLACK.with_alpha(mul_alpha(255, opacity))),
    }
}

fn gradient(kind: GradientKind, g: &usvg::BaseGradient) -> Gradient {
    let stops = g
        .stops()
        .iter()
        .map(|s| {
            let c = s.color();
            let a = mul_alpha(255, f64::from(s.opacity().get()));
            (f64::from(s.offset().get()), Rgba::rgb(c.red, c.green, c.blue).with_alpha(a))
        })
        .collect();
    Gradient { kind, stops, transform: affine(g.transform()), spread: spread(g.spread_method()) }
}

fn spread(s: usvg::SpreadMethod) -> Spread {
    match s {
        usvg::SpreadMethod::Pad => Spread::Pad,
        usvg::SpreadMethod::Reflect => Spread::Reflect,
        usvg::SpreadMethod::Repeat => Spread::Repeat,
    }
}

/// `k` (0–1) into the alpha of a `a`-alpha colour.
fn mul_alpha(a: u8, k: f64) -> u8 {
    (f64::from(a) * k.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
}

fn cap(c: usvg::LineCap) -> kurbo::Cap {
    match c {
        usvg::LineCap::Round => kurbo::Cap::Round,
        usvg::LineCap::Square => kurbo::Cap::Square,
        usvg::LineCap::Butt => kurbo::Cap::Butt,
    }
}

fn join(j: usvg::LineJoin) -> kurbo::Join {
    match j {
        usvg::LineJoin::Round => kurbo::Join::Round,
        usvg::LineJoin::Bevel => kurbo::Join::Bevel,
        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => kurbo::Join::Miter,
    }
}

/// usvg's transforms are tiny-skia's: `x' = sx·x + kx·y + tx`.
fn affine(t: usvg::Transform) -> Affine {
    Affine::new([f64::from(t.sx), f64::from(t.ky), f64::from(t.kx), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty)])
}

/// A usvg path as kurbo geometry.
fn to_bez(path: &usvg::tiny_skia_path::Path) -> Option<BezPath> {
    let p = |pt: usvg::tiny_skia_path::Point| (f64::from(pt.x), f64::from(pt.y));
    let mut bez = BezPath::new();
    for seg in path.segments() {
        match seg {
            usvg::tiny_skia_path::PathSegment::MoveTo(pt) => bez.move_to(p(pt)),
            usvg::tiny_skia_path::PathSegment::LineTo(pt) => bez.line_to(p(pt)),
            usvg::tiny_skia_path::PathSegment::QuadTo(a, b) => bez.quad_to(p(a), p(b)),
            usvg::tiny_skia_path::PathSegment::CubicTo(a, b, c) => bez.curve_to(p(a), p(b), p(c)),
            usvg::tiny_skia_path::PathSegment::Close => bez.close_path(),
        }
    }
    (!bez.elements().is_empty()).then_some(bez)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(body: &str) -> Svg {
        parse(format!("<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'>{body}</svg>").as_bytes()).expect("reads")
    }

    #[test]
    fn a_translucent_group_that_overlaps_itself_is_left_to_the_rasterizer() {
        // One shape, or a fill or stroke alone: per-shape alpha is exact.
        assert!(!svg("<g opacity='0.5'><rect width='4' height='4' fill='red'/></g>").skipped.any());
        assert!(!svg("<rect width='4' height='4' fill='red' opacity='0.5'/>").skipped.any());
        assert!(!svg("<g opacity='1'><rect width='4' height='4'/><rect x='2' width='4' height='4'/></g>").skipped.any());
        // Two shapes, or a fill under its own stroke: the group is composited once, then faded.
        assert!(
            svg("<g opacity='0.5'><rect width='4' height='4' fill='red'/><rect x='2' width='4' height='4' fill='lime'/></g>").skipped.group_opacity
        );
        assert!(svg("<rect width='4' height='4' fill='red' stroke='blue' opacity='0.5'/>").skipped.group_opacity);
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
    fn the_view_box_lands_in_the_pictures_size() {
        let s = parse(b"<svg width='48' height='24' viewBox='0 0 2 1'><rect width='2' height='1' fill='black'/></svg>").expect("reads");
        assert_eq!(s.size, (48.0, 24.0));
        let b = s.items[0].path.bounding_box();
        assert!((b.width() - 48.0).abs() < 1e-6 && (b.height() - 24.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn stroke_width_follows_the_transform() {
        let s = svg("<g transform='scale(3)'><path d='M0 0 L5 5' stroke='black' stroke-width='1'/></g>");
        let (_, pen) = s.items[0].stroke.as_ref().expect("stroked");
        assert!((pen.width - 3.0).abs() < 1e-6, "{}", pen.width);
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
    fn display_none_and_visibility_hidden_are_honoured() {
        let s = svg("<g display='none'><rect width='4' height='4'/></g>");
        assert!(s.items.is_empty());
        let s = svg("<g visibility='hidden'><rect width='4' height='4'/><rect width='4' height='4' visibility='visible'/></g>");
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn use_draws_the_element_it_names() {
        let s = svg("<defs><rect id='r' width='4' height='4' fill='red'/></defs><use href='#r' x='10' y='5'/>");
        assert_eq!(s.items.len(), 1);
        let b = s.items[0].path.bounding_box();
        assert_eq!((b.x0, b.y0), (10.0, 5.0));
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
    fn a_gradient_fill_keeps_its_stops_and_space() {
        let s = svg(
            "<defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='0'><stop stop-color='#000000'/><stop offset='1' stop-color='#ffffff'/></linearGradient></defs><rect width='4' height='4' fill='url(#g)'/>",
        );
        let Some(Paint::Gradient(g)) = s.items[0].fill.clone() else { panic!("gradient fill") };
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].1, Rgba::rgb(0, 0, 0));
        assert_eq!(g.stops[1].0, 1.0);
        // The picture's own space: an object-bounding-box ramp spans the 4 × 4 rectangle.
        let span = g.transform * bez_point(1.0, 0.0) - g.transform * bez_point(0.0, 0.0);
        assert!((span.x - 4.0).abs() < 1e-6, "{span:?}");
    }

    fn bez_point(x: f64, y: f64) -> kurbo::Point {
        kurbo::Point::new(x, y)
    }

    #[test]
    fn circles_and_rounded_rectangles_become_paths() {
        let s = svg("<circle cx='5' cy='5' r='2' fill='black'/><rect width='10' height='4' rx='2' fill='black'/>");
        assert_eq!(s.items.len(), 2);
        assert!(s.items[0].path.elements().len() > 3);
        assert!(s.items[1].path.elements().len() > 4);
    }

    #[test]
    fn text_arrives_as_glyph_outlines() {
        let s = svg("<text x='1' y='10' font-size='8' fill='black'>Hi</text>");
        assert!(!s.items.is_empty(), "the glyphs are paths");
        assert!(s.items.iter().all(|i| i.fill.is_some()));
    }

    #[test]
    fn a_clip_path_travels_with_the_shapes_it_clips() {
        let s = svg("<defs><clipPath id='c'><rect width='2' height='4'/></clipPath></defs><g clip-path='url(#c)'><rect width='4' height='4'/></g>");
        assert!(!s.skipped.any(), "clips are geometry the renderer applies");
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].clips.len(), 1);
        assert!((s.items[0].clips[0].path.bounding_box().width() - 2.0).abs() < 1e-6);
        let s = svg("<rect width='4' height='4' fill='url(#none)'/>");
        assert!(!s.skipped.any(), "a missing paint server is not a pattern");
    }

    #[test]
    fn a_pattern_fill_becomes_tiles_clipped_to_the_shape() {
        // A 2 by 2 dotted tile over a 4 by 4 square: four cells, each clipped to the square.
        let s = svg(
            "<defs><pattern id='p' width='2' height='2' patternUnits='userSpaceOnUse'><circle cx='1' cy='1' r='0.5' fill='#000000'/></pattern></defs><rect width='4' height='4' fill='url(#p)'/>",
        );
        assert!(!s.skipped.any(), "a plain pattern is tiled here");
        assert_eq!(s.items.len(), 4, "one tile per cell");
        assert!(s.items.iter().all(|i| i.fill.is_some() && i.clips.len() == 1), "each tile is clipped to the square");
    }

    #[test]
    fn a_picture_the_mapping_skips_composes_through_the_shared_rasterizer() {
        let filtered = b"<svg width='40' height='40' viewBox='0 0 40 40'><defs><filter id='f'><feGaussianBlur stdDeviation='3'/></filter></defs><rect x='10' y='10' width='20' height='20' fill='#ff0000' filter='url(#f)'/></svg>";
        let s = parse(filtered).expect("reads");
        assert!(s.skipped.filters, "the vector mapping cannot draw a filter");
        let img = rasterize(filtered, 40, 40).expect("the rasterizer can");
        assert_eq!(img.dimensions(), (40, 40));
        assert!(img.get_pixel(20, 20)[3] > 0 && img.get_pixel(20, 20)[0] > 100, "{:?}", img.get_pixel(20, 20));
        assert_eq!(rasterize(b"not xml", 10, 10), None);
    }

    #[test]
    fn sniffing_accepts_svg_svgz_and_prefixed_roots() {
        assert!(is_svg(b"\xEF\xBB\xBF<?xml version='1.0'?><svg xmlns='http://www.w3.org/2000/svg'/>"));
        assert!(is_svg(b"  <SVG viewBox='0 0 1 1'/>"));
        assert!(is_svg(b"<s:svg xmlns:s='http://www.w3.org/2000/svg'><s:rect width='1' height='1'/></s:svg>"));
        assert!(is_svg(b"\x1f\x8b\x08\x00"), "gzip: an SVGZ, decided after unpacking");
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"<svga viewBox='0 0 1 1'/>"));
    }

    #[test]
    fn broken_documents_read_as_nothing() {
        assert!(parse(b"<svg").is_none());
        assert!(parse(b"not xml").is_none());
    }
}
