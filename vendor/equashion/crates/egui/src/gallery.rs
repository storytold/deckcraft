//! The Equation tab's galleries for a host's ribbon: structure galleries, symbol sets and
//! ready-made equations, drawn with the same bundled-font faces as the palette and inserted
//! into this editor. The host owns the buttons and menus; it calls these inside them:
//!
//! ```ignore
//! ui.menu_button("Fraction", |ui| {
//!     if editor.gallery_ui(ui, "fraction") { ui.close(); }
//! });
//! ```
//!
//! Data: [`equashion_math::galleries`].

use egui::{Ui, vec2};
use equashion_math::{Editor, Math, Node, galleries};

use crate::{EquationEditor, palette};

/// Faces per row in a gallery menu.
const COLUMNS_W: f32 = 340.0;

impl EquationEditor {
    /// A rendered face of `math` at `pt` points: its texture and size in points (cached per
    /// `key` and pixel density). The face is white on transparent; paint it with the colour you
    /// want as the tint. For hosts that draw their own tiles and buttons.
    pub fn face(&mut self, ui: &Ui, key: &str, math: &Math, pt: f64) -> Option<(egui::TextureId, egui::Vec2)> {
        let sym = self.symbol(ui, key, math, pt)?;
        Some((sym.texture_id(), sym.size()))
    }

    /// Insert a gallery template at the caret (a script or fraction takes the term before it)
    /// and keep typing in the canvas.
    pub fn insert_gallery_template(&mut self, ui: &Ui, template: &Math) {
        self.palette_insert(ui, Some(template.clone()), None);
    }

    /// Insert a symbol at the caret and keep typing in the canvas.
    pub fn insert_gallery_symbol(&mut self, ui: &Ui, c: char) {
        self.palette_insert(ui, None, Some(c));
    }

    /// Replace the whole equation (a ready-made one) and keep typing in the canvas. Unlike
    /// [`EquationEditor::set_math`] this is an edit: Esc would discard it.
    pub fn replace_equation(&mut self, ui: &Ui, m: Math) {
        let (para, jc) = (self.ed.math().para, self.ed.math().jc.clone());
        let mut m = m;
        m.para = para;
        m.jc = jc;
        self.ed.set_math(m);
        self.wants_canvas = true;
        let id = self.canvas_id();
        ui.memory_mut(|mem| mem.request_focus(id));
        self.tree_edited();
        self.touched = ui.input(|i| i.time);
    }

    /// The equation is a display equation (on a line of its own).
    pub fn is_display(&self) -> bool {
        self.ed.math().para
    }

    /// Show the equation on a line of its own (`true`) or in the line of text.
    pub fn set_display(&mut self, display: bool) {
        if self.ed.math().para != display {
            self.ed.math_mut().para = display;
            self.tree_edited();
        }
    }

    /// The display equation's justification (`left`, `right`, `center`, `centerGroup`).
    pub fn justification(&self) -> Option<&str> {
        self.ed.math().jc.as_deref()
    }

    /// Justify a display equation; `None` is the default (centred as group).
    pub fn set_justification(&mut self, jc: Option<&str>) {
        let jc = jc.map(str::to_string);
        if self.ed.math().jc != jc {
            self.ed.math_mut().jc = jc;
            self.tree_edited();
        }
    }

    /// A gallery's representative face (its first template, empty places as boxes) at `pt`
    /// points, as a texture and its size in points, for a host that draws its own button.
    /// Paint it with the button's text colour as the tint.
    pub fn gallery_face(&mut self, ui: &Ui, gallery_id: &str, pt: f64) -> Option<(egui::TextureId, egui::Vec2)> {
        let t = galleries().structure(gallery_id)?.sections.first()?.templates.first()?;
        let face = Editor::new(t.math.clone()).display_math();
        let sym = self.symbol(ui, &format!("face/{gallery_id}"), &face, pt)?;
        Some((sym.texture_id(), sym.size()))
    }

    /// One structure gallery (`"fraction"`, `"script"`, `"radical"`, `"integral"`,
    /// `"largeOperator"`, `"bracket"`, `"function"`, `"accent"`, `"limitLog"`, `"operator"`,
    /// `"matrix"`): its sections, each a heading over a wrapped run of template faces. Inserts
    /// the chosen template at the caret; true when one was.
    pub fn gallery_ui(&mut self, ui: &mut Ui, gallery_id: &str) -> bool {
        let Some(g) = galleries().structure(gallery_id) else { return false };
        let mut chosen: Option<Math> = None;
        ui.set_max_width(COLUMNS_W);
        for s in &g.sections {
            ui.label(egui::RichText::new(&s.name).size(11.0).strong().color(ui.visuals().weak_text_color()));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                for t in &s.templates {
                    // The template with its empty places shown as boxes.
                    let face = Editor::new(t.math.clone()).display_math();
                    let Some(sym) = self.symbol(ui, &format!("gallery/{}", t.id), &face, 14.0) else { continue };
                    let hover = format!("{}  ({})", t.label, t.linear);
                    if palette::button(ui, sym, vec2(34.0, 30.0), 24.0, &t.label, &hover).clicked() {
                        chosen = Some(t.math.clone());
                    }
                }
            });
            ui.add_space(4.0);
        }
        let hit = chosen.is_some();
        self.palette_insert(ui, chosen, None);
        hit
    }

    /// The names of the symbol sets, in order (index them with [`EquationEditor::symbols_ui`]).
    pub fn symbol_set_names() -> Vec<&'static str> {
        galleries().symbols.iter().map(|s| s.name.as_str()).collect()
    }

    /// One symbol set as a wrapped run of faces. Inserts the chosen symbol at the caret; true
    /// when one was.
    pub fn symbols_ui(&mut self, ui: &mut Ui, set: usize) -> bool {
        let Some(s) = galleries().symbols.get(set) else { return false };
        let mut chosen: Option<char> = None;
        ui.set_max_width(COLUMNS_W);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for c in s.chars.chars() {
                let text = c.to_string();
                let face = Math::new(vec![Node::Text(text.clone())]);
                let Some(sym) = self.symbol(ui, &format!("sym-{}", c as u32), &face, 15.0) else { continue };
                let hover = format!("U+{:04X}", c as u32);
                if palette::button(ui, sym, vec2(28.0, 28.0), 22.0, &text, &hover).clicked() {
                    chosen = Some(c);
                }
            }
        });
        let hit = chosen.is_some();
        self.palette_insert(ui, None, chosen);
        hit
    }

    /// The ready-made equations as a list (face and description). Replaces the equation with
    /// the chosen one; true when one was.
    pub fn builtins_ui(&mut self, ui: &mut Ui) -> bool {
        let mut chosen: Option<Math> = None;
        for t in &galleries().builtins {
            let Some(sym) = self.symbol(ui, &format!("builtin/{}", t.id), &t.math, 15.0) else { continue };
            if palette::button(ui, sym, vec2(220.0, 34.0), 30.0, &t.label, &t.label).clicked() {
                chosen = Some(t.math.clone());
            }
        }
        let hit = chosen.is_some();
        if let Some(m) = chosen {
            self.set_math(m);
            self.wants_canvas = true;
            let id = self.canvas_id();
            ui.memory_mut(|mem| mem.request_focus(id));
        }
        hit
    }
}
