//! The DeckCraft renderer: draws slides, layouts and masters with `vello_cpu`.
//!
//! One code path serves the editor canvas, thumbnails, image/PDF-raster export and the slide show:
//! [`render_slide`] draws the background, the master and layout graphics, then the slide's shapes
//! (fills with theme shading, gradients, pictures, outlines with dashes and arrowheads, shadows,
//! glow and soft edges, text, tables, charts, groups), optionally with per-shape animation state.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bevel;
mod camera;
mod chart;
mod chartex;
mod colr;
mod extrude;
mod light;

mod compound;
mod extent;
mod images;
mod metafile;
mod morph_path;
mod morph_text;
mod paint;
mod pattern;

mod pathgrad;
mod placed;
mod radar;
mod ramp;
mod table;
mod transition;
mod warp;

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_fonts::FontDb;
use deckcraft_geom::preset::{self, FillMode, Geometry, SubPath};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::{Effects, Fill, Line};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Geom, PhType, Presentation, Shape, ShapeId, ShapeKind, Slide};
use deckcraft_text::datetime::{self, DateTime};
use deckcraft_text::{Fields, Opts};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape as _, Vec2};
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::{RenderContext, Resources, peniko};

pub use deckcraft_anim::{transition_frame, transition_layers, uses_slide_parts};
pub use extent::{Region, TILE, layout_extent, pasteboard_region, slide_extent};
pub use images::{decode as decode_image, gif_frame};
pub use morph_path::PathMorph;
pub use morph_text::TextMorph;
pub use placed::{Placed, PlacedLink, PlacedShape, PlacedText, place_master_texts, place_slide, text_outline};
pub use table::cell_body as table_cell_body;
pub use table::cell_rects as table_cell_rects;
pub use table::row_heights as table_row_heights;
pub use transition::{SlideParts, composite_transition, render_transition};

/// Largest pixmap side we render (vello_cpu uses u16 sizes).
pub const MAX_SIDE: u32 = 16_000;

/// A rendered image: premultiplied RGBA8.
#[derive(Clone, Debug, Default)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Straight (unpremultiplied) RGBA.
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px.get(3).copied().unwrap_or(255) as u32;
            if a != 0 && a != 255 {
                for c in px.iter_mut().take(3) {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// One pixel as straight (unpremultiplied) RGBA; `None` outside the image.
    pub fn straight_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize).checked_mul(self.width as usize)?.checked_add(x as usize)?.checked_mul(4)?;
        let [r, g, b, a] = <[u8; 4]>::try_from(self.pixels.get(i..i.checked_add(4)?)?).ok()?;
        if a == 0 || a == 255 {
            return Some([r, g, b, a]);
        }
        let unpremultiply = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
        Some([unpremultiply(r), unpremultiply(g), unpremultiply(b), a])
    }
    pub fn to_png(&self) -> Vec<u8> {
        let mut px = self.to_straight();
        px.resize(self.width as usize * self.height as usize * 4, 0);
        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, px) else { return Vec::new() };
        let mut buf = Vec::new();
        if let Err(e) = img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png) {
            log::error!("PNG encode: {e}");
            return Vec::new();
        }
        buf
    }

    /// PNG of this image whose `pHYs` chunk says `dpi` dots per inch.
    ///
    /// Slide space is points, 72 per inch, so a render at `scale` pixels per point is
    /// [`dpi_for_scale`]`(scale)`. Without the chunk, Preview treats the pixels as 72 dpi and a
    /// Retina (2×) export opens at twice the slide's size.
    pub fn to_png_at(&self, dpi: f64) -> Vec<u8> {
        images::stamp_png_dpi(&self.to_png(), dpi)
    }

    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        self.to_jpeg_at(quality, None)
    }

    /// JPEG of this image. `dpi` is stored as JFIF density in dots per inch when it is in range.
    pub fn to_jpeg_at(&self, quality: u8, dpi: Option<f64>) -> Vec<u8> {
        let straight = self.to_straight();
        let rgb: Vec<u8> = straight.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let Some(img) = image::RgbImage::from_raw(self.width, self.height, rgb) else { return Vec::new() };
        let mut buf = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        if let Some(d) = dpi.and_then(dpi_u16) {
            enc.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(d));
        }
        if let Err(e) = img.write_with_encoder(enc) {
            log::error!("JPEG encode: {e}");
            return Vec::new();
        }
        buf
    }
    /// Pixel at (x, y) as straight RGBA.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
        let a = p.get(3).copied().unwrap_or(0) as u32;
        let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).map(|v| v.min(255) as u8).unwrap_or(0);
        [un(p[0]), un(p[1]), un(p[2]), a as u8]
    }
}

/// Dots per inch for a slide drawn at `pixels_per_point`.
///
/// A point is 1/72 inch. The default picture export uses 2 pixels per point, which is 144 dpi:
/// the same pixels, at the slide's real size, on a Retina display.
pub fn dpi_for_scale(pixels_per_point: f64) -> f64 {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return 72.0;
    }
    (pixels_per_point * 72.0).clamp(1.0, 10_000.0)
}

fn dpi_u16(dpi: f64) -> Option<u16> {
    if !dpi.is_finite() || !(1.0..=f64::from(u16::MAX)).contains(&dpi) {
        return None;
    }
    Some(dpi.round() as u16)
}

/// Resolution a PNG (`pHYs`) or JPEG (JFIF) records, in whole dots per inch.
pub fn image_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    images::dpi(bytes)
}

/// Animation state of one shape during a slide show or preview.
#[derive(Clone, Debug)]
pub struct ShapeState {
    pub visible: bool,
    pub opacity: f64,
    /// Applied in slide space around the shape's centre: translation (pt), scale, rotation (deg).
    pub offset: Vec2,
    pub scale: (f64, f64),
    pub rotate: f64,
    /// Reveal clip in shape-local unit space (0..1): l, t, r, b.
    pub clip: Option<[f64; 4]>,
    /// Colour override for emphasis effects (fill).
    pub tint: Option<Rgba>,
    /// Per-paragraph states of by-paragraph text builds (paragraph index, state).
    pub paras: Vec<(usize, ParaState)>,
}

impl Default for ShapeState {
    fn default() -> Self {
        ShapeState { visible: true, opacity: 1.0, offset: Vec2::ZERO, scale: (1.0, 1.0), rotate: 0.0, clip: None, tint: None, paras: vec![] }
    }
}

/// Animation state of one paragraph, relative to its shape.
#[derive(Clone, Copy, Debug)]
pub struct ParaState {
    pub visible: bool,
    pub opacity: f64,
    /// Translation in shape-local points.
    pub offset: Vec2,
}

impl Default for ParaState {
    fn default() -> Self {
        ParaState { visible: true, opacity: 1.0, offset: Vec2::ZERO }
    }
}

pub struct RenderOpts<'a> {
    /// Pixels per point.
    pub scale: f64,
    /// Editor view: placeholder prompts and dashed placeholder outlines.
    pub edit: bool,
    /// Draw shapes marked hidden (Selection pane) — the editor hides them like the show does.
    pub show_hidden: bool,
    /// Per-shape animation state.
    pub state: Option<&'a dyn Fn(ShapeId) -> ShapeState>,
    /// Don't draw these shapes (being edited in place, drawn by the UI).
    pub skip: &'a [ShapeId],
    /// Hide the text of this shape (the UI draws it while editing).
    pub skip_text: Option<ShapeId>,
    /// Text being edited: replace its prompt with gray markers on empty list paragraphs.
    /// Ignored outside editor rendering.
    pub editing_text: Option<ShapeId>,
    /// Animated GIF pictures and how far into their play they are (seconds); others show frame 1.
    pub gif_times: &'a [(ShapeId, f64)],
    /// Worker threads (0 = this thread; required when effects use filters).
    pub threads: u16,
    /// Translate the slide within the output (pixels) and output size override.
    pub offset: (f64, f64),
    pub size: Option<(u32, u32)>,
    /// Draw a background colour outside the slide (output larger than slide).
    pub clear: Option<Rgba>,
    /// Show this text in every date field instead of the date.
    pub date_text: Option<String>,
    /// The local date and time date fields show (`None`: the system clock).
    pub now: Option<DateTime>,
    /// Leave the slide background out (neither the white underlay nor the background fill), so
    /// only what is on the slide is drawn over transparency: PNG export with a transparent
    /// background. Shapes filled with the background (`useBgFill`) still show it.
    pub transparent_background: bool,
}

impl Default for RenderOpts<'_> {
    fn default() -> Self {
        RenderOpts {
            scale: 1.0,
            edit: false,
            show_hidden: false,
            state: None,
            skip: &[],
            skip_text: None,
            editing_text: None,
            gif_times: &[],
            threads: 0,
            offset: (0.0, 0.0),
            size: None,
            clear: None,
            date_text: None,
            now: None,
            transparent_background: false,
        }
    }
}

struct SlideFields {
    num: u32,
    /// Text for every date field instead of the date ([`RenderOpts::date_text`]).
    date: Option<String>,
    footer: String,
    /// The local date and time date fields show; `None`: they keep their saved text.
    now: Option<DateTime>,
}

impl SlideFields {
    fn new(num: u32, footer: String, opts: &RenderOpts) -> Self {
        SlideFields { num, date: opts.date_text.clone(), footer, now: opts.now.or_else(DateTime::now_local) }
    }
}

impl Fields for SlideFields {
    fn field(&self, kind: &str) -> Option<String> {
        self.field_in(kind, None)
    }

    fn field_in(&self, kind: &str, lang: Option<&str>) -> Option<String> {
        if kind == "slidenum" {
            return Some(self.num.to_string());
        }
        if datetime::is_date_field(kind) {
            // An automatic date: today's, in the format of its type. (A fixed date is plain text
            // in the slide's placeholder, not a field.)
            return self.date.clone().or_else(|| datetime::field_text(kind, lang, self.now?));
        }
        if kind == "footer" {
            return Some(self.footer.clone());
        }
        None
    }
}

pub struct Renderer {
    resources: Resources,
    /// Outlines morphing in the frame being drawn by `blend`.
    paths: Vec<PathMorph>,
    /// The background of the slide (layout, master) being drawn, its placeholder colour and the
    /// slide's view transform: shapes filled with the background (`useBgFill`) show it through.
    bg: Option<(Fill, Option<Rgba>, Affine)>,
    /// What the frame being drawn has painted so far, in output pixels: the editor learns from it
    /// how far a slide's drawing (text spilling out of its box too) reaches off the slide.
    drawn: Option<Rect>,
    /// The transform a top-level shape is drawn under (slide to pixels); a group's children draw
    /// under it combined with the group's child-coordinate mapping, which must not scale lines.
    view: Affine,
    /// The fill of the group being drawn, with the transform and box it is laid out in: children
    /// filled with the group's fill (`a:grpFill`) show it, as one fill across the whole group.
    group_fill: Option<(Fill, Affine, Rect)>,
    /// Their aligned outlines, kept from frame to frame.
    morphs: morph_path::Cache,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer {
            resources: Resources::new(),
            paths: Vec::new(),
            bg: None,
            drawn: None,
            view: Affine::IDENTITY,
            group_fill: None,
            morphs: morph_path::Cache::default(),
        }
    }
}

static PANICS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn note_panic() {
    PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Render panics caught (and drawn as blank) since start; soak tests require it to stay 0.
pub fn panics_caught() -> u64 {
    PANICS.load(std::sync::atomic::Ordering::Relaxed)
}

thread_local! {
    static RENDERER: std::cell::RefCell<Renderer> = std::cell::RefCell::new(Renderer::default());
}

/// Does anything on this slide (or its layout/master graphics) use blur-based effects?
pub fn needs_filters(pres: &Presentation, slide: &Slide) -> bool {
    let Some(rctx) = Ctx::for_slide(pres, slide) else { return false };
    let mut any = false;
    let mut check = |shapes: &[Shape]| {
        deckcraft_model::walk(shapes, &mut |s, _| {
            if any {
                return;
            }
            let (e, _) = resolve::effects(&rctx, s);
            if e.is_some_and(|e| !e.is_empty()) || text_has_effects(&rctx, s) {
                any = true;
            }
        });
    };
    check(&slide.shapes);
    if let Some(l) = rctx.layout {
        check(&l.shapes);
    }
    check(&rctx.master.shapes);
    any
}

/// Does the shape's text have a shadow or glow (on a run, in its list style, or in the master's
/// text styles it inherits)? Such text is blurred, which needs filters.
fn text_has_effects(rctx: &Ctx, s: &Shape) -> bool {
    use deckcraft_model::text::{ListStyle, RunProps};
    let fx = |r: &RunProps| r.shadow.as_ref().is_some_and(|s| !s.inner) || r.glow.as_ref().is_some_and(|g| g.radius > 0.0);
    let styled = |l: &ListStyle| l.levels.iter().flatten().any(|lv| fx(&lv.run));
    let Some(t) = &s.text else { return false };
    t.paragraphs.iter().any(|p| p.runs.iter().any(|r| fx(&r.props)) || fx(&p.end_props))
        || styled(&t.list_style)
        || styled(&rctx.master.title_style)
        || styled(&rctx.master.body_style)
        || styled(&rctx.master.other_style)
}

/// Render slide `index` at `opts.scale` pixels per point. A panic inside the renderer becomes a
/// blank image (logged), never a crash.
pub fn render_slide(pres: &Presentation, index: usize, opts: &RenderOpts) -> Image {
    render_slide_drawn(pres, index, opts).0
}

/// [`render_slide`], also returning how far the drawing reached, in slide space (at least the
/// slide; off it, what the shapes, their effects and their text drew, the parts outside the output
/// included). The editor grows its pasteboard to it ([`pasteboard_region`]).
pub fn render_slide_drawn(pres: &Presentation, index: usize, opts: &RenderOpts) -> (Image, Rect) {
    let page = Rect::new(0.0, 0.0, pres.slide_size.width, pres.slide_size.height);
    let Some(slide) = pres.slides.get(index) else { return (Image::default(), page) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        RENDERER.with(|r| {
            let mut r = r.borrow_mut();
            r.drawn = None;
            let img = r.slide(pres, slide, index, opts);
            (img, r.drawn.take())
        })
    })) {
        Ok((img, drawn)) => {
            let k = opts.scale;
            let to_slide =
                |d: Rect| Rect::new((d.x0 - opts.offset.0) / k, (d.y0 - opts.offset.1) / k, (d.x1 - opts.offset.0) / k, (d.y1 - opts.offset.1) / k);
            let extent = match drawn {
                Some(d) if k.is_finite() && k > 0.0 && to_slide(d).is_finite() => page.union(to_slide(d)),
                _ => page,
            };
            (img, extent)
        }
        Err(_) => {
            log::error!("rendering slide {index} panicked; showing a blank slide");
            note_panic();
            RENDERER.with(|r| *r.borrow_mut() = Renderer::default());
            let (w, h) = output_size(pres, opts);
            (Image { width: w as u32, height: h as u32, pixels: vec![255; w as usize * h as usize * 4] }, page)
        }
    }
}

