//! The DeckCraft renderer: draws slides, layouts and masters with `vello_cpu`.
//!
//! One code path serves the editor canvas, thumbnails, image/PDF-raster export and the slide show:
//! [`render_slide`] draws the background, the master and layout graphics, then the slide's shapes
//! (fills with theme shading, gradients, pictures, outlines with dashes and arrowheads, shadows,
//! glow and soft edges, text, tables, charts, groups), optionally with per-shape animation state.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod chart;
mod images;
mod paint;
mod placed;
mod table;

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_fonts::FontDb;
use deckcraft_geom::preset::{self, FillMode, Geometry, SubPath};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::{Effects, Fill, Line};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Geom, PhType, Presentation, Shape, ShapeId, ShapeKind, Slide};
use deckcraft_text::{Fields, Opts};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape as _, Vec2};
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::{RenderContext, Resources, peniko};

pub use images::decode as decode_image;
pub use placed::{Placed, PlacedLink, PlacedText, place_slide};

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
    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        let straight = self.to_straight();
        let rgb: Vec<u8> = straight.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let Some(img) = image::RgbImage::from_raw(self.width, self.height, rgb) else { return Vec::new() };
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
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
    /// Worker threads (0 = this thread; required when effects use filters).
    pub threads: u16,
    /// Translate the slide within the output (pixels) and output size override.
    pub offset: (f64, f64),
    pub size: Option<(u32, u32)>,
    /// Draw a background colour outside the slide (output larger than slide).
    pub clear: Option<Rgba>,
    pub date_text: Option<String>,
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
            threads: 0,
            offset: (0.0, 0.0),
            size: None,
            clear: None,
            date_text: None,
        }
    }
}

struct SlideFields {
    num: u32,
    date: String,
    footer: String,
}

impl Fields for SlideFields {
    fn field(&self, kind: &str) -> Option<String> {
        if kind == "slidenum" {
            return Some(self.num.to_string());
        }
        if kind.starts_with("datetime") {
            return Some(self.date.clone());
        }
        if kind == "footer" {
            return Some(self.footer.clone());
        }
        None
    }
}

pub struct Renderer {
    resources: Resources,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer { resources: Resources::new() }
    }
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
            if e.is_some_and(|e| !e.is_empty()) {
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

/// Render slide `index` at `opts.scale` pixels per point. A panic inside the renderer becomes a
/// blank image (logged), never a crash.
pub fn render_slide(pres: &Presentation, index: usize, opts: &RenderOpts) -> Image {
    let Some(slide) = pres.slides.get(index) else { return Image::default() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| RENDERER.with(|r| r.borrow_mut().slide(pres, slide, index, opts)))) {
        Ok(img) => img,
        Err(_) => {
            log::error!("rendering slide {index} panicked; showing a blank slide");
            RENDERER.with(|r| *r.borrow_mut() = Renderer::default());
            let (w, h) = output_size(pres, opts);
            Image { width: w as u32, height: h as u32, pixels: vec![255; w as usize * h as usize * 4] }
        }
    }
}

