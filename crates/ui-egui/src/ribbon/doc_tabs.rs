//! A strip of tabs under the title bar, one per open presentation. It appears once two or more
//! are open, so a single-deck session looks as before. Clicking a tab shows that presentation;
//! its ✕ closes it (asking first when it has unsaved changes).

use egui::{CornerRadius, Rect, Sense, Ui, pos2, vec2};

use super::{Align2, SlideApp, Tokens, theme};

pub const TABS_H: f32 = 30.0;
const TAB_MIN: f32 = 90.0;
const TAB_MAX: f32 = 200.0;

/// What a tab shows: the title and whether it has unsaved changes.
pub(super) fn tab_labels(app: &SlideApp) -> Vec<(String, bool)> {
    app.session.documents().iter().map(|d| (d.title(), d.is_dirty())).collect()
}

/// The tab strip. Draws nothing unless two or more presentations are open.
pub fn doc_tabs(app: &mut SlideApp, ui: &mut Ui) {
    let labels = tab_labels(app);
    if labels.len() < 2 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let active = app.session.active_index();
    let mut switch_to = None;
    let mut close = None;
    egui::Panel::top("doc_tabs").exact_size(TABS_H).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect().shrink2(vec2(8.0, 2.0));
        let n = labels.len() as f32;
        let w = ((rect.width() - 4.0 * (n - 1.0)) / n).clamp(TAB_MIN, TAB_MAX);
        for (i, (title, dirty)) in labels.iter().enumerate() {
            let x = rect.min.x + i as f32 * (w + 4.0);
            let tab = Rect::from_min_size(pos2(x, rect.min.y), vec2(w, rect.height()));
            if tab.max.x > rect.max.x + 0.5 {
                break; // more tabs than fit: Next Window (⌘`) still reaches the rest
            }
            let on = active == Some(i);
            let resp = ui.interact(tab, ui.scope_id().with(("doc_tab", i)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Tab, true, title.as_str()));
            let fill = if on {
                t.card
            } else if resp.hovered() {
                t.hover
            } else {
                t.chrome
            };
            let radius = CornerRadius::same(theme::radius::CONTROL);
            ui.painter().rect_filled(tab, radius, fill);
            if on {
                ui.painter().rect_stroke(tab, radius, egui::Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
            }
            let label = if *dirty { format!("● {title}") } else { title.clone() };
            let text_rect = Rect::from_min_max(tab.min + vec2(10.0, 0.0), pos2(tab.max.x - 26.0, tab.max.y));
            let galley = ui.painter().layout_no_wrap(label, theme::font(12.5), if on { t.text } else { t.text_dim });
            let pos = pos2(text_rect.min.x, text_rect.center().y - galley.size().y / 2.0);
            ui.painter().with_clip_rect(text_rect).galley(pos, galley, t.text);
            // The close button.
            let x_rect = Rect::from_center_size(pos2(tab.max.x - 14.0, tab.center().y), vec2(18.0, 18.0));
            let x_resp = ui.interact(x_rect, ui.scope_id().with(("doc_tab_x", i)), Sense::click());
            x_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Close {title}")));
            if x_resp.hovered() {
                ui.painter().rect_filled(x_rect, CornerRadius::same(theme::radius::SMALL), t.selected);
            }
            ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "✕", theme::font(10.0), t.text_dim);
            if x_resp.clicked() {
                close = Some(i);
            } else if resp.clicked() {
                switch_to = Some(i);
            }
        }
    });
    if let Some(i) = close {
        app.session.set_active(i);
        app.close_presentation();
    } else if let Some(i) = switch_to {
        app.session.set_active(i);
    }
}
