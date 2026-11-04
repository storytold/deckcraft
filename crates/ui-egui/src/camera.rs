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
    /// The running show's identity, so a new show recomputes whether it has a cameo.
    show_instance: Option<u64>,
    /// The running show contains at least one cameo, so the camera is live on it.
    show_wants: bool,
    /// The last value of `preview || show_wants` a start/stop acted on.
    wanted: bool,
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
    /// show's identity before cloning the presentation.
    pub(crate) fn show_instance(&self) -> Option<u64> {
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

    /// Tell the preview which show is running. A show that contains a cameo wants the camera even
    /// when the editor's preview toggle is off, as in PowerPoint's slide show.
    pub fn set_show(&mut self, instance: Option<u64>, doc: Option<&Presentation>) {
        if self.show_instance == instance {
            return;
        }
        self.show_instance = instance;
        self.show_wants = instance.is_some() && doc.is_some_and(deck_has_cameo);
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
            return;
        };
        match source.start(&self.request) {
            Ok(session) => self.session = Some(session),
            Err(error) => {
                self.notice = Some(error_text(&error));
                self.status = CameraStatus::Failed(error);
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

/// Whether `doc` has a cameo anywhere.
fn deck_has_cameo(doc: &Presentation) -> bool {
    doc.slides.iter().any(|slide| {
        let mut found = false;
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| found |= shape.is_cameo());
        found
    })
}

/// Paint the live frame in every cameo of editor slide `slide`, in slide coordinates through `xf`.
pub(crate) fn paint_preview(app: &SlideApp, painter: &egui::Painter, xf: crate::canvas::Xf) {
    if !app.camera.preview_on() {
        return;
    }
    let Some(texture) = app.camera.texture().cloned() else { return };
    let Some(doc) = app.session.active() else { return };
    if doc.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let Some(slide) = doc.doc.slides.get(doc.selection.slide) else { return };
    for (shape, x) in cameo_boxes(&slide.shapes) {
        paint_cameo(painter, &texture, shape, x, &|p| xf.to_screen(p));
    }
}

/// Paint the live frame in every cameo of show slide `slide`, through the show's zoom window `uv`.
pub(crate) fn paint_show(app: &SlideApp, painter: &egui::Painter, doc: &Presentation, slide: usize, hidden: &[ShapeId], srect: Rect, uv: Rect) {
    let Some(texture) = app.camera.texture().cloned() else { return };
    let Some(slide) = doc.slides.get(slide) else { return };
    let (sw, sh) = (doc.slide_size.width as f32, doc.slide_size.height as f32);
    for (shape, x) in cameo_boxes(&slide.shapes) {
        if hidden.contains(&shape.id) {
            continue;
        }
        paint_cameo(painter, &texture, shape, x, &|p| slide_to_screen(srect, uv, p.x as f32 / sw.max(1.0), p.y as f32 / sh.max(1.0)));
    }
}

/// Cameos on a slide with their slide-space boxes, including cameos inside groups.
fn cameo_boxes(shapes: &[deckcraft_model::Shape]) -> Vec<(&deckcraft_model::Shape, deckcraft_geom::Xfrm)> {
    let mut pictures = vec![];
    crate::media::collect_pictures(shapes, None, 0, &mut pictures);
    pictures.into_iter().filter(|(shape, _)| shape.is_cameo()).collect()
}

fn slide_to_screen(srect: Rect, uv: Rect, nx: f32, ny: f32) -> Pos2 {
    let u = (nx - uv.min.x) / uv.width().max(1e-4);
    let v = (ny - uv.min.y) / uv.height().max(1e-4);
    pos2(srect.min.x + u * srect.width(), srect.min.y + v * srect.height())
}

/// Paint `texture` in one Cameo: masked to its Camera Shape, turned with the shape and crop-filled
/// so a camera fills its frame instead of stretching.
fn paint_cameo(painter: &egui::Painter, texture: &TextureHandle, shape: &Shape, x: Xfrm, to_screen: &dyn Fn(Point) -> Pos2) {
    let polygons = cameo_polygons(shape, x.w, x.h);
    if polygons.is_empty() {
        return;
    }
    let affine = x.affine();
    // A dark backing inside the same outline, so a frame that arrives with transparency or not at
    // all still reads as a camera and never as the slide behind it.
    for poly in &polygons {
        let points: Vec<Pos2> = poly.iter().map(|p| to_screen(affine * *p)).collect();
        painter.add(egui::Shape::Path(egui::epaint::PathShape { points, closed: true, fill: Color32::BLACK, stroke: egui::Stroke::NONE.into() }));
    }
    painter.add(egui::Shape::mesh(cameo_mesh(&polygons, x, texture.id(), texture.size(), to_screen)));
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

/// A textured mesh that covers `polygons` (shape-local) with the whole frame, using the shape's
/// flip and rotation and the same centre crop as [`cover_uv`].
fn cameo_mesh(polygons: &[Vec<Point>], x: Xfrm, tex: egui::TextureId, tex_size: [usize; 2], to_screen: &dyn Fn(Point) -> Pos2) -> Mesh {
    let affine = x.affine();
    let uv = cover_uv(Rect::from_min_size(Pos2::ZERO, vec2(x.w as f32, x.h as f32)), tex_size);
    let uv_at = |p: Point| {
        let u = (p.x / x.w) as f32;
        let v = (p.y / x.h) as f32;
        pos2(uv.min.x + u * uv.width(), uv.min.y + v * uv.height())
    };
    let mut mesh = Mesh::with_texture(tex);
    for poly in polygons {
        let points: Vec<Pos2> = poly.iter().map(|p| to_screen(affine * *p)).collect();
        if points.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for (p, at) in poly.iter().zip(&points) {
            mesh.vertices.push(egui::epaint::Vertex { pos: *at, uv: uv_at(*p), color: Color32::WHITE });
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

/// Start (or keep) the camera session the current show needs, if `instance` names a new show.
pub(crate) fn wants_show_camera(app: &mut SlideApp, instance: Option<u64>) {
    if app.camera.show_instance() == instance {
        return;
    }
    let doc = app.session.active().map(|d| d.doc.clone());
    app.camera.set_show(instance, doc.as_deref());
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
        host.set_show(Some(1), Some(doc.as_ref()));
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
        host.set_show(Some(1), Some(doc.as_ref()));
        assert_eq!(fake.starts(), 0);
        assert_eq!(host.status(), &CameraStatus::Off);
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

    /// Slide points as pixels, so a test can reason in the shape's own coordinates.
    fn identity(p: Point) -> Pos2 {
        pos2(p.x as f32, p.y as f32)
    }

    #[test]
    fn the_frame_is_masked_to_the_cameo_shape() {
        // The Cameo is an ellipse by default: no painted vertex may reach the box's corners.
        let (shape, x) = cameo("ellipse");
        let mesh = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, egui::TextureId::User(1), [640, 480], &identity);
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
        let mesh = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, egui::TextureId::User(1), [640, 480], &identity);
        for (dx, dy) in [(0.0, 0.0), (x.w, 0.0), (x.w, x.h), (0.0, x.h)] {
            let (px, py) = (x.x + dx, x.y + dy);
            let painted = mesh.vertices.iter().any(|v| (v.pos.x as f64 - px).abs() < 0.01 && (v.pos.y as f64 - py).abs() < 0.01);
            assert!(painted, "the corner ({px}, {py}) is not painted");
        }
    }

    #[test]
    fn a_rotated_cameo_paints_its_frame_rotated() {
        let (shape, mut x) = cameo("rect");
        x.w = 200.0;
        x.h = 100.0;
        let flat = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, egui::TextureId::User(1), [640, 480], &identity);
        x.rot = 45.0;
        let turned = cameo_mesh(&cameo_polygons(&shape, x.w, x.h), x, egui::TextureId::User(1), [640, 480], &identity);
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
}