/// Render a frame between slides `old` and `new` (indices into `pres.slides`), e.g. of Morph: the
/// backdrop (background, master and layout graphics) of `old` cross-fading into that of `new` by
/// `mix` (0..1), then `shapes` in order, each resolved against the slide it comes from (`true`:
/// `old`). `opts.state` applies to `shapes` only.
pub fn render_blend(pres: &Presentation, old: usize, new: usize, mix: f64, shapes: &[(Shape, bool)], opts: &RenderOpts) -> Image {
    let (Some(a), Some(b)) = (pres.slides.get(old), pres.slides.get(new)) else { return Image::default() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        RENDERER.with(|r| r.borrow_mut().blend(pres, (a, old), (b, new), mix, shapes, opts))
    })) {
        Ok(img) => img,
        Err(_) => {
            log::error!("rendering a frame between slides {old} and {new} panicked; showing the new slide");
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
                out.push(SubPath { path: bp, fill: p.fill, stroke: p.stroke, even_odd: false });
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

    fn fields(pres: &Presentation, index: usize, opts: &RenderOpts) -> SlideFields {
        SlideFields {
            num: pres.slide_number(index),
            date: opts.date_text.clone().unwrap_or_else(|| pres.header_footer.date_text.clone()),
            footer: pres.header_footer.footer_text.clone(),
        }
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

    fn blend(
        &mut self,
        pres: &Presentation,
        old: (&Slide, usize),
        new: (&Slide, usize),
        mix: f64,
        shapes: &[(Shape, bool)],
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
        for (s, from_old) in shapes {
            let (f, c) = if *from_old { (&fo, &co) } else { (&fnew, &cn) };
            self.shape(&mut ctx, f, c, s, view, 0);
        }
        self.finish(ctx, w, h)
    }

    fn layout(&mut self, pres: &Presentation, mi: usize, li: Option<usize>, opts: &RenderOpts) -> Image {
        let (w, h) = output_size(pres, opts);
        let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
        let view = Affine::translate((opts.offset.0, opts.offset.1)) * Affine::scale(opts.scale);
        let f = Frame { pres, opts, fields: SlideFields { num: 1, date: String::new(), footer: String::new() } };
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
        let r = Rect::new(0.0, 0.0, f.pres.slide_size.width, f.pres.slide_size.height);
        ctx.set_transform(view);
        let path = r.to_path(0.1);
        paint::fill_path(ctx, rctx, &fill, ph, &path, r, view, FillMode::Norm, 1.0, f.pres);
    }

    fn shape(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, s: &Shape, parent: Affine, depth: usize) {
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
                for c in children {
                    self.shape(ctx, f, rctx, c, gm, depth + 1);
                }
            }
            ShapeKind::Table(t) => table::draw(ctx, f.pres, rctx, t, m, x.w, x.h, &f.fields),
            ShapeKind::Chart(c) => chart::draw(ctx, rctx, c, m, x.w, x.h),
            ShapeKind::Ink { strokes } => {
                for sk in strokes {
                    let mut bp = BezPath::new();
                    for (i, (px, py, _)) in sk.points.iter().enumerate() {
                        if i == 0 {
                            bp.move_to((*px, *py));
                        } else {
                            bp.line_to((*px, *py));
                        }
                    }
                    ctx.set_transform(parent);
                    ctx.set_paint(color(sk.color, if sk.highlighter { 0.4 } else { 1.0 }));
                    ctx.set_stroke(kurbo::Stroke::new(sk.width.max(0.1)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
                    ctx.stroke_path(&bp);
                }
            }
            _ => self.geometry_shape(ctx, f, rctx, s, m, x.w, x.h, &st),
        }
        if layered {
            ctx.pop_layer();
        }
    }

    fn geometry_shape(&mut self, ctx: &mut RenderContext, f: &Frame, rctx: &Ctx, s: &Shape, m: Affine, w: f64, h: f64, st: &ShapeState) {
        let geo = shape_geometry(s, w, h);
        let (fill, fill_ph) = resolve::fill(rctx, s);
        let (line, line_ph) = resolve::line(rctx, s);
        let (effects, fx_ph) = resolve::effects(rctx, s);
        let is_pic = matches!(s.kind, ShapeKind::Picture { .. });
        let outline = geo.outline();
        // Placeholders in the editor: dashed outline and prompt, nothing else when empty.
        let empty_ph = s.ph.is_some() && s.text.as_ref().is_none_or(TextBody::is_empty) && !is_pic && !matches!(s.kind, ShapeKind::Media(_));
        let fill = if let Some(t) = st.tint { Some(Fill::solid(deckcraft_model::ColorRef::rgb(t))) } else { fill };
        // Effects below the shape.
        if let Some(e) = &effects {
            self.effects_below(ctx, rctx, e, fx_ph, &geo, &fill, m, w, h);
        }
        let soft = effects.as_ref().and_then(|e| e.soft_edge).filter(|r| *r > 0.0);
        if let Some(r) = soft {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, None, None, None);
            let _ = r;
        }
        // Fill.
        match &s.kind {
            ShapeKind::Picture { fill: pf } => {
                ctx.set_transform(m);
                let bounds = Rect::new(0.0, 0.0, w, h);
                ctx.push_clip_layer(&outline);
                paint::picture(ctx, f.pres, pf, bounds, m);
                ctx.pop_layer();
            }
            ShapeKind::Media(mc) => {
                ctx.set_transform(m);
                let bounds = Rect::new(0.0, 0.0, w, h);
                if let Some(p) = mc.poster {
                    let pf = deckcraft_model::style::PictureFill { media: p, ..Default::default() };
                    paint::picture(ctx, f.pres, &pf, bounds, m);
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
                    for sp in &geo.paths {
                        if sp.fill == FillMode::None {
                            continue;
                        }
                        ctx.set_transform(m);
                        ctx.set_fill_rule(if sp.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
                        paint::fill_path(ctx, rctx, fl, fill_ph, &sp.path, Rect::new(0.0, 0.0, w, h), m, sp.fill, 1.0, f.pres);
                        ctx.set_fill_rule(peniko::Fill::NonZero);
                    }
                }
            }
        }
        // Outline.
        if line.fill.as_ref().is_some_and(|f| !f.is_none()) {
            let lw = line.width.unwrap_or(0.75).max(0.0);
            for sp in geo.paths.iter().filter(|p| p.stroke) {
                stroke(ctx, rctx, &line, line_ph, &sp.path, m, lw);
            }
            if s.is_line() || geo.is_open() {
                arrowheads(ctx, rctx, &line, line_ph, &geo, m, lw);
            }
        }
        if let Some(r) = soft {
            // Feather: fade the shape's alpha towards the outline.
            ctx.set_transform(m);
            ctx.push_layer(
                None,
                Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestIn)),
                None,
                None,
                Some(blur(r * 0.5 * scale_of(m))),
            );
            ctx.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255));
            let inset = Rect::new(0.0, 0.0, w, h).inset(-r * 0.6);
            ctx.fill_rect(&inset);
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
            ctx.push_layer(None, None, None, None, Some(blur(sh.blur * 0.5 * scale_of(m))));
            ctx.set_paint(color(c, 1.0));
            let mut ring = Rect::new(0.0, 0.0, w, h).inflate(w.max(h), w.max(h)).to_path(0.1);
            let mut hole = outline.clone();
            hole.apply_affine(Affine::translate(d));
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
            let tr = geo.text_rect;
            if !body.is_empty() {
                let l = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields: &f.fields, prompt_color: None, no_shrink: false });
                draw_layout_with(ctx, &l, tr, m, &st.paras);
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
            let c0 = Point::new(w / 2.0, h / 2.0);
            let local = Affine::translate(c0.to_vec2())
                * Affine::new([sh.sx, sh.ky.to_radians().tan(), sh.kx.to_radians().tan(), sh.sy, 0.0, 0.0])
                * Affine::translate(-c0.to_vec2());
            // Shadow offset is in slide space (doesn't rotate with the shape unless asked).
            let sm = if sh.rotate_with_shape { m * Affine::translate(d) * local } else { pre_translate(m, d * k) * local };
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, Some((c.a as f32) / 255.0), None, Some(blur(sh.blur * 0.5 * k)));
            ctx.set_transform(sm);
            ctx.set_paint(color(c.with_alpha(255), 1.0));
            ctx.fill_path(&silhouette);
            ctx.pop_layer();
        }
        if let Some(g) = &e.glow {
            let c = rctx.color(&g.color, ph);
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, Some((c.a as f32) / 255.0), None, Some(blur(g.radius * 0.35 * k)));
            ctx.set_transform(m);
            ctx.set_paint(color(c.with_alpha(255), 1.0));
            ctx.set_stroke(kurbo::Stroke::new(g.radius.max(0.0) * 1.2).with_join(kurbo::Join::Round));
            ctx.stroke_path(&silhouette);
            ctx.fill_path(&silhouette);
            ctx.pop_layer();
        }
        if let Some(rf) = &e.reflection {
            // Mirror below the shape, fading out.
            let mm = m * Affine::translate((0.0, 2.0 * h + rf.dist)) * Affine::scale_non_uniform(1.0, -1.0);
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, Some(rf.start_alpha.clamp(0.0, 1.0) as f32 * 0.6), None, Some(blur(rf.blur * 0.5 * k)));
            ctx.set_transform(mm);
            if let Some(fl) = fill {
                paint::fill_path(ctx, rctx, fl, ph, &silhouette, Rect::new(0.0, 0.0, w, h), mm, FillMode::Norm, 1.0, rctx.pres);
            }
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

