//! The Cameo live preview: owns the host-injected camera session, drains its bounded event
//! channel once per pass, uploads the latest frame as a texture, and paints it in every cameo on
//! the canvas and in the slide show.
//!
//! The preview is off until the user asks for it (Camera Format ▸ Enable Camera Preview, or
//! Record ▸ Cameo). Enumerating devices is what triggers the macOS permission sheet, so nothing
//! here touches the camera before that: the injected [`CameraIn`] is inert until `start`.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use deckcraft_geom::{Point, Xfrm};
use deckcraft_media::camera::{
    CameraDesc, CameraError, CameraEvent, CameraFormat, CameraFrame, CameraId, CameraIn, CameraRequest, CameraSession, CameraState, CameraStatus,
};
use deckcraft_model::{Presentation, Shape, ShapeId};
use deckcraft_render::ShapeState;
use egui::{Color32, ColorImage, Mesh, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};

use crate::SlideApp;

/// While the worker opens the camera or waits for permission, poll its state (no frames yet).
const STARTING_REPAINT: Duration = Duration::from_millis(100);
/// Live preview drain/upload cadence. The producer drops stale frames; this is not a frame clock.
const LIVE_REPAINT: Duration = Duration::from_millis(16);

/// The camera session and the texture made from its latest frame.
pub struct CameraHost {
    /// `None` on the web, in tests, and when `DECKCRAFT_NO_CAMERA` is set.
    source: Option<Box<dyn CameraIn>>,
    session: Option<CameraSession>,
    status: CameraStatus,
    request: CameraRequest,
    /// The most recent frame, kept so the canvas and the show draw the same picture in one pass.
    frame: Option<Arc<CameraFrame>>,
    /// The uploaded frame's sequence and its texture.
    texture: Option<(u64, TextureHandle)>,
    /// The Cameo Preview toggle in the Camera Format tab.
    preview: bool,
    /// The running show's identity and current slide, so a new show — or an explicit jump to a
    /// hidden slide — recomputes whether it has a cameo.
    show_instance: Option<(u64, usize)>,
    /// The running show contains at least one cameo, so the camera is live on it.
    show_wants: bool,
    /// The last value of `preview || show_wants` a start/stop acted on.
    wanted: bool,
    /// The editor overlay of shapes stacked above a Cameo (repainted over the live frame so the
    /// preview keeps slide z-order): slide identity, texture size, skipped ids, grayscale, GIF
    /// frames, edited shape, texture.
    over: Option<(usize, (u32, u32), Vec<ShapeId>, bool, u64, Option<ShapeId>, TextureHandle)>,
    /// A one-shot explanation for the status bar, taken by the logic pass.
    notice: Option<String>,
}

impl Default for CameraHost {
    fn default() -> Self {
        CameraHost {
            source: None,
            session: None,
            status: CameraStatus::Off,
            request: CameraRequest::default(),
            frame: None,
            texture: None,
            preview: false,
            show_instance: None,
            show_wants: false,
            wanted: false,
            over: None,
            notice: None,
        }
    }
}

impl CameraHost {
    pub fn new(source: Option<Box<dyn CameraIn>>) -> CameraHost {
        CameraHost { source, ..Default::default() }
    }

    /// Whether the host injected a camera backend at all.
    pub fn available(&self) -> bool {
        self.source.is_some()
    }

    /// Whether the user turned the Cameo preview on.
    pub fn preview_on(&self) -> bool {
        self.preview
    }

    pub fn status(&self) -> &CameraStatus {
        &self.status
    }

    pub fn frame(&self) -> Option<&Arc<CameraFrame>> {
        self.frame.as_ref()
    }

    pub fn texture(&self) -> Option<&TextureHandle> {
        self.texture.as_ref().map(|(_, t)| t)
    }

    /// The show whose camera need is already known; the logic pass compares this to the live
    /// show's identity and slide before cloning the presentation.
    pub(crate) fn show_instance(&self) -> Option<(u64, usize)> {
        self.show_instance
    }

    /// A sentence for a tooltip or the status bar.
    pub fn status_text(&self) -> String {
        match &self.status {
            CameraStatus::Off if !self.available() => "Camera capture is not available".into(),
            CameraStatus::Off => "Camera preview is off".into(),
            CameraStatus::Starting => "Opening the camera…".into(),
            CameraStatus::Live(info) => format!("Live camera {}x{} at {:.0} fps", info.width, info.height, info.fps),
            CameraStatus::Failed(error) => error_text(error),
        }
    }

    /// Turn the Cameo preview on or off. Turning it on is the only place a device is enumerated.
    pub fn set_preview(&mut self, on: bool) {
        if self.preview == on {
            return;
        }
        self.preview = on;
        self.refresh_want();
    }

    /// Turn the Cameo preview on or off. Terminal ends and startup failures leave the toggle
    /// off, so this is always a plain switch and the next click starts a fresh attempt.
    pub fn toggle_preview(&mut self) {
        self.set_preview(!self.preview);
    }

    /// The preview's state as one of the eight names the control channel reports.
    pub fn state(&self) -> CameraState {
        self.status.state()
    }

    /// The camera the next session asks for. `None` is the platform default.
    pub fn device(&self) -> Option<&CameraId> {
        self.request.device.as_ref()
    }

    /// Choose a camera (the Camera Format tab's device menu). A running session restarts with the
    /// new device; an off preview remembers the choice for the next start.
    pub fn set_device(&mut self, device: Option<CameraId>) {
        if self.request.device == device {
            return;
        }
        self.request.device = device;
        self.restart();
    }

    /// The format a new session asks for. `None` lets the backend choose its closest default.
    pub fn format(&self) -> Option<CameraFormat> {
        self.request.format
    }

    /// Request a format. A running session restarts, because a capture stream cannot reconfigure
    /// itself in place.
    pub fn set_format(&mut self, format: Option<CameraFormat>) {
        if self.request.format == format {
            return;
        }
        self.request.format = format;
        self.restart();
    }

