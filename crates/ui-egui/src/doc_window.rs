//! View › New Window: one more window on the same presentation.
//!
//! The window shows that document live, on its own slide, and does not move the editor's
//! selection. It does not edit, and Arrange All is not here. Closing the window leaves the
//! presentation open.

use deckcraft_render::RenderOpts;
use egui::{Color32, CornerRadius, Rect, RichText, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;

/// The extra window. `uid` is the presentation it was opened on.
pub struct DocWindow {
    pub uid: u64,
    pub slide: usize,
    key: Option<TexKey>,
    tex: Option<egui::TextureHandle>,
}

/// `view.newWindow`. `{on: false}` closes it. `{slide}` or `{delta}` moves only this window.
pub fn command(app: &mut SlideApp, p: &Value) -> Result<Value, String> {
    if p.get("on").and_then(Value::as_bool) == Some(false) {
        app.doc_window = None;
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
    match app.doc_window.as_mut().filter(|w| w.uid == uid) {
        Some(w) => w.slide = slide,
        None => {
            app.doc_window = Some(DocWindow { uid, slide, key: None, tex: None });
        }
    }
    Ok(json!({"open": true, "window": 2, "slide": slide, "slides": n, "title": title}))
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

/// Draw the extra window, or drop it when its presentation has been closed.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(uid) = app.doc_window.as_ref().map(|w| w.uid) else { return };
    let found =
        app.session.documents().iter().find(|d| d.uid == uid).map(|d| (d.doc.slides.len(), window_title(&d.title()), d.revision, d.doc.clone()));
    let Some((n, title, revision, doc)) = found else {
        app.doc_window = None;
        return;
    };
    if n == 0 {
        app.doc_window = None;
        return;
    }
    let slide = app.doc_window.as_ref().map(|w| w.slide.min(n - 1)).unwrap_or(0);
    if let Some(w) = app.doc_window.as_mut() {
        w.slide = slide;
    }
    let grayscale = app.ui.grayscale;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("deckcraft_doc_window"),
        egui::ViewportBuilder::default().with_title(title.clone()).with_inner_size([960.0, 640.0]).with_min_inner_size([480.0, 360.0]),
        |ui, _| {
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
                    let tex = ensure_texture(app, ui.ctx(), &doc, (uid, revision, slide, w, h, grayscale));
                    if let Some(tex) = tex {
                        let fitted = fit_rect(avail, w, h);
                        ui.painter().rect_filled(fitted, CornerRadius::ZERO, Color32::WHITE);
                        ui.painter().image(tex.id(), fitted, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                });
            });
            if ui.input(|i| i.viewport().close_requested()) {
                app.doc_window = None;
            }
        },
    );
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

/// What a cached slide texture was drawn from: document, revision, slide, pixel size, grayscale.
type TexKey = (u64, u64, usize, u32, u32, bool);

fn ensure_texture(app: &mut SlideApp, ctx: &egui::Context, doc: &deckcraft_model::Presentation, key: TexKey) -> Option<egui::TextureHandle> {
    let (_, _, slide, w, h, grayscale) = key;
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
    use super::*;

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
}
