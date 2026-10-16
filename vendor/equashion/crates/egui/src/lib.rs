//! equaSHion egui widget: an equation editor you type the formatted equation straight into.
//!
//! [`EquationEditor`] draws a template palette, a canvas with the laid-out equation and a caret
//! inside it, and a linear-text field. Typing `x`, `^`, `2` raises the 2 at once and leaves the
//! caret in the exponent; `/` makes a fraction, `sqrt` a root, Greek names become symbols, the
//! arrow keys and Tab walk the tree, a click places the caret and a drag or shift-click selects.
//! Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y undo and redo, Ctrl+C / Ctrl+X copy and cut the selection as
//! linear text, Ctrl+A selects all, Enter commits and Esc cancels (asking first when there are
//! unsaved changes). The editing itself is `equashion_math::Editor`; this crate only draws it
//! and turns egui input into editor calls.
//!
//! The widget is safe on an `egui::Context` that other code also uses:
//! - it never calls `Context::set_fonts`, `set_visuals` or `set_style`, and only uses
//!   `FontId::proportional` / `FontId::monospace` (always bound) for its own text;
//! - equation glyphs, including every palette symbol, are rasterised by `equashion-render` from
//!   the bundled fonts, so nothing depends on the host's egui fonts having math operators;
//! - its colours come from `ui.visuals()`, so it follows a light or dark host;
//! - its egui ids derive from the widget's own [`egui::Id`] and its texture names start with
//!   `equashion/`;
//! - it only reads keyboard input while its canvas has focus, and only takes focus back when
//!   no other widget holds it.
//!
//! ```no_run
//! # fn ui(ui: &mut egui::Ui, editor: &mut equashion_egui::EquationEditor) {
//! let r = editor.show(ui);
//! if r.committed {
//!     println!("{}", editor.omml());
//! }
//! # }
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod gallery;
mod palette;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use egui::{FontId, Key, Pos2, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use equashion_layout::{DEFAULT_FAMILY, EqLayout, FontDb, Rgb, layout_equation};
use equashion_math::{Caret, Editor, Key as MKey, Math, from_linear, from_omml, to_linear, to_omml};

pub use equashion_math;

/// Font size of the equation on the canvas, points.
pub const DEFAULT_FONT_SIZE: f64 = 28.0;
/// Default height of the canvas, points.
pub const DEFAULT_CANVAS_HEIGHT: f32 = 120.0;
/// Space around the equation inside the canvas layout, points.
const EQ_PAD: f64 = 8.0;
/// Largest render scale of the canvas texture (pixels per point).
const MAX_SCALE: f64 = 4.0;
/// Largest side of the canvas texture, pixels (egui's guaranteed minimum is 2048).
const MAX_TEXTURE: f64 = 2000.0;
/// Gap between the canvas edge and the equation, points.
const VIEW_MARGIN: f32 = 10.0;
/// Smallest scale a long equation is shrunk to before the canvas scrolls instead.
const MIN_SCALE: f32 = 0.5;
/// Seconds per half blink of the caret.
const BLINK: f64 = 0.5;
/// The message shown after a first Esc with unsaved changes.
pub const DISCARD_PROMPT: &str = "Press Esc again to discard this equation, or Enter to keep it.";

/// The bundled fonts (Source Serif 4, Inter and the math fallback), loaded once per process.
pub fn bundled_fonts() -> Arc<FontDb> {
    static DB: OnceLock<Arc<FontDb>> = OnceLock::new();
    DB.get_or_init(|| Arc::new(FontDb::bundled_only())).clone()
}

/// What happened in one [`EquationEditor::show`] call.
#[derive(Debug)]
pub struct EditorResponse {
    /// The equation (tree or display flag) changed this frame.
    pub changed: bool,
    /// Enter was pressed in the canvas or the linear field: the host should keep the equation.
    pub committed: bool,
    /// Esc was pressed and confirmed (at once when nothing changed since the last load or
    /// commit, otherwise on the second press): the host should drop the edit. The widget
    /// keeps its content; call [`EquationEditor::revert`] to go back to the loaded equation.
    pub cancelled: bool,
    /// The canvas: focus, hover and the rectangle typing goes to.
    pub response: egui::Response,
    /// The whole widget (palette, canvas, linear field, message line).
    pub rect: egui::Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Structures,
    Greek,
    Operators,
}

/// The equation as drawn: the tree it was drawn from, its layout and the texture.
struct Canvas {
    shown: Math,
    layout: EqLayout,
    tex: TextureHandle,
    /// The render scale asked for (pixels per point; the texture may be smaller for size).
    want: f64,
}

/// The equation editor widget. Keep one per edited equation across frames and call
/// [`EquationEditor::show`] every frame.
pub struct EquationEditor {
    id: egui::Id,
    fonts: Arc<FontDb>,
    family: String,
    font_size: f64,
    canvas_height: f32,
    show_palette: bool,
    show_linear: bool,
    /// Just the equation: no palette, linear field or message line, no background, sized to the
    /// equation, for editing in place on the host's page.
    inline: bool,
    /// Lay the equation out in display style (large operators, limits above and below) or text
    /// style, as the host's own drawing of it is. Default display, as a dialog shows it.
    display_style: bool,
    /// With `inline`: the host draws the equation itself (it is the document's own); the widget
    /// draws only the caret, selection and active place over it.
    overlay: bool,
    /// The idle caret blinks (off for hosts that honour reduced motion).
    caret_blink: bool,
    ed: Editor,
    /// Text of the linear field.
    linear: String,
    /// The equation as last loaded or committed, to tell whether Esc would lose work.
    initial: Math,
    tab: Tab,
    /// Status line (the discard prompt).
    message: Option<String>,
    canvas: Option<Canvas>,
    palette: HashMap<String, palette::Symbol>,
    /// Frames left in which the canvas asks for the keyboard.
    auto_focus: u8,
    /// Typing goes to the canvas: it takes the keyboard back while no widget holds it, so a
    /// click on a palette button does not leave typing dead.
    wants_canvas: bool,
    /// The canvas or the linear field had the keyboard at the end of the last frame.
    focus_within: bool,
    /// Time of the last edit, so the caret stays solid while typing.
    touched: f64,
    /// True while the mouse button that started in the canvas is held.
    dragging: bool,
    /// Where the mouse press (or the shift-click's fixed end) is: selections run from here.
    origin: Caret,
    /// Esc was pressed once with unsaved work: the next one cancels.
    discard_armed: bool,
    /// Where the canvas was drawn last frame: top-left of the equation and points per layout point.
    view: (Pos2, f32),
}

impl std::fmt::Debug for EquationEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EquationEditor").field("id", &self.id).field("linear", &self.linear).field("caret", self.ed.caret()).finish_non_exhaustive()
    }
}

