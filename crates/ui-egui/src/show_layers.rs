//! Slide-show compositing, the way PowerPoint plays animations: a slide with animations or
//! animated GIFs is split once into pictures that are rasterized once, and every frame after that
//! only moves, scales, turns, fades and clips those pictures on the GPU.
//!
//! The layers, bottom to top: the backdrop with the shapes under the first animated one (one
//! slide-sized picture), then every top-level shape from the first animated one to the last on a
//! picture of its own (a sprite, as big as what it draws), then the shapes over the last animated
//! one (one slide-sized picture). A shape whose text builds by paragraph keeps each animated
//! paragraph on its own sprite. An animated GIF keeps one sprite per frame it has shown.
//!
//! What a sprite can't show by moving (a colour change, a shape animated inside a group) is drawn
//! again with that state, only that shape. A sprite that comes to rest moved, turned or scaled is
//! drawn again, once, in that pose, so a still slide is as sharp as a fully rendered one.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use deckcraft_geom::{Affine, Point};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use deckcraft_render::{ParaState, RenderOpts, ShapePart, ShapeState};
use egui::{Color32, Mesh, Pos2, Rect, TextureHandle, pos2};

/// The bytes of GIF frame sprites one slide keeps before the oldest frames are dropped.
const GIF_BUDGET: usize = if cfg!(target_arch = "wasm32") { 96 << 20 } else { 256 << 20 };

/// What the layers were built for: a change rebuilds them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayersKey {
    pub slide: usize,
    pub size: (u32, u32),
    /// The presentation's address (held alive by the show while the layers are kept).
    pub doc: usize,
    /// Media hidden while not playing.
    pub hidden: Vec<ShapeId>,
}

/// A picture of part of the slide and where it sits, in output pixels `[x0, y0, x1, y1]`.
struct Tex {
    tex: TextureHandle,
    px: [f64; 4],
}

impl Tex {
    fn bytes(&self) -> usize {
        let [x0, y0, x1, y1] = self.px;
        ((x1 - x0).max(0.0) * (y1 - y0).max(0.0) * 4.0) as usize
    }
}

enum Item {
    /// Shapes that never move, with the backdrop under them for the bottom layer. `None` until
    /// built, `Some(None)` when they draw nothing.
    Full {
        ids: Vec<ShapeId>,
        backdrop: bool,
        tex: Option<Option<TextureHandle>>,
    },
    Sprite(Box<Sprite>),
}

/// One top-level shape on its own picture.
struct Sprite {
    id: ShapeId,
    /// Shapes animated inside it (a group's children).
    inner: Vec<ShapeId>,
    /// Animated GIFs at or inside it.
    gifs: Vec<ShapeId>,
    /// Paragraphs of its text animated on their own.
    paras: Vec<usize>,
    built: bool,
    /// The still shape (its separately animated paragraphs left out), by the GIF frames it shows.
    base: HashMap<Vec<usize>, Option<Tex>>,
    /// The order the GIF frame sprites were made in, oldest first.
    base_order: Vec<Vec<usize>>,
    para_tex: Vec<(usize, Option<Tex>)>,
    /// The shape drawn in one state (a pose it rests in, or a state moving can't show).
    baked: Option<(u64, Option<Tex>)>,
    /// What it showed last frame: a sprite showing the same thing twice is at rest.
    last: Option<u64>,
    /// The GIF frames of the still picture shown this frame.
    shown: Option<Vec<usize>>,
}

/// Where the output pixels go on screen: `rect` shows the part `uv` (0–1) of a `size` picture.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    pub size: (u32, u32),
    pub rect: Rect,
    pub uv: Rect,
    /// The largest texture side the GPU takes.
    pub max_side: u32,
    /// Pixels per slide point.
    pub scale: f64,
}

impl View {
    fn screen(self, p: Point) -> Pos2 {
        let (w, h) = (self.size.0.max(1) as f64, self.size.1.max(1) as f64);
        let (uw, uh) = ((self.uv.width() as f64).max(1e-6), (self.uv.height() as f64).max(1e-6));
        let nx = (p.x / w - self.uv.min.x as f64) / uw;
        let ny = (p.y / h - self.uv.min.y as f64) / uh;
        pos2(self.rect.min.x + (nx * self.rect.width() as f64) as f32, self.rect.min.y + (ny * self.rect.height() as f64) as f32)
    }
    /// Slide points to output pixels.
    fn px(&self) -> Affine {
        Affine::scale(self.scale)
    }
}

