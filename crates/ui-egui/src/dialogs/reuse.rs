//! Reuse Slides: each listed slide's thumbnail, drawn from the picked file, so slides can be
//! told apart by what is on them, not only by their titles. Only rows in view render, a few a
//! frame, and only the first [`REUSE_THUMBS`] slides; a slide that can't be drawn keeps its
//! title beside a placeholder.

use deckcraft_model::Presentation;

use super::{Color32, CornerRadius, Rect, Sense, SlideApp, Stroke, Tokens, Ui, pos2, vec2};
use crate::textures::{Textures, device_pixel_rect};

/// Slides past this many are listed by title only.
pub const REUSE_THUMBS: usize = 200;
const THUMB_W: f32 = 96.0;
/// The width a slide's title wraps at beside its thumbnail.
pub const TITLE_W: f32 = 320.0;

/// The picked file, read once when the picker opens, and its own thumbnail cache; dropped with
/// the dialog.
pub struct ReusePreview {
    doc: Option<Presentation>,
    pub textures: Textures,
}

impl ReusePreview {
    pub fn new(name: &str, bytes: &[u8]) -> Self {
        Self { doc: deckcraft_engine::cmd::file::open_presentation(name, bytes).ok(), textures: Textures::default() }
    }
}

/// Slide `index`'s thumbnail, at the start of its row. `budget` caps the renders this frame;
/// returns whether a row in view still waits for one.
pub(super) fn reuse_thumb(app: &mut SlideApp, ui: &mut Ui, index: usize, budget: &mut u32) -> bool {
    let t = Tokens::get(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let doc = app.reuse_preview.as_ref().and_then(|p| p.doc.as_ref());
    let aspect = doc.map(|d| d.slide_size.height / d.slide_size.width.max(1.0)).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(0.5625);
    let th = (THUMB_W * aspect as f32).clamp(8.0, THUMB_W * 2.0);
    let (r, _) = ui.allocate_exact_size(vec2(THUMB_W, th), Sense::hover());
    let r = device_pixel_rect(r, ppp);
    let drawable = index < REUSE_THUMBS && doc.is_some_and(|d| d.slides.get(index).is_some());
    let near = ui.clip_rect().expand2(vec2(0.0, 2.0 * th)).intersects(r);
    let tex = match app.reuse_preview.as_mut() {
        Some(ReusePreview { doc: Some(doc), textures }) if drawable && near => {
            textures.thumb(ui.ctx(), doc, index, (r.width() * ppp).round().max(1.0) as u32, budget)
        }
        _ => None,
    };
    let p = ui.painter();
    match &tex {
        Some(tex) => p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
        None => p.rect_filled(r, CornerRadius::ZERO, if drawable { Color32::WHITE } else { t.border }),
    };
    p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    if !drawable {
        ui.put(r, egui::Label::new(egui::RichText::new("No preview").size(10.0).color(t.text_dim)));
    }
    drawable && near && tex.is_none()
}
