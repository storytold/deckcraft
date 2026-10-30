//! The vector canvas path (what the PDF export draws) against the pixel path, on the newer
//! shape, picture and effect cases: a test canvas takes the vector route (no pixel context, a
//! raster fallback) but replays every call into pixels, so both routes can be compared pixel for
//! pixel, and counts what reached it as local pixels.

use super::*;
use deckcraft_color::Rgba;
use deckcraft_model::style::{ColorRef, Gradient, GradientShape, GradientStop, PictureFill, PictureMode};
use deckcraft_model::{Presentation, Shape, ShapeId, Xfrm};
use vello_cpu::Pixmap;

/// A vector canvas that draws into pixels: no `raster()`, a fallback at 1 px per point.
struct Replay {
    px: RenderContext,
    /// Local rasters placed (what a PDF gets as images of one element).
    locals: Vec<Rect>,
    /// Pictures placed: whether each came as its original file.
    images: Vec<bool>,
}

fn premul(rgba: &[u8], w: u32, h: u32) -> Option<Arc<Pixmap>> {
    let data = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as u32;
            let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    Some(Arc::new(Pixmap::from_parts(data, u16::try_from(w).ok()?, u16::try_from(h).ok()?)))
}

impl Replay {
    fn place(&mut self, pm: Arc<Pixmap>, t: Affine, alpha: f64) {
        Canvas::set_transform(&mut self.px, t);
        let img = CanvasImage { width: pm.width() as f64, height: pm.height() as f64, pixmap: Some(pm), encoded: None };
        Canvas::draw_image(&mut self.px, &img, alpha);
    }
}

impl Canvas for Replay {
    fn set_transform(&mut self, t: Affine) {
        Canvas::set_transform(&mut self.px, t);
    }
    fn set_paint_to(&mut self, p: Paint) {
        self.px.set_paint_to(p);
    }
    fn set_stroke(&mut self, s: kurbo::Stroke) {
        Canvas::set_stroke(&mut self.px, s);
    }
    fn set_fill_rule(&mut self, r: canvas::FillRule) {
        Canvas::set_fill_rule(&mut self.px, r);
    }
    fn fill_path(&mut self, p: &BezPath) {
        Canvas::fill_path(&mut self.px, p);
    }
    fn stroke_path(&mut self, p: &BezPath) {
        Canvas::stroke_path(&mut self.px, p);
    }
    fn push_layer(&mut self, clip: Option<&BezPath>, opacity: Option<f32>) {
        Canvas::push_layer(&mut self.px, clip, opacity);
    }
    fn pop_layer(&mut self) {
        Canvas::pop_layer(&mut self.px);
    }
    fn draw_image(&mut self, img: &CanvasImage, alpha: f64) {
        self.images.push(img.encoded.is_some());
        let t = *self.px.transform();
        let pm = match (img.encoded, &img.pixmap) {
            (Some(file), _) => {
                let Ok(d) = image::load_from_memory(file) else { return };
                let d = d.to_rgba8();
                premul(d.as_raw(), d.width(), d.height())
            }
            (None, Some(pm)) => Some(pm.clone()),
            (None, None) => None,
        };
        if let Some(pm) = pm {
            // The picture's grid is `width` x `height` units of the current transform.
            let k = Affine::scale_non_uniform(img.width / pm.width() as f64, img.height / pm.height() as f64);
            self.place(pm, t * k, alpha);
        }
    }
    fn fallback(&self) -> Option<Fallback> {
        Some(Fallback { scale: 1.0, clip: Rect::new(0.0, 0.0, 960.0, 540.0) })
    }
    fn draw_pixels(&mut self, img: &Image, dest: Rect) {
        self.locals.push(dest);
        let data = img.pixels.as_chunks::<4>().0.iter().map(|p| vello_cpu::color::PremulRgba8 { r: p[0], g: p[1], b: p[2], a: p[3] }).collect();
        let pm = Arc::new(Pixmap::from_parts(data, img.width as u16, img.height as u16));
        let t = Affine::translate(dest.origin().to_vec2())
            * Affine::scale_non_uniform(dest.width() / img.width as f64, dest.height() / img.height as f64);
        self.place(pm, t, 1.0);
    }
}

/// Slide 0 of `p` at 1 px per point: drawn the pixel way, and the vector way through [`Replay`].
fn both(p: &Presentation) -> (Image, Image, Replay) {
    let opts = RenderOpts { scale: 1.0, ..Default::default() };
    let a = render_slide(p, 0, &opts);
    let (w, h) = (a.width as u16, a.height as u16);
    let px = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    let mut rec = Replay { px, locals: Vec::new(), images: Vec::new() };
    draw_slide(&mut rec, p, 0, &opts, Affine::IDENTITY);
    let px = std::mem::replace(&mut rec.px, RenderContext::new(1, 1));
    let b = RENDERER.with(|r| r.borrow_mut().finish(px, w, h));
    (a, b, rec)
}

/// Pixels whose channels differ by more than `tol` between the two routes.
fn differing(a: &Image, b: &Image, tol: u8) -> usize {
    a.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > tol))
        .count()
}

