//! Notices (after PhotoCraft's): a small stack of cards in the window's lower-right corner for
//! things to see that mustn't interrupt work: fonts a presentation uses that aren't installed,
//! and, in the canvas layout (which has no status bar), status and error messages. Each goes after
//! a few seconds or when closed; at most three show, the oldest dropping off.

use egui::{Align2, CornerRadius, Margin, Stroke, vec2};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// How long a notice stays, in milliseconds (errors a little longer).
const SHOW_MS: f64 = 8000.0;
const ERROR_MS: f64 = 12000.0;
/// At most this many notices show.
pub const MAX: usize = 3;

#[derive(Clone, Debug)]
pub struct Toast {
    pub title: String,
    pub text: String,
    pub error: bool,
    pub at: f64,
}

/// Show a notice (the same one again just refreshes it).
pub fn post(app: &mut SlideApp, title: &str, text: impl Into<String>, error: bool) {
    let text = text.into();
    let now = crate::now_ms();
    if let Some(n) = app.toasts.iter_mut().find(|n| n.title == title && n.text == text) {
        n.at = now;
        return;
    }
    app.toasts.push(Toast { title: title.into(), text, error, at: now });
    while app.toasts.len() > MAX {
        app.toasts.remove(0);
    }
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

/// Toast the active presentation's missing fonts, once per opened document.
pub fn note_missing_fonts(app: &mut SlideApp) {
    let Some(d) = app.session.active() else { return };
    if !app.fonts_noted.insert(d.uid) {
        return;
    }
    if let Some(text) = missing_fonts_message(&d.missing_fonts) {
        post(app, "Missing fonts", text, false);
    }
}

/// Draw the notices and drop the ones whose time is up, without disturbing the document.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let now = crate::now_ms();
    app.toasts.retain(|n| now - n.at <= if n.error { ERROR_MS } else { SHOW_MS });
    if app.toasts.is_empty() {
        return;
    }
    let t = Tokens::get(ctx);
    let mut close = None;
    // Above the floating toolbar / status bar, clear of the right panel's edge.
    egui::Area::new(egui::Id::unique("notices")).anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -76.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        ui.set_max_width(360.0);
        for (k, n) in app.toasts.iter().enumerate() {
            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, if n.error { t.guide } else { t.card_border }))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: t.shadow })
                .show(ui, |ui| {
                    ui.set_width(330.0);
                    ui.horizontal(|ui| {
                        let title = egui::RichText::new(&n.title).font(theme::bold(13.0)).color(if n.error { t.guide } else { t.text });
                        ui.add(egui::Label::new(title).wrap());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
                            if resp.hovered() {
                                ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                            }
                            icons::paint_mono(ui.painter(), r.shrink(4.0), Icon::Close, t.text_dim, false);
                            if resp.on_hover_text("Close").clicked() {
                                close = Some(k);
                            }
                        });
                    });
                    if !n.text.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&n.text).font(theme::font(12.5)).color(t.text_dim)).wrap());
                    }
                });
            ui.add_space(8.0);
        }
    });
    if let Some(k) = close {
        app.toasts.remove(k);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_stack_up_to_three_and_repeats_refresh() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        for k in 0..5 {
            post(&mut app, "Note", format!("number {k}"), false);
        }
        assert_eq!(app.toasts.len(), MAX);
        assert_eq!(app.toasts[0].text, "number 2", "the oldest dropped off");
        post(&mut app, "Note", "number 4", false);
        assert_eq!(app.toasts.len(), MAX, "the same notice isn't shown twice");
        // The canvas layout has no status bar: messages come as notices.
        app.ui.shell = crate::Shell::Canvas;
        app.set_status("Nothing to paste");
        assert!(app.toasts.iter().any(|n| n.text == "Nothing to paste"));
    }

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