/// Which part of a slide [`render_slide_part`] draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlidePart {
    /// The background fill with the master and layout graphics.
    Backdrop,
    /// The slide's own shapes over transparency.
    Content,
}

/// Render one part of slide `index` (transitions that move the contents over the backgrounds).
/// A panic inside the renderer becomes a transparent image (logged), never a crash.
pub fn render_slide_part(pres: &Presentation, index: usize, part: SlidePart, opts: &RenderOpts) -> Image {
    let Some(slide) = pres.slides.get(index) else { return Image::default() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| RENDERER.with(|r| r.borrow_mut().slide_part(pres, slide, index, part, opts)))) {
        Ok(img) => img,
        Err(_) => {
            log::error!("rendering part of slide {index} panicked; showing nothing");
            note_panic();
            RENDERER.with(|r| *r.borrow_mut() = Renderer::default());
            let (w, h) = output_size(pres, opts);
            Image { width: w as u32, height: h as u32, pixels: vec![0; w as usize * h as usize * 4] }
        }
    }
}

/// Render a frame between slides `old` and `new` (indices into `pres.slides`), e.g. of Morph: the
/// backdrop (background, master and layout graphics) of `old` cross-fading into that of `new` by
/// `mix` (0..1), then `shapes` in order, each resolved against the slide it comes from (`true`:
/// `old`). Pairs of shapes in `text` draw their boxes as usual but their texts morph by words or
/// characters ([`TextMorph`]), on top of the later shape of the pair. Shapes listed in `paths` draw
/// an outline morphing from another geometry into their own ([`PathMorph`]). `opts.state` applies
/// to `shapes` only.
pub fn render_blend(
    pres: &Presentation,
    old: usize,
    new: usize,
    mix: f64,
    shapes: &[(Shape, bool)],
    text: &[TextMorph],
    paths: &[PathMorph],
    opts: &RenderOpts,
) -> Image {
    let (Some(a), Some(b)) = (pres.slides.get(old), pres.slides.get(new)) else { return Image::default() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        RENDERER.with(|r| r.borrow_mut().blend(pres, (a, old), (b, new), mix, shapes, text, paths, opts))
    })) {
        Ok(img) => img,
        Err(_) => {
            log::error!("rendering a frame between slides {old} and {new} panicked; showing the new slide");
            note_panic();
            RENDERER.with(|r| *r.borrow_mut() = Renderer::default());
            render_slide(pres, new, opts)
        }
    }
}

/// Render a layout (Slide Master view thumbnail and canvas).
pub fn render_layout(pres: &Presentation, master: usize, layout: Option<usize>, opts: &RenderOpts) -> Image {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| RENDERER.with(|r| r.borrow_mut().layout(pres, master, layout, opts)))) {
        Ok(img) => img,
        Err(_) => {
            log::error!("rendering a layout panicked; showing a blank image");
            note_panic();
            RENDERER.with(|r| *r.borrow_mut() = Renderer::default());
            Image::default()
        }
    }
}

fn output_size(pres: &Presentation, opts: &RenderOpts) -> (u16, u16) {
    let (w, h) = opts
        .size
        .unwrap_or(((pres.slide_size.width * opts.scale).round().max(1.0) as u32, (pres.slide_size.height * opts.scale).round().max(1.0) as u32));
    (w.clamp(1, MAX_SIDE) as u16, h.clamp(1, MAX_SIDE) as u16)
}

