//! 3-D models on slides: the model file read once per media item, rendered by `deckcraft-mesh` at
//! the frame's size on the output, and drawn into the frame like a picture.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deckcraft_anim::ModelAnim;
use deckcraft_geom::space;
use deckcraft_mesh::{Camera, Light, LightKind, Scene};
use deckcraft_model::model3d::{self, Model3d};
use deckcraft_model::{Presentation, style::PictureFill};
use kurbo::{Affine, BezPath, Rect, Shape as _};
use vello_cpu::{Pixmap, RenderContext, peniko};

/// Decoded models kept (they can be large).
const SCENE_BUDGET: usize = 768 << 20;
/// Rendered model images kept.
const IMAGE_BUDGET: usize = 96 << 20;
/// Longest side a model is rendered at; the picture is stretched beyond.
const MAX_PX: f64 = 4096.0;

struct Scenes {
    /// By the media bytes' address; the bytes stay alive with the entry so the address can't be
    /// reused. `None`: the file can't be read.
    map: HashMap<usize, (Arc<Vec<u8>>, Option<Arc<Scene>>, u64)>,
    clock: u64,
}

struct Images {
    map: HashMap<u64, (Arc<Pixmap>, u64)>,
    bytes: usize,
    clock: u64,
}

static SCENES: Mutex<Option<Scenes>> = Mutex::new(None);
static IMAGES: Mutex<Option<Images>> = Mutex::new(None);

fn scene_bytes(s: &Scene) -> usize {
    let v: usize = s.primitives.iter().map(|p| p.positions.len() * 48 + p.indices.len() * 4).sum();
    v + s.textures.iter().map(|t| t.rgba.len() + t.data.len()).sum::<usize>()
}

/// The model in `bytes` (a glTF binary, or any file `deckcraft-mesh` reads), read once and shared.
pub fn model_scene(bytes: &Arc<Vec<u8>>) -> Option<Arc<Scene>> {
    let addr = Arc::as_ptr(bytes) as usize;
    {
        let mut g = SCENES.lock().unwrap_or_else(|e| e.into_inner());
        let c = g.get_or_insert_with(|| Scenes { map: HashMap::new(), clock: 0 });
        c.clock += 1;
        let now = c.clock;
        if let Some(e) = c.map.get_mut(&addr) {
            e.2 = now;
            return e.1.clone();
        }
    }
    // Read outside the lock: other renders go on meanwhile.
    let scene = match deckcraft_mesh::load("model.glb", bytes, &deckcraft_mesh::no_files) {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            log::warn!("3-D model: {e}");
            None
        }
    };
    let mut g = SCENES.lock().unwrap_or_else(|e| e.into_inner());
    let c = g.get_or_insert_with(|| Scenes { map: HashMap::new(), clock: 0 });
    c.clock += 1;
    let now = c.clock;
    c.map.insert(addr, (bytes.clone(), scene.clone(), now));
    // Over budget: drop the least recently used, never the one just read. The bytes each entry
    // keeps alive count too, so files that couldn't be read are let go as well.
    loop {
        let total: usize = c.map.values().map(|e| e.0.len() + e.1.as_ref().map_or(0, |s| scene_bytes(s))).sum();
        if total <= SCENE_BUDGET || c.map.len() <= 1 {
            break;
        }
        let Some(old) = c.map.iter().filter(|(k, _)| **k != addr).min_by_key(|(_, e)| e.2).map(|(k, _)| *k) else { break };
        c.map.remove(&old);
    }
    scene
}