    /// Enumerate cameras on the backend's worker. The Camera Format tab drains the receiver with
    /// `try_recv` on later passes; the enumeration itself never runs on the UI thread.
    pub fn devices(&mut self) -> Receiver<Result<Vec<CameraDesc>, CameraError>> {
        match self.source.as_mut() {
            Some(source) => source.devices(),
            None => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(Err(CameraError::Unsupported));
                rx
            }
        }
    }

    /// Tell the preview which show is running and which slide it shows. Only that slide's Cameos
    /// count: normal navigation skips hidden slides, but an explicit jump can land on one, and a
    /// Cameo only there must not start the camera. A show that contains a cameo wants the camera
    /// even when the editor's preview toggle is off, as in PowerPoint's slide show.
    pub fn set_show(&mut self, instance: Option<(u64, usize)>, doc: Option<&Presentation>) {
        if self.show_instance == instance {
            return;
        }
        self.show_instance = instance;
        self.show_wants = match (instance, doc) {
            (Some((_, slide)), Some(doc)) => doc.slides.get(slide).is_some_and(|s| !s.hidden && !cameo_boxes(&s.shapes).is_empty()),
            _ => false,
        };
        self.refresh_want();
    }

    /// Drain events and upload the newest frame. Returns a one-shot message for the status bar.
    /// Call once per logic pass; it never blocks.
    pub fn tick(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut ended = false;
        if let Some(session) = self.session.as_ref() {
            loop {
                match session.events.try_recv() {
                    Ok(CameraEvent::Opened(info)) => self.status = CameraStatus::Live(info),
                    Ok(CameraEvent::Frame(frame)) => self.frame = Some(Arc::new(frame)),
                    Ok(CameraEvent::Failed(error)) => {
                        self.notice = Some(error_text(&error));
                        self.status = CameraStatus::Failed(error);
                        ended = true;
                        break;
                    }
                    Ok(CameraEvent::Closed) => {
                        if !matches!(self.status, CameraStatus::Failed(_)) {
                            self.status = CameraStatus::Off;
                        }
                        ended = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if !matches!(self.status, CameraStatus::Failed(_)) {
                            self.status = CameraStatus::Off;
                        }
                        ended = true;
                        break;
                    }
                }
            }
        }
        if ended {
            self.session = None;
            self.frame = None;
            self.texture = None;
            // A terminal end leaves the preview off, not stranded on: the toggle and its tooltip
            // stay honest, and the next click starts a fresh attempt from the off state.
            self.preview = false;
            self.wanted = false;
        }
        if let Some(frame) = self.frame.clone()
            && self.visible()
            && self.texture.as_ref().is_none_or(|(seq, _)| *seq != frame.seq)
        {
            self.upload(ctx, &frame);
        }
        match &self.status {
            CameraStatus::Starting => ctx.request_repaint_after(STARTING_REPAINT),
            CameraStatus::Live(_) if self.visible() => ctx.request_repaint_after(LIVE_REPAINT),
            _ => {}
        }
        self.notice.take()
    }

    fn visible(&self) -> bool {
        self.preview || self.show_wants
    }

    /// Reopen the camera after a device or format change, if a session is wanted.
    fn restart(&mut self) {
        if !self.wanted {
            return;
        }
        self.stop();
        self.start();
    }

    fn refresh_want(&mut self) {
        let want = self.visible();
        if want == self.wanted {
            return;
        }
        self.wanted = want;
        if want { self.start() } else { self.stop() }
    }

    fn start(&mut self) {
        self.frame = None;
        self.texture = None;
        self.status = CameraStatus::Starting;
        let Some(source) = self.source.as_mut() else {
            self.status = CameraStatus::Failed(CameraError::Unsupported);
            self.notice = Some(error_text(&CameraError::Unsupported));
            self.preview = false;
            self.wanted = false;
            return;
        };
        match source.start(&self.request) {
            Ok(session) => self.session = Some(session),
            Err(error) => {
                self.notice = Some(error_text(&error));
                self.status = CameraStatus::Failed(error);
                self.preview = false;
                self.wanted = false;
            }
        }
    }

    fn stop(&mut self) {
        if let Some(source) = self.source.as_mut() {
            source.stop();
        }
        self.session = None;
        self.status = CameraStatus::Off;
        self.frame = None;
        self.texture = None;
    }

    fn upload(&mut self, ctx: &egui::Context, frame: &CameraFrame) {
        let Some(image) = color_image(frame) else { return };
        self.texture = Some(match self.texture.take() {
            Some((_, mut texture)) => {
                texture.set(image, TextureOptions::LINEAR);
                (frame.seq, texture)
            }
            None => (frame.seq, ctx.load_texture("cameo-preview", image, TextureOptions::LINEAR)),
        });
    }
}

/// A sentence for an error, with the permission fix where the platform has one.
pub fn error_text(error: &CameraError) -> String {
    match error {
        CameraError::NoDevice => "No camera was found. Connect one and turn the preview on again.".into(),
        CameraError::PermissionDenied => format!("Camera access is denied. {}", permission_hint()),
        CameraError::InUse => "Another application is using the camera. Close it and try again.".into(),
        CameraError::UnsupportedFormat => "The camera cannot produce the requested format.".into(),
        CameraError::Lost => "The camera was disconnected. Reconnect it and turn the preview on again.".into(),
        CameraError::Unsupported => "Camera capture is not available on this platform.".into(),
        CameraError::Backend(message) => format!("The camera failed: {message}"),
    }
}

fn permission_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "In Settings > Privacy & security > Camera, let desktop apps use the camera."
    } else if cfg!(target_os = "macos") {
        "In System Settings > Privacy & Security > Camera, allow DeckCraft."
    } else {
        "Check that your user can open the camera device."
    }
}

/// The frame as an RGBA texture image. `None` for a malformed or empty frame.
fn color_image(frame: &CameraFrame) -> Option<ColorImage> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let count = width.checked_mul(height)?;
    let bytes = count.checked_mul(4)?;
    if count == 0 || frame.rgba.len() < bytes {
        return None;
    }
    let pixels = frame.rgba[..bytes].as_chunks::<4>().0.iter().map(|c| Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3])).collect();
    Some(ColorImage::new([width, height], pixels))
}

/// Paint the live frame in every cameo of editor slide `slide`, in slide coordinates through `xf`.
pub(crate) fn paint_preview(app: &mut SlideApp, painter: &egui::Painter, xf: crate::canvas::Xf, ctx: &egui::Context) {
    if !app.camera.preview_on() {
        app.camera.over = None;
        return;
    }
    let Some(texture) = app.camera.texture().cloned() else { return };
    let (doc, idx, ptr, editing) = match app.session.active() {
        Some(d) if d.selection.target == deckcraft_engine::Target::Slides => {
            let ptr = d.doc.slides.get(d.selection.slide).map(|s| std::sync::Arc::as_ptr(s) as usize);
            let editing = d.selection.text.as_ref().filter(|t| !t.notes).map(|t| t.shape);
            (d.doc.clone(), d.selection.slide, ptr, editing)
        }
        _ => return,
    };
    let Some(ptr) = ptr else { return };
    let Some(slide) = doc.slides.get(idx) else { return };
    // The animation preview can hide, move, scale or fade a Cameo: the live frame follows the
    // same state the slide texture uses, or it would show before an entrance or after an exit.
    let frame = app.anim_preview_draw();
    for (shape, x) in cameo_boxes(&slide.shapes) {
        let st = frame.as_ref().map(|(tl, t)| crate::canvas::preview::shape_state_at(tl, shape.id, *t));
        if st.as_ref().is_some_and(|s| !s.visible || s.opacity <= 0.0) {
            continue;
        }
        let (opacity, to_screen): (f64, Box<dyn Fn(Point) -> Pos2>) = match &st {
            Some(s) => {
                // Shape-local animation first, then the slide mapping, then the slide-space
                // offset: the renderer composes `translate(offset) * shape * around`, so a
                // flipped Cameo's spin and shift both turn with it.
                let centre = Point::new(x.w / 2.0, x.h / 2.0);
                (s.opacity, Box::new(move |p: Point| xf.to_screen(x.affine() * animated(p, centre, s) + s.offset)))
            }
            None => (1.0, Box::new(move |p: Point| xf.to_screen(x.affine() * p))),
        };
        paint_cameo(painter, &texture, shape, x, opacity, &to_screen);
    }
    paint_preview_overlay(app, OverlayPaint { painter, xf, doc: &doc, idx, ptr, editing, ctx });
}