pub(crate) fn color(c: Rgba, alpha: f64) -> peniko::Color {
    let a = (c.a as f64 * alpha.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8;
    peniko::Color::from_rgba8(c.r, c.g, c.b, a)
}

fn shade(c: Rgba, m: FillMode) -> Rgba {
    match m {
        FillMode::Lighten => c.lerp(Rgba::WHITE.with_alpha(c.a), 0.4),
        FillMode::LightenLess => c.lerp(Rgba::WHITE.with_alpha(c.a), 0.2),
        FillMode::Darken => c.lerp(Rgba::BLACK.with_alpha(c.a), 0.4),
        FillMode::DarkenLess => c.lerp(Rgba::BLACK.with_alpha(c.a), 0.2),
        _ => c,
    }
}

/// Geometry of a shape at its size.
pub fn shape_geometry(shape: &Shape, w: f64, h: f64) -> Geometry {
    match &shape.geom {
        Geom::Preset { name, adj } => preset::build(name, w, h, adj).or_else(|| preset::build("rect", w, h, &[])).unwrap_or(Geometry {
            paths: vec![],
            text_rect: Rect::new(0.0, 0.0, w, h),
            handles: vec![],
            sites: vec![],
        }),
        Geom::Custom { paths } => {
            let mut out = vec![];
            for p in paths {
                let (pw, ph) = (if p.w > 0.0 { p.w } else { w.max(1e-9) }, if p.h > 0.0 { p.h } else { h.max(1e-9) });
                let mut bp = parse_path(&p.d);
                bp.apply_affine(Affine::scale_non_uniform(w / pw, h / ph));
                // PowerPoint fills each custom path's sub-paths alternately (even-odd): a sub-path
                // inside another cuts a hole whichever way it winds, as in icons and outlined text.
                out.push(SubPath { path: bp, fill: p.fill, stroke: p.stroke, even_odd: true });
            }
            Geometry { paths: out, text_rect: Rect::new(0.0, 0.0, w, h), handles: vec![], sites: vec![] }
        }
    }
}

/// Parse our custom path syntax (`M x y L x y C … Q … Z`).
pub fn parse_path(d: &str) -> BezPath {
    let mut bp = BezPath::new();
    let toks: Vec<&str> = d.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let mut i = 0;
    let num = |i: usize| toks.get(i).and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite());
    let mut started = false;
    while i < toks.len() {
        let t = toks.get(i).copied().unwrap_or("");
        match t {
            "M" => {
                if let (Some(x), Some(y)) = (num(i + 1), num(i + 2)) {
                    bp.move_to((x, y));
                    started = true;
                }
                i += 3;
            }
            "L" => {
                if let (Some(x), Some(y)) = (num(i + 1), num(i + 2)) {
                    if !started {
                        bp.move_to((x, y));
                        started = true;
                    } else {
                        bp.line_to((x, y));
                    }
                }
                i += 3;
            }
            "C" => {
                if let (Some(a), Some(b), Some(c), Some(d2), Some(e), Some(f)) =
                    (num(i + 1), num(i + 2), num(i + 3), num(i + 4), num(i + 5), num(i + 6))
                    && started
                {
                    bp.curve_to((a, b), (c, d2), (e, f));
                }
                i += 7;
            }
            "Q" => {
                if let (Some(a), Some(b), Some(c), Some(d2)) = (num(i + 1), num(i + 2), num(i + 3), num(i + 4))
                    && started
                {
                    bp.quad_to((a, b), (c, d2));
                }
                i += 5;
            }
            "Z" | "z" => {
                if started {
                    bp.close_path();
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    bp
}

/// The transform a 3-D camera applies to shape `s` of size `w` x `h` (approximated by the affine
/// map that fits it best), as drawn; `None` for no camera, and for groups, tables and charts.
pub fn camera_turn(rctx: &Ctx, s: &Shape, w: f64, h: f64) -> Option<Affine> {
    if matches!(s.kind, ShapeKind::Group { .. } | ShapeKind::Table(_) | ShapeKind::Chart(_)) {
        return None;
    }
    let (e, _) = resolve::effects(rctx, s);
    e.as_ref().and_then(|e| e.raw3d.as_deref()).and_then(camera::from_raw).and_then(|c| camera::affine(&c, w, h))
}

/// The rectangle a picture's image occupies given its crop (fractions; negative = padding).
pub fn crop_dest(bounds: Rect, crop: [f64; 4]) -> Rect {
    let [l, t, r, b] = crop.map(|v| if v.is_finite() { v.clamp(-10.0, 0.99) } else { 0.0 });
    let vis_w = (1.0 - l - r).max(1e-6);
    let vis_h = (1.0 - t - b).max(1e-6);
    let fw = bounds.width() / vis_w;
    let fh = bounds.height() / vis_h;
    Rect::new(bounds.x0 - l * fw, bounds.y0 - t * fh, bounds.x0 - l * fw + fw, bounds.y0 - t * fh + fh)
}

struct Frame<'a> {
    pres: &'a Presentation,
    opts: &'a RenderOpts<'a>,
    fields: SlideFields,
}

impl Renderer {
    fn canvas(pres: &Presentation, opts: &RenderOpts, filters: bool) -> (RenderContext, u16, u16, Affine) {
        let (w, h) = output_size(pres, opts);
        let mut ctx = RenderContext::new_with(
            w,
            h,
            vello_cpu::RenderSettings { num_threads: if cfg!(target_arch = "wasm32") || filters { 0 } else { opts.threads }, ..Default::default() },
        );
        if let Some(c) = opts.clear {
            ctx.set_paint(color(c, 1.0));
            ctx.fill_rect(&Rect::new(0.0, 0.0, w as f64, h as f64));
        }
        let view = Affine::translate((opts.offset.0, opts.offset.1)) * Affine::scale(opts.scale);
        (ctx, w, h, view)
    }

    /// Note that `r` (local space, placed by `m`) was drawn.
    fn track(&mut self, m: Affine, r: Rect) {
        let b = m.transform_rect_bbox(r);
        if b.is_finite() {
            self.drawn = Some(self.drawn.map_or(b, |d| d.union(b)));
        }
    }

    fn fields(pres: &Presentation, index: usize, opts: &RenderOpts) -> SlideFields {
        SlideFields::new(pres.slide_number(index), pres.header_footer.footer_text.clone(), opts)
    }

    /// Background plus the master and layout graphics the slide shows.
    fn backdrop(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, slide: &Slide, view: Affine) {
        self.background(ctx, f, rctx, Some(slide), view);
        let (show_layout, show_master) = resolve::show_master_shapes(slide, rctx.layout);
        if show_master {
            let mctx = Ctx::for_master(f.pres, rctx.master);
            for s in &rctx.master.shapes {
                if s.ph.is_none() {
                    self.shape(ctx, f, &mctx, s, view, 0);
                }
            }
        }
        if show_layout && let Some(l) = rctx.layout {
            let lctx = Ctx::for_layout(f.pres, rctx.master, l);
            for s in &l.shapes {
                if s.ph.is_none() {
                    self.shape(ctx, f, &lctx, s, view, 0);
                }
            }
        }
    }

    fn slide(&mut self, pres: &Presentation, slide: &Slide, index: usize, opts: &RenderOpts) -> Image {
        let (mut ctx, w, h, view) = Self::canvas(pres, opts, needs_filters(pres, slide));
        let f = Frame { pres, opts, fields: Self::fields(pres, index, opts) };
        if let Some(rctx) = Ctx::for_slide(pres, slide) {
            self.backdrop(&mut ctx, &f, &rctx, slide, view);
            for s in &slide.shapes {
                self.shape(&mut ctx, &f, &rctx, s, view, 0);
            }
        }
        self.finish(ctx, w, h)
    }

    fn slide_part(&mut self, pres: &Presentation, slide: &Slide, index: usize, part: SlidePart, opts: &RenderOpts) -> Image {
        let (mut ctx, w, h, view) = Self::canvas(pres, opts, needs_filters(pres, slide));
        let f = Frame { pres, opts, fields: Self::fields(pres, index, opts) };
        if let Some(rctx) = Ctx::for_slide(pres, slide) {
            match part {
                SlidePart::Backdrop => self.backdrop(&mut ctx, &f, &rctx, slide, view),
                SlidePart::Content => {
                    // Shapes filled with the background (`useBgFill`) still show it.
                    let (fill, ph) = resolve::background(&rctx, Some(slide));
                    self.bg = Some((fill, ph, view));
                    for s in &slide.shapes {
                        self.shape(&mut ctx, &f, &rctx, s, view, 0);
                    }
                }
            }
        }
        self.finish(ctx, w, h)
    }

    #[allow(clippy::too_many_arguments)]
    fn blend(
        &mut self,
        pres: &Presentation,
        old: (&Slide, usize),
        new: (&Slide, usize),
        mix: f64,
        shapes: &[(Shape, bool)],
        text: &[TextMorph],
        paths: &[PathMorph],
        opts: &RenderOpts,
    ) -> Image {
        let mix = if mix.is_finite() { mix.clamp(0.0, 1.0) } else { 1.0 };
        let (mut ctx, w, h, view) = Self::canvas(pres, opts, needs_filters(pres, old.0) || needs_filters(pres, new.0));
        let (Some(co), Some(cn)) = (Ctx::for_slide(pres, old.0), Ctx::for_slide(pres, new.0)) else { return self.finish(ctx, w, h) };
        // Backdrop graphics never take shape states: their ids may clash with the frame's shapes.
        let bopts = RenderOpts { state: None, date_text: opts.date_text.clone(), ..*opts };
        if mix < 1.0 {
            let f = Frame { pres, opts: &bopts, fields: Self::fields(pres, old.1, opts) };
            self.backdrop(&mut ctx, &f, &co, old.0, view);
        }
        if mix > 0.0 {
            let f = Frame { pres, opts: &bopts, fields: Self::fields(pres, new.1, opts) };
            let layered = mix < 1.0;
            if layered {
                ctx.push_layer(None, None, Some(mix as f32), None, None);
            }
            self.backdrop(&mut ctx, &f, &cn, new.0, view);
            if layered {
                ctx.pop_layer();
            }
        }
        let fo = Frame { pres, opts, fields: Self::fields(pres, old.1, opts) };
        let fnew = Frame { pres, opts, fields: Self::fields(pres, new.1, opts) };
        let mut drawn: Vec<ShapeId> = Vec::new();
        // Set only now: the backdrop's ids may clash with the frame's.
        self.paths = paths.to_vec();
        for (s, from_old) in shapes {
            let (f, c) = if *from_old { (&fo, &co) } else { (&fnew, &cn) };
            let Some(tm) = text.iter().find(|m| m.old == s.id || m.new == s.id) else {
                self.shape(&mut ctx, f, c, s, view, 0);
                continue;
            };
            self.shape(&mut ctx, f, c, &Shape { text: None, ..s.clone() }, view, 0);
            drawn.push(s.id);
            if !(drawn.contains(&tm.old) && drawn.contains(&tm.new)) {
                continue;
            }
            let find = |id: ShapeId| shapes.iter().map(|(s, _)| s).find(|s| s.id == id);
            if let (Some(a), Some(b)) = (find(tm.old), find(tm.new)) {
                let now = b.xfrm.unwrap_or_else(|| resolve::xfrm(&cn, b));
                morph_text::draw(&mut ctx, (&co, a, &fo.fields), (&cn, b, &fnew.fields), tm, now, view);
            }
        }
        self.paths.clear();
        self.morphs.sweep();
        self.finish(ctx, w, h)
    }

    fn layout(&mut self, pres: &Presentation, mi: usize, li: Option<usize>, opts: &RenderOpts) -> Image {
        let (w, h) = output_size(pres, opts);
        let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
        let view = Affine::translate((opts.offset.0, opts.offset.1)) * Affine::scale(opts.scale);
        let f = Frame { pres, opts, fields: SlideFields::new(1, String::new(), opts) };
        if let Some(m) = pres.masters.get(mi) {
            match li.and_then(|i| m.layouts.get(i)) {
                Some(l) => {
                    let c = Ctx::for_layout(pres, m, l);
                    self.background(&mut ctx, &f, &c, None, view);
                    if l.show_master_shapes {
                        let mc = Ctx::for_master(pres, m);
                        for s in m.shapes.iter().filter(|s| s.ph.is_none()) {
                            self.shape(&mut ctx, &f, &mc, s, view, 0);
                        }
                    }
                    for s in &l.shapes {
                        self.shape(&mut ctx, &f, &c, s, view, 0);
                    }
                }
                None => {
                    let c = Ctx::for_master(pres, m);
                    self.background(&mut ctx, &f, &c, None, view);
                    for s in &m.shapes {
                        self.shape(&mut ctx, &f, &c, s, view, 0);
                    }
                }
            }
        }
        self.finish(ctx, w, h)
    }

    fn finish(&mut self, mut ctx: RenderContext, w: u16, h: u16) -> Image {
        ctx.flush();
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut pixels) {
            ctx.render(pm, &mut self.resources);
        }
        Image { width: w as u32, height: h as u32, pixels }
    }

    fn background(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, slide: Option<&Slide>, view: Affine) {
        let (fill, ph) = resolve::background(rctx, slide);
        if f.opts.transparent_background {
            self.bg = Some((fill, ph, view));
            return;
        }
        let r = Rect::new(0.0, 0.0, f.pres.slide_size.width, f.pres.slide_size.height);
        ctx.set_transform(view);
        let path = r.to_path(0.1);
        // PowerPoint lays every background over white: a picture stretched into part of the
        // slide, or a see-through colour, leaves white around and under it.
        ctx.set_paint(color(Rgba::WHITE, 1.0));
        ctx.fill_path(&path);
        paint::fill_path(ctx, rctx, &fill, ph, &path, r, view, FillMode::Norm, 1.0, f.pres);
        self.bg = Some((fill, ph, view));
    }

    fn shape(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, s: &Shape, parent: Affine, depth: usize) {
        if depth == 0 {
            let outer = std::mem::replace(&mut self.view, parent);
            self.shape_at(ctx, f, rctx, s, parent, depth);
            self.view = outer;
        } else {
            self.shape_at(ctx, f, rctx, s, parent, depth);
        }
    }

    /// How lines are drawn for a shape under `m` at `depth`: the transform to stroke under and the
    /// one that takes the shape's outline into it. Inside a group they are the slide-level view and
    /// the rest of the chain, so the group's child-coordinate scale moves and sizes the outline but
    /// never the line width; outside a group, `m` itself and no change to the outline.
    fn line_space(&self, m: Affine, depth: usize) -> (Affine, Affine) {
        let view = self.view;
        if depth > 0 && view.determinant().abs() > 1e-12 && view.is_finite() { (view, view.inverse() * m) } else { (m, Affine::IDENTITY) }
    }

    /// Shape-space units per point: a group's child coordinates can scale a shape's box (a legacy
    /// 1360-unit child space makes one unit hundreds of EMUs), but 3-D sizes kept in points
    /// (contour width, extrusion depth, bevel) stay points, as PowerPoint draws them. 1 outside a
    /// group.
    fn per_point(&self, m: Affine, depth: usize) -> f64 {
        let k = scale_of(self.line_space(m, depth).1);
        if k.is_finite() && k > 1e-12 { 1.0 / k } else { 1.0 }
    }

    /// The size and transform a shape's text is set in: inside a group whose child coordinates
    /// scale the shape, the shape's size in slide points and `m` with that scale taken out, so
    /// insets and font sizes stay in points; otherwise `w`, `h` and `m` themselves.
    fn text_space(&self, m: Affine, depth: usize, w: f64, h: f64) -> (f64, f64, Affine) {
        let (_, rel) = self.line_space(m, depth);
        let c = rel.as_coeffs();
        let (sx, sy) = (c[0].hypot(c[1]), c[2].hypot(c[3]));
        let ok = |k: f64| k.is_finite() && k > 1e-9;
        if depth == 0 || !ok(sx) || !ok(sy) || ((sx - 1.0).abs() < 1e-9 && (sy - 1.0).abs() < 1e-9) {
            return (w, h, m);
        }
        let (tw, th) = (w * sx, h * sy);
        if !tw.is_finite() || !th.is_finite() {
            return (w, h, m);
        }
        (tw, th, m * Affine::scale_non_uniform(1.0 / sx, 1.0 / sy))
    }

    /// A shape's geometry in its own `w` × `h` space, built at its size on the slide: inside a
    /// group whose child coordinates scale the shape unevenly, the preset's rounded corners and
    /// other size-derived features stay as they would be on a plain shape of that size (#1083).
    fn scaled_geometry(&self, s: &Shape, m: Affine, depth: usize, w: f64, h: f64) -> Geometry {
        let (tw, th, _) = self.text_space(m, depth, w, h);
        if (tw, th) == (w, h) {
            return shape_geometry(s, w, h);
        }
        let mut geo = shape_geometry(s, tw, th);
        let k = |a: f64, b: f64| if b.abs() > 1e-12 { a / b } else { 1.0 };
        let back = Affine::scale_non_uniform(k(w, tw), k(h, th));
        for p in &mut geo.paths {
            p.path.apply_affine(back);
        }
        geo.text_rect = back.transform_rect_bbox(geo.text_rect);
        for p in &mut geo.sites {
            *p = back * *p;
        }
        geo
    }

    fn shape_at(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, s: &Shape, parent: Affine, depth: usize) {
        if depth > 32 || (s.hidden && !f.opts.show_hidden) || f.opts.skip.contains(&s.id) {
            return;
        }
        // Footer placeholders on layouts/masters show only on slides that include them.
        let st = f.opts.state.map(|g| g(s.id)).unwrap_or_default();
        if !st.visible || st.opacity <= 0.001 {
            return;
        }
        let x = resolve::xfrm(rctx, s);
        if !x.is_finite() {
            return;
        }
        let mut m = parent * x.affine();
        if st.offset != Vec2::ZERO || st.scale != (1.0, 1.0) || st.rotate != 0.0 {
            let c = Point::new(x.w / 2.0, x.h / 2.0);
            let around = Affine::translate(c.to_vec2())
                * Affine::rotate(st.rotate.to_radians())
                * Affine::scale_non_uniform(st.scale.0, st.scale.1)
                * Affine::translate(-c.to_vec2());
            m = parent * Affine::translate(st.offset) * x.affine() * around;
        }
        if let Some(a) = camera_turn(rctx, s, x.w, x.h) {
            m *= a;
        }
        let layered = st.opacity < 0.999 || st.clip.is_some();
        if layered {
            ctx.set_transform(m);
            let clip = st.clip.map(|[l, t, r, b]| Rect::new(l * x.w, t * x.h, r * x.w, b * x.h).to_path(0.1));
            ctx.push_layer(clip.as_ref(), None, Some(st.opacity.clamp(0.0, 1.0) as f32), None, None);
        }
        match &s.kind {
            ShapeKind::Group { children, child } => {
                let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
                let gm = parent * deckcraft_geom::group_child_affine(&x, ch);
                // A group's own fill (a group filled with its parent's passes that on).
                let own = match &s.fill {
                    Some(Fill::Group) | None => self.group_fill.clone(),
                    Some(fl) => Some((fl.clone(), m, Rect::new(0.0, 0.0, x.w, x.h))),
                };
                let outer = std::mem::replace(&mut self.group_fill, own);
                // A group's outer shadow is cast by everything in it, as PowerPoint draws it: the
                // children drawn once more under the shadow's offset, scale and skew, recoloured to
                // the shadow colour and blurred, below the group.
                if let Some(sh) = s.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()) {
                    let c = rctx.color(&sh.color, None);
                    let k = scale_of(m);
                    let d = Vec2::new(sh.dir.to_radians().cos(), sh.dir.to_radians().sin()) * sh.dist;
                    let local = shadow_local(sh, x.w, x.h);
                    let (own, pre) =
                        if sh.rotate_with_shape { (Affine::translate(d) * local, Affine::IDENTITY) } else { (local, Affine::translate(d * k)) };
                    let det = m.determinant();
                    if det.is_finite() && det.abs() > 1e-12 {
                        let shift = pre * m * own * m.inverse();
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.push_layer(None, None, Some((c.a as f32) / 255.0), None, Some(blur(sh.blur / 3.0 * k)));
                        for ch in children {
                            self.shape(ctx, f, rctx, ch, shift * gm, depth + 1);
                        }
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.push_layer(None, Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::SrcIn)), None, None, None);
                        ctx.set_paint(color(c.with_alpha(255), 1.0));
                        let (w, h) = (f.opts.scale * f.pres.slide_size.width, f.opts.scale * f.pres.slide_size.height);
                        ctx.fill_rect(&Rect::new(-w, -h, 3.0 * w, 3.0 * h).inflate(1e4, 1e4));
                        ctx.pop_layer();
                        ctx.pop_layer();
                    }
                }
                for c in children {
                    self.shape(ctx, f, rctx, c, gm, depth + 1);
                }
                self.group_fill = outer;
            }
            ShapeKind::Table(t) => {
                // Rows grow with their text: what the cells cover, borders included.
                let cells = table::draw(ctx, f.pres, rctx, t, m, x.w, x.h, &f.fields, f.opts.edit && f.opts.editing_text == Some(s.id));
                self.track(m, cells.unwrap_or(Rect::new(0.0, 0.0, x.w, x.h)).inflate(2.0, 2.0));
            }
            ShapeKind::Chart(c) => {
                self.track(m, Rect::new(0.0, 0.0, x.w, x.h));
                chart::draw(ctx, rctx, c, m, x.w, x.h)
            }
            ShapeKind::Ink { strokes } => {
                for sk in strokes {
                    let legacy = deckcraft_model::ink_pressure_is_legacy(&sk.points);
                    let pen = if sk.width.is_finite() { sk.width.abs() } else { 0.0 };
                    for (px, py, pr) in &sk.points {
                        let w = deckcraft_model::ink_sample_width(pen, *pr, legacy);
                        self.track(parent, Rect::new(*px, *py, *px, *py).inflate(w, w));
                    }
                    let pts: Vec<(f64, f64, f32)> = sk.points.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    let (base, rel) = self.line_space(parent, depth);
                    ctx.set_transform(base);
                    ctx.set_paint(color(sk.color, if sk.highlighter { 0.4 } else { 1.0 }));
                    // Uniform-width and highlighter strokes are one path: segments drawn one by one
                    // overlap at the joins, which darkens see-through highlighter ink in beads.
                    if legacy || sk.highlighter {
                        let mut bp = BezPath::new();
                        for (i, (px, py, _)) in pts.iter().enumerate() {
                            if i == 0 {
                                bp.move_to((*px, *py));
                            } else {
                                bp.line_to((*px, *py));
                            }
                        }
                        bp.apply_affine(rel);
                        ctx.set_stroke(kurbo::Stroke::new(pen.max(0.1)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
                        ctx.stroke_path(&bp);
                        continue;
                    }
                    for pair in pts.windows(2) {
                        let [p0, p1] = pair else { continue };
                        let mut bp = BezPath::new();
                        bp.move_to((p0.0, p0.1));
                        bp.line_to((p1.0, p1.1));
                        bp.apply_affine(rel);
                        let a = deckcraft_model::ink_sample_width(pen, p0.2, legacy);
                        let b = deckcraft_model::ink_sample_width(pen, p1.2, legacy);
                        ctx.set_stroke(kurbo::Stroke::new(((a + b) * 0.5).max(0.1)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
                        ctx.stroke_path(&bp);
                    }
                }
            }
            _ => self.geometry_shape(ctx, f, rctx, s, m, x.w, x.h, &st, depth),
        }
        if layered {
            ctx.pop_layer();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn geometry_shape(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        rctx: &Ctx,
        s: &Shape,
        m: Affine,
        w: f64,
        h: f64,
        st: &ShapeState,
        depth: usize,
    ) {
        let (geo, fades) = match self.paths.iter().find(|p| p.id == s.id) {
            Some(pm) => self.morphs.geometry(pm, s, w, h),
            None => (self.scaled_geometry(s, m, depth, w, h), Vec::new()),
        };
        let fade = |i: usize| fades.get(i).copied().unwrap_or(deckcraft_geom::morph::Fade { fill: 1.0, stroke: 1.0 });
        let (fill, fill_ph) = resolve::fill(rctx, s);
        let (line, line_ph) = resolve::line(rctx, s);
        let (effects, fx_ph) = resolve::effects(rctx, s);
        let reach = extent::spill(Some(&line), effects.as_ref(), w, h);
        self.track(m, Rect::new(0.0, 0.0, w, h).inflate(reach, reach));
        let is_pic = matches!(s.kind, ShapeKind::Picture { .. });
        let outline = geo.outline();
        // Placeholders in the editor: dashed outline and prompt, nothing else when empty.
        let empty_ph = s.ph.is_some() && s.text.as_ref().is_none_or(TextBody::is_empty) && !is_pic && !matches!(s.kind, ShapeKind::Media(_));
        let fill = if let Some(t) = st.tint { Some(Fill::solid(deckcraft_model::ColorRef::rgb(t))) } else { fill };
        // Effects below the shape.
        if let Some(e) = &effects {
            // A picture (or picture fill) casts its shadow from its pixels, not its frame.
            let content = match (&s.kind, &fill) {
                (ShapeKind::Picture { fill: pf }, _) | (_, Some(Fill::Picture(pf))) => Some(paint::resolved(rctx, pf, None).into_owned()),
                _ => None,
            };
            // What casts the shadow: the fill as see-through as it is, and the outline.
            let fill_alpha = fill_opacity(rctx, &fill, fill_ph);
            let outline = line.fill.as_ref().filter(|f| !f.is_none()).map(|lf| {
                let a = match lf {
                    Fill::Solid { color: c } => rctx.color(c, line_ph).a as f64 / 255.0,
                    _ => 1.0,
                };
                let (base, rel) = self.line_space(m, depth);
                let lw = line.width.unwrap_or(0.75).max(0.0);
                let outside = if border_outside(is_pic, effects.as_ref()) { outside_line(&line, lw) } else { None };
                Caster { line: outside.map_or(std::borrow::Cow::Borrowed(&line), std::borrow::Cow::Owned), ph: line_ph, alpha: a, base, rel }
            });
            self.effects_below(ctx, rctx, e, fx_ph, &geo, &fill, fill_alpha, outline, content.as_ref(), m, w, h);
        }
        let soft = effects.as_ref().and_then(|e| e.soft_edge).filter(|r| *r > 0.0);
        if let Some(r) = soft {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, None, None, None);
            let _ = r;
        }
        // Shape-space units per point for the 3-D sizes, measured without the camera's turn (which
        // `shape_at` puts in `m`): its foreshortening is not a child-coordinate scale.
        let pt = {
            let turn = effects
                .as_ref()
                .and_then(|e| e.raw3d.as_deref())
                .and_then(camera::from_raw)
                .and_then(|c| camera::affine(&c, w, h))
                .filter(|t| t.determinant().abs() > 1e-9);
            self.per_point(turn.map_or(m, |t| m * t.inverse()), depth)
        };
        // A 3-D extrusion under a turned camera: the sides, swept behind the face.
        if let Some(raw) = effects.as_ref().and_then(|e| e.raw3d.as_deref())
            && let Some(ex) = extrude::from_raw(raw)
            && let Some(cam) = camera::from_raw(raw)
            && let Some(turn) = camera::affine(&cam, w, h)
            && turn.determinant().abs() > 1e-9
        {
            let face = match &fill {
                Some(Fill::Solid { color: c }) => Some(rctx.color(c, fill_ph)),
                _ => None,
            };
            let pre = m * turn.inverse();
            let lit = light::Lighting::from_raw(raw);
            let d = ex.depth * pt;
            extrude::draw(ctx, &outline, w, h, &cam, d, pre, turn, extrude::side_color(rctx, &ex, face), &lit);
        }
        // A 3-D contour: a band around the outline, behind the face (and in front of any sides).
        if let Some(raw) = effects.as_ref().and_then(|e| e.raw3d.as_deref())
            && let Some(c) = extrude::contour_from_raw(raw)
        {
            let face = match &fill {
                Some(Fill::Solid { color: col }) => Some(rctx.color(col, fill_ph)),
                _ => None,
            };
            let col = c.color.as_ref().map(|k| rctx.color(k, face)).or(face).unwrap_or(Rgba::rgb(0xff, 0xff, 0xff));
            extrude::draw_contour(ctx, &outline, m, c.width * pt, col, &light::Lighting::from_raw(raw));
        }
        // Fill.
        match &s.kind {
            ShapeKind::Picture { fill: pf } => {
                ctx.set_transform(m);
                let bounds = Rect::new(0.0, 0.0, w, h);
                ctx.push_clip_layer(&outline);
                let t = f.opts.gif_times.iter().find(|g| g.0 == s.id).map(|g| g.1);
                paint::picture(ctx, f.pres, &paint::resolved(rctx, pf, None), bounds, m, t);
                ctx.pop_layer();
            }
            // An object DeckCraft doesn't model (an embedded OLE object, an unknown graphic): its
            // preview picture, stretched over the frame, as PowerPoint draws it.
            ShapeKind::Opaque { preview: Some(media), .. } => {
                ctx.set_transform(m);
                let pf = deckcraft_model::style::PictureFill { media: *media, ..Default::default() };
                paint::picture(ctx, f.pres, &pf, Rect::new(0.0, 0.0, w, h), m, None);
            }
            ShapeKind::Media(mc) => {
                ctx.set_transform(m);
                let bounds = Rect::new(0.0, 0.0, w, h);
                if let Some(p) = mc.poster {
                    let pf = deckcraft_model::style::PictureFill { media: p, crop: mc.crop, ..Default::default() };
                    ctx.push_clip_layer(&outline);
                    paint::picture(ctx, f.pres, &pf, bounds, m, None);
                    ctx.pop_layer();
                } else if mc.video {
                    ctx.set_paint(peniko::Color::from_rgba8(20, 20, 24, 255));
                    ctx.fill_rect(&bounds);
                }
                if !mc.video || mc.poster.is_none() {
                    paint::media_glyph(ctx, mc.video, bounds);
                }
            }
            _ => {
                if let Some(fl) = &fill
                    && !(empty_ph && f.opts.edit && s.fill.is_none())
                {
                    for (i, sp) in geo.paths.iter().enumerate() {
                        if sp.fill == FillMode::None || fade(i).fill <= 0.0 {
                            continue;
                        }
                        ctx.set_transform(m);
                        ctx.set_fill_rule(if sp.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
                        match (fl, &self.bg) {
                            // `useBgFill`: the slide's background, laid out on the slide (not the
                            // shape), shows through the shape.
                            (Fill::Background, Some((bg, ph, view))) => {
                                let slide = Rect::new(0.0, 0.0, f.pres.slide_size.width, f.pres.slide_size.height);
                                let path = (view.inverse() * m) * sp.path.clone();
                                paint::fill_path(ctx, rctx, bg, *ph, &path, slide, *view, sp.fill, fade(i).fill, f.pres);
                            }
                            // `grpFill`: the group's fill, laid out over the whole group.
                            (Fill::Group, _) => {
                                if let Some((gf, gm, gbox)) = &self.group_fill
                                    && gm.determinant().abs() > 1e-12
                                {
                                    let path = (gm.inverse() * m) * sp.path.clone();
                                    paint::fill_path(ctx, rctx, gf, None, &path, *gbox, *gm, sp.fill, fade(i).fill, f.pres);
                                }
                            }
                            _ => paint::fill_path(ctx, rctx, fl, fill_ph, &sp.path, Rect::new(0.0, 0.0, w, h), m, sp.fill, fade(i).fill, f.pres),
                        }
                        ctx.set_fill_rule(peniko::Fill::NonZero);
                    }
                }
            }
        }
        // `a:fillOverlay`: a fill blended over the shape's fill (PowerPoint's "Flow" themes multiply
        // a gradient over a tiled picture), inside the filled outline, under the line.
        if let Some(ov) = effects.as_ref().and_then(|e| e.fill_overlay.as_ref())
            && fill.as_ref().is_some_and(|f| !f.is_none())
        {
            let mix = match ov.blend.as_str() {
                "mult" => peniko::Mix::Multiply,
                "screen" => peniko::Mix::Screen,
                "darken" => peniko::Mix::Darken,
                "lighten" => peniko::Mix::Lighten,
                _ => peniko::Mix::Normal,
            };
            for (i, sp) in geo.paths.iter().enumerate().filter(|(i, sp)| sp.fill != FillMode::None && fade(*i).fill > 0.0) {
                ctx.set_transform(m);
                ctx.push_layer(Some(&sp.path), Some(peniko::BlendMode::new(mix, peniko::Compose::SrcOver)), None, None, None);
                paint::fill_path(ctx, rctx, &ov.fill, fx_ph, &sp.path, Rect::new(0.0, 0.0, w, h), m, FillMode::Norm, fade(i).fill, f.pres);
                ctx.pop_layer();
            }
        }
        // Some presets' outlines default to sharp corners (PowerPoint's own per-shape default).
        let line = match (&s.geom, line.join) {
            (Geom::Preset { name, .. }, None) if preset::mitred_by_default(name) => {
                Line { join: Some(deckcraft_model::style::LineJoin::Miter), ..line }
            }
            _ => line,
        };
        // A 3-D bevel: the face and a band inside the outline lit as surfaces.
        if let Some(bv) = effects.as_ref().and_then(|e| e.raw3d.as_deref()).and_then(bevel::from_raw) {
            let bv = bevel::Bevel { w: bv.w * pt, h: bv.h * pt, ..bv };
            let solid = match &fill {
                Some(Fill::Solid { color: c }) => Some(rctx.color(c, fill_ph)),
                _ => None,
            };
            let filled = is_pic || fill.as_ref().is_some_and(|f| !f.is_none());
            if filled {
                let mut area = BezPath::new();
                for sp in geo.paths.iter().filter(|sp| sp.fill != FillMode::None) {
                    area.extend(sp.path.iter());
                }
                if is_pic || area.elements().is_empty() {
                    area = outline.clone();
                }
                bevel::draw(ctx, &bv, if is_pic { None } else { solid }, &area, Rect::new(0.0, 0.0, w, h), m, 1.0);
            }
        }
        // Outline.
        if line.fill.as_ref().is_some_and(|f| !f.is_none()) {
            let lw = line.width.unwrap_or(0.75).max(0.0);
            let (base, rel) = self.line_space(m, depth);
            // Arrowheads go on lines, arcs and open freeforms (filled or not), never on closed
            // outlines or on presets like brackets.
            let ends = s.is_line()
                || geo.is_open()
                || (matches!(s.geom, Geom::Custom { .. })
                    && geo.paths.iter().find(|p| p.stroke).is_some_and(|p| !p.path.elements().iter().any(|e| matches!(e, PathEl::ClosePath))));
            let trims = if ends { head_trims(&line, lw) } else { (0.0, 0.0) };
            let has_heads = ends && [&line.head, &line.tail].iter().any(|e| e.as_ref().is_some_and(|e| e.kind != "none"));
            // A see-through line and its arrowheads are one shape in PowerPoint: drawn opaque into
            // a layer that is then faded, so they don't darken where they meet.
            let see_through = match &line.fill {
                Some(Fill::Solid { color: c }) if has_heads => Some(rctx.color(c, line_ph)).filter(|c| c.a < 255),
                _ => None,
            };
            let line = match see_through {
                Some(c) => {
                    ctx.set_transform(Affine::IDENTITY);
                    ctx.push_layer(None, None, Some(c.a as f32 / 255.0), None, None);
                    std::borrow::Cow::Owned(Line {
                        fill: Some(Fill::solid(deckcraft_model::style::ColorRef::rgb(c.with_alpha(255)))),
                        ..line.clone()
                    })
                }
                None => std::borrow::Cow::Borrowed(&line),
            };
            // A picture's border lies outside the picture, as PowerPoint draws it: stroked twice
            // as wide, outside the outline only (see `border_outside`).
            let outside = border_outside(is_pic, effects.as_ref()).then(|| outside_line(&line, lw)).flatten();
            let clipped = outside.is_some();
            let (line, lw) = match outside {
                Some(o) => {
                    let mut clip = Rect::new(0.0, 0.0, w, h).inflate(w + h + lw * 8.0, w + h + lw * 8.0).to_path(0.1);
                    clip.extend(outline.iter());
                    ctx.set_transform(m);
                    ctx.set_fill_rule(peniko::Fill::EvenOdd);
                    ctx.push_clip_layer(&clip);
                    ctx.set_fill_rule(peniko::Fill::NonZero);
                    (std::borrow::Cow::Owned(o), lw * 2.0)
                }
                None => (line, lw),
            };
            let first_stroked = geo.paths.iter().position(|p| p.stroke);
            for (i, sp) in geo.paths.iter().enumerate().filter(|(i, p)| p.stroke && fade(*i).stroke > 0.0) {
                let mut path = if rel == Affine::IDENTITY {
                    std::borrow::Cow::Borrowed(&sp.path)
                } else {
                    let mut moved = sp.path.clone();
                    moved.apply_affine(rel);
                    std::borrow::Cow::Owned(moved)
                };
                // The line stops inside its arrowheads, so a wide line doesn't poke out of them.
                let whole = path.clone();
                if Some(i) == first_stroked && trims != (0.0, 0.0) {
                    path = std::borrow::Cow::Owned(trim_ends(&path, trims.0, trims.1));
                }
                stroke(ctx, rctx, &line, line_ph, &path, Some(&whole), base, lw, fade(i).stroke);
            }
            if ends {
                arrowheads(ctx, rctx, &line, line_ph, &geo, rel, base, lw);
            }
            if clipped {
                ctx.pop_layer();
            }
            if see_through.is_some() {
                ctx.pop_layer();
            }
        }
        if let Some(r) = soft {
            // Feather: the shape (fill and line) fades to nothing at its outline. Measured on
            // PowerPoint's export: the mask is the outline shrunk by 0.85 r, blurred by a third of
            // r, so it's half opaque 0.85 r inside the edge and transparent at the edge itself.
            ctx.set_transform(m);
            ctx.push_layer(
                None,
                Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestIn)),
                None,
                None,
                Some(blur(r / 3.0 * scale_of(m))),
            );
            ctx.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255));
            ctx.fill_path(&outline);
            ctx.push_blend_layer(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestOut));
            ctx.set_stroke(kurbo::Stroke::new(r * 1.7).with_join(kurbo::Join::Round));
            ctx.stroke_path(&outline);
            ctx.pop_layer();
            ctx.pop_layer();
            ctx.pop_layer();
        }
        // Inner shadow.
        if let Some(e) = &effects
            && let Some(sh) = &e.inner_shadow
        {
            ctx.set_transform(m);
            ctx.push_clip_layer(&outline);
            let c = rctx.color(&sh.color, fx_ph);
            let d = Vec2::new(sh.dir.to_radians().cos(), sh.dir.to_radians().sin()) * sh.dist;
            // As PowerPoint draws it (measured on its export): the shadow falls inside along the
            // edges it points to (45 degrees: the bottom and right), blurred by a third of the
            // blur radius.
            ctx.push_layer(None, None, None, None, Some(blur(sh.blur / 3.0 * scale_of(m))));
            ctx.set_paint(color(c, 1.0));
            let mut ring = Rect::new(0.0, 0.0, w, h).inflate(w.max(h), w.max(h)).to_path(0.1);
            let mut hole = outline.clone();
            hole.apply_affine(Affine::translate(-d));
            ring.extend(hole.iter());
            ctx.set_fill_rule(peniko::Fill::EvenOdd);
            ctx.fill_path(&ring);
            ctx.set_fill_rule(peniko::Fill::NonZero);
            ctx.pop_layer();
            ctx.pop_layer();
        }
        // Text (or the placeholder prompt in the editor).
        let skip_text = f.opts.skip_text == Some(s.id);
        if let Some(body) = &s.text
            && !skip_text
        {
            // Text in a group is set at the shape's size on the slide, in points: a group's
            // child-coordinate scale sizes and places the box, never the type (#233's twin).
            let (tw, th, m) = self.text_space(m, depth, w, h);
            let text_geo;
            let geo = if (tw, th) == (w, h) {
                &geo
            } else {
                text_geo = shape_geometry(s, tw, th);
                &text_geo
            };
            let tr = geo.text_rect;
            let editing = f.opts.edit && f.opts.editing_text == Some(s.id);
            if !body.is_empty() || editing {
                // Imported placeholders can have no paragraph until the first keystroke.
                let mut empty;
                let body = if editing && body.paragraphs.is_empty() {
                    empty = body.clone();
                    empty.paragraphs.push(Default::default());
                    &empty
                } else {
                    body
                };
                let mut l = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields: &f.fields, prompt_color: None, no_shrink: false });
                if editing {
                    preview_empty_bullets(&mut l);
                }
                let tm = text_transform(m, tr, l.rotation);
                for b in l.lines.iter().filter_map(line_bounds) {
                    // Glyphs reach a little past their carets and line box (italics, swashes).
                    let pad = b.height() * 0.3;
                    self.track(tm, b.inflate(pad, pad));
                }
                match text_warp(rctx, s, tr).filter(|_| l.rotation == 0.0) {
                    Some(w) => warp::draw(ctx, &l, tr, m, &st.paras, &w),
                    None => draw_layout_with(ctx, &l, tr, m, &st.paras),
                }
            } else if f.opts.edit && s.ph.is_some() {
                let kind = s.ph_type().unwrap_or(PhType::Body);
                let prompt_body = prompt_body(rctx, s, kind);
                if !prompt_body.is_empty() {
                    let grey = Rgba::rgb(0x59, 0x59, 0x59);
                    draw_text(ctx, rctx, s, &prompt_body, tr, m, &f.fields, Some(grey));
                }
            }
        }
        if empty_ph && f.opts.edit {
            // Dashed placeholder outline.
            ctx.set_transform(m);
            let px = 1.0 / scale_of(m).max(1e-6);
            ctx.set_paint(peniko::Color::from_rgba8(0xA6, 0xA6, 0xA6, 255));
            ctx.set_stroke(kurbo::Stroke::new(px).with_dashes(0.0, [3.0 * px, 2.0 * px]));
            ctx.stroke_path(&Rect::new(0.0, 0.0, w, h).to_path(0.1));
            if matches!(s.ph_type(), Some(PhType::Picture | PhType::Chart | PhType::Table | PhType::Media | PhType::Diagram)) {
                paint::placeholder_icon(ctx, s.ph_type().unwrap_or(PhType::Picture), Rect::new(0.0, 0.0, w, h));
            }
        }
    }

    fn effects_below(
        &mut self,
        ctx: &mut RenderContext,
        rctx: &Ctx,
        e: &Effects,
        ph: Option<Rgba>,
        geo: &Geometry,
        fill: &Option<Fill>,
        fill_alpha: Option<f64>,
        outline: Option<Caster>,
        content: Option<&deckcraft_model::style::PictureFill>,
        m: Affine,
        w: f64,
        h: f64,
    ) {
        let silhouette = {
            let mut p = BezPath::new();
            for sp in &geo.paths {
                if sp.fill != FillMode::None || fill.is_none() {
                    p.extend(sp.path.iter());
                }
            }
            if p.elements().is_empty() { geo.outline() } else { p }
        };
        let k = scale_of(m);
        if let Some(sh) = &e.outer_shadow {
            let c = rctx.color(&sh.color, ph);
            let d = Vec2::new(sh.dir.to_radians().cos(), sh.dir.to_radians().sin()) * sh.dist;
            let local = shadow_local(sh, w, h);
            // Shadow offset is in slide space (doesn't rotate with the shape unless asked).
            let (own, pre) = if sh.rotate_with_shape { (Affine::translate(d) * local, Affine::IDENTITY) } else { (local, Affine::translate(d * k)) };
            let sm = pre * m * own;
            ctx.set_transform(Affine::IDENTITY);
            // Blurred by a third of the blur radius, as PowerPoint's export shows.
            ctx.push_layer(None, None, Some((c.a as f32) / 255.0), None, Some(blur(sh.blur / 3.0 * k)));
            ctx.set_transform(sm);
            match content {
                // As PowerPoint draws it: the shadow is the picture's own coverage (a logo on a
                // transparent background casts the logo's shadow): the picture, clipped to the
                // shape, recoloured to the shadow colour.
                Some(pf) => {
                    ctx.push_clip_layer(&silhouette);
                    paint::picture(ctx, rctx.pres, pf, Rect::new(0.0, 0.0, w, h), sm, None);
                    // Drawing the picture leaves its own pixel transform set: back to the shape's,
                    // or the recolouring below covers only part of a large picture's shadow.
                    ctx.set_transform(sm);
                    ctx.push_layer(None, Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::SrcIn)), None, None, None);
                    ctx.set_paint(color(c.with_alpha(255), 1.0));
                    ctx.fill_rect(&Rect::new(0.0, 0.0, w, h).inflate(1.0, 1.0));
                    ctx.pop_layer();
                    ctx.pop_layer();
                }
                // The shadow is cast by what is drawn, as PowerPoint casts it: the filled part as
                // see-through as its fill (none for an outline-only shape) …
                None => {
                    if let Some(a) = fill_alpha {
                        let mut filled = BezPath::new();
                        for sp in geo.paths.iter().filter(|sp| sp.fill != FillMode::None) {
                            filled.extend(sp.path.iter());
                        }
                        ctx.set_paint(color(c.with_alpha(255), a));
                        ctx.fill_path(&filled);
                    }
                }
            }
            // … and the outline (a picture's border too).
            if let Some(o) = &outline {
                let cast = Line { fill: Some(Fill::solid(deckcraft_model::style::ColorRef::rgb(c.with_alpha(255)))), ..o.line.as_ref().clone() };
                let lw = o.line.width.unwrap_or(0.75).max(0.0);
                for sp in geo.paths.iter().filter(|sp| sp.stroke) {
                    let mut path = sp.path.clone();
                    path.apply_affine(o.rel * own);
                    stroke(ctx, rctx, &cast, o.ph, &path, None, pre * o.base, lw, o.alpha);
                }
            }
            ctx.pop_layer();
        }
        // A shape with no fill and no line (a text box) draws nothing to glow around: its text's
        // own glow is all PowerPoint shows, not a halo round the box.
        let glows = fill_alpha.is_some() || content.is_some() || outline.is_some();
        if let Some(g) = e.glow.as_ref().filter(|_| glows) {
            let c = rctx.color(&g.color, ph);
            ctx.set_transform(Affine::IDENTITY);
            // Measured on PowerPoint's export: a glow of radius r is half strength about half of r
            // out from the outline and gone by r.
            ctx.push_layer(None, None, Some((c.a as f32) / 255.0), None, Some(blur(g.radius * 0.17 * k)));
            ctx.set_transform(m);
            ctx.set_paint(color(c.with_alpha(255), 1.0));
            ctx.set_stroke(kurbo::Stroke::new(g.radius.max(0.0) * 0.95).with_join(kurbo::Join::Round));
            ctx.stroke_path(&silhouette);
            // A shape without a fill (an outline, a frame) glows along its line only (#225).
            if fill_alpha.is_some() || content.is_some() {
                ctx.fill_path(&silhouette);
            }
            ctx.pop_layer();
        }
        if let Some(rf) = &e.reflection {
            // Mirror below the shape, fading out.
            let mm = m * Affine::translate((0.0, 2.0 * h + rf.dist)) * Affine::scale_non_uniform(1.0, -1.0);
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, Some(rf.start_alpha.clamp(0.0, 1.0) as f32 * 0.6), None, Some(blur(rf.blur * 0.5 * k)));
            ctx.set_transform(mm);
            match content {
                // A picture reflects its pixels (it has no fill of its own).
                Some(pf) => {
                    ctx.push_clip_layer(&silhouette);
                    paint::picture(ctx, rctx.pres, pf, Rect::new(0.0, 0.0, w, h), mm, None);
                    ctx.pop_layer();
                }
                None => {
                    if let Some(fl) = fill {
                        paint::fill_path(ctx, rctx, fl, ph, &silhouette, Rect::new(0.0, 0.0, w, h), mm, FillMode::Norm, 1.0, rctx.pres);
                    }
                }
            }
            ctx.set_transform(mm);
            // Fade mask.
            ctx.push_layer(None, Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestIn)), None, None, None);
            let grad = peniko::Gradient::new_linear(Point::new(0.0, h), Point::new(0.0, h * (1.0 - rf.end_pos.clamp(0.01, 1.0))))
                .with_stops([(0.0, peniko::Color::from_rgba8(0, 0, 0, 255)), (1.0, peniko::Color::from_rgba8(0, 0, 0, 0))].as_slice());
            ctx.set_paint(grad);
            ctx.fill_rect(&Rect::new(0.0, 0.0, w, h).inflate(4.0, 4.0));
            ctx.pop_layer();
            ctx.pop_layer();
        }
    }
}