/// The lights of `m` in camera space, for the renderer.
pub fn camera_lights(m: &Model3d, view: &space::Mat4) -> Vec<Light> {
    // No lights is no light, as in PowerPoint: only what glows shows.
    m.lights
        .iter()
        .take(16)
        .map(|l| {
            let k = if l.intensity.is_finite() { l.intensity.max(0.0) } else { 0.0 };
            let color = l.color.map(|c| (if c.is_finite() { c.max(0.0) } else { 0.0 } * k) as f32);
            let kind = match l.kind {
                model3d::LightKind::Ambient => LightKind::Ambient,
                model3d::LightKind::Point => LightKind::Point,
                model3d::LightKind::Directional => LightKind::Directional,
                model3d::LightKind::Spot => {
                    let a = if l.angle.is_finite() { l.angle.clamp(1.0, 89.0) } else { 30.0 };
                    LightKind::Spot { cos_inner: (a * 0.8).to_radians().cos() as f32, cos_outer: a.to_radians().cos() as f32 }
                }
            };
            Light { kind, color, pos: space::mat4_point(view, l.pos), dir: space::mat4_dir(view, l.dir) }
        })
        .collect()
}

/// How a model is seen for a frame `w` × `h` points rendered at `px` × `py` pixels.
pub fn camera_for(m: &Model3d, w: f64, h: f64, px: f64, py: f64) -> Camera {
    camera_region(m, w, h, Rect::new(0.0, 0.0, w, h), px, py)
}

/// How a model is seen when the part `region` (frame-local points) of its `w` × `h` frame is
/// rendered at `px` × `py` pixels.
fn camera_region(m: &Model3d, w: f64, h: f64, region: Rect, px: f64, py: f64) -> Camera {
    let k = if region.width() > 0.0 { px / region.width() } else { 1.0 };
    let _ = py;
    Camera { model_view: m.model_view(), focal: m.focal(h) * k, ortho: m.is_ortho(), center: ((w / 2.0 - region.x0) * k, (h / 2.0 - region.y0) * k) }
}

/// Render `m` for a frame `w` × `h` points at `px` × `py` pixels (premultiplied RGBA).
pub fn render_model(scene: &Scene, m: &Model3d, w: f64, h: f64, px: u32, py: u32) -> deckcraft_mesh::Raster {
    render_region(scene, m, w, h, Rect::new(0.0, 0.0, w, h), px, py, false)
}

/// Render the part `region` of `m`'s frame; `moving` (an animation is playing) renders without
/// supersampling to keep up.
fn render_region(scene: &Scene, m: &Model3d, w: f64, h: f64, region: Rect, px: u32, py: u32, moving: bool) -> deckcraft_mesh::Raster {
    let cam = camera_region(m, w, h, region, px as f64, py as f64);
    let lights = camera_lights(m, &m.view_matrix());
    let samples = if !moving && (px as u64) * (py as u64) <= 1_500_000 { 2 } else { 1 };
    deckcraft_mesh::render(scene, &cam, &lights, px, py, samples)
}

/// `m` as a 3-D effect poses it: turned about the view's axes after its own rotation and about its
/// own axes before it, scaled along its own axes and moved in the scene.
pub fn posed(m: &Model3d, a: &ModelAnim) -> Model3d {
    let mut p = m.clone();
    let r = space::mat3_mul(&space::euler_xyz(a.view_turn), &space::mat3_mul(&space::euler_xyz(m.rot), &space::euler_xyz(a.own_turn)));
    p.rot = space::euler_xyz_of(&r);
    p.scale = [0, 1, 2].map(|k| m.scale[k] * a.scale[k]);
    p.post = space::add(m.post, a.shift);
    p
}

/// The part of the slide (frame-local points) an animated model may cover: the square its bounding
/// sphere fills (any turn stays inside), moved as far as the effect moves it, with the frame.
fn motion_region(m: &Model3d, a: &ModelAnim, w: f64, h: f64) -> Rect {
    let side = m.span(h) * 1.1;
    let frame = Rect::new(0.0, 0.0, w, h);
    let dist = space::length(space::sub(m.camera.pos, m.camera.look_at));
    let k = if m.is_ortho() { m.focal(h) } else { m.focal(h) / dist.max(1e-6) };
    let lift = a.shift[1] * k;
    let sq = Rect::from_center_size((w / 2.0, h / 2.0 - lift), (side, side));
    let r = frame.union(sq).union(Rect::from_center_size((w / 2.0, h / 2.0), (side, side)));
    // Never more than three frames across.
    let cap = Rect::new(-w, -h, 2.0 * w, 2.0 * h);
    let r = r.intersect(cap);
    if r.is_finite() && r.width() > 0.0 && r.height() > 0.0 { r } else { frame }
}

