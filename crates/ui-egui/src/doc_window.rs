//! View › New Window and Arrange All.
//!
//! The extra window shows one presentation live, on its own slide. Pointer gestures on that
//! slide move, resize and draw through the same tools as the editor, then the editor's selection
//! is put back, so the first window does not change slides. The text caret stays in the first
//! window. Arrange All tiles the two windows once; it does not keep moving them.

use deckcraft_engine::{Mods, PointerEvent, PointerKind, Selection, Target};
use deckcraft_model::ShapeId;
use deckcraft_render::RenderOpts;
use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;

/// The extra window. `uid` is the presentation it was opened on.
pub struct DocWindow {
    pub uid: u64,
    pub slide: usize,
    /// Shapes selected in this window, separate from the editor's selection.
    shapes: Vec<ShapeId>,
    /// This window started the tool gesture, so the editor must not also consume it.
    owns_drag: bool,
    key: Option<(u64, u64, usize, u32, u32, bool)>,
    tex: Option<egui::TextureHandle>,
}

impl DocWindow {
    pub(crate) fn owns_drag(&self) -> bool {
        self.owns_drag
    }
}

/// Where Arrange All should put the two windows. Consumed on the frame that can move them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrange {
    /// The monitor the main window is on.
    Screen,
    /// A known monitor size, in points. Tests pass this; the command does too when given one.
    Size { width: f32, height: f32 },
}

/// `view.newWindow`. `{on: false}` closes it. `{slide}` or `{delta}` moves only this window.
pub fn command(app: &mut SlideApp, p: &Value) -> Result<Value, String> {
    if p.get("on").and_then(Value::as_bool) == Some(false) {
        close(app);
        return Ok(json!({"open": false}));
    }
    let (uid, n, title, editor_slide) = {
        let active = app.session.active().ok_or("New Window needs an open presentation")?;
        let n = active.doc.slides.len();
        if n == 0 {
            return Err("New Window needs a slide".into());
        }
        (active.uid, n, window_title(&active.title()), active.selection.slide.min(n - 1))
    };
    let mut slide = app.doc_window.as_ref().filter(|w| w.uid == uid).map(|w| w.slide).unwrap_or(editor_slide);
    if let Some(i) = p.get("slide").and_then(Value::as_u64) {
        slide = i as usize;
    }
    if let Some(delta) = p.get("delta").and_then(Value::as_i64) {
        slide = shift(slide, delta, n);
    }
    if slide >= n {
        slide = n - 1;
    }
    let open_here = app.doc_window.as_ref().is_some_and(|w| w.uid == uid);
    if open_here {
        let changed = app.doc_window.as_ref().is_some_and(|w| w.slide != slide);
        if changed {
            release_drag(app);
            if let Some(w) = app.doc_window.as_mut() {
                w.shapes.clear();
                w.slide = slide;
            }
        }
    } else {
        app.doc_window = Some(DocWindow { uid, slide, shapes: Vec::new(), owns_drag: false, key: None, tex: None });
    }
    Ok(json!({"open": true, "window": 2, "slide": slide, "slides": n, "title": title}))
}

/// `view.arrangeAll`. Tiles the main window and the extra window on the next frame.
/// `{width, height}` is the monitor size in points; without it, the monitor is read from the window.
pub fn arrange(app: &mut SlideApp, p: &Value) -> Result<Value, String> {
    if app.show.is_some() {
        return Err("Arrange All waits until the slide show ends".into());
    }
    if app.doc_window.is_none() {
        return Err("Arrange All needs New Window open".into());
    }
    let width = p.get("width").and_then(Value::as_f64);
    let height = p.get("height").and_then(Value::as_f64);
    match (width, height) {
        (None, None) => {
            app.arrange = Some(Arrange::Screen);
            Ok(json!({"arranged": true, "windows": 2}))
        }
        (Some(w), Some(h)) if w.is_finite() && h.is_finite() && w >= 64.0 && h >= 64.0 => {
            let (main, extra) = arrange_within(w as f32, h as f32, MAIN_MIN, EXTRA_MIN);
            app.arrange = Some(Arrange::Size { width: w as f32, height: h as f32 });
            Ok(json!({"arranged": true, "windows": 2, "main": rect_json(main), "extra": rect_json(extra)}))
        }
        _ => Err("Arrange All needs a screen size".into()),
    }
}

pub fn window_title(name: &str) -> String {
    format!("{name}:2")
}