/// Whether a shape's border is drawn outside its outline: a picture's is, unless the picture is
/// drawn in 3-D (a bevel seen through a 3-D scene), where PowerPoint centres it on the edge as on
/// any shape (observed: an 18 pt border covers 9 pt of the picture then).
fn border_outside(is_pic: bool, effects: Option<&Effects>) -> bool {
    is_pic && !effects.is_some_and(|e| e.bevel.is_some() && e.raw3d.as_deref().is_some_and(|r| r.contains("scene3d")))
}

/// The line that draws `line` (`lw` wide) outside a picture: twice as wide, so the half outside
/// the outline is the border, with dashes kept their length. `None` for compound lines, whose
/// bands would not survive the doubling (they stay centred on the outline).
fn outside_line(line: &Line, lw: f64) -> Option<Line> {
    if line.compound.is_some_and(|c| c != deckcraft_model::style::Compound::Single) {
        return None;
    }
    let dash = line.dash.as_ref().map(|d| {
        let p = d.pattern();
        deckcraft_model::style::Dash::Custom {
            pattern: p.chunks(2).map(|c| (c.first().copied().unwrap_or(0.0) / 2.0, c.get(1).copied().unwrap_or(0.0) / 2.0)).collect(),
        }
    });
    Some(Line { width: Some(lw * 2.0), dash: dash.or_else(|| line.dash.clone()), ..line.clone() })
}