/// What a frame shows: each shape's animation state and the GIFs' clocks and frames.
pub(crate) struct Frame<'a> {
    pub state: &'a dyn Fn(ShapeId) -> ShapeState,
    pub gif_times: &'a [(ShapeId, f64)],
    pub gif_frames: &'a [(ShapeId, usize)],
    pub threads: u16,
}

/// One textured quadrilateral of a composited frame: corners in output pixels, where they come
/// from in the texture (0–1), and how opaque it is drawn.
#[derive(Clone, Debug)]
pub(crate) struct Quad {
    tex: egui::TextureId,
    at: [Point; 4],
    uv: [Pos2; 4],
    alpha: u8,
}

/// Draw a composited frame's quads into `view.rect` (showing the part `view.uv` of the slide).
pub(crate) fn draw(painter: &egui::Painter, view: &View, quads: &[Quad]) {
    let painter = painter.with_clip_rect(view.rect);
    for q in quads {
        if q.at.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            continue;
        }
        let color = Color32::from_white_alpha(q.alpha);
        let mut mesh = Mesh::with_texture(q.tex);
        for (p, uv) in q.at.iter().zip(q.uv.iter()) {
            mesh.vertices.push(egui::epaint::Vertex { pos: view.screen(*p), uv: *uv, color });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
}

pub(crate) struct Layers {
    pub key: LayersKey,
    items: Vec<Item>,
    /// Pictures made for this slide (names only; the textures go with their handles).
    made: usize,
}

fn walk<'a>(s: &'a Shape, depth: usize, out: &mut Vec<&'a Shape>) {
    if depth > 32 {
        return;
    }
    out.push(s);
    if let ShapeKind::Group { children, .. } = &s.kind {
        for c in children {
            walk(c, depth + 1, out);
        }
    }
}

fn hash_state(st: &ShapeState, h: &mut impl Hasher) {
    st.visible.hash(h);
    for v in [st.opacity, st.offset.x, st.offset.y, st.scale.0, st.scale.1, st.rotate] {
        v.to_bits().hash(h);
    }
    st.clip.map(|c| c.map(f64::to_bits)).hash(h);
    st.tint.map(|t| (t.r, t.g, t.b, t.a)).hash(h);
    for (i, p) in &st.paras {
        (i, p.visible, p.opacity.to_bits(), p.offset.x.to_bits(), p.offset.y.to_bits()).hash(h);
    }
}

/// Nothing moved, turned, scaled or clipped (a sprite drawn as is shows it exactly).
fn still_pose(st: &ShapeState) -> bool {
    let turned = st.rotate.rem_euclid(360.0);
    st.offset.x == 0.0
        && st.offset.y == 0.0
        && st.scale == (1.0, 1.0)
        && (turned == 0.0 || turned.is_nan())
        && st.clip.is_none_or(|[l, t, r, b]| l <= 0.0 && t <= 0.0 && r >= 1.0 && b >= 1.0)
        && st.paras.iter().all(|(_, p)| p.offset.x == 0.0 && p.offset.y == 0.0)
}

fn untouched(st: &ShapeState) -> bool {
    st.visible && st.opacity >= 1.0 && still_pose(st) && st.tint.is_none() && st.paras.iter().all(|(_, p)| p.visible && p.opacity >= 1.0)
}