impl Default for EquationEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl EquationEditor {
    /// A blank editor using the bundled fonts. Its [`egui::Id`] is unique to this instance;
    /// use [`EquationEditor::id_salt`] for a stable one.
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        EquationEditor {
            id: egui::Id::new(("equashion-editor", n)),
            fonts: bundled_fonts(),
            family: DEFAULT_FAMILY.to_string(),
            font_size: DEFAULT_FONT_SIZE,
            canvas_height: DEFAULT_CANVAS_HEIGHT,
            show_palette: true,
            show_linear: true,
            inline: false,
            overlay: false,
            display_style: true,
            caret_blink: true,
            ed: Editor::new(Math::default()),
            linear: String::new(),
            initial: Math::default(),
            tab: Tab::Structures,
            message: None,
            canvas: None,
            palette: HashMap::new(),
            auto_focus: 4,
            wants_canvas: true,
            focus_within: false,
            touched: 0.0,
            dragging: false,
            origin: Caret::default(),
            discard_armed: false,
            view: (pos2(0.0, 0.0), 1.0),
        }
    }

    /// An editor on the equation typed as linear text (`x^2`, `\frac{a}{b}`, `sqrt(x)`).
    pub fn with_linear(s: &str) -> Self {
        let mut e = Self::new();
        e.set_linear(s);
        e
    }

    /// An editor on an OMML equation (`m:oMath` or `m:oMathPara`). OMML import never fails:
    /// unknown elements are kept verbatim and broken input gives what could be read.
    pub fn with_omml(xml: &str) -> Self {
        let mut e = Self::new();
        e.set_math(from_omml(xml));
        e
    }

    /// Derive the widget's egui ids from `salt` instead of a per-instance counter.
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id = egui::Id::new(("equashion-editor", salt));
        self.canvas = None;
        self
    }

    /// Lay out with these fonts instead of [`bundled_fonts`].
    pub fn fonts(mut self, fonts: Arc<FontDb>) -> Self {
        self.fonts = fonts;
        self.canvas = None;
        self.palette.clear();
        self
    }

    /// The font family of the equation (default [`DEFAULT_FAMILY`], "Source Serif 4").
    pub fn family(mut self, family: &str) -> Self {
        self.family = family.to_string();
        self.canvas = None;
        self
    }

    /// The equation's font size on the canvas, points (default 28).
    pub fn font_size(mut self, pt: f64) -> Self {
        self.font_size = if pt.is_finite() { pt.clamp(6.0, 200.0) } else { DEFAULT_FONT_SIZE };
        self.canvas = None;
        self
    }

    /// The canvas height, points (default 120).
    pub fn canvas_height(mut self, h: f32) -> Self {
        self.canvas_height = if h.is_finite() { h.clamp(40.0, 2000.0) } else { DEFAULT_CANVAS_HEIGHT };
        self
    }

    /// Edit in place: only the canvas, no background, as large as the equation. The host puts it
    /// where the equation sits (see [`EquationEditor::inline_pad`]) and draws what is behind it.
    pub fn inline(mut self, on: bool) -> Self {
        self.inline = on;
        self
    }

    /// Display style (the default) or text style, to match how the host draws the equation.
    pub fn display_style(mut self, display: bool) -> Self {
        self.display_style = display;
        self
    }

    /// [`EquationEditor::display_style`] on an existing editor: the next frame lays out again.
    pub fn set_display_style(&mut self, display: bool) {
        if self.display_style != display {
            self.display_style = display;
            self.canvas = None;
        }
    }

    /// With [`EquationEditor::inline`]: do not draw the equation, only the caret, selection and
    /// active place. The host draws the same equation underneath (same fonts, size and place), so
    /// typing reads as typing in the text itself.
    pub fn overlay(mut self, on: bool) -> Self {
        self.overlay = on;
        self
    }

    /// Where the baseline sits, points from the top of an inline canvas.
    pub fn inline_baseline(&self) -> Option<f32> {
        self.canvas.as_ref().map(|c| c.layout.baseline() as f32)
    }

    /// Space between an inline canvas's left / top edge and the equation's own left / top edge,
    /// in points at the editor's scale: place the canvas at the equation's position minus this.
    pub fn inline_pad() -> egui::Vec2 {
        vec2(VIEW_MARGIN + EQ_PAD as f32, EQ_PAD as f32)
    }

    /// The size of the laid-out equation in points (the canvas it last drew), if it has drawn.
    pub fn content_size(&self) -> Option<egui::Vec2> {
        self.canvas.as_ref().map(|c| vec2(c.layout.width() as f32, c.layout.height() as f32))
    }

    /// Change the font size (points); the next frame lays the equation out again.
    pub fn set_font_size(&mut self, pt: f64) {
        if (self.font_size - pt).abs() > f64::EPSILON {
            self.font_size = pt;
            self.canvas = None;
        }
    }

    /// Show the template palette (default on).
    pub fn show_palette(mut self, on: bool) -> Self {
        self.show_palette = on;
        self
    }

    /// Show the linear-text field and the display toggle (default on).
    pub fn show_linear_field(mut self, on: bool) -> Self {
        self.show_linear = on;
        self
    }

    /// Blink the caret while idle (default on). Off keeps it solid, for hosts that honour
    /// a reduced-motion preference.
    pub fn caret_blink(mut self, on: bool) -> Self {
        self.caret_blink = on;
        self
    }

    /// [`EquationEditor::caret_blink`] on an existing editor.
    pub fn set_caret_blink(&mut self, on: bool) {
        self.caret_blink = on;
    }

    /// Take the keyboard in the first frames after creation (default on).
    pub fn auto_focus(mut self, on: bool) -> Self {
        self.auto_focus = if on { 4 } else { 0 };
        self.wants_canvas = on;
        self
    }

    /// The widget's id; the canvas and the linear field use ids derived from it.
    pub fn id(&self) -> egui::Id {
        self.id
    }

    /// The id of the canvas (the focus target typing goes to).
    pub fn canvas_id(&self) -> egui::Id {
        self.id.with("canvas")
    }

    /// The id of the linear-text field.
    pub fn linear_id(&self) -> egui::Id {
        self.id.with("linear")
    }

    /// The equation in linear form (canonical: `to_linear` of the tree, not the field text).
    pub fn linear(&self) -> String {
        to_linear(self.ed.math())
    }

    /// The equation as OMML.
    pub fn omml(&self) -> String {
        to_omml(self.ed.math())
    }

    /// The equation tree.
    pub fn math(&self) -> &Math {
        self.ed.math()
    }

    /// The edit model (caret, selection, undo).
    pub fn editor(&self) -> &Editor {
        &self.ed
    }

    /// The edit model, mutable. Call [`EquationEditor::sync_linear`] after changing the tree.
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.ed
    }

    /// Backslash commands of the linear field that the parser did not know.
    pub fn unknown_commands(&self) -> Vec<String> {
        equashion_math::unknown_commands(self.ed.math())
    }

    /// Load the equation typed as linear text. This is a load: Esc compares against it.
    pub fn set_linear(&mut self, s: &str) {
        self.set_math(from_linear(s));
        self.linear = s.to_string();
    }

    /// Load the equation from OMML. This is a load: Esc compares against it.
    pub fn set_omml(&mut self, xml: &str) {
        self.set_math(from_omml(xml));
    }

    /// Load an equation tree. This is a load: Esc compares against it.
    pub fn set_math(&mut self, m: Math) {
        self.ed = Editor::new(m);
        self.linear = to_linear(self.ed.math());
        self.initial = self.ed.math().clone();
        self.message = None;
        self.discard_armed = false;
    }

    /// Go back to the equation as last loaded or committed.
    pub fn revert(&mut self) {
        let m = self.initial.clone();
        self.set_math(m);
    }

    /// True when the equation differs from the one last loaded or committed.
    pub fn is_dirty(&self) -> bool {
        self.ed.math() != &self.initial
    }

    /// Accept the current equation as the new baseline (what Enter does).
    pub fn mark_clean(&mut self) {
        self.ed.flush();
        self.initial = self.ed.math().clone();
        self.discard_armed = false;
        self.message = None;
    }

    /// Refresh the linear field from the tree (after [`EquationEditor::editor_mut`]).
    pub fn sync_linear(&mut self) {
        self.linear = to_linear(self.ed.math());
    }

    /// The status message (the discard prompt), if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The text of the linear field changed: rebuild the tree from it.
    fn linear_edited(&mut self) {
        let para = self.ed.math().para;
        let mut m = from_linear(&self.linear);
        m.para = para;
        self.ed.set_math(m);
    }

    /// The tree changed through the canvas or the palette: show it in the linear field.
    fn tree_edited(&mut self) {
        self.linear = to_linear(self.ed.math());
        self.message = None;
        self.discard_armed = false;
    }

    /// Esc: the first press with unsaved work only warns, the second cancels. True when the
    /// edit is cancelled.
    fn escape(&mut self) -> bool {
        if !self.is_dirty() || self.discard_armed {
            self.discard_armed = false;
            self.message = None;
            return true;
        }
        self.discard_armed = true;
        self.message = Some(DISCARD_PROMPT.to_string());
        false
    }

    fn commit(&mut self) {
        self.mark_clean();
    }

    /// Draw the widget: palette, canvas, linear field and message line.
    pub fn show(&mut self, ui: &mut Ui) -> EditorResponse {
        let before = self.ed.math().clone();
        let esc = self.focus_within && ui.input(|i| i.key_pressed(Key::Escape));
        let mut committed = false;
        let id = self.id;
        let inner = ui.push_id(id, |ui| {
            if self.show_palette && !self.inline {
                self.palette_ui(ui);
                ui.add_space(4.0);
            }
            let canvas = self.canvas_ui(ui, &mut committed);
            if !self.inline {
                ui.add_space(4.0);
                if self.show_linear {
                    self.linear_ui(ui, &mut committed);
                }
                self.message_ui(ui);
            }
            canvas
        });
        let mut cancelled = false;
        if esc {
            cancelled = self.escape();
        }
        if committed {
            self.commit();
        }
        let (c, l) = (self.canvas_id(), self.linear_id());
        self.focus_within = ui.memory(|m| m.has_focus(c) || m.has_focus(l));
        EditorResponse { changed: before != *self.ed.math(), committed, cancelled, response: inner.inner, rect: inner.response.rect }
    }

    /// Egui input to editor calls. True when the tree or the caret changed.
    fn handle_input(&mut self, ui: &Ui, committed: &mut bool) -> bool {
        let events = ui.input(|i| i.events.clone());
        if events.is_empty() {
            return false;
        }
        let before = (self.ed.math().clone(), self.ed.caret().clone(), self.ed.selection());
        // Platforms that send Copy / Cut events also send the key; act on one of them only.
        let clipboard_events = events.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut));
        for ev in &events {
            match ev {
                egui::Event::Text(t) => self.ed.type_str(t),
                egui::Event::Paste(t) => self.ed.type_str(t),
                egui::Event::Copy => self.copy(ui),
                egui::Event::Cut => self.cut(ui),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let nav = match key {
                        Key::ArrowLeft => Some(MKey::Left),
                        Key::ArrowRight => Some(MKey::Right),
                        Key::ArrowUp => Some(MKey::Up),
                        Key::ArrowDown => Some(MKey::Down),
                        Key::Home => Some(MKey::Home),
                        Key::End => Some(MKey::End),
                        Key::Backspace => Some(MKey::Backspace),
                        Key::Delete => Some(MKey::Delete),
                        Key::Tab => Some(MKey::Tab),
                        Key::Enter => {
                            *committed = true;
                            None
                        }
                        Key::Z if modifiers.command => Some(if modifiers.shift { MKey::Redo } else { MKey::Undo }),
                        Key::Y if modifiers.command => Some(MKey::Redo),
                        _ => None,
                    };
                    if let Some(k) = nav {
                        self.ed.key(k, modifiers.shift);
                    } else if *key == Key::A && modifiers.command {
                        self.ed.select_all();
                    } else if *key == Key::C && modifiers.command && !clipboard_events {
                        self.copy(ui);
                    } else if *key == Key::X && modifiers.command && !clipboard_events {
                        self.cut(ui);
                    }
                }
                _ => {}
            }
        }
        let tree = before.0 != *self.ed.math();
        if tree {
            self.tree_edited();
        }
        tree || before.1 != *self.ed.caret() || before.2 != self.ed.selection()
    }

    fn copy(&mut self, ui: &Ui) {
        if let Some(t) = self.ed.selected_linear() {
            ui.ctx().copy_text(t);
        }
    }

    fn cut(&mut self, ui: &Ui) {
        if let Some(t) = self.ed.cut() {
            ui.ctx().copy_text(t);
        }
    }

    /// Lay the current tree out and render it, unless the last canvas already shows it.
    fn refresh_canvas(&mut self, ui: &Ui) {
        // Empty places draw their own dotted box in the layout, so the tree is shown as it is: the
        // widget's geometry is then exactly that of a page laying the same equation out.
        let shown = self.ed.math().clone();
        let ppp = f64::from(ui.ctx().pixels_per_point()).max(1.0);
        let want = (ppp * 2.0).min(MAX_SCALE);
        if self.canvas.as_ref().is_some_and(|c| c.shown == shown && c.want == want) {
            return;
        }
        // Ink in white on transparent; the painter tints it with the theme's text colour.
        let layout = layout_equation(&*self.fonts, &shown, &self.family, self.font_size, self.display_style, Rgb::WHITE, EQ_PAD);
        // Never beyond the largest texture the GPU takes, however wide the equation grows.
        let scale = (MAX_TEXTURE / layout.width().max(layout.height()).max(1.0)).min(want);
        let img = equashion_render::render_scene(&layout.scene, &*self.fonts, scale, None);
        let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
        let tex = match self.canvas.take() {
            Some(mut c) => {
                c.tex.set(ci, TextureOptions::LINEAR);
                c.tex
            }
            None => ui.ctx().load_texture(format!("equashion/editor/{:?}/canvas", self.id), ci, TextureOptions::LINEAR),
        };
        self.canvas = Some(Canvas { shown, layout, tex, want });
    }

    /// Keyboard focus: the canvas takes it in the first frames and whenever nothing holds it
    /// while typing belongs to it; it never takes it from another widget.
    fn manage_focus(&mut self, ui: &Ui) {
        let canvas = self.canvas_id();
        if self.auto_focus > 0 {
            self.auto_focus -= 1;
            ui.memory_mut(|m| m.request_focus(canvas));
            return;
        }
        match ui.memory(|m| m.focused()) {
            Some(f) if f == canvas => {}
            Some(_) => self.wants_canvas = false,
            None if self.wants_canvas => ui.memory_mut(|m| m.request_focus(canvas)),
            None => {}
        }
    }

    /// The editing canvas: the rendered equation, selection, caret, placeholders and mouse.
    fn canvas_ui(&mut self, ui: &mut Ui, committed: &mut bool) -> egui::Response {
        let id = self.canvas_id();
        self.manage_focus(ui);
        // Keys the editor uses must not move the focus away; Esc is the editor's too.
        let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true };
        ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));

        let (width, height) = if self.inline {
            // As large as the equation (what it drew last frame; a default the first time).
            let (w, h) = self
                .canvas
                .as_ref()
                .map_or((120.0, self.font_size as f32 * 2.0), |c| (c.layout.width() as f32 + 2.0 * VIEW_MARGIN, c.layout.height() as f32));
            if self.canvas.is_some() { (w.max(2.0 * VIEW_MARGIN + 8.0), h) } else { (w.max(48.0), h.max(self.font_size as f32 * 1.4)) }
        } else {
            (ui.available_width().max(160.0), self.canvas_height)
        };
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let resp = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Text);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), "Equation"));
        let focused = resp.has_focus();
        let now = ui.input(|i| i.time);

        if focused && self.handle_input(ui, committed) {
            self.touched = now;
        }
        self.refresh_canvas(ui);
        let Some(cv) = self.canvas.as_ref() else { return resp };

        // Where the equation sits: left-aligned so it grows to the right as you type, vertically
        // centred, shrunk (to at most half size) to fit, then scrolled to keep the caret in view.
        let (w, h) = (cv.layout.width() as f32, cv.layout.height() as f32);
        let room = width - 2.0 * VIEW_MARGIN;
        let k = (room / w.max(1.0)).max(MIN_SCALE).min(height / h.max(1.0)).min(1.0);
        let size = vec2(w * k, h * k);
        let caret_x = cv.layout.caret_rect(self.ed.caret()).map_or(0.0, |r| r.x1 as f32 * k);
        let scroll = if size.x > room { (caret_x - room * 0.85).clamp(0.0, size.x - room) } else { 0.0 };
        let origin = pos2(rect.min.x + VIEW_MARGIN - scroll, rect.center().y - size.y / 2.0);
        self.view = (origin, k);
        let to_layout = |p: Pos2| ((p.x - origin.x) / k.max(1e-3), (p.y - origin.y) / k.max(1e-3));

        // Mouse: press places the caret, dragging or shift-click selects (across rows too).
        let (pressed, down, pointer, shift) =
            ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.interact_pos(), i.modifiers.shift));
        if let Some(p) = pointer {
            let (x, y) = to_layout(p);
            if pressed && rect.contains(p) && resp.hovered() {
                ui.memory_mut(|m| m.request_focus(id));
                self.wants_canvas = true;
                let hit = cv.layout.hit(f64::from(x), f64::from(y));
                if shift {
                    // Shift-click: extend from the fixed end of the current selection.
                    self.origin = self.ed.anchor_caret();
                    self.ed.select_between(&self.origin, &hit);
                } else {
                    self.ed.set_caret(hit.clone());
                    self.origin = hit;
                }
                self.dragging = true;
                self.touched = now;
            } else if self.dragging && down {
                let hit = cv.layout.hit(f64::from(x), f64::from(y));
                self.ed.select_between(&self.origin, &hit);
            }
        }
        if !down {
            self.dragging = false;
        }

        let v = ui.visuals();
        let accent = v.selection.stroke.color;
        let ink = v.strong_text_color();
        let painter = ui.painter_at(rect);
        // The host's control corner radius, so the canvas matches its other inputs.
        let corner = v.widgets.inactive.corner_radius;
        if !self.inline {
            painter.rect_filled(rect, corner, v.extreme_bg_color);
        }
        let to_screen = |r: equashion_layout::Rect| {
            egui::Rect::from_min_max(
                pos2(origin.x + r.x0 as f32 * k, origin.y + r.y0 as f32 * k),
                pos2(origin.x + r.x1 as f32 * k, origin.y + r.y1 as f32 * k),
            )
        };
        // The active place, tinted like MathLive's active placeholder: empty slots become a
        // visible box, filled ones a soft band behind their content. The top level stays plain.
        if !self.ed.caret().path.is_empty()
            && let Some(r) = cv.layout.row_rect(&self.ed.caret().path)
        {
            let r = to_screen(r).expand2(vec2(2.5, 1.0));
            painter.rect_filled(r, 2.5, accent.gamma_multiply(0.13));
            painter.rect_stroke(r, 2.5, egui::Stroke::new(1.0, accent.gamma_multiply(0.45)), egui::StrokeKind::Inside);
        }
        // Selection under the glyphs.
        if let Some((a, b)) = self.ed.selection()
            && let Some(r) = cv.layout.span_rect(&self.ed.caret().path, a, b)
        {
            painter.rect_filled(to_screen(r), 1.0, v.selection.bg_fill.gamma_multiply(0.7));
        }
        if !(self.inline && self.overlay) {
            painter.image(cv.tex.id(), egui::Rect::from_min_size(origin, size), egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), ink);
        }
        let dim = v.weak_text_color();
        if self.ed.is_blank() && !self.inline {
            painter.text(
                pos2(rect.min.x + 14.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "Type here: x^2, a/b, \\frac, \\sqrt, \\alpha",
                FontId::proportional(13.0),
                dim,
            );
        }
        // The caret: solid while editing, blinking when idle.
        if focused {
            let on = !self.caret_blink || now - self.touched < BLINK || (((now - self.touched) / BLINK) as u64).is_multiple_of(2);
            if on && let Some(r) = cv.layout.caret_rect(self.ed.caret()) {
                let mut r = to_screen(r);
                // Keep the caret off a radical's bar or a bracket at the start of a place.
                if self.ed.caret().pos == 0 && !self.ed.caret().path.is_empty() {
                    r = r.translate(vec2(2.0 * k.max(0.5), 0.0));
                }
                painter.line_segment([pos2(r.min.x, r.min.y), pos2(r.min.x, r.max.y)], egui::Stroke::new(1.6, accent));
            }
            if self.caret_blink {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
        } else if !self.inline {
            painter.text(pos2(rect.max.x - 8.0, rect.min.y + 6.0), egui::Align2::RIGHT_TOP, "Click here to type", FontId::proportional(11.0), dim);
        }
        let stroke = if focused { egui::Stroke::new(2.0, accent) } else { egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color) };
        if !(self.inline && self.overlay) {
            painter.rect_stroke(rect, if self.inline { 3.into() } else { corner }, stroke, egui::StrokeKind::Inside);
        }
        resp
    }

    /// Linear text, the secondary way in, and the display toggle.
    fn linear_ui(&mut self, ui: &mut Ui, committed: &mut bool) {
        let id = self.linear_id();
        ui.horizontal(|ui| {
            ui.label("Linear");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.linear)
                    .id(id)
                    .desired_width(f32::INFINITY)
                    .font(FontId::monospace(14.0))
                    .hint_text("a^2+b^2=c^2, x=(-b+-sqrt(b^2-4ac))/2a"),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                *committed = true;
            }
            if r.has_focus() || r.gained_focus() {
                self.wants_canvas = false;
            }
            if r.changed() {
                self.linear_edited();
                self.message = None;
                self.discard_armed = false;
            }
        });
        let mut display = self.ed.math().para;
        if ui.checkbox(&mut display, "Display on its own line").changed() {
            self.ed.math_mut().para = display;
        }
    }

    /// One line is always reserved for the message, so showing it does not move what follows.
    fn message_ui(&mut self, ui: &mut Ui) {
        let w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if let Some(m) = &self.message {
                ui.colored_label(ui.visuals().warn_fg_color, m);
            }
        });
    }

    /// Insert a palette template or symbol at the caret, keeping typing in the canvas.
    fn palette_insert(&mut self, ui: &Ui, tpl: Option<Math>, sym: Option<char>) {
        if tpl.is_none() && sym.is_none() {
            return;
        }
        if let Some(t) = tpl {
            self.ed.insert_template(&t);
        }
        if let Some(c) = sym {
            self.ed.insert_symbol(c);
        }
        self.wants_canvas = true;
        let id = self.canvas_id();
        ui.memory_mut(|m| m.request_focus(id));
        self.tree_edited();
        self.touched = ui.input(|i| i.time);
    }
}

#[cfg(test)]
mod tests;