/// A shape's outline as it casts a shadow: its line, that line's placeholder colour and opacity,
/// and the space it is stroked in (see `Renderer::line_space`).
struct Caster<'a> {
    line: std::borrow::Cow<'a, Line>,
    ph: Option<Rgba>,
    alpha: f64,
    base: Affine,
    rel: Affine,
}

/// How opaque a fill is overall (`None` when nothing is filled): a solid colour's alpha, a
/// gradient's mean stop alpha, otherwise opaque.
fn fill_opacity(rctx: &Ctx, fill: &Option<Fill>, ph: Option<Rgba>) -> Option<f64> {
    match fill {
        None | Some(Fill::None) => None,
        Some(Fill::Solid { color: c }) => Some(rctx.color(c, ph).a as f64 / 255.0),
        Some(Fill::Gradient(g)) if !g.stops.is_empty() => {
            Some(g.stops.iter().map(|s| rctx.color(&s.color, ph).a as f64 / 255.0).sum::<f64>() / g.stops.len() as f64)
        }
        Some(_) => Some(1.0),
    }
}

/// A shadow's own scale and skew (`sx`, `sy`, `kx`, `ky`) in the shape's space, about the point
/// of the shape's box its alignment names (`algn`: `b` is the middle of the bottom edge, `tl` the
/// top-left corner…), as PowerPoint applies them: skewed, then scaled. A perspective shadow
/// (`algn="b"`, `sy` 23%, `kx` -20°) so lies flat along the shape's bottom edge, not around its
/// middle.
fn shadow_local(sh: &deckcraft_model::style::Shadow, w: f64, h: f64) -> Affine {
    let (ax, ay) = match sh.align.as_str() {
        "tl" => (0.0, 0.0),
        "t" => (0.5, 0.0),
        "tr" => (1.0, 0.0),
        "l" => (0.0, 0.5),
        "r" => (1.0, 0.5),
        "bl" => (0.0, 1.0),
        "b" => (0.5, 1.0),
        "br" => (1.0, 1.0),
        _ => (0.5, 0.5),
    };
    let a = Vec2::new(w * ax, h * ay);
    let (tx, ty) = (sh.kx.to_radians().tan(), sh.ky.to_radians().tan());
    let k = Affine::new([sh.sx, sh.sy * ty, sh.sx * tx, sh.sy, 0.0, 0.0]);
    if !k.is_finite() {
        return Affine::IDENTITY;
    }
    Affine::translate(a) * k * Affine::translate(-a)
}

