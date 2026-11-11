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
    /// Stays until the user closes it: a failure that must not scroll away unseen.
    pub sticky: bool,
}

/// The notice for fonts a presentation uses that aren't installed, or `None` when all are.
pub fn missing_fonts_message(fonts: &[String]) -> Option<String> {
    let names = match fonts {
        [] => return None,
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, c, rest @ ..] => format!("{a}, {b}, {c} and {} more", rest.len()),
    };
    let (it, them) = if fonts.len() == 1 { ("isn't", "it") } else { ("aren't", "them") };
    Some(format!("{names} {it} installed on this computer. Text that uses {them} is shown in a similar font."))
}

/// Show `text` for a few seconds — unless a sticky notice is up. That one is a failure the user
/// must read, so a transient note waits instead of replacing it unseen.
pub fn note(app: &mut SlideApp, text: impl Into<String>) {
    let text = text.into();
    if app.toast.as_ref().is_some_and(|t| t.sticky) {
        // Wait behind the failure the user must read, so an open error or a font note is not lost.
        if app.toast_queue.len() < 8 && !app.toast_queue.contains(&text) {
            app.toast_queue.push(text);
        }
        return;
    }
    app.toast = Some(Toast { text, at: crate::now_ms(), sticky: false });
}

/// Toast the active presentation's missing fonts, once per opened document.
pub fn note_missing_fonts(app: &mut SlideApp) {
    let Some(d) = app.session.active() else { return };
    if !app.fonts_noted.insert(d.uid) {
        return;
    }
    if let Some(text) = missing_fonts_message(&d.missing_fonts) {
        note(app, text);
    }
}

/// Show the notice that waited behind a sticky one, oldest first.
pub(crate) fn show_queued(app: &mut SlideApp) {
    if app.toast.is_some() || app.toast_queue.is_empty() {
        return;
    }
    let text = app.toast_queue.remove(0);
    app.toast = Some(Toast { text, at: crate::now_ms(), sticky: false });
}

/// Render and dismiss the active transient toast without disturbing the document.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(toast) = &app.toast else {
        show_queued(app);
        return;
    };
    if !toast.sticky && crate::now_ms() - toast.at > SHOW_MS {
        app.toast = None;
        show_queued(app);
        return;
    }
    let t = Tokens::get(ctx);
    let text = toast.text.clone();
    let mut close = false;
    egui::Area::new(egui::Id::unique("toast")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -44.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.card_border))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(14, 10))
            .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: t.shadow })
            .show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&text).font(theme::font(13.0)).color(t.text)).wrap());
                    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
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
        show_queued(app);
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_fonts() {
        let f = |v: &[&str]| missing_fonts_message(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(f(&[]), None);
        assert_eq!(f(&["Aptos"]).unwrap(), "Aptos isn't installed on this computer. Text that uses it is shown in a similar font.");
        assert!(f(&["Aptos", "Sora"]).unwrap().starts_with("Aptos and Sora aren't installed"));
        assert!(f(&["A", "B", "C"]).unwrap().starts_with("A, B and C aren't"));
        assert!(f(&["A", "B", "C", "D", "E"]).unwrap().starts_with("A, B, C and 2 more aren't"));
    }
}