fn pre_translate(m: Affine, d: Vec2) -> Affine {
    Affine::translate(d) * m
}

pub(crate) fn scale_of(m: Affine) -> f64 {
    m.determinant().abs().sqrt()
}

fn blur(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur { std_deviation: sigma.clamp(0.0, 500.0) as f32, edge_mode: EdgeMode::None })
}

fn stroke(ctx: &mut RenderContext, rctx: &Ctx, line: &Line, ph: Option<Rgba>, path: &BezPath, m: Affine, lw: f64) {
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
    let mut sk = kurbo::Stroke::new(lw.max(min_w)).with_caps(cap).with_join(join);
    if let Some(d) = &line.dash {
        let pat: Vec<f64> = d.pattern().iter().map(|v| v * lw.max(0.75)).collect();
        if !pat.is_empty() && pat.iter().all(|v| *v > 0.0) {
            sk = sk.with_dashes(0.0, pat);
        }
    }
    ctx.set_transform(m);
    match &line.fill {
        Some(Fill::Solid { color: c }) => {
            ctx.set_paint(color(rctx.color(c, ph), 1.0));
        }
        Some(Fill::Gradient(g)) => {
            let c = g.stops.first().map(|s| rctx.color(&s.color, ph)).unwrap_or(Rgba::BLACK);
            ctx.set_paint(color(c, 1.0));
        }
        _ => ctx.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255)),
    }
    ctx.set_stroke(sk);
    ctx.stroke_path(path);
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