impl Layers {
    /// Plan the layers of slide `key.slide`: `animated` are the shapes with effects, `gifs` the
    /// animated GIF pictures. `None` when nothing on the slide moves.
    pub fn plan(doc: &Presentation, key: LayersKey, animated: &[ShapeId], gifs: &[ShapeId], paras: &dyn Fn(ShapeId) -> Vec<usize>) -> Option<Layers> {
        let slide = doc.slides.get(key.slide)?;
        let moving = |s: &Shape| {
            let mut all = vec![];
            walk(s, 0, &mut all);
            all.iter().any(|x| animated.contains(&x.id) || gifs.contains(&x.id))
        };
        let first = slide.shapes.iter().position(moving)?;
        let last = slide.shapes.iter().rposition(moving)?;
        let mut items = vec![Item::Full { ids: slide.shapes.iter().take(first).map(|s| s.id).collect(), backdrop: true, tex: None }];
        for s in slide.shapes.iter().take(last + 1).skip(first) {
            let mut all = vec![];
            walk(s, 0, &mut all);
            let inner: Vec<ShapeId> = all.iter().skip(1).map(|x| x.id).filter(|id| animated.contains(id)).collect();
            let gifs: Vec<ShapeId> = all.iter().map(|x| x.id).filter(|id| gifs.contains(id)).collect();
            let paras = if animated.contains(&s.id) { paras(s.id) } else { vec![] };
            items.push(Item::Sprite(Box::new(Sprite {
                id: s.id,
                inner,
                gifs,
                paras,
                built: false,
                base: HashMap::new(),
                base_order: vec![],
                para_tex: vec![],
                baked: None,
                last: None,
                shown: None,
            })));
        }
        let top: Vec<ShapeId> = slide.shapes.iter().skip(last + 1).map(|s| s.id).collect();
        items.push(Item::Full { ids: top, backdrop: false, tex: None });
        Some(Layers { key, items, made: 0 })
    }

    pub fn ready(&self) -> bool {
        self.items.iter().all(|i| match i {
            Item::Full { tex, .. } => tex.is_some(),
            Item::Sprite(s) => s.built,
        })
    }

    /// Build the next layer that isn't built yet (all of them with `all`). True once every layer
    /// is ready.
    pub fn build(&mut self, ctx: &egui::Context, doc: &Presentation, view: &View, f: &Frame, all: bool) -> bool {
        let idx = self.key.slide;
        for n in 0..self.items.len() {
            let name = format!("show-layer-{}", self.made);
            let Some(item) = self.items.get_mut(n) else { break };
            let did = match item {
                Item::Full { ids, backdrop, tex } if tex.is_none() => {
                    let img = if ids.is_empty() && !*backdrop {
                        None
                    } else {
                        let opts = opts(view, view.size, (0.0, 0.0), Some(f.state), f.gif_times, f.threads);
                        let (img, _) = deckcraft_render::render_slide_shapes(doc, idx, ids, ShapePart::Whole, *backdrop, &opts);
                        Some(crate::textures::upload(ctx, name, crate::textures::to_color_image(&img, false)))
                    };
                    *tex = Some(img);
                    true
                }
                Item::Sprite(s) if !s.built => {
                    s.build(ctx, doc, idx, view, f, &name);
                    true
                }
                _ => false,
            };
            if did {
                self.made += 1;
                if !all {
                    return self.ready();
                }
            }
        }
        self.ready()
    }

    /// The quads of frame `f`, bottom to top, drawing any picture it needs that isn't there yet.
    /// The layers must be [`ready`](Self::ready).
    pub fn frame(&mut self, ctx: &egui::Context, doc: &Presentation, view: &View, f: &Frame) -> Vec<Quad> {
        let idx = self.key.slide;
        let (w, h) = (view.size.0 as f64, view.size.1 as f64);
        let mut out = vec![];
        for n in 0..self.items.len() {
            let name = format!("show-layer-{}", self.made);
            let Some(item) = self.items.get_mut(n) else { break };
            match item {
                Item::Full { tex: Some(Some(t)), .. } => out.push(Quad {
                    tex: t.id(),
                    at: [Point::new(0.0, 0.0), Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)],
                    uv: [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)],
                    alpha: 255,
                }),
                Item::Full { .. } => {}
                Item::Sprite(s) => {
                    if s.frame(ctx, doc, idx, view, f, &name, &mut out) {
                        self.made += 1;
                    }
                }
            }
        }
        out
    }
}

fn opts<'a>(
    view: &View,
    size: (u32, u32),
    offset: (f64, f64),
    state: Option<&'a dyn Fn(ShapeId) -> ShapeState>,
    gif_times: &'a [(ShapeId, f64)],
    threads: u16,
) -> RenderOpts<'a> {
    RenderOpts { scale: view.scale, size: Some(size), offset, state, gif_times, threads, ..Default::default() }
}