fn pre_translate(m: Affine, d: Vec2) -> Affine {
    Affine::translate(d) * m
}

pub(crate) fn scale_of(m: Affine) -> f64 {
    m.determinant().abs().sqrt()
}

fn blur(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur { std_deviation: sigma.clamp(0.0, 500.0) as f32, edge_mode: EdgeMode::None })
}

/// Stroke `path` with `line`. A gradient line is laid out over `extent` (the untrimmed outline the
/// line belongs to; `path` itself when `None`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn stroke(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    line: &Line,
    ph: Option<Rgba>,
    path: &BezPath,
    extent: Option<&BezPath>,
    m: Affine,
    lw: f64,
    alpha: f64,
) {
    use deckcraft_model::style::{LineCap, LineJoin};
    let cap = match line.cap.unwrap_or_default() {
        LineCap::Flat => kurbo::Cap::Butt,
        LineCap::Round => kurbo::Cap::Round,
        LineCap::Square => kurbo::Cap::Square,
    };
    let join = match line.join.unwrap_or_default() {
        LineJoin::Round => kurbo::Join::Round,
        LineJoin::Bevel => kurbo::Join::Bevel,
        LineJoin::Miter => kurbo::Join::Miter,
    };
    // Hairlines stay visible at any zoom.
    let min_w = 0.5 / scale_of(m).max(1e-6);
    let sw = lw.max(min_w);
    // PowerPoint's default miter limit is 8 widths (`a:miter lim="800000"`).
    let mut sk = kurbo::Stroke::new(sw).with_caps(cap).with_join(join).with_miter_limit(8.0);
    // Square caps on a dashed line square off only the line's own two ends (where a dash is
    // drawn): each dash itself ends flat, as PowerPoint draws them.
    let mut square_ends = None;
    if let Some(d) = &line.dash {
        let pat: Vec<f64> = d.pattern().iter().map(|v| v * lw.max(0.75)).collect();
        let pat = dash_with_caps(pat, cap, sw);
        let pat = limit_dash_count(path, pat);
        if !pat.is_empty() && pat.iter().all(|v| *v > 0.0) {
            if cap == kurbo::Cap::Square {
                sk = sk.with_caps(kurbo::Cap::Butt);
                square_ends = Some(square_dash_ends(path, &pat, sw));
            }
            sk = sk.with_dashes(0.0, pat);
        }
    }
    ctx.set_transform(m);
    match &line.fill {
        Some(Fill::Solid { color: c }) => {
            ctx.set_paint(color(rctx.color(c, ph), alpha));
        }
        Some(Fill::Gradient(g)) if !g.stops.is_empty() => ctx.set_paint(line_gradient(rctx, g, ph, extent.unwrap_or(path), lw, alpha)),
        _ => ctx.set_paint(color(Rgba::BLACK, alpha)),
    }
    // Compound lines: each band stroked along the path moved to its place across the width.
    let compound = line.compound.unwrap_or_default();
    if compound != deckcraft_model::style::Compound::Single && lw * scale_of(m) >= 1.5 {
        let tolerance = 0.1 / scale_of(m).max(1e-6);
        for &(a, b) in compound::bands(compound) {
            let d = (-0.5 + (a + b) / 2.0) * lw;
            let mut band = sk.clone();
            band.width = (b - a) * lw;
            ctx.set_stroke(band);
            ctx.stroke_path(&compound::offset(path, d, tolerance));
        }
        return;
    }
    ctx.set_stroke(sk);
    ctx.stroke_path(path);
    if let Some(ends) = square_ends {
        ctx.fill_path(&ends);
    }
}

/// The square caps of a dashed line drawn `width` wide with dash `pattern`: a half-width square
/// past the start of each open sub-path (every dash pattern starts with a dash) and past its end
/// when the pattern is in a dash there.
fn square_dash_ends(path: &BezPath, pattern: &[f64], width: f64) -> BezPath {
    use kurbo::{ParamCurve, ParamCurveArclen};
    let mut out = BezPath::new();
    let period: f64 = pattern.iter().sum();
    let mut cap = |at: Point, dir: Vec2| {
        let n = Vec2::new(-dir.y, dir.x) * (width / 2.0);
        let back = dir * (width / 2.0);
        out.move_to(at + n);
        out.line_to(at + n + back);
        out.line_to(at - n + back);
        out.line_to(at - n);
        out.close_path();
    };
    let dir = |seg: &kurbo::PathSeg, t0: f64, t1: f64| {
        let d = seg.eval(t1) - seg.eval(t0);
        let d = if d.hypot() > 1e-9 { d } else { seg.end() - seg.start() };
        if d.hypot() > 1e-9 { Some(d.normalize()) } else { None }
    };
    let mut subs: Vec<(Vec<kurbo::PathSeg>, bool)> = vec![];
    let mut cur: Vec<kurbo::PathSeg> = vec![];
    let mut closed = false;
    let mut last = Point::ZERO;
    let mut start = Point::ZERO;
    for el in path.elements() {
        let seg = match *el {
            PathEl::MoveTo(p) => {
                if !cur.is_empty() {
                    subs.push((std::mem::take(&mut cur), closed));
                }
                closed = false;
                last = p;
                start = p;
                continue;
            }
            PathEl::LineTo(p) => kurbo::PathSeg::Line(kurbo::Line::new(last, p)),
            PathEl::QuadTo(a, p) => kurbo::PathSeg::Quad(kurbo::QuadBez::new(last, a, p)),
            PathEl::CurveTo(a, b, p) => kurbo::PathSeg::Cubic(kurbo::CubicBez::new(last, a, b, p)),
            PathEl::ClosePath => {
                closed = true;
                last = start;
                continue;
            }
        };
        last = seg.end();
        cur.push(seg);
    }
    if !cur.is_empty() {
        subs.push((cur, closed));
    }
    for segs in subs.iter().filter(|(s, c)| !c && !s.is_empty()).map(|(s, _)| s) {
        let (Some(first), Some(end)) = (segs.first(), segs.last()) else { continue };
        if let Some(d) = dir(first, 1e-3, 0.0) {
            cap(first.start(), d);
        }
        let len: f64 = segs.iter().map(|s| s.arclen(1e-3)).sum();
        if !(period > 0.0 && len.is_finite()) {
            continue;
        }
        let mut pos = len % period;
        // Ending exactly on a period boundary is the end of a gap.
        let mut on = pos > period * 1e-6;
        for v in pattern.iter().cycle().take(pattern.len()) {
            if pos <= *v {
                break;
            }
            pos -= v;
            on = !on;
        }
        if on && let Some(d) = dir(end, 1.0 - 1e-3, 1.0) {
            cap(end.end(), d);
        }
    }
    out
}

/// A gradient line's paint: laid out, as PowerPoint lays it, over the box the stroke covers (the
/// path's bounds grown by half the line width), so a horizontal line's vertical gradient runs
/// across its width.
fn line_gradient(rctx: &Ctx, g: &deckcraft_model::style::Gradient, ph: Option<Rgba>, path: &BezPath, lw: f64, alpha: f64) -> peniko::Gradient {
    let bounds = path.bounding_box().inflate(lw / 2.0, lw / 2.0);
    let bounds = paint::tile_box(bounds, g.tile);
    let stops: Vec<(f64, Rgba)> = g.stops.iter().map(|s| (s.pos, rctx.color(&s.color, ph))).collect();
    paint::gradient(&g.shape, stops, bounds, alpha)
}

/// A dash pattern (on, off, on, off… lengths) for a line drawn `width` wide with `cap`. As
/// PowerPoint draws them, round caps sit inside each dash's length rather than adding to it: a
/// "round dot" (`sysDot`, one width on and one off) is a row of separate circles, not a solid
/// line. So each dash loses a width (half at each end, where the caps go) and each gap gains it; a
/// dash no longer than the width becomes just its two caps. (Square-capped dashes end flat; see
/// `square_dash_ends`.)
fn dash_with_caps(mut pat: Vec<f64>, cap: kurbo::Cap, width: f64) -> Vec<f64> {
    if cap != kurbo::Cap::Round || !pat.len().is_multiple_of(2) || !(width.is_finite() && width > 0.0) {
        return pat;
    }
    for (i, v) in pat.iter_mut().enumerate() {
        *v = if i.is_multiple_of(2) { (*v - width).max(width * 1e-3) } else { *v + width };
    }
    pat
}

/// The most dashes one stroke is cut into. Stroking expands every dash into its own outline, so a
/// tiny dash on a huge shape (a 1 EMU-wide `custDash` on a 2147483647 EMU rectangle) would
/// otherwise need gigabytes and abort the app when the slide is drawn. Past this count the
/// pattern is scaled up evenly, which looks the same: the dashes are far below a pixel by then.
const MAX_DASHES: f64 = 20_000.0;

fn limit_dash_count(path: &BezPath, pat: Vec<f64>) -> Vec<f64> {
    use kurbo::ParamCurveArclen;
    let period: f64 = pat.iter().sum();
    if !(period.is_finite() && period > 0.0) {
        return pat;
    }
    let len: f64 = path.segments().map(|s| s.arclen(1e-2)).sum();
    let dashes = len / period;
    if !len.is_finite() {
        return vec![];
    }
    if dashes <= MAX_DASHES {
        return pat;
    }
    let k = dashes / MAX_DASHES;
    pat.into_iter().map(|v| v * k).collect()
}