fn image_key(addr: usize, m: &Model3d, w: f64, h: f64, region: Rect, moving: bool, px: u32, py: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hs = std::hash::DefaultHasher::new();
    addr.hash(&mut hs);
    (px, py, moving).hash(&mut hs);
    for v in [w, h, region.x0, region.y0, region.x1, region.y1, m.meter_per_unit, m.viewport, m.camera.fov, m.camera.ortho]
        .into_iter()
        .chain(m.pre)
        .chain(m.scale)
        .chain(m.rot)
        .chain(m.post)
        .chain(m.camera.pos)
        .chain(m.camera.up)
        .chain(m.camera.look_at)
    {
        v.to_bits().hash(&mut hs);
    }
    for l in &m.lights {
        (l.kind as u8).hash(&mut hs);
        for v in l.color.into_iter().chain(l.pos).chain(l.dir).chain([l.intensity, l.angle]) {
            v.to_bits().hash(&mut hs);
        }
    }
    hs.finish()
}

fn pixmap(r: &deckcraft_mesh::Raster) -> Option<Pixmap> {
    let (w, h) = (u16::try_from(r.width).ok()?, u16::try_from(r.height).ok()?);
    let data: Vec<vello_cpu::color::PremulRgba8> =
        r.pixels.as_chunks::<4>().0.iter().map(|&[r, g, b, a]| vello_cpu::color::PremulRgba8 { r, g, b, a }).collect();
    (data.len() == w as usize * h as usize).then(|| Pixmap::from_parts(data, w, h))
}

/// The part `region` of the model's frame rendered at the output's scale, from the cache when it was
/// drawn the same way before.
fn rendered(bytes: &Arc<Vec<u8>>, m: &Model3d, w: f64, h: f64, region: Rect, moving: bool, scale: f64) -> Option<Arc<Pixmap>> {
    let scene = model_scene(bytes)?;
    let side = |v: f64| (v * scale).ceil().clamp(1.0, MAX_PX) as u32;
    let (px, py) = (side(region.width()), side(region.height()));
    let key = image_key(Arc::as_ptr(bytes) as usize, m, w, h, region, moving, px, py);
    {
        let mut g = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
        let c = g.get_or_insert_with(|| Images { map: HashMap::new(), bytes: 0, clock: 0 });
        c.clock += 1;
        let now = c.clock;
        if let Some(e) = c.map.get_mut(&key) {
            e.1 = now;
            return Some(e.0.clone());
        }
    }
    let pm = Arc::new(pixmap(&render_region(&scene, m, w, h, region, px, py, moving))?);
    let size = pm.width() as usize * pm.height() as usize * 4;
    let mut g = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    let c = g.get_or_insert_with(|| Images { map: HashMap::new(), bytes: 0, clock: 0 });
    c.clock += 1;
    let now = c.clock;
    if c.map.insert(key, (pm.clone(), now)).is_none() {
        c.bytes += size;
    }
    while c.bytes > IMAGE_BUDGET && c.map.len() > 1 {
        let Some(old) = c.map.iter().filter(|(k, _)| **k != key).min_by_key(|(_, e)| e.1).map(|(k, _)| *k) else { break };
        if let Some((p, _)) = c.map.remove(&old) {
            c.bytes = c.bytes.saturating_sub(p.width() as usize * p.height() as usize * 4);
        }
    }
    Some(pm)
}

