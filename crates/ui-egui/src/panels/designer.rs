use super::*;

/// Design Ideas: offline suggestions — the current slide in each theme and layout variations.
/// Rendered idea thumbnails, kept until the deck, slide or pane width changes (rendering eight
/// slides every frame made the pane cost ~25 ms a frame).
#[derive(Clone, Default)]
struct IdeaThumbs {
    key: (u64, u64, usize, u32),
    tex: Vec<(String, egui::TextureHandle, u32, u32)>,
}

/// Render per-slide Design Ideas and whole-presentation theme thumbnails; a click runs
/// `design.applyIdea` or `design.theme`.
pub(super) fn designer(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("Pick a design for this slide. Ideas are generated on your device.").color(t.text_dim));
    ui.add_space(6.0);
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let idx = st.selection.slide;
    let sel = st.selection.clone();
    let key_doc = (st.uid, st.revision);
    let w = ui.available_width() - 8.0;
    let ppp = ui.ctx().pixels_per_point();
    let px = (w * ppp).round().max(16.0) as u32;
    let ideas = deckcraft_engine::cmd::ideas::ideas_for(&doc, idx);
    let themes = deckcraft_model::theme::builtin_themes();
    let cache_id = egui::Id::unique("design-idea-thumbs");
    let key = (key_doc.0, key_doc.1, idx, px);
    let mut cache: IdeaThumbs = ui.ctx().data_mut(|d| d.get_temp(cache_id)).unwrap_or_default();
    if cache.key != key || cache.tex.is_empty() {
        cache = IdeaThumbs { key, tex: vec![] };
        let mut render = |name: String, d: &deckcraft_model::Presentation| {
            let (px, h) = crate::textures::fit_size(
                (px, (px as f64 * d.slide_size.height / d.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
                crate::textures::max_side(ui.ctx()),
            );
            let img = deckcraft_render::render_slide(
                d,
                idx,
                &deckcraft_render::RenderOpts { scale: px as f64 / d.slide_size.width.max(1.0), size: Some((px, h)), ..Default::default() },
            );
            let tex = crate::textures::upload_editor(ui.ctx(), format!("idea-{name}"), crate::textures::to_color_image(&img, false));
            cache.tex.push((name, tex, px, h));
        };
        for (id, _) in &ideas {
            let mut d = (*doc).clone();
            if deckcraft_engine::cmd::ideas::apply_idea(&mut d, &sel, idx, id).is_ok() {
                render(format!("layout:{id}"), &d);
            }
        }
        for th in &themes {
            let mut d = (*doc).clone();
            if let Some(m) = d.masters.first_mut() {
                std::sync::Arc::make_mut(m).theme = th.clone();
            }
            render(format!("theme:{}", th.name), &d);
        }
        ui.ctx().data_mut(|d| d.insert_temp(cache_id, cache.clone()));
    }
    let thumb = |ui: &mut Ui, name: &str, tip: &str| -> bool {
        let Some((_, tex, tw, th)) = cache.tex.iter().find(|e| e.0 == name) else { return false };
        let (r, resp) = ui.allocate_exact_size(vec2(w, w * *th as f32 / (*tw).max(1) as f32), Sense::click());
        // On the physical pixel grid, one texel per device pixel (#184).
        let at = crate::textures::device_pixel_rect(r, ppp);
        let at = Rect::from_min_size(at.min, vec2(*tw as f32 / ppp, *th as f32 / ppp));
        ui.painter().image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        ui.painter().rect_stroke(
            at,
            CornerRadius::same(2),
            Stroke::new(if resp.hovered() { 2.5 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
            egui::StrokeKind::Outside,
        );
        let clicked = resp.on_hover_text(tip).clicked();
        ui.add_space(8.0);
        clicked
    };
    if ideas.is_empty() {
        ui.label(egui::RichText::new("Add a title with a picture or some text to get layout ideas.").color(t.text_dim));
    }
    for (id, label) in &ideas {
        if thumb(ui, &format!("layout:{id}"), label) {
            run(app, "design.applyIdea", json!({"id": id}));
        }
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Themes (whole presentation)").color(t.text_dim));
    ui.add_space(4.0);
    for th in &themes {
        if thumb(ui, &format!("theme:{}", th.name), &th.name) {
            run(app, "design.theme", json!({"name": th.name}));
        }
    }
}