fn shift(slide: usize, delta: i64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let last = (n as i64) - 1;
    (slide as i64).saturating_add(delta).clamp(0, last) as usize
}

/// Desktop window minimum from `apps/deckcraft` (`with_min_inner_size`). Tiles respect it so the
/// main window is not clamped on top of the other one.
const MAIN_MIN: (f32, f32) = (900.0, 560.0);
/// Extra window minimum (`with_min_inner_size` on that viewport).
const EXTRA_MIN: (f32, f32) = (320.0, 240.0);

/// Two rectangles that tile a monitor of `width` by `height`, origin at its top left.
/// A wide monitor splits left and right; a tall one splits top and bottom.
pub fn arrange_rects(width: f32, height: f32) -> (Rect, Rect) {
    arrange_within(width, height, (0.0, 0.0), (0.0, 0.0))
}

/// Like [`arrange_rects`], but each window keeps at least `min_main` / `min_extra` when the
/// monitor is large enough. The wide axis is preferred when both orientations fit.
fn arrange_within(width: f32, height: f32, min_main: (f32, f32), min_extra: (f32, f32)) -> (Rect, Rect) {
    let horiz = split_axis(width, min_main.0, min_extra.0)
        .map(|(a, b)| (Rect::from_min_size(pos2(0.0, 0.0), vec2(a, height)), Rect::from_min_size(pos2(a, 0.0), vec2(b, height))));
    let vert = split_axis(height, min_main.1, min_extra.1)
        .map(|(a, b)| (Rect::from_min_size(pos2(0.0, 0.0), vec2(width, a)), Rect::from_min_size(pos2(0.0, a), vec2(width, b))));
    match (horiz, vert) {
        (Some(h), Some(v)) => {
            if width >= height {
                h
            } else {
                v
            }
        }
        (Some(h), None) => h,
        (None, Some(v)) => v,
        (None, None) => {
            if width >= height {
                let left = (width / 2.0).floor();
                (Rect::from_min_size(pos2(0.0, 0.0), vec2(left, height)), Rect::from_min_size(pos2(left, 0.0), vec2(width - left, height)))
            } else {
                let top = (height / 2.0).floor();
                (Rect::from_min_size(pos2(0.0, 0.0), vec2(width, top)), Rect::from_min_size(pos2(0.0, top), vec2(width, height - top)))
            }
        }
    }
}

fn split_axis(total: f32, min_a: f32, min_b: f32) -> Option<(f32, f32)> {
    if min_a + min_b > total + 0.5 {
        return None;
    }
    let mut a = (total / 2.0).floor().max(min_a);
    let mut b = total - a;
    if b < min_b {
        b = min_b;
        a = total - b;
    }
    if a + 0.5 < min_a || b + 0.5 < min_b { None } else { Some((a, b)) }
}