fn end_points(path: &BezPath) -> Option<((Point, Point), (Point, Point))> {
    let els: Vec<PathEl> = path.elements().to_vec();
    let mut pts: Vec<Point> = vec![];
    for e in &els {
        match e {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push(*p),
            PathEl::QuadTo(a, b) => {
                pts.push(*a);
                pts.push(*b);
            }
            PathEl::CurveTo(a, b, c) => {
                pts.push(*a);
                pts.push(*b);
                pts.push(*c);
            }
            PathEl::ClosePath => {}
        }
    }
    if pts.len() < 2 {
        return None;
    }
    let first = *pts.first()?;
    let second = pts.iter().skip(1).find(|p| (**p - first).hypot() > 1e-6).copied().unwrap_or(*pts.get(1)?);
    let last = *pts.last()?;
    let prev = pts.iter().rev().skip(1).find(|p| (**p - last).hypot() > 1e-6).copied().unwrap_or(*pts.get(pts.len() - 2)?);
    Some(((first, second), (last, prev)))
}

/// An arrowhead's width and length for a line `lw` wide. PowerPoint sizes them in multiples of
/// the line width (small 2, medium 3, large 5), but never smaller than for a 2 pt line.
fn head_size(e: &deckcraft_model::style::LineEnd, lw: f64) -> (f64, f64) {
    let sz = |s: &str| match s {
        "sm" => 2.0,
        "lg" => 5.0,
        _ => 3.0,
    };
    let k = lw.max(2.0);
    (sz(&e.w) * k, sz(&e.len) * k)
}

/// How far back from the tip an open arrowhead's legs (`lw` wide) meet along their centre lines:
/// PowerPoint puts the outside of the point, not the legs' centre lines, on the line's end.
fn open_arrow_setback(wl: f64, ll: f64, lw: f64) -> f64 {
    let half = (wl / 2.0).atan2(ll);
    if half.sin() > 1e-6 { (lw / 2.0 / half.sin()).min(ll) } else { 0.0 }
}

/// How much of a `lw`-wide line to leave off at its start and end, under its arrowheads: up to
/// a triangle's base, into a stealth arrow's notch far enough for its barbs to cover the line's
/// corners, and to where an open arrow's legs meet. Ovals and diamonds sit on the end itself.
fn head_trims(line: &Line, lw: f64) -> (f64, f64) {
    // A square cap reaches half a width past the line's end: leave that off too.
    let cap = if line.cap == Some(deckcraft_model::style::LineCap::Square) { lw / 2.0 } else { 0.0 };
    let trim = |e: &Option<deckcraft_model::style::LineEnd>| {
        let Some(e) = e else { return 0.0 };
        let (wl, ll) = head_size(e, lw);
        match e.kind.as_str() {
            "triangle" => ll + cap,
            "stealth" => (ll * (0.6 + 0.4 * lw / wl.max(1e-9))).min(ll) + cap,
            "arrow" => open_arrow_setback(wl, ll, lw.max(0.75)) + cap,
            _ => 0.0,
        }
    };
    (trim(&line.head), trim(&line.tail))
}

/// `path` with `start` left off the beginning of its first sub-path and `end` off the end of its
/// last (by arc length), where those are open. A sub-path shorter than its trims disappears.
fn trim_ends(path: &BezPath, start: f64, end: f64) -> BezPath {
    use kurbo::{ParamCurve, ParamCurveArclen};
    const ACC: f64 = 1e-3;
    let mut subs: Vec<(Vec<kurbo::PathSeg>, bool)> = vec![];
    let (mut last, mut first) = (Point::ZERO, Point::ZERO);
    for el in path.elements() {
        let seg = match *el {
            PathEl::MoveTo(p) => {
                subs.push((vec![], false));
                last = p;
                first = p;
                continue;
            }
            PathEl::ClosePath => {
                if let Some(s) = subs.last_mut() {
                    s.1 = true;
                }
                last = first;
                continue;
            }
            PathEl::LineTo(p) => kurbo::PathSeg::Line(kurbo::Line::new(last, p)),
            PathEl::QuadTo(a, p) => kurbo::PathSeg::Quad(kurbo::QuadBez::new(last, a, p)),
            PathEl::CurveTo(a, b, p) => kurbo::PathSeg::Cubic(kurbo::CubicBez::new(last, a, b, p)),
        };
        last = seg.end();
        if subs.is_empty() {
            subs.push((vec![], false));
        }
        if let Some(s) = subs.last_mut() {
            s.0.push(seg);
        }
    }
    if let Some((segs, false)) = subs.first_mut()
        && start > 0.0
    {
        let mut left = start;
        while let Some(seg) = segs.first().copied() {
            let l = seg.arclen(ACC);
            if l > left {
                if let Some(s0) = segs.first_mut() {
                    *s0 = seg.subsegment(seg.inv_arclen(left, ACC)..1.0);
                }
                break;
            }
            left -= l;
            segs.remove(0);
        }
    }
    if let Some((segs, false)) = subs.last_mut()
        && end > 0.0
    {
        let mut left = end;
        while let Some(seg) = segs.last().copied() {
            let l = seg.arclen(ACC);
            if l > left {
                if let Some(s1) = segs.last_mut() {
                    *s1 = seg.subsegment(0.0..seg.inv_arclen(l - left, ACC));
                }
                break;
            }
            left -= l;
            segs.pop();
        }
    }
    let mut out = BezPath::new();
    for (segs, closed) in subs {
        let Some(s0) = segs.first() else { continue };
        out.move_to(s0.start());
        for seg in &segs {
            match seg {
                kurbo::PathSeg::Line(l) => out.line_to(l.p1),
                kurbo::PathSeg::Quad(q) => out.quad_to(q.p1, q.p2),
                kurbo::PathSeg::Cubic(c) => out.curve_to(c.p1, c.p2, c.p3),
            }
        }
        if closed {
            out.close_path();
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arrowheads(ctx: &mut RenderContext, rctx: &Ctx, line: &Line, ph: Option<Rgba>, geo: &Geometry, rel: Affine, m: Affine, lw: f64) {
    let Some(sp) = geo.paths.iter().find(|p| p.stroke) else { return };
    let Some(((a, a2), (b, b2))) = end_points(&sp.path) else { return };
    // The outline's ends in the space the arrowheads are sized and drawn in.
    let (a, a2, b, b2) = (rel * a, rel * a2, rel * b, rel * b2);
    let col = match &line.fill {
        Some(Fill::Solid { color: c }) => rctx.color(c, ph),
        _ => Rgba::BLACK,
    };
    // A gradient line's heads take the gradient where they are.
    let grad = match &line.fill {
        Some(Fill::Gradient(g)) if !g.stops.is_empty() => {
            let mut moved = sp.path.clone();
            moved.apply_affine(rel);
            Some(line_gradient(rctx, g, ph, &moved, lw, 1.0))
        }
        _ => None,
    };
    let set_paint = |ctx: &mut RenderContext| match &grad {
        Some(g) => ctx.set_paint(g.clone()),
        None => ctx.set_paint(color(col, 1.0)),
    };
    for (end, tip, from) in [(&line.head, a, a2), (&line.tail, b, b2)] {
        let Some(e) = end else { continue };
        if e.kind == "none" {
            continue;
        }
        let (wl, ll) = head_size(e, lw);
        let dir = (tip - from).normalize();
        if !dir.x.is_finite() {
            continue;
        }
        let n = Vec2::new(-dir.y, dir.x);
        let base = tip - dir * ll;
        let mut p = BezPath::new();
        match e.kind.as_str() {
            "oval" => {
                let c = kurbo::Ellipse::new(tip, (ll / 2.0, wl / 2.0), dir.y.atan2(dir.x));
                p = c.to_path(0.05);
            }
            "diamond" => {
                p.move_to(tip + dir * ll / 2.0);
                p.line_to(tip + n * wl / 2.0);
                p.line_to(tip - dir * ll / 2.0);
                p.line_to(tip - n * wl / 2.0);
                p.close_path();
            }
            "stealth" => {
                p.move_to(tip);
                p.line_to(base + n * wl / 2.0);
                p.line_to(tip - dir * ll * 0.6);
                p.line_to(base - n * wl / 2.0);
                p.close_path();
            }
            "arrow" => {
                // Legs the line's width with round ends, meeting in a sharp point whose outside is
                // on the line's end.
                let leg = lw.max(0.75);
                let back = dir * open_arrow_setback(wl, ll, leg);
                let mut o = BezPath::new();
                o.move_to(base + n * wl / 2.0 - back);
                o.line_to(tip - back);
                o.line_to(base - n * wl / 2.0 - back);
                ctx.set_transform(m);
                set_paint(ctx);
                ctx.set_stroke(kurbo::Stroke::new(leg).with_join(kurbo::Join::Miter).with_miter_limit(100.0).with_caps(kurbo::Cap::Round));
                ctx.stroke_path(&o);
                continue;
            }
            _ => {
                p.move_to(tip);
                p.line_to(base + n * wl / 2.0);
                p.line_to(base - n * wl / 2.0);
                p.close_path();
            }
        }
        ctx.set_transform(m);
        set_paint(ctx);
        ctx.fill_path(&p);
    }
}

/// The prompt shown in an empty placeholder, formatted like its content would be.
fn prompt_body(rctx: &Ctx, s: &Shape, kind: PhType) -> TextBody {
    // Layout/master prompt text when the layout supplies a custom one.
    let (lp, _) = resolve::parents(rctx, s);
    let custom = lp.and_then(|l| l.ph.as_ref().filter(|p| p.has_custom_prompt).and(l.text.as_ref())).filter(|t| !t.is_empty());
    if let Some(t) = custom {
        return t.clone();
    }
    if matches!(rctx.owner, resolve::Owner::Layout | resolve::Owner::Master) {
        return TextBody::default();
    }
    let txt = kind.prompt();
    if txt.is_empty() || kind.is_footer_kind() {
        return TextBody::default();
    }
    let mut t = TextBody::from_text(txt);
    if matches!(kind, PhType::Picture | PhType::Chart | PhType::Table | PhType::Media | PhType::Diagram | PhType::ClipArt) {
        t.body.anchor = Some(deckcraft_model::text::Anchor::Bottom);
        if let Some(p) = t.paragraphs.first_mut() {
            p.props.align = Some(deckcraft_model::text::Align::Center);
            p.props.bullet = Some(deckcraft_model::text::Bullet::None);
            p.props.margin_left = Some(0.0);
            p.props.indent = Some(0.0);
            for r in &mut p.runs {
                r.props.size = Some(14.0);
            }
        }
    }
    t
}

thread_local! {
    static GLYPH_CACHE: std::cell::RefCell<HashMap<(u32, u32), Arc<BezPath>>> = std::cell::RefCell::new(HashMap::new());
}

pub(crate) fn glyph(face: &deckcraft_fonts::FontFace, gid: u32) -> Arc<BezPath> {
    let key = (face.id(), gid);
    if let Some(p) = GLYPH_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return p;
    }
    let p = FontDb::global().outline(face, gid);
    GLYPH_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 200_000 {
            c.clear();
        }
        c.insert(key, p.clone());
    });
    p
}

/// Draw a text body into shape space `m` within the text rect.
pub fn draw_text(ctx: &mut RenderContext, rctx: &Ctx, s: &Shape, body: &TextBody, tr: Rect, m: Affine, fields: &dyn Fields, prompt: Option<Rgba>) {
    let l = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields, prompt_color: prompt, no_shrink: false });
    draw_layout(ctx, &l, tr, m);
}

/// Add transient list markers to an editor layout, using the document's own bullet glyphs.
fn preview_empty_bullets(l: &mut deckcraft_text::TextLayout) {
    for mut run in l.empty_bullets.drain(..) {
        run.color = Rgba::rgb(0xB3, 0xB3, 0xB3);
        run.alpha = 1.0;
        l.runs.push(run);
    }
}

/// Where text laid out in text rect `tr` is drawn, for a shape drawn with `m` (shape-local →
/// output): `m`, without mirroring, then the layout's `rotation` about the rect's centre.
///
/// Flips mirror a shape's geometry, never its text. When `m` reflects (flipH or flipV, the shape's
/// own or a group's) the text rect keeps its mirrored place and the text is mirrored back about
/// the rect's centre: a flipH shape's text stays readable and a flipV shape's text is turned 180°
/// (both flips: 180°), as in PowerPoint. Rendering, hit testing and the caret all use this.
pub fn text_transform(m: Affine, tr: Rect, rotation: f64) -> Affine {
    let c = tr.center().to_vec2();
    let mut t = m;
    if m.determinant() < 0.0 && c.is_finite() {
        t = t * Affine::translate(c) * Affine::scale_non_uniform(-1.0, 1.0) * Affine::translate(-c);
    }
    if rotation != 0.0 && rotation.is_finite() && c.is_finite() {
        t = t * Affine::translate(c) * Affine::rotate(rotation.to_radians()) * Affine::translate(-c);
    }
    t
}

/// The WordArt warp of `s`'s text over its text rectangle `tr`, if it has one DeckCraft draws.
pub fn text_warp(rctx: &Ctx, s: &Shape, tr: Rect) -> Option<deckcraft_geom::warp::Warp> {
    let bp = resolve::body(rctx, s);
    let name = bp.warp.as_deref()?;
    deckcraft_geom::warp::warp(name, &bp.warp_adj, tr)
}

/// Draw an already laid-out text block.
pub fn draw_layout(ctx: &mut RenderContext, l: &deckcraft_text::TextLayout, tr: Rect, m: Affine) {
    draw_layout_with(ctx, l, tr, m, &[]);
}