/// Render top-level shape `id` (as `part`) into a picture just big enough for what it draws.
/// `None` when it draws nothing on or near the slide.
#[allow(clippy::too_many_arguments)]
fn sprite(
    ctx: &egui::Context,
    doc: &Presentation,
    idx: usize,
    id: ShapeId,
    part: ShapePart,
    view: &View,
    state: Option<&dyn Fn(ShapeId) -> ShapeState>,
    f: &Frame,
    name: &str,
) -> Option<Tex> {
    let ids = [id];
    // Measure: a 1×1 output costs next to nothing and still reports the whole drawing.
    let probe = opts(view, (1, 1), (0.0, 0.0), state, f.gif_times, 0);
    let (_, drawn) = deckcraft_render::render_slide_shapes(doc, idx, &ids, part, false, &probe);
    let d = drawn?;
    let (w, h) = (view.size.0 as f64, view.size.1 as f64);
    // A sprite may reach off the slide (a shape that flies in from there), within reason.
    let x0 = (d.x0.floor() - 2.0).max(-w);
    let y0 = (d.y0.floor() - 2.0).max(-h);
    let x1 = (d.x1.ceil() + 2.0).min(2.0 * w);
    let y1 = (d.y1.ceil() + 2.0).min(2.0 * h);
    let max = view.max_side.max(1) as f64;
    let (sw, sh) = ((x1 - x0).min(max), (y1 - y0).min(max));
    if !(sw >= 1.0 && sh >= 1.0) || !x0.is_finite() || !y0.is_finite() {
        return None;
    }
    let o = opts(view, (sw as u32, sh as u32), (-x0, -y0), state, f.gif_times, f.threads);
    let (img, _) = deckcraft_render::render_slide_shapes(doc, idx, &ids, part, false, &o);
    let tex = crate::textures::upload(ctx, name.to_string(), crate::textures::to_color_image(&img, false));
    Some(Tex { tex, px: [x0, y0, x0 + sw, y0 + sh] })
}