fn rect_json(r: Rect) -> Value {
    json!({"x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height()})
}

fn close(app: &mut SlideApp) {
    release_drag(app);
    app.doc_window = None;
    app.arrange = None;
}

fn release_drag(app: &mut SlideApp) {
    if app.doc_window.as_ref().is_some_and(|w| w.owns_drag) {
        let kind = app.session.tool.kind.clone();
        app.session.set_tool(kind);
    }
    if let Some(w) = app.doc_window.as_mut() {
        w.owns_drag = false;
    }
}

/// Draw the extra window, or drop it when its presentation has been closed.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(uid) = app.doc_window.as_ref().map(|w| w.uid) else {
        app.arrange = None;
        return;
    };
    let found =
        app.session.documents().iter().find(|d| d.uid == uid).map(|d| (d.doc.slides.len(), window_title(&d.title()), d.revision, d.doc.clone()));
    let Some((n, title, revision, doc)) = found else {
        close(app);
        return;
    };
    if n == 0 {
        close(app);
        return;
    }
    let slide = app.doc_window.as_ref().map(|w| w.slide.min(n - 1)).unwrap_or(0);
    if let Some(w) = app.doc_window.as_mut() {
        w.slide = slide;
    }
    let grayscale = app.ui.grayscale;
    let monitor = ctx.input(|i| i.viewport().monitor_size);
    let tiles = take_tiles(app, monitor);
    if let Some((main, _)) = tiles {
        apply_tile(ctx, main);
    }
    let extra = tiles.map(|(_, extra)| extra);
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("deckcraft_doc_window"),
        egui::ViewportBuilder::default().with_title(title.clone()).with_inner_size([960.0, 640.0]).with_min_inner_size([320.0, 240.0]),
        |ui, _| {
            if let Some(tile) = extra {
                apply_tile(ui.ctx(), tile);
            }
            let t = crate::theme::Tokens::get(ui);
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
                egui::Panel::top("doc_window_bar")
                    .exact_size(40.0)
                    .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 8)))
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.label(RichText::new(title.clone()).font(crate::theme::bold(13.0)).color(t.text));
                            ui.add_space(16.0);
                            ui.label(RichText::new(format!("Slide {} of {n}", slide + 1)).color(t.text_dim).font(crate::theme::font(13.0)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(slide + 1 < n, egui::Button::new("Next")).clicked() {
                                    let _ = app.run("view.newWindow", json!({"delta": 1}));
                                }
                                if ui.add_enabled(slide > 0, egui::Button::new("Previous")).clicked() {
                                    let _ = app.run("view.newWindow", json!({"delta": -1}));
                                }
                            });
                        });
                    });
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                    let avail = ui.available_rect_before_wrap();
                    let (w, h) = pixel_size(&doc, avail.width());
                    let fitted = fit_rect(avail, w, h);
                    let resp = ui.interact(fitted, ui.id().with("doc_window_slide"), Sense::click_and_drag());
                    feed_pointer(app, ui, &resp, doc.slide_size.width, doc.slide_size.height, fitted);
                    let (revision, doc) =
                        app.session.documents().iter().find(|d| d.uid == uid).map(|d| (d.revision, d.doc.clone())).unwrap_or((revision, doc.clone()));
                    let tex = ensure_texture(app, ui.ctx(), &doc, uid, revision, slide, w, h, grayscale);
                    if let Some(tex) = tex {
                        ui.painter().rect_filled(fitted, CornerRadius::ZERO, Color32::WHITE);
                        ui.painter().image(tex.id(), fitted, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    let ids = app.doc_window.as_ref().map(|w| w.shapes.clone()).unwrap_or_default();
                    paint_selection(ui, &doc, slide, &ids, fitted);
                    if app.doc_window.as_ref().is_some_and(|w| w.owns_drag) {
                        ui.ctx().request_repaint();
                        ctx.request_repaint();
                    }
                });
            });
            if ui.input(|i| i.viewport().close_requested()) {
                close(app);
            }
        },
    );
}

fn take_tiles(app: &mut SlideApp, monitor: Option<egui::Vec2>) -> Option<(Rect, Rect)> {
    if app.show.is_some() {
        return None;
    }
    let plan = app.arrange?;
    let tiles = match plan {
        Arrange::Size { width, height } => arrange_within(width, height, MAIN_MIN, EXTRA_MIN),
        Arrange::Screen => {
            let size = monitor?;
            if size.x < 64.0 || size.y < 64.0 {
                return None;
            }
            arrange_within(size.x, size.y, MAIN_MIN, EXTRA_MIN)
        }
    };
    app.arrange = None;
    Some(tiles)
}

/// Move and size one window so its frame fills `tile`. Sent once, not every frame.
fn apply_tile(ctx: &egui::Context, tile: Rect) {
    let chrome = ctx.input(|i| {
        let v = i.viewport();
        match (v.outer_rect, v.inner_rect) {
            (Some(outer), Some(inner)) => vec2((outer.width() - inner.width()).max(0.0), (outer.height() - inner.height()).max(0.0)),
            _ => vec2(0.0, 0.0),
        }
    });
    let inner = vec2((tile.width() - chrome.x).max(320.0), (tile.height() - chrome.y).max(240.0));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(tile.min));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
}