/// The vector route draws what the pixel route draws, up to anti-aliasing at a few edge pixels.
fn assert_same(name: &str, p: &Presentation, locals: usize) -> Replay {
    assert_close(name, p, locals, 400).2
}

fn assert_close(name: &str, p: &Presentation, locals: usize, most: usize) -> (Image, Image, Replay) {
    let (a, b, rec) = both(p);
    let n = differing(&a, &b, 40);
    assert!(n <= most, "{name}: {n} pixels differ between the pixel and the vector route");
    assert_eq!(rec.locals.len(), locals, "{name}: local rasters {:?}", rec.locals);
    (a, b, rec)
}

fn deck_with(shapes: Vec<Shape>) -> Presentation {
    let mut p = Presentation::default();
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = shapes;
    p
}

fn rgb(r: u8, g: u8, b: u8) -> ColorRef {
    ColorRef::rgb(Rgba::rgb(r, g, b))
}

fn no_line() -> Option<Line> {
    Some(Line { fill: Some(Fill::None), ..Default::default() })
}

fn box_shape(id: u32, fill: Fill) -> Shape {
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(100.0, 100.0, 400.0, 200.0)), fill: Some(fill), line: no_line(), ..Default::default() }
}

fn two_stops() -> Vec<GradientStop> {
    vec![GradientStop { pos: 0.0, color: rgb(255, 255, 255) }, GradientStop { pos: 1.0, color: rgb(0, 0, 160) }]
}

/// Circle gradients from a focus off the centre (PowerPoint's two-circle gradient) and through a
/// tile rectangle are vector paints, the same as on screen.
#[test]
fn off_centre_and_tiled_radial_gradients_stay_vector() {
    for (name, focus, tile) in [
        ("off centre", [0.2, 0.2, 0.8, 0.8], [0.0; 4]),
        ("centred", [0.5, 0.5, 0.5, 0.5], [0.0; 4]),
        ("corner through its tile", [0.0, 0.0, 1.0, 1.0], [-1.0, -1.0, 0.0, 0.0]),
    ] {
        let g = Gradient { stops: two_stops(), shape: GradientShape::Path { path: "circle".into(), focus }, rotate_with_shape: true, tile };
        assert_same(name, &deck_with(vec![box_shape(40, Fill::Gradient(g))]), 0);
    }
    // A linear one through a tile rectangle, and one that keeps to the slide in a turned shape.
    let g = Gradient {
        stops: two_stops(),
        shape: GradientShape::Linear { angle: 30.0, scaled: false },
        rotate_with_shape: true,
        tile: [0.25, 0.0, 0.25, 0.0],
    };
    assert_same("linear tile", &deck_with(vec![box_shape(41, Fill::Gradient(g))]), 0);
    let g = Gradient { stops: two_stops(), shape: GradientShape::Linear { angle: 0.0, scaled: false }, rotate_with_shape: false, tile: [0.0; 4] };
    let mut sh = box_shape(42, Fill::Gradient(g));
    sh.xfrm = Some(Xfrm { rot: 60.0, ..Xfrm::new(200.0, 150.0, 300.0, 120.0) });
    assert_same("rotWithShape=0", &deck_with(vec![sh]), 0);
}

