//! 3-D models (`ShapeKind::Model3d`): the embedded `.glb` read once and drawn by
//! [`deckcraft_mesh::raster`] in the model's pose, fitted inside the frame with its proportions
//! kept, as PowerPoint draws a model. Pictures are made at quarter-octave sizes and cached like
//! decoded pictures, so zooming and redraws don't rasterize again. A model that can't be read
//! shows its fallback picture (the poster an `mc:AlternateContent` carried), else a grey box.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deckcraft_mesh::Scene;
use deckcraft_mesh::raster::{self, Camera, View};
use deckcraft_model::{MediaId, Model3dPose, Presentation};
use kurbo::{Affine, Rect};
use vello_cpu::{Pixmap, RenderContext, peniko};

/// Decoded meshes kept, by vertex count.
const SCENE_BUDGET: usize = 8_000_000;
/// Model pictures kept, by bytes.
const PICTURE_BUDGET: usize = 192 << 20;
/// Longest side a model picture is drawn at.
const MAX_SIDE: f64 = 2048.0;

struct SceneEntry {
    _bytes: Arc<Vec<u8>>,
    scene: Option<Arc<Scene>>,
    stamp: u64,
}

struct PictureEntry {
    _bytes: Arc<Vec<u8>>,
    pm: Arc<Pixmap>,
    stamp: u64,
}

#[derive(Default)]
struct Cache {
    scenes: HashMap<usize, SceneEntry>,
    vertices: usize,
    pictures: HashMap<(usize, u32, u32, u64), PictureEntry>,
    bytes: usize,
    clock: u64,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(Cache::default))
}

fn vertex_count(s: &Option<Arc<Scene>>) -> usize {
    s.as_ref().map_or(1, |s| s.vertex_count().max(1))
}

/// The mesh in `bytes` (a `.glb`), read once per media item; `None` when it can't be read.
pub(crate) fn scene(bytes: &Arc<Vec<u8>>) -> Option<Arc<Scene>> {
    let addr = Arc::as_ptr(bytes) as usize;
    let hit = with(|c| {
        c.clock += 1;
        let clock = c.clock;
        c.scenes.get_mut(&addr).filter(|e| Arc::ptr_eq(&e._bytes, bytes)).map(|e| {
            e.stamp = clock;
            e.scene.clone()
        })
    });
    if let Some(s) = hit {
        return s;
    }
    let scene = match deckcraft_mesh::glb::read(bytes) {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            log::info!("render: 3-D model not drawn: {e}");
            None
        }
    };
    with(|c| {
        c.clock += 1;
        c.vertices += vertex_count(&scene);
        c.scenes.insert(addr, SceneEntry { _bytes: bytes.clone(), scene: scene.clone(), stamp: c.clock });
        while c.vertices > SCENE_BUDGET && c.scenes.len() > 1 {
            let Some(oldest) = c.scenes.iter().filter(|(k, _)| **k != addr).min_by_key(|(_, e)| e.stamp).map(|(k, _)| *k) else { break };
            if let Some(e) = c.scenes.remove(&oldest) {
                c.vertices = c.vertices.saturating_sub(vertex_count(&e.scene));
            }
        }
    });
    scene
}

/// The view a model's pose describes (the camera's directions only; positions in any unit).
pub fn view_of(pose: &Model3dPose) -> View {
    let v = |a: [i64; 3]| [a[0] as f64, a[1] as f64, a[2] as f64];
    View { rot: pose.rot, camera: pose.camera.as_ref().map(|c| Camera { pos: v(c.pos), up: v(c.up), look_at: v(c.look_at) }) }
}