/// Feed one pointer event to the tool as if the editor were on this window's slide, then put
/// the editor's selection back. The document change stays.
pub fn edit_pointer(app: &mut SlideApp, ev: PointerEvent) -> Result<Value, String> {
    let Some(win) = app.doc_window.as_ref() else { return Err("New Window is closed".into()) };
    let uid = win.uid;
    let slide = win.slide;
    let shapes = win.shapes.clone();
    let owns = win.owns_drag;
    if app.session.tool.dragging() && !owns {
        return Ok(Value::Null);
    }
    let prev = app.session.active_index();
    let Some(index) = app.session.documents().iter().position(|d| d.uid == uid) else {
        return Err("The presentation for this window is closed".into());
    };
    app.session.set_active(index);
    let Some((editor, undo_before, had_ix)) = arm_window(app, slide, shapes) else {
        if let Some(i) = prev {
            app.session.set_active(i);
        }
        return Err("New Window needs a slide".into());
    };
    let result = app.session.pointer(ev).map_err(|e| e.to_string());
    let new_shapes = app.session.active().map(|d| d.selection.shapes.clone()).unwrap_or_default();
    let dragging = app.session.tool.dragging();
    if let Some(st) = app.session.active_mut() {
        if !had_ix && let Some(ix) = st.interaction.as_mut() {
            ix.selection = editor.clone();
        }
        for entry in st.history.undo.iter_mut().skip(undo_before) {
            entry.selection = editor.clone();
        }
        st.selection = editor;
    }
    if let Some(i) = prev {
        app.session.set_active(i);
    }
    if let Some(w) = app.doc_window.as_mut().filter(|w| w.uid == uid) {
        w.shapes = new_shapes;
        w.owns_drag = dragging;
    }
    result
}

/// Point the active document at this window's slide and shapes. `None` when it has no slides.
fn arm_window(app: &mut SlideApp, slide: usize, shapes: Vec<ShapeId>) -> Option<(Selection, usize, bool)> {
    let st = app.session.active_mut()?;
    let n = st.doc.slides.len();
    if n == 0 {
        return None;
    }
    let editor = st.selection.clone();
    st.selection.slide = slide.min(n - 1);
    st.selection.shapes = shapes;
    st.selection.text = None;
    st.selection.cells = None;
    st.selection.target = Target::Slides;
    Some((editor, st.history.undo.len(), st.interaction.is_some()))
}

fn feed_pointer(app: &mut SlideApp, ui: &egui::Ui, resp: &egui::Response, slide_w: f64, slide_h: f64, fitted: Rect) {
    let scale = fitted.width() / (slide_w as f32).max(1.0);
    let tol = (8.0 / scale.max(0.05)) as f64;
    let m = ui.input(|i| Mods { shift: i.modifiers.shift, alt: i.modifiers.alt, cmd: i.modifiers.command || i.modifiers.ctrl });
    let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
    let (x, y) = to_slide(fitted, slide_w, slide_h, pos);
    let ev = |kind| PointerEvent { kind, x, y, mods: m, tol, pressure: None };
    let owns = app.doc_window.as_ref().is_some_and(|w| w.owns_drag);
    let primary = ui.input(|i| i.pointer.primary_pressed());
    let kind = if resp.double_clicked() {
        Some(PointerKind::DoubleClick)
    } else if primary && resp.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        Some(PointerKind::Down)
    } else if owns && ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
        Some(PointerKind::Up)
    } else if owns && ui.input(|i| i.pointer.primary_down()) {
        Some(PointerKind::Drag)
    } else if resp.hovered() && !app.session.tool.dragging() {
        Some(PointerKind::Move)
    } else {
        None
    };
    if let Some(kind) = kind
        && let Err(e) = edit_pointer(app, ev(kind))
    {
        app.set_status(e);
    }
}

fn to_slide(fitted: Rect, slide_w: f64, slide_h: f64, pos: Pos2) -> (f64, f64) {
    let x = ((pos.x - fitted.min.x) / fitted.width().max(1.0)) as f64 * slide_w;
    let y = ((pos.y - fitted.min.y) / fitted.height().max(1.0)) as f64 * slide_h;
    (x, y)
}

fn to_screen(fitted: Rect, slide_w: f64, slide_h: f64, x: f64, y: f64) -> Pos2 {
    pos2(fitted.min.x + (x as f32 / slide_w.max(1.0) as f32) * fitted.width(), fitted.min.y + (y as f32 / slide_h.max(1.0) as f32) * fitted.height())
}

