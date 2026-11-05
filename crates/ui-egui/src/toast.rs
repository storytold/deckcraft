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
        [a, b] => crate::i18n::format("{0} and {1}", &[a, b]),
        [a, b, c] => crate::i18n::format("{0}, {1} and {2}", &[a, b, c]),
        [a, b, c, rest @ ..] => {
            let n_str = rest.len().to_string();
            crate::i18n::format("{0}, {1}, {2} and {3} more", &[a, b, c, &n_str])
        }
    };
    let pattern = if fonts.len() == 1 {
        "{0} isn't installed on this computer. Text that uses it is shown in a similar font."
    } else {
        "{0} aren't installed on this computer. Text that uses them is shown in a similar font."
    };
    Some(crate::i18n::format(pattern, &[&names]))
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
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &crate::i18n::tr("Close")));
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                    }
                    icons::paint(ui.painter(), r.shrink(3.0), Icon::Close, t.text_dim, false);
                    if resp.on_hover_text(crate::i18n::tr("Close")).clicked() {
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

    #[test]
    fn the_message_names_the_fonts() {
        let f = |v: &[&str]| missing_fonts_message(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(f(&[]), None);
        assert_eq!(f(&["Aptos"]).unwrap(), "Aptos isn't installed on this computer. Text that uses it is shown in a similar font.");
        assert!(f(&["Aptos", "Sora"]).unwrap().starts_with("Aptos and Sora aren't installed"));
        assert!(f(&["A", "B", "C"]).unwrap().starts_with("A, B and C aren't"));
        assert!(f(&["A", "B", "C", "D", "E"]).unwrap().starts_with("A, B, C and 2 more aren't"));
    }

    #[test]
    fn test_missing_fonts_message_portuguese() {
        let f = |v: &[&str]| missing_fonts_message(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        crate::i18n::set_current(crate::i18n::Language::PtBr);
        assert_eq!(
            f(&["Aptos"]).unwrap(),
            "A fonte Aptos não está instalada neste computador. O texto que a utiliza é exibido em uma fonte semelhante."
        );
        assert_eq!(
            f(&["Aptos", "Sora"]).unwrap(),
            "As fontes Aptos e Sora não estão instaladas neste computador. O texto que as utiliza é exibido em uma fonte semelhante."
        );
        assert_eq!(
            f(&["A", "B", "C", "D", "E"]).unwrap(),
            "As fontes A, B, C e mais 2 não estão instaladas neste computador. O texto que as utiliza é exibido em uma fonte semelhante."
        );
        crate::i18n::set_current(crate::i18n::Language::En);
    }
}
