//! The template palette. Every button face (structures, Greek letters, operators) is a small
//! equation laid out and rasterised by equashion-render from the bundled fonts, so the palette
//! never depends on the host's egui fonts having the symbols.

use egui::{Sense, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};
use equashion_layout::{Rgb, layout_equation};
use equashion_math::{Editor, Math, Node, templates, to_linear};

use crate::{EquationEditor, Tab};

/// Palette symbols: Greek letters and operators (WordCraft's sets).
pub const GREEK_LOWER: &str = "αβγδεζηθικλμνξοπρστυφχψω";
pub const GREEK_UPPER: &str = "ΓΔΘΛΞΠΣΦΨΩ";
pub const OPERATORS: &str = "±∓×÷·∘∗≠≈≡≅∼∝≤≥≪≫≺≻∞∂∇∅∈∉⊂⊃⊆⊇∪∩∧∨¬∀∃→←↔⇒⇔↦∴∵…⋯°′″";

/// The template groups, in palette order.
const GROUPS: [&str; 8] = ["Fractions", "Scripts", "Radicals", "Large operators", "Brackets", "Matrices", "Functions", "Accents"];

/// A rendered button face.
pub(crate) struct Symbol {
    tex: TextureHandle,
    /// Size of the face in points.
    size: Vec2,
}

impl Symbol {
    pub(crate) fn texture_id(&self) -> egui::TextureId {
        self.tex.id()
    }

    pub(crate) fn size(&self) -> Vec2 {
        self.size
    }
}

/// The short accessible label of a structure's palette button (WordCraft's button faces).
pub fn label(id: &str) -> Option<&'static str> {
    Some(match id {
        "frac" => "a/b",
        "sup" => "x^n",
        "sub" => "x_n",
        "subsup" => "x_n^m",
        "sqrt" => "\u{221A}",
        "root" => "n\u{221A}",
        "sum" => "\u{2211}",
        "prod" => "\u{220F}",
        "int" => "\u{222B}",
        "intlim" => "\u{222B}ab",
        "paren" => "( )",
        "bracket" => "[ ]",
        "brace" => "{ }",
        "abs" => "| |",
        "matrix2" => "2\u{00D7}2",
        "matrix3" => "3\u{00D7}3",
        "bmatrix2" => "[2\u{00D7}2]",
        "bmatrix3" => "[3\u{00D7}3]",
        "cases" => "{ cases",
        "overline" => "over",
        "underline" => "under",
        "overbrace" => "brace",
        _ => return None,
    })
}

/// A palette button: the rendered face tinted with the widget text colour, the egui button
/// frame, and an accessible label.
pub(crate) fn button(ui: &mut Ui, sym: &Symbol, min: Vec2, max_face_h: f32, label: &str, hover: &str) -> egui::Response {
    let k = (max_face_h / sym.size.y.max(1.0)).min(1.0);
    let face = sym.size * k;
    let pad = vec2(6.0, 3.0);
    let size = vec2((face.x + 2.0 * pad.x).max(min.x), min.y.max(face.y + 2.0 * pad.y));
    // Clickable but not focusable: a click must not take the keyboard from the canvas.
    let (rect, resp) = ui.allocate_exact_size(size, Sense::CLICK);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if ui.is_rect_visible(rect) {
        let vis = ui.style().interact(&resp);
        ui.painter().rect(rect.expand(vis.expansion), vis.corner_radius, vis.weak_bg_fill, vis.bg_stroke, egui::StrokeKind::Inside);
        let r = egui::Rect::from_center_size(rect.center(), face);
        ui.painter().image(sym.tex.id(), r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), vis.text_color());
    }
    resp.on_hover_text(hover)
}

impl EquationEditor {
    /// The face of `math` at `pt` points, rendered once per pixel density and cached.
    pub(crate) fn symbol(&mut self, ui: &Ui, key: &str, math: &Math, pt: f64) -> Option<&Symbol> {
        let ppp = ui.ctx().pixels_per_point();
        let ck = format!("{key}@{ppp}");
        if !self.palette.contains_key(&ck) {
            let l = layout_equation(&*self.fonts, math, &self.family, pt, false, Rgb::WHITE, 1.0);
            let scale = f64::from(ppp.max(1.0)) * 2.0;
            let img = equashion_render::render_scene(&l.scene, &*self.fonts, scale, None);
            let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            let tex = ui.ctx().load_texture(format!("equashion/palette/{key}"), ci, TextureOptions::LINEAR);
            self.palette.insert(ck.clone(), Symbol { tex, size: vec2(l.width() as f32, l.height() as f32) });
        }
        self.palette.get(&ck)
    }

    pub(crate) fn palette_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (tab, name) in [(Tab::Structures, "Structures"), (Tab::Greek, "Greek"), (Tab::Operators, "Operators")] {
                if ui.selectable_label(self.tab == tab, name).clicked() {
                    self.tab = tab;
                }
            }
        });
        let mut chosen_template: Option<Math> = None;
        let mut chosen_symbol: Option<char> = None;
        match self.tab {
            Tab::Structures => {
                // Every group at once, compact buttons in one flow; the full name is the tooltip.
                let all = templates();
                for g in GROUPS {
                    ui.horizontal_wrapped(|ui| {
                        let dim = ui.visuals().weak_text_color();
                        ui.add_sized(vec2(92.0, 26.0), egui::Label::new(egui::RichText::new(g).size(11.0).strong().color(dim)));
                        for tpl in all.iter().filter(|t| t.group == g) {
                            // The template with its empty places shown as boxes.
                            let face = Editor::new(tpl.math.clone()).display_math();
                            let Some(sym) = self.symbol(ui, tpl.id, &face, 13.0) else { continue };
                            let name = label(tpl.id).unwrap_or(tpl.label);
                            let hover = format!("{}  ({})", tpl.label, to_linear(&tpl.math));
                            if button(ui, sym, vec2(30.0, 28.0), 22.0, name, &hover).clicked() {
                                chosen_template = Some(tpl.math.clone());
                            }
                        }
                    });
                }
            }
            Tab::Greek | Tab::Operators => {
                let sets: &[&str] = if self.tab == Tab::Greek { &[GREEK_LOWER, GREEK_UPPER] } else { &[OPERATORS] };
                for chars in sets {
                    ui.horizontal_wrapped(|ui| {
                        for c in chars.chars() {
                            let s = c.to_string();
                            let face = Math::new(vec![Node::Text(s.clone())]);
                            let Some(sym) = self.symbol(ui, &format!("sym-{}", c as u32), &face, 15.0) else { continue };
                            let hover = format!("U+{:04X}", c as u32);
                            if button(ui, sym, vec2(28.0, 28.0), 22.0, &s, &hover).clicked() {
                                chosen_symbol = Some(c);
                            }
                        }
                    });
                }
            }
        }
        self.palette_insert(ui, chosen_template, chosen_symbol);
    }
}