fn paint_selection(ui: &egui::Ui, doc: &deckcraft_model::Presentation, slide: usize, ids: &[ShapeId], fitted: Rect) {
    let Some(shapes) = doc.slides.get(slide).map(|s| &s.shapes) else { return };
    let t = crate::theme::Tokens::get(ui);
    let (sw, sh) = (doc.slide_size.width, doc.slide_size.height);
    let sel = deckcraft_engine::Selection { slide, target: Target::Slides, ..Default::default() };
    for id in ids {
        let Some(shape) = deckcraft_model::find_shape(shapes, *id) else { continue };
        let x = deckcraft_engine::cmd::xfrm_of(doc, &sel, shape);
        let (handles, rot) = deckcraft_engine::tools::box_handles(&x);
        let corners = [0, 2, 4, 6].map(|i| to_screen(fitted, sw, sh, handles[i].x, handles[i].y));
        for i in 0..4 {
            ui.painter().line_segment([corners[i], corners[(i + 1) % 4]], Stroke::new(1.0, t.selection));
        }
        let top = to_screen(fitted, sw, sh, handles[1].x, handles[1].y);
        let rot_s = to_screen(fitted, sw, sh, rot.x, rot.y);
        ui.painter().line_segment([top, rot_s], Stroke::new(1.0, t.selection));
        ui.painter().circle(rot_s, 4.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
        for h in handles {
            let p = to_screen(fitted, sw, sh, h.x, h.y);
            ui.painter().circle(p, 4.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
        }
    }
}

fn pixel_size(doc: &deckcraft_model::Presentation, width_pts: f32) -> (u32, u32) {
    let w = (width_pts.round() as u32).clamp(64, 1400);
    let aspect = doc.slide_size.height / doc.slide_size.width.max(1.0);
    let h = ((w as f64) * aspect).round().clamp(1.0, 1400.0) as u32;
    (w, h)
}

fn fit_rect(avail: Rect, w: u32, h: u32) -> Rect {
    let aspect = h as f32 / (w as f32).max(1.0);
    let mut width = avail.width();
    let mut height = width * aspect;
    if height > avail.height() {
        height = avail.height();
        width = height / aspect.max(0.01);
    }
    Rect::from_center_size(avail.center(), vec2(width.max(1.0), height.max(1.0)))
}

fn ensure_texture(
    app: &mut SlideApp,
    ctx: &egui::Context,
    doc: &deckcraft_model::Presentation,
    uid: u64,
    revision: u64,
    slide: usize,
    w: u32,
    h: u32,
    grayscale: bool,
) -> Option<egui::TextureHandle> {
    let key = (uid, revision, slide, w, h, grayscale);
    if let Some(win) = app.doc_window.as_ref()
        && win.key == Some(key)
        && let Some(tex) = win.tex.clone()
    {
        return Some(tex);
    }
    let img = deckcraft_render::render_slide(
        doc,
        slide,
        &RenderOpts { scale: w as f64 / doc.slide_size.width.max(1.0), size: Some((w, h)), edit: true, threads: 0, ..Default::default() },
    );
    let tex = crate::textures::upload(ctx, "doc-window-slide", crate::textures::to_color_image(&img, grayscale));
    if let Some(win) = app.doc_window.as_mut() {
        win.key = Some(key);
        win.tex = Some(tex.clone());
    }
    Some(tex)
}

#[cfg(test)]
mod tests {
    use super::{Arrange, arrange_rects, edit_pointer, shift, take_tiles, window_title};
    use crate::Services;
    use crate::SlideApp;
    use deckcraft_engine::{Mods, PointerEvent, PointerKind, Session};
    use deckcraft_geom::Xfrm;
    use egui::{Rect, pos2, vec2};
    use serde_json::json;

    #[test]
    fn the_window_title_marks_the_second_window() {
        assert_eq!(window_title("Quarterly review"), "Quarterly review:2");
    }

    #[test]
    fn shifting_stops_at_the_ends() {
        assert_eq!(shift(0, -1, 3), 0);
        assert_eq!(shift(1, 1, 3), 2);
        assert_eq!(shift(2, 5, 3), 2);
        assert_eq!(shift(0, 1, 0), 0);
    }

    #[test]
    fn arrange_all_splits_the_monitor() {
        let (main, extra) = arrange_rects(1440.0, 900.0);
        assert_eq!(main, Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, 900.0)));
        assert_eq!(extra, Rect::from_min_size(pos2(720.0, 0.0), vec2(720.0, 900.0)));
        let (top, bottom) = arrange_rects(800.0, 1200.0);
        assert_eq!(top.height(), 600.0);
        assert_eq!(bottom.min.y, 600.0);
        assert!((bottom.height() - 600.0).abs() < 0.01);
    }

    #[test]
    fn arrange_all_needs_the_second_window_and_reports_the_tiles() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        let err = app.run("view.arrangeAll", json!({})).unwrap_err();
        assert!(err.contains("New Window"), "{err}");
        app.run("view.newWindow", json!({})).unwrap();
        let v = app.run("view.arrangeAll", json!({"width": 1440.0, "height": 900.0})).unwrap();
        assert_eq!(v["arranged"], true);
        assert_eq!(v["windows"], 2);
        assert!((v["main"]["w"].as_f64().unwrap() - 900.0).abs() < 0.01, "the main window keeps its 900pt minimum");
        assert!((v["extra"]["x"].as_f64().unwrap() - 900.0).abs() < 0.01);
        assert!((v["extra"]["w"].as_f64().unwrap() - 540.0).abs() < 0.01);
        assert!(matches!(app.arrange, Some(Arrange::Size { .. })));
        let tiles = take_tiles(&mut app, None).unwrap();
        assert!(app.arrange.is_none(), "the plan is applied once");
        assert!((tiles.0.width() - 900.0).abs() < 0.01);
        assert!((tiles.1.min.x - 900.0).abs() < 0.01);

        app.run("view.arrangeAll", json!({})).unwrap();
        assert_eq!(app.arrange, Some(Arrange::Screen));
        assert!(take_tiles(&mut app, None).is_none(), "without a monitor the request waits");
        assert_eq!(app.arrange, Some(Arrange::Screen));
        let tiles = take_tiles(&mut app, Some(vec2(800.0, 1200.0))).unwrap();
        assert!(app.arrange.is_none());
        assert!((tiles.0.height() - 600.0).abs() < 0.01);
        assert!((tiles.1.min.y - 600.0).abs() < 0.01);
    }

    #[test]
    fn the_second_window_moves_and_resizes_without_moving_the_editor() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.session.prefs.snap_to_grid = false;
        app.session.prefs.smart_guides = false;
        let id = app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 80, 40]})).unwrap()["id"].as_u64().unwrap();
        app.session.execute("slide.new", &json!({})).unwrap();
        app.session.execute("slide.go", &json!({"index": 1})).unwrap();
        app.run("view.newWindow", json!({"slide": 0})).unwrap();
        let editor = app.session.active().unwrap().selection.clone();
        assert_eq!(editor.slide, 1);

        let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 4.0, pressure: None };
        edit_pointer(&mut app, ev(PointerKind::Down, 140.0, 120.0)).unwrap();
        assert!(app.doc_window.as_ref().unwrap().owns_drag());
        assert_eq!(app.session.active().unwrap().selection.slide, 1);
        edit_pointer(&mut app, ev(PointerKind::Drag, 220.0, 160.0)).unwrap();
        edit_pointer(&mut app, ev(PointerKind::Up, 220.0, 160.0)).unwrap();
        assert!(!app.doc_window.as_ref().unwrap().owns_drag());
        assert_eq!(app.session.active().unwrap().selection, editor);
        let moved = xfrm_of(&app, id);
        assert!((moved.x - 180.0).abs() < 0.5, "moved x {}", moved.x);
        assert!((moved.y - 140.0).abs() < 0.5, "moved y {}", moved.y);
        assert!(app.doc_window.as_ref().unwrap().shapes.iter().any(|s| s.0 == id as u32));

        let se = (moved.x + moved.w, moved.y + moved.h);
        edit_pointer(&mut app, ev(PointerKind::Down, se.0, se.1)).unwrap();
        edit_pointer(&mut app, ev(PointerKind::Drag, se.0 + 40.0, se.1 + 30.0)).unwrap();
        edit_pointer(&mut app, ev(PointerKind::Up, se.0 + 40.0, se.1 + 30.0)).unwrap();
        let sized = xfrm_of(&app, id);
        assert!(sized.w > moved.w + 10.0, "resized w {} from {}", sized.w, moved.w);
        assert!(sized.h > moved.h + 10.0, "resized h {} from {}", sized.h, moved.h);
        assert_eq!(app.session.active().unwrap().selection, editor);

        app.session.execute("edit.undo", &json!({})).unwrap();
        let undone = xfrm_of(&app, id);
        assert!((undone.w - moved.w).abs() < 0.5, "undo resize w {}", undone.w);
        app.session.execute("edit.undo", &json!({})).unwrap();
        let back = xfrm_of(&app, id);
        assert!((back.x - 100.0).abs() < 0.5, "undo move x {}", back.x);
        assert_eq!(app.session.active().unwrap().selection.slide, 1, "undo stays on the editor's slide");
    }

    fn xfrm_of(app: &SlideApp, id: u64) -> Xfrm {
        app.session.documents()[0].doc.slides[0].shapes.iter().find(|s| s.id.0 == id as u32).and_then(|s| s.xfrm).unwrap()
    }
}