fn view_key(view: &View) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for v in view.rot {
        v.to_bits().hash(&mut h);
    }
    if let Some(c) = view.camera {
        for v in c.pos.iter().chain(&c.up).chain(&c.look_at) {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// The largest box of `aspect` (width / height) inside `b`, centred.
fn fit(b: Rect, aspect: f64) -> Rect {
    let (w, h) = (b.width(), b.height());
    let (fw, fh) = if w / h.max(1e-12) > aspect { (h * aspect, h) } else { (w, w / aspect) };
    Rect::new(b.x0 + (w - fw) / 2.0, b.y0 + (h - fh) / 2.0, b.x0 + (w + fw) / 2.0, b.y0 + (h + fh) / 2.0)
}

/// The model's width / height on screen, kept within 1:64 either way.
fn aspect(scene: &Scene, view: &View) -> f64 {
    raster::projected_size(scene, view).map(|(w, h)| w as f64 / h as f64).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(1.0).clamp(1.0 / 64.0, 64.0)
}

/// The picture of `scene` `iw` × `ih` px, cached for the media bytes it came from.
fn picture(bytes: &Arc<Vec<u8>>, scene: &Scene, view: &View, iw: u32, ih: u32) -> Option<Arc<Pixmap>> {
    let key = (Arc::as_ptr(bytes) as usize, iw, ih, view_key(view));
    let hit = with(|c| {
        c.clock += 1;
        let clock = c.clock;
        c.pictures.get_mut(&key).filter(|e| Arc::ptr_eq(&e._bytes, bytes)).map(|e| {
            e.stamp = clock;
            e.pm.clone()
        })
    });
    if hit.is_some() {
        return hit;
    }
    let img = raster::render(scene, view, iw, ih)?;
    let data: Vec<vello_cpu::color::PremulRgba8> = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as u16;
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    let pm = Arc::new(Pixmap::from_parts(data, u16::try_from(iw).ok()?, u16::try_from(ih).ok()?));
    with(|c| {
        c.clock += 1;
        c.bytes += iw as usize * ih as usize * 4;
        c.pictures.insert(key, PictureEntry { _bytes: bytes.clone(), pm: pm.clone(), stamp: c.clock });
        while c.bytes > PICTURE_BUDGET && c.pictures.len() > 1 {
            let Some(oldest) = c.pictures.iter().filter(|(k, _)| **k != key).min_by_key(|(_, e)| e.stamp).map(|(k, _)| *k) else { break };
            if let Some(e) = c.pictures.remove(&oldest) {
                c.bytes = c.bytes.saturating_sub(e.pm.width() as usize * e.pm.height() as usize * 4);
            }
        }
    });
    Some(pm)
}

/// Picture size for a model drawn `want` px along its longer side: quarter-octave steps from
/// 16 px up to [`MAX_SIDE`].
fn bucket(want: f64) -> f64 {
    let want = if want.is_finite() { want.clamp(16.0, MAX_SIDE) } else { 16.0 };
    (2f64.powf((want.log2() * 4.0).ceil() / 4.0)).min(MAX_SIDE).round()
}

/// Draws the model `media` in `pose` (or its fallback picture `poster`) over `bounds` in shape
/// space, `m` being the shape-to-device transform.
pub(crate) fn draw(
    ctx: &mut RenderContext,
    pres: &Presentation,
    media: MediaId,
    poster: Option<MediaId>,
    pose: &Model3dPose,
    bounds: Rect,
    m: Affine,
) {
    let item = pres.media(media);
    let found = item.and_then(|item| scene(&item.data).map(|s| (item.data.clone(), s)));
    let Some((bytes, scene)) = found else {
        match poster {
            Some(p) => {
                let pf = deckcraft_model::style::PictureFill { media: p, ..Default::default() };
                crate::paint::picture(ctx, pres, &pf, bounds, m, None);
            }
            None => crate::paint::missing(ctx, bounds),
        }
        return;
    };
    let view = view_of(pose);
    let a = aspect(&scene, &view);
    let dest = fit(bounds, a);
    let side = bucket(dest.width().max(dest.height()) * crate::scale_of(m));
    let (iw, ih) = if a >= 1.0 { (side, (side / a).round().max(1.0)) } else { ((side * a).round().max(1.0), side) };
    let Some(pm) = picture(&bytes, &scene, &view, iw as u32, ih as u32) else { return };
    let image = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(pm),
        sampler: peniko::ImageSampler { quality: peniko::ImageQuality::High, ..Default::default() },
    };
    crate::paint::draw_image(ctx, &image, iw, ih, dest, m, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_step_by_quarter_octaves() {
        assert_eq!(bucket(1.0), 16.0);
        assert_eq!(bucket(f64::NAN), 16.0);
        assert_eq!(bucket(100.0), 108.0);
        assert_eq!(bucket(107.0), 108.0, "2^6.75 = 107.6");
        assert_eq!(bucket(1e9), MAX_SIDE);
        assert!(bucket(500.0) >= 500.0 && bucket(500.0) < 500.0 * 1.19);
    }

    #[test]
    fn fit_keeps_the_aspect_and_centres() {
        let r = fit(Rect::new(0.0, 0.0, 200.0, 100.0), 1.0);
        assert_eq!(r, Rect::new(50.0, 0.0, 150.0, 100.0));
        let r = fit(Rect::new(0.0, 0.0, 100.0, 100.0), 4.0);
        assert_eq!(r, Rect::new(0.0, 37.5, 100.0, 62.5));
    }

    use deckcraft_mesh::Primitive;
    use deckcraft_model::{Shape, ShapeId, ShapeKind, Xfrm};

    use crate::{RenderOpts, render_slide};

    /// A red square facing +Z, so every view shows it.
    fn square_glb() -> Vec<u8> {
        let p = Primitive {
            name: "sq".into(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![],
            indices: vec![0, 1, 2, 0, 2, 3],
            color: [1.0, 0.0, 0.0, 1.0],
        };
        deckcraft_mesh::glb::write(&Scene { primitives: vec![p] }).expect("glb")
    }

    fn blue_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).expect("png");
        out.into_inner()
    }

    fn slide_with_model(glb: Vec<u8>, with_poster: bool) -> Presentation {
        let mut p = Presentation::default();
        let media = p.add_media("m.glb", "model/gltf-binary", glb).expect("media");
        let poster = with_poster.then(|| p.add_media("p.png", "image/png", blue_png()).expect("poster"));
        let shape = Shape {
            id: ShapeId(900),
            xfrm: Some(Xfrm::new(100.0, 100.0, 300.0, 200.0)),
            kind: ShapeKind::Model3d { media, poster, pose: Default::default() },
            ..Default::default()
        };
        std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![shape];
        p
    }

    #[test]
    fn a_model_is_drawn_inside_its_frame() {
        let p = slide_with_model(square_glb(), true);
        let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
        let mid = img.pixel(250, 200);
        assert!(mid[0] > 120 && mid[1] < 60 && mid[2] < 60, "the model (red, shaded), not its poster: {mid:?}");
        assert_eq!(img.pixel(60, 60), [255, 255, 255, 255], "outside the frame");
        // The model keeps its proportions: the frame's far left is empty slide.
        assert_eq!(img.pixel(105, 200), [255, 255, 255, 255]);
        // Drawn twice, the picture comes from the cache and is the same.
        let again = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
        assert_eq!(again.pixel(250, 200), mid);
    }

    #[test]
    fn an_unreadable_model_shows_its_poster_or_a_box() {
        let broken = b"glTF\x02\0\0\0\x0c\0\0\0".to_vec();
        let img = render_slide(&slide_with_model(broken.clone(), true), 0, &RenderOpts { scale: 1.0, ..Default::default() });
        assert_eq!(img.pixel(250, 200)[..3], [0, 0, 255], "the fallback picture");
        let img = render_slide(&slide_with_model(broken, false), 0, &RenderOpts { scale: 1.0, ..Default::default() });
        let px = img.pixel(120, 190);
        assert!(px[0] == px[1] && px[1] == px[2] && px[0] < 250, "a grey box: {px:?}");
    }
}
