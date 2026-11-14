//! Brief notices over the window's bottom edge: shown once, gone after a few seconds or when
//! closed. Used to say a presentation's fonts aren't installed when it is first opened.

use egui::{Align2, CornerRadius, Margin, Stroke, vec2};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// How long a toast stays, in milliseconds.
const SHOW_MS: f64 = 8000.0;

pub struct Toast {
    pub text: String,
    pub at: f64,
}

/// The notice for fonts a presentation uses that aren't installed, or `None` when all are.
pub fn missing_fonts_message(fonts: &[String]) -> Option<String> {
    let names = match fonts {
        [] => return None,
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        list => {
            let (last, rest) = list.split_last()?;
            let joined = rest.join(", ");
            format!("{joined} and {last}")
        }
    };
    let (it, them) = if fonts.len() == 1 { ("isn't", "it") } else { ("aren't", "them") };
    Some(format!("{names} {it} installed on this computer. Text that uses {them} is shown in a similar font."))
}

/// Toast the active presentation's missing fonts, once per opened document.
pub fn note_missing_fonts(app: &mut SlideApp) {
    let Some(d) = app.session.active() else { return };
    if !app.fonts_noted.insert(d.uid) {
        return;
    }
    if let Some(text) = missing_fonts_message(&d.missing_fonts) {
        app.toast = Some(Toast { text, at: crate::now_ms() });
    }
}

/// Render and dismiss the active transient toast without disturbing the document.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(toast) = &app.toast else { return };
    if crate::now_ms() - toast.at > SHOW_MS {
        app.toast = None;
        return;
    }
    let t = Tokens::get(ctx);
    let text = toast.text.clone();
    let mut close = false;
    let viewport = ctx.content_rect();
    // Include the frame margins/stroke and leave room above the bottom-edge anchor.
    let width = (viewport.width() - 46.0).clamp(0.0, 520.0);
    let message_height = (viewport.height() - 110.0).clamp(0.0, 280.0);
    egui::Area::new(egui::Id::unique("toast")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -44.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.card_border))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(14, 10))
            .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: t.shadow })
            .show(ui, |ui| {
                ui.set_width(width);
                egui::ScrollArea::vertical().max_height(message_height).min_scrolled_height(0.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&text).font(theme::font(13.0)).color(t.text)).wrap());
                });
                // Controls get their own row, outside the scroll area: the label cannot
                // consume their width or scroll them out of view on a small viewport.
                ui.horizontal(|ui| {
                    // Copy button
                    let (copy_r, copy_resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                    copy_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Copy"));
                    if copy_resp.hovered() {
                        ui.painter().rect_filled(copy_r, CornerRadius::same(4), t.hover);
                    }
                    if copy_resp.has_focus() {
                        ui.painter().rect_stroke(copy_r, CornerRadius::same(4), Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                    }
                    icons::paint(ui.painter(), copy_r.shrink(3.0), Icon::Copy, t.text_dim, false);
                    if copy_resp.on_hover_text("Copy warning to clipboard").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                    ui.add_space(4.0);
                    // Close button
                    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                    }
                    if resp.has_focus() {
                        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                    }
                    icons::paint(ui.painter(), r.shrink(3.0), Icon::Close, t.text_dim, false);
                    if resp.on_hover_text("Close").clicked() {
                        close = true;
                    }
                });
            });
    });
    if close {
        app.toast = None;
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_engine::Session;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness(size: egui::Vec2, text: String) -> Harness<'static, SlideApp> {
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.toast = Some(Toast { text, at: crate::now_ms() });
        let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app| show(app, ui.ctx()), app);
        h.run_steps(3);
        h
    }

    #[test]
    fn long_warnings_keep_the_message_and_controls_in_the_viewport() {
        let fonts: Vec<String> = (0..1000).map(|i| format!("MissingFont{i}")).collect();
        let text = missing_fonts_message(&fonts).unwrap();
        for size in [vec2(240.0, 200.0), vec2(800.0, 600.0)] {
            let h = harness(size, text.clone());
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for name in ["Copy", "Close"] {
                assert!(viewport.contains_rect(h.get_by_role_and_label(egui::Role::Button, name).rect()), "{name} must remain visible");
            }
            let warning = h
                .output()
                .shapes
                .iter()
                .find(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.text().starts_with("MissingFont0")))
                .expect("the warning must be painted");
            assert!(warning.clip_rect.height() <= 280.0);
            assert!(viewport.contains_rect(warning.clip_rect));
        }
    }

    #[test]
    fn copy_shows_keyboard_focus_and_copies_the_complete_warning() {
        let text = "Missing fonts: Alpha, Beta and Gamma".to_string();
        let mut h = harness(vec2(320.0, 240.0), text.clone());
        h.key_press(egui::Key::Tab);
        h.run_steps(2);
        let copy = h.get_by_role_and_label(egui::Role::Button, "Copy").rect();
        let accent = Tokens::get(&h.ctx).accent;
        assert!(h.get_by_role_and_label(egui::Role::Button, "Copy").is_focused(), "Tab must focus Copy");
        assert!(
            h.output().shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::epaint::Shape::Rect(r) if r.rect == copy && r.stroke.color == accent && r.stroke.width >= 1.0)
            }),
            "Copy must paint its keyboard focus"
        );
        h.key_down(egui::Key::Enter);
        h.run_steps(1);
        assert!(
            h.output().platform_output.commands.iter().any(|command| { matches!(command, egui::OutputCommand::CopyText(copied) if copied == &text) })
        );
        h.key_up(egui::Key::Enter);
        h.get_by_role_and_label(egui::Role::Button, "Close").click();
        h.run_steps(1);
        assert!(h.state().toast.is_none());
    }

    #[test]
    fn the_message_names_the_fonts() {
        let f = |v: &[&str]| missing_fonts_message(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(f(&[]), None);
        assert_eq!(f(&["Aptos"]).unwrap(), "Aptos isn't installed on this computer. Text that uses it is shown in a similar font.");
        assert!(f(&["Aptos", "Sora"]).unwrap().starts_with("Aptos and Sora aren't installed"));
        assert!(f(&["A", "B", "C"]).unwrap().starts_with("A, B and C aren't"));
        assert!(f(&["A", "B", "C", "D", "E"]).unwrap().starts_with("A, B, C, D and E aren't"));
    }
}