fn arrowheads(ctx: &mut RenderContext, rctx: &Ctx, line: &Line, ph: Option<Rgba>, geo: &Geometry, m: Affine, lw: f64) {
    let Some(sp) = geo.paths.iter().find(|p| p.stroke) else { return };
    let Some(((a, a2), (b, b2))) = end_points(&sp.path) else { return };
    let col = match &line.fill {
        Some(Fill::Solid { color: c }) => rctx.color(c, ph),
        _ => Rgba::BLACK,
    };
    let lw = lw.max(0.75);
    for (end, tip, from) in [(&line.head, a, a2), (&line.tail, b, b2)] {
        let Some(e) = end else { continue };
        if e.kind == "none" {
            continue;
        }
        let sz = |s: &str| match s {
            "sm" => 2.0,
            "lg" => 5.0,
            _ => 3.0,
        };
        let (wl, ll) = (sz(&e.w) * lw, sz(&e.len) * lw);
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
                let mut o = BezPath::new();
                o.move_to(base + n * wl / 2.0);
                o.line_to(tip);
                o.line_to(base - n * wl / 2.0);
                ctx.set_transform(m);
                ctx.set_paint(color(col, 1.0));
                ctx.set_stroke(kurbo::Stroke::new(lw).with_join(kurbo::Join::Miter));
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
        ctx.set_paint(color(col, 1.0));
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

fn glyph(face: &deckcraft_fonts::FontFace, gid: u32) -> Arc<BezPath> {
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

/// Draw an already laid-out text block.
pub fn draw_layout(ctx: &mut RenderContext, l: &deckcraft_text::TextLayout, tr: Rect, m: Affine) {
    draw_layout_with(ctx, l, tr, m, &[]);
}

/// Draw a laid-out text block with per-paragraph animation states.
pub fn draw_layout_with(ctx: &mut RenderContext, l: &deckcraft_text::TextLayout, tr: Rect, m: Affine, paras: &[(usize, ParaState)]) {
    let para = |i: usize| paras.iter().find(|(p, _)| *p == i).map(|(_, s)| *s).unwrap_or_default();
    let m = if l.rotation != 0.0 {
        let c = tr.center().to_vec2();
        m * Affine::translate(c) * Affine::rotate(l.rotation.to_radians()) * Affine::translate(-c)
    } else {
        m
    };
    let deco = |ctx: &mut RenderContext, d: &deckcraft_text::Deco| {
        let ps = para(d.para);
        if !ps.visible || ps.opacity <= 0.001 {
            return;
        }
        ctx.set_transform(m * Affine::translate(ps.offset));
        ctx.set_paint(color(d.color, ps.opacity));
        ctx.fill_rect(&d.rect);
    };
    for d in l.decos.iter().filter(|d| d.behind) {
        deco(ctx, d);
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
        let alpha = run.alpha * ps.opacity;
        let k = run.size / run.face.upem.max(1.0);
        ctx.set_paint(color(run.color, alpha));
        for (gid, x, y) in &run.glyphs {
            let path = glyph(&run.face, *gid);
            if path.elements().is_empty() {
                continue;
            }
            let skew = if run.fake_italic { Affine::new([1.0, 0.0, -0.2, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
            let gm = m * Affine::translate((*x, *y)) * skew * Affine::scale(k);
            ctx.set_transform(gm);
            if alpha > 0.0 && run.color.a > 0 {
                ctx.fill_path(&path);
                if run.fake_bold {
                    ctx.set_stroke(kurbo::Stroke::new(run.face.upem * 0.025).with_join(kurbo::Join::Round));
                    ctx.stroke_path(&path);
                }
            }
            if let Some((oc, ow)) = run.outline {
                ctx.set_paint(color(oc, ps.opacity));
                ctx.set_stroke(kurbo::Stroke::new(ow / k.max(1e-9)).with_join(kurbo::Join::Round));
                ctx.stroke_path(&path);
                ctx.set_paint(color(run.color, alpha));
            }
        }
    }
    for d in l.decos.iter().filter(|d| !d.behind) {
        deco(ctx, d);
    }
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
    let f = Frame { pres, opts: &opts, fields: SlideFields { num: 1, date: String::new(), footer: String::new() } };
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