/// A PNG of model `m` for a frame `w` × `h` points, `scale` pixels per point (the fallback picture
/// written with the model to PowerPoint files). `None` when the model can't be read.
pub fn model_png(pres: &Presentation, m: &Model3d, w: f64, h: f64, scale: f64) -> Option<Vec<u8>> {
    let bytes = &pres.media(m.media)?.data;
    let scene = model_scene(bytes)?;
    let side = |v: f64| (v * scale).ceil().clamp(1.0, 2048.0) as u32;
    let r = render_model(&scene, m, w, h, side(w), side(h));
    let img = crate::Image { width: r.width, height: r.height, pixels: r.pixels };
    let png = img.to_png();
    (!png.is_empty()).then_some(png)
}

/// Draw model `m` into its `w` × `h` frame under `tr` (frame-local points → output pixels), posed
/// by the 3-D effect `anim` playing on it: the model clipped to its frame (or, while an effect
/// plays, to the area it may move over), or its poster picture, or a placeholder when it can't be
/// read.
pub fn draw(ctx: &mut RenderContext, pres: &Presentation, m: &Model3d, w: f64, h: f64, tr: Affine, anim: &ModelAnim) {
    let bounds = Rect::new(0.0, 0.0, w, h);
    let moving = !anim.is_identity();
    let (m, region) =
        if moving { (std::borrow::Cow::Owned(posed(m, anim)), motion_region(m, anim, w, h)) } else { (std::borrow::Cow::Borrowed(m), bounds) };
    let m = m.as_ref();
    let image = pres.media(m.media).filter(|it| !it.data.is_empty()).and_then(|it| rendered(&it.data, m, w, h, region, moving, crate::scale_of(tr)));
    match image {
        Some(pm) => {
            let (iw, ih) = (pm.width() as f64, pm.height() as f64);
            let img = vello_cpu::Image {
                image: vello_cpu::ImageSource::Pixmap(pm),
                sampler: peniko::ImageSampler { quality: peniko::ImageQuality::Medium, ..Default::default() },
            };
            ctx.set_transform(tr);
            ctx.push_clip_layer(&region.to_path(0.1));
            ctx.set_transform(
                tr * Affine::translate(region.origin().to_vec2()) * Affine::scale_non_uniform(region.width() / iw, region.height() / ih),
            );
            ctx.set_paint(img);
            ctx.fill_rect(&Rect::new(0.0, 0.0, iw, ih));
            ctx.set_transform(tr);
            ctx.pop_layer();
        }
        None => match m.poster.filter(|p| pres.media(*p).is_some()) {
            Some(p) => {
                ctx.set_transform(tr);
                crate::paint::picture(ctx, pres, &PictureFill { media: p, ..Default::default() }, bounds, tr, None);
            }
            None => placeholder(ctx, bounds, tr),
        },
    }
}

/// A light box with a cube outline: a model that can't be drawn.
fn placeholder(ctx: &mut RenderContext, b: Rect, tr: Affine) {
    ctx.set_transform(tr);
    ctx.set_paint(peniko::Color::from_rgba8(236, 236, 236, 255));
    ctx.fill_rect(&b);
    let s = b.width().min(b.height()) * 0.18;
    let c = b.center();
    let (dx, dy) = (s * 0.866, s * 0.5);
    let mut p = BezPath::new();
    p.move_to((c.x, c.y - s));
    p.line_to((c.x + dx, c.y - dy));
    p.line_to((c.x + dx, c.y + dy));
    p.line_to((c.x, c.y + s));
    p.line_to((c.x - dx, c.y + dy));
    p.line_to((c.x - dx, c.y - dy));
    p.close_path();
    p.move_to((c.x - dx, c.y - dy));
    p.line_to((c.x, c.y));
    p.line_to((c.x + dx, c.y - dy));
    p.move_to((c.x, c.y));
    p.line_to((c.x, c.y + s));
    ctx.set_paint(peniko::Color::from_rgba8(150, 150, 150, 255));
    ctx.set_stroke(kurbo::Stroke::new((s * 0.08).max(0.5)));
    ctx.stroke_path(&p);
}