/// Repaint the shapes stacked above a Cameo over its live frame, so the editor preview keeps
/// slide z-order as the show does (`below_live` plus its overlay texture): without this, a text
/// box over a Cameo would vanish while the preview is on. The overlay renders the slide's own
/// shapes with everything at and below the lowest live Cameo skipped, at the slide texture's
/// pixel size so it lands texel-aligned, and is cached on the slide identity, size and skip list
/// (re-rendered while the animation preview moves shapes).
struct OverlayPaint<'a> {
    painter: &'a egui::Painter,
    xf: crate::canvas::Xf,
    doc: &'a Presentation,
    idx: usize,
    ptr: usize,
    editing: Option<ShapeId>,
    ctx: &'a egui::Context,
}

fn paint_preview_overlay(app: &mut SlideApp, o: OverlayPaint<'_>) {
    let OverlayPaint { painter, xf, doc, idx, ptr, editing, ctx } = o;
    let live: Vec<ShapeId> = cameo_boxes(&doc.slides[idx].shapes).into_iter().map(|(s, _)| s.id).collect();
    let skip = crate::show::render::below_live(doc, idx, &live);
    if skip.is_empty() {
        app.camera.over = None;
        return;
    }
    let moving = app.anim_preview_draw().is_some();
    let Some(rect) = app.slide_rect else { return };
    let ppp = ctx.pixels_per_point();
    let full = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    // The slide texture avoids a picture the GPU cannot hold (`fit_texture`/`max_side`); the
    // overlay renders at the same size so it still lands texel-aligned with it.
    let size = crate::textures::fit_size(full, crate::textures::max_side(ctx));
    let gray = app.ui.grayscale;
    // A playing GIF above the Cameo must not freeze at its first frame in the overlay: the slide
    // texture keys on the GIF frames, so the overlay renders with the same times and keys on them.
    let now = ctx.input(|i| i.time);
    let gifs = app.session.active().map(|d| app.media.gifs(d.uid, &d.doc, idx, now, &app.session.gif_paused)).unwrap_or_default();
    let times: Vec<(ShapeId, f64)> = gifs.iter().map(|g| (g.id, g.t)).collect();
    let gkey = crate::media::gif_key(&gifs);
    // The shape being edited draws its text through the UI, not the texture: the overlay must
    // skip it exactly when the slide texture does, or stale text doubles under the live edit.
    let fresh = app.camera.over.as_ref().is_some_and(|o| o.0 == ptr && o.1 == size && o.2 == skip && o.3 == gray && o.4 == gkey && o.5 == editing);
    if !fresh || moving {
        let frame = app.anim_preview_draw();
        let state_of = |id: ShapeId| frame.as_ref().map(|(tl, t)| crate::canvas::preview::shape_state_at(tl, id, *t)).unwrap_or_default();
        let state: Option<&dyn Fn(ShapeId) -> deckcraft_render::ShapeState> = Some(&state_of);
        let opts = deckcraft_render::RenderOpts {
            scale: size.0 as f64 / doc.slide_size.width.max(1.0),
            edit: true,
            size: Some(size),
            state,
            skip: &skip,
            editing_text: editing,
            gif_times: &times,
            threads: crate::show::render::render_threads(),
            ..Default::default()
        };
        let img = deckcraft_render::render_slide_part(doc, idx, deckcraft_render::SlidePart::Content, &opts);
        let ci = crate::textures::to_color_image(&img, gray);
        let same_slot =
            app.camera.over.as_ref().is_some_and(|o| o.0 == ptr && o.1 == size && o.2 == skip && o.3 == gray && o.4 == gkey && o.5 == editing);
        if same_slot {
            if let Some((_, _, _, _, _, _, tex)) = app.camera.over.as_mut() {
                crate::textures::replace_editor(ctx, tex, ci);
            }
        } else {
            app.camera.over = Some((ptr, size, skip.clone(), gray, gkey, editing, crate::textures::upload_editor(ctx, "cameo-overlay", ci)));
        }
    }
    if app.camera.over.is_some() {
        let w = doc.slide_size.width as f32 * xf.scale;
        let h = doc.slide_size.height as f32 * xf.scale;
        let tex = app.camera.over.as_ref().map(|(_, _, _, _, _, _, t)| t.clone());
        if let Some(tex) = tex {
            painter.image(tex.id(), Rect::from_min_size(xf.origin, vec2(w, h)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}

/// Where a show paints a Cameo's live frame: the slide's rect on screen, the zoom window, the
/// media hidden until it plays, and the show's animation state (a Cameo enters, leaves, moves and
/// fades like any other shape). `painter` must be clipped to `srect`, so a Cameo that hangs over
/// the slide's edge (or out of the zoom window) cannot paint outside it.
pub(crate) struct ShowFrame<'a> {
    pub srect: Rect,
    pub uv: Rect,
    pub hidden: &'a [ShapeId],
    pub state: &'a dyn Fn(ShapeId) -> ShapeState,
}

pub(crate) fn paint_show(painter: &egui::Painter, texture: &TextureHandle, doc: &Presentation, slide: usize, frame: ShowFrame<'_>) {
    let ShowFrame { srect, uv, hidden, state } = frame;
    let Some(slide) = doc.slides.get(slide) else { return };
    let (sw, sh) = (doc.slide_size.width as f32, doc.slide_size.height as f32);
    for (shape, x) in cameo_boxes(&slide.shapes) {
        let st = state(shape.id);
        let Some(opacity) = live_opacity(hidden, shape.id, &st) else { continue };
        // Shape-local animation first, then the slide mapping (with the zoom window), then the
        // slide-space offset: the renderer composes `translate(offset) * shape * around`.
        let centre = Point::new(x.w / 2.0, x.h / 2.0);
        let to_screen = |p: Point| {
            let p = x.affine() * animated(p, centre, &st) + st.offset;
            slide_to_screen(srect, uv, p.x as f32 / sw.max(1.0), p.y as f32 / sh.max(1.0))
        };
        paint_cameo(painter, texture, shape, x, opacity, &to_screen);
    }
}

/// The opacity a live frame is painted at, or `None` when the show does not show the Cameo: media
/// hidden until it plays (`hidden`), or the animation's own visibility and fade.
pub(crate) fn live_opacity(hidden: &[ShapeId], id: ShapeId, st: &ShapeState) -> Option<f64> {
    if hidden.contains(&id) || !st.visible || st.opacity <= 0.0 {
        return None;
    }
    Some(st.opacity)
}

/// The Cameos of show slide `slide`, in stack order: the shapes the UI paints live, so the ones
/// stacked above them go into the overlay that is drawn over their frames.
pub(crate) fn cameo_ids(doc: &Presentation, slide: usize) -> Vec<ShapeId> {
    doc.slides.get(slide).map(|s| cameo_boxes(&s.shapes).into_iter().map(|(shape, _)| shape.id).collect()).unwrap_or_default()
}

/// `p` after an animation state in shape-local space: scaled and turned about `centre` (shape
/// points, usually the box centre), without the effect's offset. The caller adds `st.offset`
/// after the shape's own transform, matching the slide renderer (`parent * translate(offset) *
/// shape * around`): an offset is slide space, so a flipped Cameo must not mirror it.
pub(crate) fn animated(p: Point, centre: Point, st: &ShapeState) -> Point {
    let d = p - centre;
    let (x, y) = (d.x * st.scale.0, d.y * st.scale.1);
    let a = st.rotate.to_radians();
    let (sin, cos) = (a.sin(), a.cos());
    Point::new(centre.x + x * cos - y * sin, centre.y + x * sin + y * cos)
}

/// Cameos on a slide with their slide-space boxes, including cameos inside groups. Hidden Cameos
/// and Cameos inside a hidden group are skipped, matching the renderer: a hidden Cameo must not
/// paint its live frame in the editor or the show (see `placed::place`).
pub(crate) fn cameo_boxes(shapes: &[deckcraft_model::Shape]) -> Vec<(&deckcraft_model::Shape, deckcraft_geom::Xfrm)> {
    let mut pictures = vec![];
    crate::media::collect_pictures(shapes, None, false, 0, &mut pictures);
    pictures.into_iter().filter(|(shape, _)| shape.is_cameo()).collect()
}

fn slide_to_screen(srect: Rect, uv: Rect, nx: f32, ny: f32) -> Pos2 {
    let u = (nx - uv.min.x) / uv.width().max(1e-4);
    let v = (ny - uv.min.y) / uv.height().max(1e-4);
    pos2(srect.min.x + u * srect.width(), srect.min.y + v * srect.height())
}

/// Paint `texture` in one Cameo: masked to its Camera Shape, turned with the shape and crop-filled
/// so a camera fills its frame instead of stretching. `opacity` is the show's animation fade.
fn paint_cameo(painter: &egui::Painter, texture: &TextureHandle, shape: &Shape, x: Xfrm, opacity: f64, to_screen: &dyn Fn(Point) -> Pos2) {
    let polygons = cameo_polygons(shape, x.w, x.h);
    if polygons.is_empty() {
        return;
    }
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let crop = match &shape.kind {
        deckcraft_model::ShapeKind::Picture { fill } => fill.crop,
        _ => [0.0; 4],
    };
    let mesh = cameo_mesh(&polygons, x, crop, texture.id(), texture.size(), tint, to_screen);
    if mesh.is_empty() {
        return;
    }
    // A dark backing over the same triangles, so a frame that arrives with transparency or not at
    // all still reads as a camera and never covers the slide outside the Camera Shape. `PathShape`
    // fill would only be correct for convex outlines, and Camera Shape offers concave presets.
    let mut backing = Mesh::default();
    backing.indices.clone_from(&mesh.indices);
    backing.vertices =
        mesh.vertices.iter().map(|v| egui::epaint::Vertex { pos: v.pos, uv: Pos2::ZERO, color: Color32::from_black_alpha(tint.a()) }).collect();
    painter.add(egui::Shape::mesh(backing));
    painter.add(egui::Shape::mesh(mesh));
}

/// [`paint_cameo`], but the caller clips the painter: the caller owns clipping because a Cameo
/// may hang over the slide's edge or out of the zoom window.
pub(crate) fn paint_cameo_clipped(
    painter: &egui::Painter,
    texture: &TextureHandle,
    shape: &Shape,
    x: Xfrm,
    opacity: f64,
    to_screen: &dyn Fn(Point) -> Pos2,
) {
    paint_cameo(painter, texture, shape, x, opacity, to_screen);
}

/// The filled outline of `shape` as simple polygons in shape-local space (`0..w`, `0..h`),
/// flattened, so a curved Camera Shape masks the frame instead of showing its box's corners.
fn cameo_polygons(shape: &Shape, w: f64, h: f64) -> Vec<Vec<Point>> {
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    let mut out: Vec<Vec<Point>> = vec![];
    for sub in deckcraft_render::shape_geometry(shape, w, h).paths.iter().filter(|p| p.fill != deckcraft_geom::preset::FillMode::None) {
        let mut cur: Vec<Point> = vec![];
        deckcraft_geom::preset_flatten(&sub.path, &mut |el| match el {
            deckcraft_geom::PathEl::MoveTo(p) => {
                push_polygon(&mut cur, &mut out);
                cur.push(p);
            }
            deckcraft_geom::PathEl::LineTo(p) => cur.push(p),
            deckcraft_geom::PathEl::ClosePath => push_polygon(&mut cur, &mut out),
            _ => {}
        });
        push_polygon(&mut cur, &mut out);
    }
    out
}

fn push_polygon(cur: &mut Vec<Point>, out: &mut Vec<Vec<Point>>) {
    if cur.len() > 2 {
        out.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
}

/// A textured mesh that covers `polygons` (shape-local) with the frame: the caller owns the
/// shape transform (flips, rotation, animation) inside `to_screen`, so points arrive here in
/// shape-local space. UVs still index the shape box, and [`PictureFill::crop`] stretches its
/// selection over the box as PowerPoint shows a cropped video (plus the centre crop from
/// [`cover_uv`] for the rest).
fn cameo_mesh(
    polygons: &[Vec<Point>],
    x: Xfrm,
    crop: [f64; 4],
    tex: egui::TextureId,
    tex_size: [usize; 2],
    tint: Color32,
    to_screen: &dyn Fn(Point) -> Pos2,
) -> Mesh {
    let uv = cover_uv(Rect::from_min_size(Pos2::ZERO, vec2(x.w as f32, x.h as f32)), tex_size);
    let [l, t, r, b] = crop.map(|v| if v.is_finite() { v.clamp(0.0, 0.99) as f32 } else { 0.0 });
    let cropped = crop.iter().any(|v| *v > 0.0);
    let uv_at = |p: Point| {
        let mut u = (p.x / x.w) as f32;
        let mut v = (p.y / x.h) as f32;
        if cropped {
            u = l + u * ((1.0 - r).max(l + 0.01) - l);
            v = t + v * ((1.0 - b).max(t + 0.01) - t);
        }
        pos2(uv.min.x + u * uv.width(), uv.min.y + v * uv.height())
    };
    let mut mesh = Mesh::with_texture(tex);
    for poly in polygons {
        let points: Vec<Pos2> = poly.iter().map(|p| to_screen(*p)).collect();
        if points.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for (p, at) in poly.iter().zip(&points) {
            mesh.vertices.push(egui::epaint::Vertex { pos: *at, uv: uv_at(*p), color: tint });
        }
        for [a, b, c] in triangulate_polygon(&points) {
            mesh.add_triangle(base + a, base + b, base + c);
        }
    }
    mesh
}

/// Triangles for one simple polygon in screen space: a fan when it is convex (the usual Camera
/// Shapes), ear clipping otherwise.
fn triangulate_polygon(points: &[Pos2]) -> Vec<[u32; 3]> {
    let n = points.len();
    if n < 3 {
        return vec![];
    }
    if is_convex(points) {
        return (1..n as u32 - 1).map(|i| [0, i, i + 1]).collect();
    }
    crate::icons::triangulate(points)
}

fn is_convex(points: &[Pos2]) -> bool {
    let n = points.len();
    let mut sign = 0.0_f32;
    for i in 0..n {
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross;
        } else if (cross > 0.0) != (sign > 0.0) {
            return false;
        }
    }
    sign != 0.0
}

/// Normalised texture rectangle that covers `rect` without stretching (centre crop).
fn cover_uv(rect: Rect, tex: [usize; 2]) -> Rect {
    let tex_aspect = tex[0].max(1) as f32 / tex[1].max(1) as f32;
    let rect_aspect = rect.width() / rect.height().max(1.0);
    if tex_aspect > rect_aspect {
        let w = (rect_aspect / tex_aspect).clamp(0.0, 1.0);
        let x = (1.0 - w) * 0.5;
        Rect::from_min_max(pos2(x, 0.0), pos2(x + w, 1.0))
    } else {
        let h = (tex_aspect / rect_aspect).clamp(0.0, 1.0);
        let y = (1.0 - h) * 0.5;
        Rect::from_min_max(pos2(0.0, y), pos2(1.0, y + h))
    }
}

/// The camera API the Camera Format section and the Record tab use.
impl SlideApp {
    pub fn camera_available(&self) -> bool {
        self.camera.available()
    }

    pub fn camera_preview_on(&self) -> bool {
        self.camera.preview_on()
    }

    pub fn camera_status(&self) -> &CameraStatus {
        self.camera.status()
    }

    pub fn camera_status_text(&self) -> String {
        self.camera.status_text()
    }

    pub fn camera_frame(&self) -> Option<&Arc<CameraFrame>> {
        self.camera.frame()
    }

    pub fn set_camera_preview(&mut self, on: bool) {
        self.camera.set_preview(on);
    }

    pub fn toggle_camera_preview(&mut self) {
        self.camera.toggle_preview();
    }

    pub fn camera_state(&self) -> CameraState {
        self.camera.state()
    }

    pub fn camera_device(&self) -> Option<&CameraId> {
        self.camera.device()
    }

    pub fn set_camera_device(&mut self, device: Option<CameraId>) {
        self.camera.set_device(device);
    }

    pub fn set_camera_format(&mut self, format: Option<CameraFormat>) {
        self.camera.set_format(format);
    }

    pub fn camera_devices(&mut self) -> Receiver<Result<Vec<CameraDesc>, CameraError>> {
        self.camera.devices()
    }
}

/// Start (or keep) the camera session the current show needs, if `current` names a new show or
/// slide. The slide matters: an explicit jump can land on a hidden slide whose Cameo must not
/// start the camera.
pub(crate) fn wants_show_camera(app: &mut SlideApp, current: Option<(u64, usize)>) {
    if app.camera.show_instance() == current {
        return;
    }
    let doc = app.session.active().map(|d| d.doc.clone());
    app.camera.set_show(current, doc.as_deref());
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use deckcraft_media::camera::testing::FakeCamera;

    fn started(fake: &FakeCamera) -> CameraHost {
        let mut host = CameraHost::new(Some(Box::new(fake.clone())));
        host.set_preview(true);
        host
    }

    #[test]
    fn enabling_the_preview_starts_once_and_reports_live() {
        let fake = FakeCamera::default().sized(64, 48);
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        assert_eq!(fake.starts(), 1);
        assert_eq!(host.status(), &CameraStatus::Starting);
        assert!(host.tick(&ctx).is_none());
        assert!(host.status().is_live(), "{:?}", host.status());
        assert_eq!(host.state(), CameraState::Live);
        let frame = host.frame().expect("first frame");
        assert_eq!((frame.width, frame.height), (64, 48));
        assert_eq!(frame.rgba.len(), 64 * 48 * 4);
        assert!(host.texture().is_some());
    }

    #[test]
    fn a_failed_session_leaves_the_preview_off_for_a_fresh_retry() {
        let fake = FakeCamera::default().sized(64, 48);
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        assert!(host.status().is_live());
        fake.fail(CameraError::Lost);
        host.tick(&ctx);
        assert!(matches!(host.status(), CameraStatus::Failed(_)), "{:?}", host.status());
        assert!(!host.preview_on(), "a terminal failure leaves the toggle off");
        host.toggle_preview();
        assert!(host.preview_on(), "the next click starts a fresh attempt");
        assert_eq!(fake.starts(), 2, "the session restarts");
    }

    #[test]
    fn the_channel_is_bounded_and_the_host_shows_the_newest_delivered_frame() {
        let fake = FakeCamera::default().sized(8, 8).frames_on_start(0);
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        // Ten frames, two buffered slots: the burst cannot grow the backlog.
        for _ in 0..10 {
            fake.push_frame();
        }
        host.tick(&ctx);
        let shown = host.frame().expect("a frame");
        assert_eq!(shown.seq, 1, "only the first two frames fit; the newest buffered is seq 1");
        // Draining makes room for the next two.
        fake.push_frame();
        fake.push_frame();
        host.tick(&ctx);
        assert_eq!(host.frame().expect("a frame").seq, 11);
    }

    #[test]
    fn start_keeps_the_whole_initial_burst() {
        // `Opened`, one queued event and three configured frames: all five must arrive, not just
        // the two a fixed capacity-2 channel would hold.
        use deckcraft_media::camera::{CameraEvent, CameraRequest};
        let fake = FakeCamera::default().frames_on_start(3);
        fake.queue(CameraEvent::Closed);
        let mut fake = fake;
        let session = fake.start(&CameraRequest::default()).expect("start");
        let mut events = vec![];
        while let Ok(e) = session.events.try_recv() {
            events.push(e);
        }
        assert_eq!(events.len(), 5, "Opened + queued + 3 frames: {events:?}");
        assert!(matches!(events[0], CameraEvent::Opened(_)));
        assert!(matches!(events[1], CameraEvent::Closed));
        assert_eq!(events.iter().filter(|e| matches!(e, CameraEvent::Frame(_))).count(), 3);
    }

    #[test]
    fn turning_the_preview_off_stops_the_session_and_forgets_the_frame() {
        let fake = FakeCamera::default();
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        assert!(host.frame().is_some());
        host.set_preview(false);
        assert_eq!(fake.stops(), 1);
        assert_eq!(host.status(), &CameraStatus::Off);
        assert!(host.frame().is_none());
        assert!(host.texture().is_none());
    }

    #[test]
    fn a_denied_start_reports_the_reason_and_a_retry_can_succeed() {
        let fake = FakeCamera::default().fail_start(CameraError::PermissionDenied);
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        assert_eq!(host.status(), &CameraStatus::Failed(CameraError::PermissionDenied));
        assert_eq!(host.state(), CameraState::Denied);
        let notice = host.tick(&ctx).expect("a notice");
        assert!(notice.contains("denied"), "{notice}");
        // Off and on asks again, which is what the user does after fixing the setting.
        host.set_preview(false);
        host.set_preview(true);
        assert_eq!(fake.starts(), 2);
        host.tick(&ctx);
        assert!(host.status().is_live(), "{:?}", host.status());
    }

    #[test]
    fn a_failure_mid_stream_ends_the_session_and_keeps_the_reason() {
        let fake = FakeCamera::default();
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        fake.fail(CameraError::InUse);
        let notice = host.tick(&ctx).expect("a notice");
        assert!(notice.contains("using the camera"), "{notice}");
        assert_eq!(host.status(), &CameraStatus::Failed(CameraError::InUse));
        assert!(host.frame().is_none());
        // No restart from the drain itself.
        assert_eq!(fake.starts(), 1);
    }

    #[test]
    fn a_clean_close_returns_to_off() {
        let fake = FakeCamera::default();
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        fake.finish();
        assert!(host.tick(&ctx).is_none());
        assert_eq!(host.status(), &CameraStatus::Off);
        assert!(host.frame().is_none());
        assert!(!host.preview_on(), "a clean close leaves the toggle off");
        // The next click starts a fresh attempt from the off state.
        host.toggle_preview();
        assert!(host.preview_on(), "the next click starts a fresh attempt");
        assert_eq!(fake.starts(), 2, "the session reopens");
    }

    #[test]
    fn without_a_backend_the_toggle_reports_unsupported() {
        let ctx = egui::Context::default();
        let mut host = CameraHost::new(None);
        assert!(!host.available());
        host.set_preview(true);
        assert_eq!(host.status(), &CameraStatus::Failed(CameraError::Unsupported));
        assert!(host.tick(&ctx).expect("notice").contains("not available"));
        assert!(host.texture().is_none());
    }

    #[test]
    fn choosing_another_device_restarts_a_running_session() {
        let fake = FakeCamera::default().frames_on_start(0);
        let ctx = egui::Context::default();
        let mut host = started(&fake);
        host.tick(&ctx);
        host.set_device(Some("other-camera".into()));
        assert_eq!(host.device().map(String::as_str), Some("other-camera"));
        assert_eq!(fake.starts(), 2);
        assert_eq!(fake.stops(), 1);
        // The same device again changes nothing.
        host.set_device(Some("other-camera".into()));
        assert_eq!(fake.starts(), 2);
    }

    #[test]
    fn a_show_with_a_cameo_wants_the_camera_and_ending_it_does_not() {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        let doc = session.active().expect("document").doc.clone();
        let fake = FakeCamera::default();
        let mut host = CameraHost::new(Some(Box::new(fake.clone())));
        host.set_show(Some((1, 0)), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 1);
        assert!(!host.preview_on());
        host.set_show(None, None);
        assert_eq!(fake.stops(), 1);
    }

    #[test]
    fn a_show_without_a_cameo_never_opens_the_camera() {
        let session = deckcraft_engine::Session::with_new();
        let doc = session.active().expect("document").doc.clone();
        let fake = FakeCamera::default();
        let mut host = CameraHost::new(Some(Box::new(fake.clone())));
        host.set_show(Some((1, 0)), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 0);
        assert_eq!(host.status(), &CameraStatus::Off);
    }

    #[test]
    fn a_cameo_on_only_a_hidden_slide_never_opens_the_camera() {
        // Normal show navigation skips hidden slides, so their Cameos never reach the screen:
        // starting a show must not open the camera for them (no indicator, no permission sheet).
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        session.execute("slide.hide", &serde_json::json!({})).expect("hide the only slide");
        let doc = session.active().expect("document").doc.clone();
        assert!(doc.slides[0].hidden, "the slide is hidden");
        let fake = FakeCamera::default();
        let mut host = CameraHost::new(Some(Box::new(fake.clone())));
        host.set_show(Some((1, 0)), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 0, "the camera stays closed on a hidden slide's Cameo");
        assert_eq!(host.status(), &CameraStatus::Off);
    }

    #[test]
    fn jumping_to_a_hidden_slide_does_not_open_the_camera() {
        // An explicit jump can land on a hidden slide; only that slide's Cameos count, so the
        // camera stays closed there and opens again when the show returns to a shown slide.
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &serde_json::json!({})).expect("second slide");
        session.execute("slide.go", &serde_json::json!({"index": 1})).expect("go to the second slide");
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo on the shown slide");
        session.execute("slide.go", &serde_json::json!({"index": 0})).expect("back to the first slide");
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo on the hidden slide");
        session.execute("slide.hide", &serde_json::json!({"index": 0})).expect("hide the first slide");
        let doc = session.active().expect("document").doc.clone();
        assert!(doc.slides[0].hidden && !doc.slides[1].hidden, "only the first slide is hidden");
        let fake = FakeCamera::default();
        let mut host = CameraHost::new(Some(Box::new(fake.clone())));
        host.set_show(Some((1, 1)), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 1, "the shown slide's Cameo opens the camera");
        host.set_show(Some((1, 0)), Some(doc.as_ref()));
        assert_eq!(fake.stops(), 1, "jumping to the hidden slide closes it again");
        assert_eq!(host.status(), &CameraStatus::Off);
        host.set_show(Some((1, 1)), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 2, "returning to the shown slide reopens it");
    }

    #[test]
    fn the_cover_crop_never_stretches_the_frame() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let uv = cover_uv(rect, [1280, 720]);
        assert!((uv.width() - 720.0 / 1280.0).abs() < 1e-5);
        assert_eq!(uv.height(), 1.0);
        let portrait = cover_uv(rect, [480, 640]);
        assert_eq!(portrait.width(), 1.0);
        assert!((portrait.height() - 480.0 / 640.0).abs() < 1e-5);
    }

    /// The live frame is a layer over the slide texture, so it must follow the animation itself:
    /// hidden media, an animation-hidden shape and a zero fade paint nothing, and a fade tints both
    /// the frame and its backing.
    #[test]
    fn only_a_visible_cameo_paints_its_live_frame() {
        let id = ShapeId(7);
        let shown = ShapeState::default();
        assert_eq!(live_opacity(&[], id, &shown), Some(1.0));
        assert_eq!(live_opacity(&[id], id, &shown), None, "media hidden until it plays");
        assert_eq!(live_opacity(&[], id, &ShapeState { visible: false, ..shown.clone() }), None, "before the entrance");
        assert_eq!(live_opacity(&[], id, &ShapeState { opacity: 0.0, ..shown.clone() }), None, "faded out");
        let fading = ShapeState { opacity: 0.4, ..shown };
        assert_eq!(live_opacity(&[], id, &fading), Some(0.4));
    }

    /// An entrance that moves and turns the Cameo moves and turns its frame with it. The offset
    /// is slide space: callers add it after the shape transform (renderer order), and a flipped
    /// Cameo must not mirror it.
    #[test]
    fn an_animated_cameo_paints_its_frame_moved_and_turned() {
        let st = ShapeState { scale: (2.0, 2.0), rotate: 90.0, offset: deckcraft_geom::Vec2::new(5.0, -3.0), ..Default::default() };
        let centre = Point::new(50.0, 50.0);
        // The helper turns shape-local points only; the slide-space offset lands afterwards.
        let turned = animated(Point::new(51.0, 50.0), centre, &st);
        // Scaled to two units right, turned a quarter turn (y is down, so it goes down the
        // slide): (52, 50) — the frame arrives turned, not just faded.
        assert!((turned.x - 52.0).abs() < 1e-9 && (turned.y - 50.0).abs() < 1e-9, "({}, {})", turned.x, turned.y);
        let moved = turned + st.offset;
        assert!((moved.x - 57.0).abs() < 1e-9 && (moved.y - 47.0).abs() < 1e-9, "({}, {})", moved.x, moved.y);
        let none = ShapeState::default();
        assert_eq!(animated(Point::new(51.0, 50.0), centre, &none), Point::new(51.0, 50.0));
    }

    /// A horizontally flipped Cameo with a rotation animation spins the same way as the slide
    /// renderer: the animation applies first in shape-local space, then the shape's own flips
    /// and rotation (`parent * translate(offset) * shape * around` in `renderer.rs`).
    #[test]
    fn a_flipped_cameo_spins_with_the_slide_renderer() {
        use deckcraft_geom::Affine;
        let mut x = Xfrm::new(10.0, 20.0, 100.0, 60.0);
        x.flip_h = true;
        let st = ShapeState { rotate: 90.0, ..Default::default() };
        // Shape-local point one unit below the shape's centre (off the flip's mirror axis, so
        // mirroring visibly changes the spun position); the animation turns points about the
        // shape's own centre, in shape space.
        let local = Point::new(50.0, 31.0);
        let shape_centre = Point::new(x.w / 2.0, x.h / 2.0);
        // The live frame: animation first (shape-local), then the Cameo's own transform.
        let frame = x.affine() * animated(local, shape_centre, &st);
        // The slide renderer: `translate(offset) * shape * around`, with no offset here.
        let a = st.rotate.to_radians();
        let around = Affine::translate(shape_centre.to_vec2()) * Affine::rotate(a) * Affine::translate(-shape_centre.to_vec2());
        let slide = x.affine() * around * local;
        assert!(
            (frame.x - slide.x).abs() < 1e-9 && (frame.y - slide.y).abs() < 1e-9,
            "frame ({}, {}) vs slide ({}, {})",
            frame.x,
            frame.y,
            slide.x,
            slide.y
        );
        // And the flip really matters: without mirroring, the spin would go the other way.
        let mut plain = x;
        plain.flip_h = false;
        let unflipped = plain.affine() * animated(local, shape_centre, &st);
        assert!((frame.x - unflipped.x).abs() > 1.0, "the flip must change the spun position");
    }

    /// The Cameo's box appears once in the ids the show's overlay is built from, so shapes stacked
    /// above it are drawn over the live frame.
    #[test]
    fn the_cameo_ids_of_a_slide_are_its_cameos() {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        let doc = session.active().expect("document").doc.clone();
        let ids = cameo_ids(&doc, 0);
        assert_eq!(ids.len(), 1);
        let shape = doc.slides[0].shape(ids[0]).expect("the Cameo");
        assert!(shape.is_cameo());
        assert!(cameo_ids(&doc, 9).is_empty(), "a slide that does not exist has no Cameos");
    }

    /// A Cameo the Selection pane hides paints no live frame: neither the hidden Cameo itself
    /// nor one inside a hidden group reaches the editor overlay or the show's `cameo_ids`.
    #[test]
    fn hidden_cameos_leave_the_live_overlay() {
        use deckcraft_model::ShapeKind;
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        let doc = session.active().expect("document");
        let id = doc.selected_shapes().first().map(|s| s.id).expect("a selected Cameo");
        session.execute("shape.visible", &serde_json::json!({"id": id.0, "visible": false})).expect("hide");
        let doc = session.active().expect("document").doc.clone();
        assert!(cameo_boxes(&doc.slides[0].shapes).is_empty(), "a hidden Cameo paints no frame");
        assert!(cameo_ids(&doc, 0).is_empty());
        // A Cameo inside a hidden group stays hidden too: the renderer draws nothing below a
        // hidden shape, so the overlay must not paint a frame there either.
        let mut grouped = (*doc.slides[0].shapes.iter().find(|s| s.id == id).expect("the Cameo")).clone();
        grouped.hidden = false;
        let group = Shape {
            id: ShapeId(900),
            name: "group".into(),
            xfrm: grouped.xfrm,
            kind: ShapeKind::Group { children: vec![grouped], child: deckcraft_geom::Xfrm::default() },
            geom: deckcraft_model::Geom::preset("rect"),
            hidden: true,
            ..Shape::default()
        };
        assert!(cameo_boxes(std::slice::from_ref(&group)).is_empty(), "a Cameo in a hidden group paints no frame");
    }

    /// A shape stacked above a Cameo stays visible while the editor preview is on: the canvas
    /// repaints everything above the live frame into the camera overlay (`over`). With nothing
    /// above the Cameo no overlay is needed.
    #[test]
    fn shapes_above_a_cameo_repaint_over_the_preview() {
        fn harness_with(above: bool) -> egui_kittest::Harness<'static, SlideApp> {
            let mut session = deckcraft_engine::Session::with_new();
            session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
            if above {
                session
                    .execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [0.0, 0.0, 200.0, 200.0]}))
                    .expect("a shape above the Cameo");
            }
            let services = crate::Services { camera_in: Some(Box::new(FakeCamera::default().sized(64, 48))), ..crate::Services::default() };
            let mut app = SlideApp::new(session, services);
            app.camera.set_preview(true);
            let mut h = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(5);
            h
        }
        let h = harness_with(true);
        assert!(h.state().camera.texture().is_some(), "the fake camera uploaded a frame");
        assert!(h.state().camera.over.is_some(), "shapes above the Cameo get an overlay");
        let h = harness_with(false);
        assert!(h.state().camera.over.is_none(), "nothing above the Cameo: no overlay");
    }

    /// A Cameo nested in a group keeps the editor's stack order too: a shape above the group is
    /// repainted over the live frame (the overlay covers grouped Cameos, not only top-level ones).
    /// A shape stacked above the Cameo *inside the same group* is a known limitation: the group
    /// paints as one skipped unit (same as the show's `below_live`), so the frame covers that
    /// sibling — matching how the renderer would treat the group's own layer.
    #[test]
    fn a_grouped_cameo_keeps_shapes_above_it_over_the_preview() {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        session.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [300.0, 300.0, 60.0, 60.0]})).expect("buddy");
        let doc = session.active().unwrap().doc.clone();
        let ids: Vec<u32> = doc.slides[0].shapes.iter().map(|s| s.id.0).collect();
        session.execute("arrange.group", &serde_json::json!({"ids": ids})).expect("group");
        session.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [0.0, 0.0, 200.0, 200.0]})).expect("a shape above the group");
        let services = crate::Services { camera_in: Some(Box::new(FakeCamera::default().sized(64, 48))), ..crate::Services::default() };
        let mut app = SlideApp::new(session, services);
        app.camera.set_preview(true);
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(5);
        assert!(h.state().camera.texture().is_some(), "the fake camera uploaded a frame");
        assert!(h.state().camera.over.is_some(), "a shape above the grouped Cameo gets an overlay");
    }

    /// Starting (or ending) a text edit above a Cameo refreshes the camera overlay in the same
    /// session: toggling the edit changes the cache key, so stale text cannot double under the
    /// live edit.
    #[test]
    fn editing_above_a_cameo_refreshes_the_preview_overlay() {
        use deckcraft_engine::TextSel;
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        let above = session
            .execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [0.0, 0.0, 200.0, 200.0]}))
            .expect("a shape above the Cameo");
        let above = deckcraft_model::ShapeId(above["id"].as_u64().unwrap() as u32);
        session.execute("text.set", &serde_json::json!({"id": above.0, "text": "over the Cameo"})).expect("text above the Cameo");
        let services = crate::Services { camera_in: Some(Box::new(FakeCamera::default().sized(64, 48))), ..crate::Services::default() };
        let mut app = SlideApp::new(session, services);
        app.camera.set_preview(true);
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(5);
        assert!(h.state().camera.over.is_some(), "a shape above the Cameo gets an overlay");
        let plain_key = h.state().camera.over.as_ref().map(|o| o.5);
        // Start editing the shape above the Cameo: the overlay must refresh for the edit.
        h.state_mut().session.active_mut().expect("document").selection.text = Some(TextSel { shape: above, ..Default::default() });
        h.run_steps(5);
        assert!(h.state().camera.over.is_some(), "editing above the Cameo keeps an overlay");
        let editing_key = h.state().camera.over.as_ref().map(|o| o.5);
        assert_ne!(plain_key, editing_key, "starting the edit changes the overlay key");
        // End the edit again: the overlay refreshes back.
        h.state_mut().session.active_mut().expect("document").selection.text = None;
        h.run_steps(5);
        assert_eq!(h.state().camera.over.as_ref().map(|o| o.5), plain_key, "ending the edit restores the overlay key");
    }

    /// A Cameo with Camera Shape `geom`, and its box, from the command that inserts one.
    fn cameo(geom: &str) -> (Shape, Xfrm) {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("insert.cameo", &serde_json::json!({})).expect("cameo");
        let doc = session.active().expect("document");
        let mut shape = doc.selected_shapes().first().map(|s| (*s).clone()).expect("a selected Cameo");
        shape.geom = deckcraft_model::Geom::preset(geom);
        let x = shape.xfrm.expect("the Cameo has a box");
        (shape, x)
    }

    /// Shape-local points transformed by the Cameo's own box, as paint call sites do: with the
    /// mesh callback owning only placement, the transform lives here.
    fn placed(x: Xfrm) -> impl Fn(Point) -> Pos2 {
        move |p: Point| {
            let q = x.affine() * p;
            pos2(q.x as f32, q.y as f32)
        }
    }

    #[test]
    fn the_frame_is_masked_to_the_cameo_shape() {
        // The Cameo is an ellipse by default: no painted vertex may reach the box's corners.
        let (shape, x) = cameo("ellipse");
        let mesh = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        assert!(!mesh.indices.is_empty(), "the ellipse is painted");
        let inv = x.affine().inverse();
        let (cx, cy) = (x.w / 2.0, x.h / 2.0);
        for v in &mesh.vertices {
            let p = inv * Point::new(v.pos.x as f64, v.pos.y as f64);
            let e = ((p.x - cx) / cx).powi(2) + ((p.y - cy) / cy).powi(2);
            assert!(e <= 1.001, "a vertex at ({}, {}) falls outside the ellipse ({e})", p.x, p.y);
            assert!((0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y), "uv {:?} leaves the frame", v.uv);
        }
    }

    #[test]
    fn a_rectangular_camera_shape_still_fills_its_box() {
        let (shape, x) = cameo("rect");
        let mesh = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        for (dx, dy) in [(0.0, 0.0), (x.w, 0.0), (x.w, x.h), (0.0, x.h)] {
            let (px, py) = (x.x + dx, x.y + dy);
            let painted = mesh.vertices.iter().any(|v| (v.pos.x as f64 - px).abs() < 0.01 && (v.pos.y as f64 - py).abs() < 0.01);
            assert!(painted, "the corner ({px}, {py}) is not painted");
        }
    }

    #[test]
    fn a_cropped_cameo_shows_its_crop_selection() {
        // Crop 50% off the left: the box's left edge must sample half a frame further right than
        // the uncropped mesh (which centre-crops the 640x480 frame into the wide box).
        let (shape, x) = cameo("rect");
        let polys = cameo_polygons(&shape, x.w, x.h);
        let plain = cameo_mesh(&polys, x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        let cropped = cameo_mesh(&polys, x, [0.5, 0.0, 0.0, 0.0], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        let at_left =
            |m: &Mesh, x0: f64| m.vertices.iter().filter(|v| (v.pos.x as f64 - x0).abs() < 0.01).map(|v| v.uv.x).fold(f32::INFINITY, f32::min);
        let (u_plain, u_cropped) = (at_left(&plain, x.x), at_left(&cropped, x.x));
        assert!(u_plain.is_finite() && u_cropped.is_finite(), "both meshes paint the left edge");
        assert!(u_cropped - u_plain > 0.3, "crop shifts the sample right (plain u={u_plain}, cropped u={u_cropped})");
    }

    #[test]
    fn a_concave_camera_shape_is_filled_by_its_own_triangles() {
        // Star reachable from Camera Shape: an ear-clipped fill, not a fan that spans the notches.
        let (shape, x) = cameo("star5");
        let polygons = cameo_polygons(&shape, x.w, x.h);
        assert_eq!(polygons.len(), 1, "one filled outline");
        let mesh = cameo_mesh(&polygons, x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        assert!(!mesh.indices.is_empty(), "the star is painted");
        let inv = x.affine().inverse();
        for t in mesh.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].pos);
            let centroid = inv * Point::new(((a.x + b.x + c.x) / 3.0) as f64, ((a.y + b.y + c.y) / 3.0) as f64);
            assert!(inside_polygon(&polygons[0], centroid), "a triangle at ({}, {}) fills a notch", centroid.x, centroid.y);
        }
    }

    /// Even-odd containment for a simple polygon (test helper).
    fn inside_polygon(pts: &[Point], p: Point) -> bool {
        let n = pts.len();
        let mut inside = false;
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    }

    #[test]
    fn a_rotated_cameo_paints_its_frame_rotated() {
        let (shape, mut x) = cameo("rect");
        x.w = 200.0;
        x.h = 100.0;
        let flat = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        x.rot = 45.0;
        let turned = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        let inv = x.affine().inverse();
        for v in &turned.vertices {
            let p = inv * Point::new(v.pos.x as f64, v.pos.y as f64);
            // The vertices round-trip through `f32` screen positions, hence the 0.01 pt slack.
            assert!((-0.01..=x.w + 0.01).contains(&p.x) && (-0.01..=x.h + 0.01).contains(&p.y), "({}, {}) is outside the shape's box", p.x, p.y);
        }
        let outside = |v: &egui::epaint::Vertex| {
            let (px, py) = (v.pos.x as f64, v.pos.y as f64);
            px < x.x - 0.01 || px > x.x + x.w + 0.01 || py < x.y - 0.01 || py > x.y + x.h + 0.01
        };
        assert!(!flat.vertices.iter().any(outside), "an upright Cameo stays in its box");
        assert!(turned.vertices.iter().any(outside), "the frame did not turn with the shape");
    }

    /// The mesh callback owns the shape transform exactly once: a translated Cameo paints at
    /// its slide position, not at the origin and not twice-translated.
    #[test]
    fn a_translated_cameo_paints_at_its_slide_position() {
        let (shape, mut x) = cameo("rect");
        x.x = 100.0;
        x.y = 50.0;
        x.w = 200.0;
        x.h = 100.0;
        let mesh = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, [0.0; 4], egui::TextureId::User(1), [640, 480], Color32::WHITE, &placed(x));
        assert!(!mesh.vertices.is_empty(), "the Cameo paints");
        for v in &mesh.vertices {
            // Exactly one translation: inside the slide box, and far from the doubly-applied
            // box (which would start at twice the offset).
            assert!((x.x..=x.x + x.w).contains(&(v.pos.x as f64)), "x={} is outside the Cameo box", v.pos.x);
            assert!((x.y..=x.y + x.h).contains(&(v.pos.y as f64)), "y={} is outside the Cameo box", v.pos.y);
        }
        let at_origin = mesh.vertices.iter().any(|v| v.pos.x < 1.0 && v.pos.y < 1.0);
        assert!(!at_origin, "no vertex leaks to the origin: the transform applied once");
    }
}