/// `t` drawn with its pixel `p` at `to(p)` (output pixels), faded by `alpha`. With `clip`, only
/// the quadrilateral `clip.0` (output pixels where it is drawn), taking its pixels from `clip.1`
/// (where they are in `t`'s still picture), is drawn.
fn quad(out: &mut Vec<Quad>, t: &Tex, to: Affine, alpha: f64, clip: Option<([Point; 4], [Point; 4])>) {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a == 0 {
        return;
    }
    let [x0, y0, x1, y1] = t.px;
    let (tw, th) = ((x1 - x0).max(1e-6), (y1 - y0).max(1e-6));
    let uv = |p: Point| pos2(((p.x - x0) / tw) as f32, ((p.y - y0) / th) as f32);
    let corners = [Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
    let (at, from): ([Point; 4], [Point; 4]) = match clip {
        Some((at, from)) => (at, from),
        None => (corners.map(|p| to * p), corners),
    };
    out.push(Quad { tex: t.tex.id(), at, uv: from.map(uv), alpha: a });
}

impl Sprite {
    fn gif_key(&self, f: &Frame) -> Vec<usize> {
        self.gifs.iter().map(|g| f.gif_frames.iter().find(|x| x.0 == *g).map_or(0, |x| x.1)).collect()
    }

    /// The state the still sprite is drawn in: every separately animated paragraph left out.
    fn body_state(&self) -> impl Fn(ShapeId) -> ShapeState + use<> {
        let (id, paras) = (self.id, self.paras.clone());
        move |sid| {
            if sid == id {
                ShapeState { paras: paras.iter().map(|p| (*p, ParaState { visible: false, ..Default::default() })).collect(), ..Default::default() }
            } else {
                ShapeState::default()
            }
        }
    }

    fn ensure_base(&mut self, ctx: &egui::Context, doc: &Presentation, idx: usize, view: &View, f: &Frame, name: &str) -> bool {
        let key = self.gif_key(f);
        if self.base.contains_key(&key) {
            return false;
        }
        let body = self.body_state();
        let t = sprite(ctx, doc, idx, self.id, ShapePart::Whole, view, Some(&body), f, name);
        let held: usize = self.base.values().flatten().map(Tex::bytes).sum::<usize>() + t.as_ref().map_or(0, Tex::bytes);
        if held > GIF_BUDGET {
            // Drop the oldest frames until the new one fits.
            let mut held = held;
            while held > GIF_BUDGET && !self.base_order.is_empty() {
                let old = self.base_order.remove(0);
                if let Some(Some(o)) = self.base.remove(&old) {
                    held = held.saturating_sub(o.bytes());
                }
            }
        }
        self.base.insert(key.clone(), t);
        self.base_order.push(key);
        true
    }

    fn build(&mut self, ctx: &egui::Context, doc: &Presentation, idx: usize, view: &View, f: &Frame, name: &str) {
        self.ensure_base(ctx, doc, idx, view, f, name);
        self.para_tex = self
            .paras
            .iter()
            .enumerate()
            .map(|(k, p)| (*p, sprite(ctx, doc, idx, self.id, ShapePart::Paragraph(*p), view, None, f, &format!("{name}-p{k}"))))
            .collect();
        self.built = true;
    }

    /// This frame's quads of the shape. True when a new texture was made.
    #[allow(clippy::too_many_arguments)]
    fn frame(&mut self, ctx: &egui::Context, doc: &Presentation, idx: usize, view: &View, f: &Frame, name: &str, out: &mut Vec<Quad>) -> bool {
        let st = (f.state)(self.id);
        if !st.visible || st.opacity <= 0.001 {
            self.last = None;
            return false;
        }
        let gif = self.gif_key(f);
        let inner: Vec<ShapeState> = self.inner.iter().map(|i| (f.state)(*i)).collect();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        hash_state(&st, &mut h);
        for s in &inner {
            hash_state(s, &mut h);
        }
        gif.hash(&mut h);
        let key = h.finish();
        let resting = self.last == Some(key);
        self.last = Some(key);
        // Moving the still picture shows the frame unless something inside the shape changed
        // (a colour, a shape in a group) or it rests in a pose resampling would soften.
        let movable = st.tint.is_none() && inner.iter().all(untouched);
        let motion = deckcraft_render::shape_motion(doc, idx, self.id, &st);
        if movable
            && (!resting || still_pose(&st))
            && let Some(m) = motion
        {
            let made = self.ensure_base(ctx, doc, idx, view, f, name);
            self.shown = Some(gif);
            self.composite(out, view, &st, &m);
            return made;
        }
        let made = match &self.baked {
            Some((k, _)) if *k == key => false,
            _ => {
                let state = f.state;
                self.baked = Some((key, sprite(ctx, doc, idx, self.id, ShapePart::Whole, view, Some(state), f, &format!("{name}-baked"))));
                true
            }
        };
        if let Some((_, Some(t))) = &self.baked {
            quad(out, t, Affine::IDENTITY, 1.0, None);
        }
        made
    }

    /// The still pictures moved as `st` moves the shape (`m`: how, in slide points).
    fn composite(&self, out: &mut Vec<Quad>, view: &View, st: &ShapeState, m: &deckcraft_render::Motion) {
        let px = view.px();
        let (still, moved) = (px * m.still, px * m.moved);
        let det = still.determinant();
        if !det.is_finite() || det.abs() <= 1e-12 {
            return;
        }
        let back = still.inverse();
        // The reveal clip, in the shape's own space (points from its box's top-left corner).
        let clip = st.clip.filter(|[l, t, r, b]| !(*l <= 0.0 && *t <= 0.0 && *r >= 1.0 && *b >= 1.0)).map(|[l, t, r, b]| {
            [Point::new(l * m.w, t * m.h), Point::new(r * m.w, t * m.h), Point::new(r * m.w, b * m.h), Point::new(l * m.w, b * m.h)]
        });
        if let Some(Some(t)) = self.current_base() {
            let c = clip.map(|q| (q.map(|p| moved * p), q.map(|p| still * p)));
            quad(out, t, moved * back, st.opacity, c);
        }
        for (p, tex) in &self.para_tex {
            let Some(t) = tex else { continue };
            let ps = st.paras.iter().find(|(i, _)| i == p).map(|(_, s)| *s).unwrap_or_default();
            if !ps.visible || ps.opacity <= 0.001 {
                continue;
            }
            let shift = Affine::translate(ps.offset);
            // The clip stays with the shape; the paragraph moves under it.
            let c = clip.map(|q| (q.map(|p| moved * p), q.map(|p| still * (shift.inverse() * p))));
            quad(out, t, moved * shift * back, st.opacity * ps.opacity, c);
        }
    }

    fn current_base(&self) -> Option<&Option<Tex>> {
        self.shown.as_ref().and_then(|k| self.base.get(k))
    }
}