#[allow(clippy::unwrap_used)]
fn half_red_half_blue() -> Vec<u8> {
    let img = image::RgbImage::from_fn(8, 4, |x, _| if x < 4 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) });
    let mut out = std::io::Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Tiled picture fills (tile size from the picture's resolution, alignment, mirrored tiles) reach
/// a vector canvas as one embedded picture per tile, mirrored ones drawn flipped: the same tiling
/// as the pixel route's repeating image paint. (Where two tiles meet, the repeating paint blends
/// across the seam while separate pictures don't: only there may the routes differ.)
#[test]
fn tiled_pictures_tile_flip_and_align_on_vector_canvases() {
    let bounds = Rect::new(100.0, 100.0, 340.0, 170.0);
    for (flip, align) in [("none", "tl"), ("x", "tl"), ("y", "ctr"), ("xy", "br")] {
        let mut p = Presentation::default();
        let media = p.add_media("t.png", "image/png", half_red_half_blue()).expect("media");
        let mode = PictureMode::Tile { tx: 3.0, ty: -2.0, sx: 5.0, sy: 5.0, flip: flip.into(), align: align.into() };
        let mut sh = box_shape(43, Fill::Picture(PictureFill { media, mode, ..Default::default() }));
        sh.xfrm = Some(Xfrm::new(bounds.x0, bounds.y0, bounds.width(), bounds.height()));
        std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![sh];
        let (a, b, rec) = assert_close(&format!("tiles {flip} {align}"), &p, 0, 3000);
        assert!(rec.images.len() > 8 && rec.images.iter().all(|e| *e), "{flip}: {:?}", rec.images);
        // 8 x 4 px at 96 dpi scaled five times: 30 x 15 pt tiles. The middle of each half of
        // each tile is the same colour both ways.
        let o = paint::tile_origin(Rect::new(0.0, 0.0, bounds.width(), bounds.height()), align, (30.0, 15.0), (3.0, -2.0));
        for j in -1..6 {
            for i in -1..9 {
                for half in [7.5, 22.5] {
                    let (x, y) = (bounds.x0 + o.x + 30.0 * i as f64 + half, bounds.y0 + o.y + 15.0 * j as f64 + 7.5);
                    if !(x > bounds.x0 + 1.0 && x < bounds.x1 - 1.0 && y > bounds.y0 + 1.0 && y < bounds.y1 - 1.0) {
                        continue;
                    }
                    let (pa, pb) = (a.pixel(x as u32, y as u32), b.pixel(x as u32, y as u32));
                    assert!(pa.iter().zip(pb.iter()).all(|(u, v)| u.abs_diff(*v) <= 40), "{flip} {align} at ({x}, {y}): {pa:?} vs {pb:?}");
                }
            }
        }
    }
}

/// 3-D bevels and lit faces are pixels: a vector canvas gets them as a local raster over the
/// shape, never dropped.
#[test]
fn bevels_and_lit_faces_reach_vector_canvases_as_local_pixels() {
    let scene = r#"<a:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></a:scene3d>"#;
    for (name, sp3d) in [("bevel", r#"<a:sp3d><a:bevelT w="152400" h="76200"/></a:sp3d>"#), ("face", "<a:sp3d/>")] {
        let mut sh = box_shape(44, Fill::solid(rgb(91, 155, 213)));
        sh.effects = Some(deckcraft_model::Effects { raw3d: Some(format!("{scene}{sp3d}")), ..Default::default() });
        let rec = assert_same(name, &deck_with(vec![sh]), 1);
        let r = rec.locals[0];
        assert!(r.x0 >= 99.0 && r.y0 >= 99.0 && r.x1 <= 501.0 && r.y1 <= 301.0, "{name}: {r:?}");
    }
}

fn offset_shadow() -> deckcraft_model::Effects {
    let sh = deckcraft_model::style::Shadow {
        color: ColorRef::rgb(Rgba::BLACK),
        blur: 4.0,
        dist: 20.0,
        dir: 0.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: "b".into(),
        rotate_with_shape: false,
    };
    deckcraft_model::Effects { outer_shadow: Some(sh), ..Default::default() }
}

/// A group's shadow is cast by its children: on a vector canvas it is a local raster under the
/// group, the children themselves stay vector. Children filled with the group's fill keep it
/// laid out across the group, inside a local raster too.
#[test]
fn group_shadows_and_group_fills_on_vector_canvases() {
    let child = |id: u32, x: f64, fill: Fill| Shape {
        id: ShapeId(id),
        xfrm: Some(Xfrm::new(x, 0.0, 80.0, 100.0)),
        fill: Some(fill),
        line: no_line(),
        ..Default::default()
    };
    let group = |children: Vec<Shape>, fill: Option<Fill>, effects| Shape {
        id: ShapeId(45),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 100.0)),
        fill,
        effects,
        kind: ShapeKind::Group { children, child: Xfrm::new(0.0, 0.0, 200.0, 100.0) },
        ..Default::default()
    };
    let red = Fill::solid(rgb(255, 0, 0));
    let g = group(vec![child(46, 0.0, red.clone()), child(47, 120.0, red)], None, Some(offset_shadow()));
    let rec = assert_same("group shadow", &deck_with(vec![g]), 1);
    let r = rec.locals[0];
    assert!(r.x0 <= 100.0 && r.x1 >= 320.0 && r.y1 <= 230.0, "{r:?}");
    // grpFill children, one with a shadow of its own (a local raster laid out through the view).
    let grad = Fill::Gradient(Gradient {
        stops: two_stops(),
        shape: GradientShape::Linear { angle: 0.0, scaled: false },
        rotate_with_shape: true,
        tile: [0.0; 4],
    });
    let mut shadowed = child(49, 120.0, Fill::Group);
    shadowed.effects = Some(offset_shadow());
    let g = group(vec![child(48, 0.0, Fill::Group), shadowed], Some(grad), None);
    assert_same("group fill", &deck_with(vec![g]), 1);
}

/// Gradient lines and their arrowheads are vector strokes and fills with the gradient paint.
#[test]
fn gradient_lines_and_heads_stay_vector() {
    let grad = Fill::Gradient(Gradient {
        stops: vec![GradientStop { pos: 0.0, color: rgb(255, 0, 0) }, GradientStop { pos: 1.0, color: rgb(0, 0, 255) }],
        shape: GradientShape::Linear { angle: 0.0, scaled: false },
        rotate_with_shape: true,
        tile: [0.0; 4],
    });
    let head = deckcraft_model::style::LineEnd { kind: "triangle".into(), w: "med".into(), len: "med".into() };
    let sh = Shape {
        id: ShapeId(50),
        xfrm: Some(Xfrm::new(100.0, 100.0, 300.0, 0.0)),
        geom: deckcraft_model::Geom::preset("line"),
        line: Some(Line { fill: Some(grad), width: Some(12.0), tail: Some(head), ..Default::default() }),
        ..Default::default()
    };
    assert_same("gradient line", &deck_with(vec![sh]), 0);
}