/// Draw a laid-out text block with per-paragraph animation states.
pub fn draw_layout_with(ctx: &mut RenderContext, l: &deckcraft_text::TextLayout, tr: Rect, m: Affine, paras: &[(usize, ParaState)]) {
    let para = |i: usize| paras.iter().find(|(p, _)| *p == i).map(|(_, s)| *s).unwrap_or_default();
    let m = text_transform(m, tr, l.rotation);
    let deco = |ctx: &mut RenderContext, d: &deckcraft_text::Deco| {
        let ps = para(d.para);
        if !ps.visible || ps.opacity <= 0.001 {
            return;
        }
        ctx.set_transform(m * Affine::translate(ps.offset));
        ctx.set_paint(color(d.color, ps.opacity));
        fill_deco(ctx, d);
    };
    for d in l.decos.iter().filter(|d| d.behind) {
        deco(ctx, d);
    }
    // Text shadows, then glows, under all the text.
    for pass in [TextEffect::Shadow, TextEffect::Glow] {
        for run in &l.runs {
            let ps = para(run.para);
            if ps.visible && ps.opacity > 0.001 {
                text_effect(ctx, run, pass, m * Affine::translate(ps.offset), ps.opacity);
            }
        }
    }
    for run in &l.runs {
        if (run.alpha <= 0.0 || run.color.a == 0) && run.outline.is_none() {
            continue;
        }
        let ps = para(run.para);
        if !ps.visible || ps.opacity <= 0.001 {
            continue;
        }
        let m = m * Affine::translate(ps.offset);
        // A gradient fill starts over on every line and spans that line's text (from its top to
        // its bottom, from its first to its last character), as in PowerPoint.
        let mut grads: Vec<(usize, peniko::Gradient)> = Vec::new();
        for (gid, x, y) in &run.glyphs {
            let at = m * glyph_place(run, *gid, *x, *y);
            let Some(g) = run.gradient.as_ref().filter(|g| !g.stops.is_empty()) else {
                draw_glyph(ctx, run, *gid, at, ps.opacity);
                continue;
            };
            let li = line_at(l, run.para, *y);
            if !grads.iter().any(|(i, _)| *i == li) {
                let scheme = deckcraft_color::ColorScheme { name: String::new(), colors: [Rgba::BLACK; 12] };
                let stops = g.stops.iter().map(|s| (s.pos, s.color.resolve(&scheme, None))).collect();
                let bounds = l.lines.get(li).and_then(line_bounds).unwrap_or(tr);
                grads.push((li, paint::gradient(&g.shape, stops, bounds, run.alpha * ps.opacity)));
            }
            let fill = grads.iter().find(|(i, _)| *i == li).map(|(_, g)| (g, Affine::translate((-*x, -*y))));
            draw_glyph_with(ctx, run, *gid, at, ps.opacity, fill);
        }
    }
    for d in l.decos.iter().filter(|d| !d.behind) {
        deco(ctx, d);
    }
}

/// Where glyph `gid` of `run` with its pen at (`x`, `y`) is drawn, in text space: there, turned a
/// quarter turn back about its em box's centre when it stands upright in vertical text.
pub(crate) fn glyph_place(run: &deckcraft_text::GlyphRun, gid: u32, x: f64, y: f64) -> Affine {
    let at = Affine::translate((x, y));
    match run.upright_centre(gid, x, y) {
        Some((cx, cy)) => Affine::translate((cx, cy)) * Affine::rotate(-std::f64::consts::FRAC_PI_2) * Affine::translate((-cx, -cy)) * at,
        None => at,
    }
}

/// Fill an underline, strikethrough or highlight with the current paint and transform: its box,
/// its dashes, or its zigzag stroked.
pub(crate) fn fill_deco(ctx: &mut RenderContext, d: &deckcraft_text::Deco) {
    if let Some((pts, w)) = d.wave() {
        let mut path = BezPath::new();
        for (i, p) in pts.iter().enumerate() {
            if i == 0 {
                path.move_to(*p);
            } else {
                path.line_to(*p);
            }
        }
        ctx.set_stroke(kurbo::Stroke::new(w.max(0.01)));
        ctx.stroke_path(&path);
        return;
    }
    for r in d.pieces() {
        ctx.fill_rect(&r);
    }
}

/// The line of paragraph `para` holding a glyph on baseline `y` (the nearest baseline).
fn line_at(l: &deckcraft_text::TextLayout, para: usize, y: f64) -> usize {
    l.lines
        .iter()
        .enumerate()
        .filter(|(_, li)| li.para == para)
        .min_by(|a, b| (a.1.baseline - y).abs().total_cmp(&(b.1.baseline - y).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// The box a line's text covers: its top and bottom, its first and last caret.
fn line_bounds(li: &deckcraft_text::LineInfo) -> Option<Rect> {
    let (x0, x1) = li.caret_x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
    let r = Rect::new(x0, li.top, x1, li.bottom);
    (r.is_finite() && r.width() > 0.0 && r.height() > 0.0).then_some(r)
}

#[derive(Clone, Copy, PartialEq)]
enum TextEffect {
    Shadow,
    Glow,
}

/// Draw `run`'s shadow or glow (if it has one) under its glyphs, placed by `m`.
fn text_effect(ctx: &mut RenderContext, run: &deckcraft_text::GlyphRun, which: TextEffect, m: Affine, opacity: f64) {
    let Some(fx) = run.effects else { return };
    let k = run.size / run.face.upem.max(1.0);
    let skew = if run.fake_italic { Affine::new([1.0, 0.0, -0.2, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
    let px = scale_of(m);
    let (c, sigma, offset, grow) = match (which, fx.shadow, fx.glow) {
        (TextEffect::Shadow, Some(s), _) => (s.color, s.blur * 0.5 * px, Vec2::new(s.dx, s.dy), 0.0),
        // Measured on PowerPoint's export: a text glow of radius r is solid to about a third of r
        // past the glyph's edge and fades out by about two thirds of r.
        (TextEffect::Glow, _, Some((gc, r))) => (gc, r * 0.15 * px, Vec2::ZERO, r * 0.68),
        _ => return,
    };
    let alpha = (c.a as f64 / 255.0) * opacity;
    if alpha <= 0.0 {
        return;
    }
    ctx.set_transform(Affine::IDENTITY);
    ctx.push_layer(None, None, Some(alpha as f32), None, Some(blur(sigma)));
    ctx.set_paint(color(c.with_alpha(255), 1.0));
    for (gid, x, y) in &run.glyphs {
        let path = glyph(&run.face, *gid);
        if path.elements().is_empty() {
            continue;
        }
        // The offset is in slide space: a shadow falls the same way whatever the text's rotation.
        let at = pre_translate(m * glyph_place(run, *gid, *x, *y), offset * px) * skew * Affine::scale(k);
        ctx.set_transform(at);
        ctx.fill_path(&path);
        if grow > 0.0 {
            ctx.set_stroke(kurbo::Stroke::new(grow / k.max(1e-9)).with_join(kurbo::Join::Round));
            ctx.stroke_path(&path);
        }
    }
    ctx.pop_layer();
}

/// Draw glyph `gid` of `run` with its origin (on the baseline) mapped by `at`, faded by `opacity`.
pub(crate) fn draw_glyph(ctx: &mut RenderContext, run: &deckcraft_text::GlyphRun, gid: u32, at: Affine, opacity: f64) {
    draw_glyph_with(ctx, run, gid, at, opacity, None);
}

/// [`draw_glyph`], filled with `fill` (a gradient laid out in the text's space, and the map from
/// the glyph's origin to that space) instead of the run's colour.
fn draw_glyph_with(
    ctx: &mut RenderContext,
    run: &deckcraft_text::GlyphRun,
    gid: u32,
    at: Affine,
    opacity: f64,
    fill: Option<(&peniko::Gradient, Affine)>,
) {
    let alpha = run.alpha * opacity;
    if (alpha <= 0.0 || run.color.a == 0) && run.outline.is_none() {
        return;
    }
    let k = run.size / run.face.upem.max(1.0);
    // Colour glyphs (emoji) paint their own colours; an outline (text outline effect) still
    // strokes the plain glyph below.
    if alpha > 0.0 && run.outline.is_none() && run.face.is_color() && colr::paint(ctx, &run.face, gid, at, k, run.color, alpha) {
        return;
    }
    let path = glyph(&run.face, gid);
    if path.elements().is_empty() {
        return;
    }
    let skew = if run.fake_italic { Affine::new([1.0, 0.0, -0.2, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
    ctx.set_transform(at * skew * Affine::scale(k));
    if let Some((g, to_text)) = fill
        && alpha > 0.0
    {
        ctx.set_paint(g.clone());
        ctx.set_paint_transform((skew * Affine::scale(k)).inverse() * to_text);
        ctx.fill_path(&path);
        ctx.reset_paint_transform();
    } else if alpha > 0.0 && run.color.a > 0 {
        ctx.set_paint(color(run.color, alpha));
        ctx.fill_path(&path);
        if run.fake_bold {
            ctx.set_stroke(kurbo::Stroke::new(run.face.upem * 0.025).with_join(kurbo::Join::Round));
            ctx.stroke_path(&path);
        }
    }
    if let Some((oc, ow)) = run.outline {
        ctx.set_paint(color(oc, opacity));
        ctx.set_stroke(kurbo::Stroke::new(ow / k.max(1e-9)).with_join(kurbo::Join::Round));
        ctx.stroke_path(&path);
    }
}

/// Render `ids` together onto a transparent canvas, in their slide positions (copy as a picture).
pub fn render_shapes(pres: &Presentation, slide: &Slide, ids: &[ShapeId], scale: f64) -> Option<Image> {
    if ids.is_empty() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let rctx = Ctx::for_slide(pres, slide)?;
    let mut items = Vec::new();
    let mut bounds: Option<Rect> = None;
    for id in ids.iter().take(256) {
        let Some(s) = slide.shape(*id) else { continue };
        let b = resolve::xfrm(&rctx, s).bounds();
        if !b.width().is_finite() || !b.height().is_finite() || b.width() < 0.0 || b.height() < 0.0 {
            continue;
        }
        bounds = Some(match bounds {
            Some(u) => u.union(b),
            None => b,
        });
        items.push(s);
    }
    let b = bounds?.inflate(8.0, 8.0);
    if !(b.width() > 0.0 && b.height() > 0.0 && b.width().is_finite() && b.height().is_finite()) {
        return None;
    }
    let fit = (2048.0 / b.width().max(b.height())).min(scale);
    if !(fit.is_finite() && fit > 0.0) {
        return None;
    }
    let w = (b.width() * fit).ceil().clamp(1.0, 2048.0) as u16;
    let h = (b.height() * fit).ceil().clamp(1.0, 2048.0) as u16;
    let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    let opts = RenderOpts { scale: fit, ..Default::default() };
    let f = Frame { pres, opts: &opts, fields: SlideFields::new(1, String::new(), &opts) };
    let view = Affine::scale(fit) * Affine::translate(-b.origin().to_vec2());
    RENDERER.with(|r| {
        let mut r = r.borrow_mut();
        for s in items {
            r.shape(&mut ctx, &f, &rctx, s, view, 0);
        }
        Some(r.finish(ctx, w, h))
    })
}

/// Render one shape alone onto a transparent canvas (drag previews, copy as picture).
pub fn render_shape(pres: &Presentation, slide: &Slide, id: ShapeId, scale: f64) -> Option<Image> {
    let rctx = Ctx::for_slide(pres, slide)?;
    let s = slide.shape(id)?;
    let x = resolve::xfrm(&rctx, s);
    let b = x.bounds().inflate(8.0, 8.0);
    let w = (b.width() * scale).ceil().clamp(1.0, MAX_SIDE as f64) as u16;
    let h = (b.height() * scale).ceil().clamp(1.0, MAX_SIDE as f64) as u16;
    let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    let opts = RenderOpts { scale, ..Default::default() };
    let f = Frame { pres, opts: &opts, fields: SlideFields::new(1, String::new(), &opts) };
    let view = Affine::scale(scale) * Affine::translate(-b.origin().to_vec2());
    RENDERER.with(|r| {
        let mut r = r.borrow_mut();
        r.shape(&mut ctx, &f, &rctx, s, view, 0);
        Some(r.finish(ctx, w, h))
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_wild;

#[cfg(test)]
mod mt_tests {
    use super::*;

    /// Regression: multithreaded rendering of a slide with shadows panicked inside vello_cpu.
    #[test]
    fn effects_render_with_threads_requested() {
        let mut s = deckcraft_model::Presentation::default();
        let sl = std::sync::Arc::make_mut(&mut s.slides[0]);
        let mut sh = deckcraft_model::Shape {
            id: ShapeId(900),
            xfrm: Some(deckcraft_geom::Xfrm::new(10.0, 10.0, 100.0, 100.0)),
            style: Some(deckcraft_model::ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1)),
            ..Default::default()
        };
        sh.effects = Some(deckcraft_model::Effects { soft_edge: Some(5.0), ..Default::default() });
        sl.shapes.push(sh);
        assert!(needs_filters(&s, &s.slides[0]));
        let img = RENDERER.with(|r| r.borrow_mut().slide(&s, &s.slides[0], 0, &RenderOpts { scale: 0.5, threads: 4, ..Default::default() }));
        assert_eq!(img.width, 480);
        // The shape itself is drawn (not a blank fallback).
        assert_ne!(img.pixel(30, 30), [255, 255, 255, 255]);
    }
}
