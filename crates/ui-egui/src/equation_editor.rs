//! The equation editor: equaSHion's [`EquationEditor`] widget, bare, over the equation where it
//! sits on the slide.
//!
//! It opens from Insert › Equation (a new equation, in the middle of the slide) and from a
//! double-click on an equation (the engine asks for the `equation` dialog with
//! `{edit: true, id, cell?, paragraph, run}`). The dialog is only the open request: nothing is
//! drawn in a window. The Equation ribbon tab (see [`crate::equation_tab`]) does the choosing and
//! inserts into the widget. The widget edits a copy, loaded from the run's OMML through
//! `equation.get`; the presentation changes once, through the engine's own commands, so undo and
//! save work as for any other edit: `insert.equation` (a new text box) or `equation.update`
//! (that run), one undo step. An empty equation is not applied.
//!
//! The widget shares the app's `egui::Context` and never touches its fonts, visuals or style: it
//! draws the equation with its own bundled fonts.

use equashion_egui::{Edge, EquationEditor};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::dialogs::Dialog;
use crate::theme::Tokens;

/// Parameters that say which equation run an edit targets.
const TARGET_KEYS: [&str; 4] = ["id", "cell", "paragraph", "run"];

/// The editor while the Equation dialog is open.
pub(crate) struct Open {
    widget: EquationEditor,
    /// The dialog parameters it was opened with (a new request reloads).
    params: Value,
    /// Where the edited equation lives (`id`, `cell`, `paragraph`, `run`); `None` inserts a new one.
    target: Option<Value>,
    /// Why loading failed.
    error: Option<String>,
    /// The undo history length before this edit's first change (its steps fold into one).
    undo_base: Option<usize>,
}

/// The equation editor's state in [`SlideApp`].
#[derive(Default)]
pub struct EquationHost {
    pub(crate) open: Option<Open>,
    /// The ribbon has already switched to the Equation tab for this editor.
    pub(crate) tab_shown: bool,
    /// The ribbon's Close Equation asked for the edit to be applied, as Insert / Update does.
    pub(crate) commit_requested: bool,
    /// Draws the faces in Insert › Equation's menu of ready-made equations (no editor is open then).
    pub(crate) preview: Option<EquationEditor>,
    /// The editor is about to open because an arrow key crossed into the equation: the caret
    /// starts at this edge and that same key press must not also move it.
    pub(crate) entering: Option<Edge>,
}

impl EquationHost {
    /// The Equation dialog's editor is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open editor's widget.
    pub fn widget(&self) -> Option<&EquationEditor> {
        self.open.as_ref().map(|o| &o.widget)
    }

    /// The open editor's widget, mutable.
    pub fn widget_mut(&mut self) -> Option<&mut EquationEditor> {
        self.open.as_mut().map(|o| &mut o.widget)
    }

    /// The equation being edited (`id`, `cell`, `paragraph`, `run`), or `None` for a new one.
    pub fn target(&self) -> Option<&Value> {
        self.open.as_ref().and_then(|o| o.target.as_ref())
    }
}

/// Open the editor for the dialog's parameters: blank, or on the equation `{edit: true, ...}`
/// names, read with `equation.get` (a query: the presentation and its history stay as they are).
fn open(app: &mut SlideApp, params: &Value) -> Open {
    let mut o = Open {
        // Arrow keys at either end of the equation hand the caret back to the text around it.
        widget: EquationEditor::new().inline(true).overlay(true).exit_at_edges(true).id_salt("deckcraft-equation-editor"),
        params: params.clone(),
        target: None,
        error: None,
        undo_base: None,
    };
    // A ready-made equation from Insert › Equation's arrow: a new equation, started from it.
    if let Some(id) = params.get("builtin").and_then(Value::as_str) {
        if let Some(b) = equashion_egui::equashion_math::galleries().builtins.iter().find(|b| b.id == id) {
            o.widget.set_math(b.math.clone());
        }
        return o;
    }
    if params.get("edit").and_then(Value::as_bool) != Some(true) {
        return o;
    }
    let mut loc = serde_json::Map::new();
    for k in TARGET_KEYS {
        if let Some(v) = params.get(k).filter(|v| !v.is_null()) {
            loc.insert(k.into(), v.clone());
        }
    }
    match app.session.execute("equation.get", &Value::Object(loc)) {
        Ok(v) => {
            if let Some(x) = v.get("omml").and_then(Value::as_str) {
                o.widget.set_omml(x);
                if let Some(edge) = app.equation.entering {
                    o.widget.caret_to(edge);
                }
                // Update the very run that was read, even if the selection moves meanwhile.
                let mut t = serde_json::Map::new();
                for k in TARGET_KEYS {
                    if let Some(x) = v.get(k).filter(|x| !x.is_null()) {
                        t.insert(k.into(), x.clone());
                    }
                }
                o.target = Some(Value::Object(t));
            }
        }
        Err(e) => o.error = Some(e.to_string()),
    }
    o
}

/// Apply the widget's equation to the presentation as it is typed: `equation.update` on the
/// equation being edited, or `insert.equation` the first time a new one has something in it. The
/// undo steps these make are folded into one (the state before the first), so one Undo takes the
/// whole edit back.
fn sync(app: &mut SlideApp, o: &mut Open) -> Result<(), String> {
    o.widget.editor_mut().flush();
    if o.widget.editor().is_blank() {
        return Ok(());
    }
    let mut p = json!({"omml": o.widget.omml()});
    let base = app.session.active().map_or(0, |d| d.history.undo.len());
    let (cmd, inserted) = match (&o.target, p.as_object_mut()) {
        (Some(Value::Object(t)), Some(obj)) => {
            obj.extend(t.clone());
            ("equation.update", false)
        }
        _ => ("insert.equation", true),
    };
    let r = app.run(cmd, p)?;
    if inserted {
        o.undo_base = Some(base);
        if let Some(id) = r.get("id").and_then(Value::as_u64) {
            o.target = Some(json!({"id": id, "paragraph": 0, "run": 0}));
        }
    } else if o.undo_base.is_none() {
        o.undo_base = Some(base);
    }
    // One undo step for the whole edit: keep the first entry (the document before it began).
    if let (Some(b), Some(d)) = (o.undo_base, app.session.active_mut())
        && d.history.undo.len() > b + 1
    {
        d.history.undo.truncate(b + 1);
        if let Some(e) = d.history.undo.get_mut(b) {
            e.label = "Edit Equation".into();
        }
    }
    Ok(())
}

/// The equation, edited where it sits on the slide: typing goes into the equation itself, drawn
/// by the document as it always is, and the widget draws only the caret and selection over it.
/// A new equation starts in the middle of the slide. True when the editor should close.
///
/// - Typing and the Equation tab edit the presentation live (see [`sync`]).
/// - Enter, Esc, a click on the slide outside the equation or the tab's Close Equation leave it.
/// - Clicks in the ribbon and its menus do not leave it.
pub fn in_place(app: &mut SlideApp, ctx: &egui::Context, d: &mut Dialog) -> bool {
    let mut o = match app.equation.open.take() {
        Some(o) if o.params == d.params => o,
        _ => open(app, &d.params),
    };
    if let Some(e) = o.error.take() {
        app.set_status(e);
        return true;
    }
    // A new equation that starts with something in it (a ready-made one) goes in at once.
    if o.target.is_none()
        && !o.widget.editor().is_blank()
        && let Err(e) = sync(app, &mut o)
    {
        app.set_status(e);
    }
    let Some(slide) = app.slide_rect else {
        app.equation.open = Some(o);
        return false;
    };
    let scale = app.canvas_scale.max(0.05);
    let pad = EquationEditor::inline_pad();
    // Where the equation is drawn now (the document is the truth once it exists).
    let placed = o.target.as_ref().and_then(|t| {
        let Value::Object(t) = t else { return None };
        app.session.execute("equation.place", &Value::Object(t.clone())).ok()
    });
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let (left, baseline, pt) = match &placed {
        Some(v) if num(v, "baseline").is_some() => {
            let rect = v.get("rect").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).unwrap_or_default();
            let x = rect.first().copied().unwrap_or(0.0);
            (
                slide.min.x + x as f32 * scale,
                slide.min.y + num(v, "baseline").unwrap_or(0.0) as f32 * scale,
                num(v, "size").unwrap_or(24.0) * f64::from(scale),
            )
        }
        _ => (slide.center().x - 70.0, slide.center().y, 24.0 * f64::from(scale)),
    };
    o.widget.set_font_size(pt.clamp(4.0, 1200.0));
    // Laid out in the style the slide draws it in (alone in its paragraph: display).
    if let Some(d) = placed.as_ref().and_then(|v| v.get("displayStyle")).and_then(Value::as_bool) {
        o.widget.set_display_style(d);
    }
    // The canvas's top-left: its baseline on the equation's (a guess until it has been drawn).
    let base_in_canvas = o.widget.inline_baseline().unwrap_or((pad.y + 0.8 * pt as f32).max(1.0));
    let top_left = egui::pos2(left - pad.x, baseline - base_in_canvas);

    let t = Tokens::get(ctx);
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    // The arrow press that brought the caret into the equation is spent.
    if app.equation.entering.take().is_some() {
        ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
        });
    }
    let mut response = None;
    let area = egui::Area::new(egui::Id::new("deckcraft-equation-in-place"))
        .order(egui::Order::Foreground)
        .fixed_pos(top_left)
        .show(ctx, |ui| {
            ui.visuals_mut().selection.stroke.color = t.accent;
            response = Some(o.widget.show(ui));
        });
    if response.as_ref().is_some_and(|r| r.changed)
        && let Err(e) = sync(app, &mut o)
    {
        app.set_status(e);
    }
    let outside = ctx.input(|i| i.pointer.primary_pressed())
        && ctx.pointer_interact_pos().is_some_and(|p| {
            !area.response.rect.contains(p)
                && app.canvas_rect.is_some_and(|c| c.contains(p))
                // The slide itself, not a menu or window above it.
                && ctx.layer_id_at(p).is_some_and(|l| l.order == egui::Order::Background)
        });
    let from_ribbon = std::mem::take(&mut app.equation.commit_requested);
    let leave = esc || outside || from_ribbon || response.as_ref().is_some_and(|r| r.committed || r.cancelled);
    // An arrow at the end of the top row: keep the edit and carry on in the text, on the side
    // the arrow pointed to.
    let exit = response.as_ref().and_then(|r| r.exit).filter(|_| !leave);
    if !leave && exit.is_none() {
        app.equation.open = Some(o);
        ctx.request_repaint();
        return false;
    }
    // Whatever the last frame left unsynced.
    if o.widget.is_dirty()
        && let Err(e) = sync(app, &mut o)
    {
        app.set_status(e);
    }
    if let Some(edge) = exit {
        caret_beside(app, o.target.as_ref(), edge);
    }
    true
}

/// The equation run `target` names, as (shape id, table cell, paragraph, run, char offset of its
/// start, its length in characters).
fn locate(app: &SlideApp, target: &Value) -> Option<(u64, Option<(usize, usize)>, usize, usize, usize, usize, bool)> {
    let id = target.get("id")?.as_u64()?;
    let para = target.get("paragraph").and_then(Value::as_u64).unwrap_or(0) as usize;
    let run = target.get("run").and_then(Value::as_u64).unwrap_or(0) as usize;
    let cell = target.get("cell").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize)));
    let d = app.session.active()?;
    let sh = d.shape(deckcraft_model::ShapeId(id as u32))?;
    let body = match (&sh.kind, cell) {
        (deckcraft_model::ShapeKind::Table(t), Some((r, c))) => &t.cell(r, c)?.text,
        _ => sh.text.as_ref()?,
    };
    let runs = &body.paragraphs.get(para)?.runs;
    let start: usize = runs.iter().take(run).map(|r| r.char_len()).sum();
    // The same rule the arrow keys use, so "right" means what the screen shows.
    let sel = deckcraft_engine::TextSel { shape: deckcraft_model::ShapeId(id as u32), cell, ..Default::default() };
    let rtl = deckcraft_engine::cmd::text::paragraph_is_rtl(d, &sel, para);
    Some((id, cell, para, run, start, runs.get(run)?.char_len(), rtl))
}

/// Put the text caret next to the equation `target` (after it for Edge::Right, before it for
/// Edge::Left), so typing carries on in the text.
fn caret_beside(app: &mut SlideApp, target: Option<&Value>, edge: Edge) {
    let Some((id, cell, para, _, start, len, rtl)) = target.and_then(|t| locate(app, t)) else { return };
    // Edge is on screen: the right side of a right-to-left paragraph's equation is its start.
    let forward = (edge == Edge::Right) != rtl;
    let ch = if forward { start + len } else { start };
    let mut p = json!({"id": id, "at": [para, ch]});
    if let (Some((r, c)), Some(o)) = (cell, p.as_object_mut()) {
        o.insert("cell".into(), json!([r, c]));
    }
    let _ = app.run("text.edit", p);
}

/// Left or Right with the text caret directly after or before an equation (nothing selected) goes
/// into the equation, at that end, instead of stepping over it; in right-to-left text the
/// arrows' directions flip, as they do for the caret.
pub(crate) fn enter_by_arrow(app: &mut SlideApp, params: &Value) -> Option<Result<Value, String>> {
    if app.equation.is_open() || params.get("extend").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let left = match params.get("to").and_then(Value::as_str)? {
        "left" => true,
        "right" => false,
        _ => return None,
    };
    let d = app.session.active()?;
    let t = d.selection.text.clone()?;
    if t.notes || t.anchor != t.caret {
        return None;
    }
    let sh = d.shape(t.shape)?;
    let body = match (&sh.kind, t.cell) {
        (deckcraft_model::ShapeKind::Table(tb), Some((r, c))) => &tb.cell(r, c)?.text,
        _ => sh.text.as_ref()?,
    };
    let para = body.paragraphs.get(t.caret.0)?;
    let rtl = deckcraft_engine::cmd::text::paragraph_is_rtl(d, &t, t.caret.0);
    // Left and right are on screen: in a right-to-left paragraph Left goes forward in the text.
    let forward = left == rtl;
    let mut at = 0;
    for (ri, r) in para.runs.iter().enumerate() {
        let end = at + r.char_len();
        let is_math = matches!(r.kind, deckcraft_model::text::RunKind::Math { .. });
        if is_math && r.char_len() > 0 && ((!forward && end == t.caret.1) || (forward && at == t.caret.1)) {
            let mut p = json!({"edit": true, "id": t.shape.0, "paragraph": t.caret.0, "run": ri});
            if let (Some((cr, cc)), Some(o)) = (t.cell, p.as_object_mut()) {
                o.insert("cell".into(), json!([cr, cc]));
            }
            // Entering from the right of the equation puts the caret at its right end.
            app.equation.entering = Some(if left { Edge::Right } else { Edge::Left });
            app.session.ui_requests.push(deckcraft_engine::UiRequest::Dialog { id: "equation".into(), params: p });
            app.drain_requests_pub();
            return Some(Ok(json!({"equationEditor": true})));
        }
        at = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use deckcraft_engine::Session;
    use deckcraft_model::text::RunKind;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use egui::vec2;

    use super::*;
    use crate::Services;

    fn harness() -> Harness<'static, SlideApp> {
        let mut h = Harness::builder().with_size(vec2(1280.0, 860.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        // The app keeps requesting repaints, so step frames instead of run().
        h.run_steps(3);
        h
    }

    fn steps(h: &mut Harness<'static, SlideApp>, n: usize) {
        h.run_steps(n);
    }

    fn type_text(h: &mut Harness<'static, SlideApp>, t: &str) {
        h.event(egui::Event::Text(t.into()));
        steps(h, 2);
    }

    /// (linear text, OMML) of every equation run on the current slide.
    fn equations(app: &SlideApp) -> Vec<(String, String)> {
        let Some(sl) = app.session.active().and_then(|d| d.current_slide()) else { return vec![] };
        sl.shapes
            .iter()
            .filter_map(|sh| sh.text.as_ref())
            .flat_map(|b| b.paragraphs.iter())
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| match &r.kind {
                RunKind::Math { omml } => Some((r.text.clone(), omml.clone())),
                _ => None,
            })
            .collect()
    }

    fn undo_depth(app: &SlideApp) -> usize {
        app.session.active().map_or(0, |d| d.history.undo.len())
    }

    fn open_new(h: &mut Harness<'static, SlideApp>) {
        h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
        steps(h, 4);
    }

    /// The Equation tab: it appears with the dialog, its galleries insert into the widget, and
    /// it goes away with the dialog.
    #[test]
    fn the_equation_tab_inserts_from_its_galleries() {
        let mut h = harness();
        // The whole tab is about 1,300 points wide.
        h.set_size(vec2(1700.0, 860.0));
        assert_ne!(h.state().ui.tab, "Equation");
        open_new(&mut h);
        assert_eq!(h.state().ui.tab, "Equation", "editing an equation brings up the tab");
        h.get_by_label("Fraction").click();
        steps(&mut h, 3);
        h.get_by_label("Stacked Fraction").click();
        steps(&mut h, 3);
        let eq = h.state().equation.widget().map(|w| w.math().clone()).unwrap();
        assert!(matches!(eq.body.first().map(equashion_egui::equashion_math::Node::bare), Some(equashion_egui::equashion_math::Node::Frac { .. })), "{eq:?}");
        // The symbol strip is on the tab itself: the first set's first symbols.
        h.get_by_label("\u{221e}").click();
        steps(&mut h, 3);
        assert!(h.state().equation.widget().is_some_and(|w| w.linear().contains('\u{221e}')), "{:?}", h.state().equation.widget().map(|w| w.linear()));
        // Display and Inline switch the equation's flag.
        assert!(!h.state().equation.widget().unwrap().is_display());
        h.get_by_label("Display").click();
        steps(&mut h, 2);
        assert!(h.state().equation.widget().unwrap().is_display());
        h.state_mut().dialog = None;
        steps(&mut h, 3);
        assert!(!h.state().equation.is_open());
        assert_ne!(h.state().ui.tab, "Equation", "leaving the equation goes back");
    }

    /// Insert › Equation's arrow: a ready-made equation opens the editor started from it, and
    /// Enter inserts it as one undo step.
    #[test]
    fn a_ready_made_equation_opens_the_editor_started_from_it() {
        let mut h = harness();
        h.set_size(vec2(1700.0, 860.0));
        h.state_mut().ui.tab = "Insert".into();
        steps(&mut h, 3);
        let id = equashion_egui::equashion_math::galleries().builtins.first().map(|b| b.id.clone()).expect("a built-in equation");
        let depth = undo_depth(h.state());
        let mut d = crate::dialogs::Dialog::new("equation");
        d.params = json!({"builtin": id});
        h.state_mut().dialog = Some(d);
        steps(&mut h, 4);
        let w = h.state().equation.widget().expect("the editor is open");
        assert!(!w.linear().is_empty(), "started from the ready-made equation");
        // A ready-made equation goes onto the slide at once, as one undo step; leaving keeps it.
        assert_eq!(equations(h.state()).len(), 1);
        h.key_press(egui::Key::Enter);
        steps(&mut h, 3);
        assert_eq!(equations(h.state()).len(), 1);
        assert_eq!(undo_depth(h.state()), depth + 1, "one undo step");
    }

    #[test]
    fn insert_opens_the_widget_in_place_and_enter_inserts_one_undo_step() {
        let mut h = harness();
        let fonts_before = h.ctx.fonts(|f| f.definitions().clone());
        let style_before = h.ctx.global_style();
        open_new(&mut h);
        assert!(h.state().equation.is_open(), "Insert › Equation opens the equaSHion widget");
        assert!(h.query_by_label("Insert").is_none() && h.query_by_label("Cancel").is_none(), "no dialog buttons: the ribbon does the choosing");
        let canvas = h.state().equation.widget().map(|w| w.canvas_id());
        assert!(canvas.is_some_and(|c| h.ctx.memory(|m| m.has_focus(c))), "typing goes to the widget");
        let depth = undo_depth(h.state());
        type_text(&mut h, "x^2");
        assert_eq!(h.state().equation.widget().map(|w| w.linear()), Some("x^2".to_string()));
        // Typed into the equation itself: the slide has it already.
        let eqs = equations(h.state());
        assert_eq!(eqs.len(), 1, "the equation is on the slide as it is typed");
        assert!(eqs[0].1.contains("m:sSup"), "{eqs:?}");
        type_text(&mut h, "+1");
        h.key_press(egui::Key::Enter);
        steps(&mut h, 3);
        assert!(h.state().dialog.is_none() && !h.state().equation.is_open(), "Enter leaves the equation");
        assert_eq!(equations(h.state()).len(), 1, "still one equation");
        assert_eq!(undo_depth(h.state()), depth + 1, "one undo step for the whole edit");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        steps(&mut h, 2);
        assert!(equations(h.state()).is_empty(), "one Undo takes the equation back");
        // The shared context keeps DeckCraft's fonts and style.
        assert!(h.ctx.fonts(|f| f.definitions().clone()) == fonts_before, "the widget installed fonts");
        assert!(*h.ctx.global_style() == *style_before, "the widget changed the shared style");
    }

    /// What a double-click on the equation asks for (see the engine's pointer tests).
    fn edit_request(h: &mut Harness<'static, SlideApp>, id: u64) {
        h.state_mut()
            .session
            .ui_requests
            .push(deckcraft_engine::UiRequest::Dialog { id: "equation".into(), params: json!({"edit": true, "id": id, "paragraph": 0, "run": 0}) });
        h.state_mut().run("edit.select", json!({"ids": [id]})).unwrap();
        steps(h, 4);
    }

    fn text_caret(app: &SlideApp) -> Option<(usize, usize)> {
        app.session.active().and_then(|d| d.selection.text.as_ref().map(|t| t.caret))
    }

    /// An arrow at the end of the equation goes on in the text after it, and an arrow from the
    /// text goes back in at that end; at the start the caret goes before the equation.
    #[test]
    fn arrows_cross_the_equation_boundary_both_ways() {
        let mut h = harness();
        let r = h.state_mut().run("insert.equation", json!({"linear": "x/y", "rect": [100, 100, 300, 60]})).unwrap();
        let id = r["id"].as_u64().unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 0]})).unwrap();
        h.state_mut().run("text.insert", json!({"text": "hi "})).unwrap();
        steps(&mut h, 2);
        let n = h.state().session.active().and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.text.as_ref().map(|b| b.paragraphs[0].char_len()))).unwrap();
        // Caret at the end, right after the equation: Left goes into it, at its right end.
        h.state_mut().run("text.edit", json!({"id": id, "end": true})).unwrap();
        h.state_mut().run("text.move", json!({"to": "left"})).unwrap();
        steps(&mut h, 4);
        assert!(h.state().equation.is_open(), "Left next to the equation goes into it");
        type_text(&mut h, "z");
        assert!(h.state().equation.widget().is_some_and(|w| w.linear().ends_with('z')), "entered at the right end: {:?}", h.state().equation.widget().map(|w| w.linear()));
        // Right at the end: out, caret after the equation (the end of the text).
        h.key_press(egui::Key::ArrowRight);
        steps(&mut h, 4);
        assert!(!h.state().equation.is_open(), "Right at the end leaves the equation");
        let end = h.state().session.active().and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.text.as_ref().map(|b| b.paragraphs[0].char_len()))).unwrap();
        assert!(end > n, "the typed z is in the equation");
        assert_eq!(text_caret(h.state()), Some((0, end)), "caret right after the equation");
        // Typing goes on as text after it.
        h.state_mut().run("text.insert", json!({"text": "!"})).unwrap();
        // Back in from the left of the equation: Right at its start enters at the left end.
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 3]})).unwrap();
        h.state_mut().run("text.move", json!({"to": "right"})).unwrap();
        steps(&mut h, 4);
        assert!(h.state().equation.is_open(), "Right next to the equation goes into it");
        h.key_press(egui::Key::ArrowLeft);
        steps(&mut h, 4);
        assert!(!h.state().equation.is_open(), "Left at the start leaves the equation");
        assert_eq!(text_caret(h.state()), Some((0, 3)), "caret right before the equation");
    }

    /// Text typed after leaving an equation by the arrow keeps the formatting of the text around
    /// it, not the equation's own (its size, font and italics).
    #[test]
    fn text_after_the_equation_has_the_surrounding_formatting() {
        let mut h = harness();
        let r = h.state_mut().run("insert.equation", json!({"linear": "x/y", "rect": [100, 100, 300, 60]})).unwrap();
        let id = r["id"].as_u64().unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 0]})).unwrap();
        h.state_mut().run("text.insert", json!({"text": "hi "})).unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "end": true})).unwrap();
        h.state_mut().run("text.move", json!({"to": "left"})).unwrap();
        steps(&mut h, 4);
        h.key_press(egui::Key::ArrowRight);
        steps(&mut h, 4);
        assert!(!h.state().equation.is_open());
        h.state_mut().run("text.insert", json!({"text": "after"})).unwrap();
        let runs: Vec<(String, Option<f64>, Option<String>, Option<bool>, bool)> = h
            .state()
            .session
            .active()
            .and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.text.clone()))
            .map(|b| {
                b.paragraphs[0]
                    .runs
                    .iter()
                    .map(|r| (r.text.clone(), r.props.size, r.props.font.clone(), r.props.italic, matches!(r.kind, RunKind::Math { .. })))
                    .collect()
            })
            .unwrap_or_default();
        eprintln!("RUNS {runs:?}");
        let before = runs.iter().find(|r| r.0.starts_with("hi")).cloned().expect("the text before");
        let after = runs.iter().find(|r| r.0.contains("after")).cloned().expect("the typed text");
        assert_eq!((after.1, after.2.clone(), after.3), (before.1, before.2.clone(), before.3), "typed text takes the surrounding text's formatting: {runs:?}");
    }

    /// An equation pasted into formatted text, then left by the arrow: what is typed next looks
    /// like the text it sits in (here 18 pt bold), not like the equation run.
    #[test]
    fn text_typed_after_a_pasted_equation_matches_its_neighbours() {
        let mut h = harness();
        let r = h.state_mut().run("insert.textBox", json!({"rect": [100, 100, 400, 60], "text": "hello world"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 0]})).unwrap();
        h.state_mut().run("text.selectAll", json!({})).unwrap();
        h.state_mut().run("format.size", json!({"size": 18})).unwrap();
        h.state_mut().run("format.bold", json!({"on": true})).unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 6]})).unwrap();
        h.state_mut().run("edit.paste", json!({"text": "$x^2$"})).unwrap();
        steps(&mut h, 2);
        let dump = |h: &Harness<'static, SlideApp>| -> Vec<(String, Option<f64>, Option<bool>, bool)> {
            h.state()
                .session
                .active()
                .and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.text.clone()))
                .map(|b| b.paragraphs[0].runs.iter().map(|r| (r.text.clone(), r.props.size, r.props.bold, matches!(r.kind, RunKind::Math { .. }))).collect())
                .unwrap_or_default()
        };
        eprintln!("PASTED {:?}", dump(&h));
        let caret = text_caret(h.state());
        eprintln!("CARET {caret:?}");
        // Into the equation from the right, and out again with Right.
        h.state_mut().run("text.move", json!({"to": "left"})).unwrap();
        steps(&mut h, 4);
        assert!(h.state().equation.is_open(), "Left next to the pasted equation goes into it");
        h.key_press(egui::Key::ArrowRight);
        steps(&mut h, 4);
        h.state_mut().run("text.insert", json!({"text": "ZZ"})).unwrap();
        let runs = dump(&h);
        eprintln!("AFTER {runs:?}");
        let hello = runs.iter().find(|r| r.0.starts_with("hello")).cloned().expect("hello");
        let zz = runs.iter().find(|r| r.0.contains("ZZ")).cloned().expect("typed");
        assert_eq!((zz.1, zz.2), (hello.1, hello.2), "{runs:?}");
    }

    /// In a right-to-left paragraph the arrows are on screen: Right at the equation's right end
    /// (its logical start) goes on in the text on that side, and Left from the text enters it.
    #[test]
    fn arrows_follow_the_screen_in_a_right_to_left_paragraph() {
        let mut h = harness();
        let r = h.state_mut().run("insert.equation", json!({"linear": "x/y", "rect": [100, 100, 300, 60]})).unwrap();
        let id = r["id"].as_u64().unwrap();
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 0]})).unwrap();
        h.state_mut().run("text.insert", json!({"text": "hi "})).unwrap();
        h.state_mut().run("format.rtl", json!({"on": true})).unwrap();
        steps(&mut h, 2);
        let len = |h: &Harness<'static, SlideApp>| {
            h.state().session.active().and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.text.as_ref().map(|b| b.paragraphs[0].char_len()))).unwrap()
        };
        let end = len(&h);
        // The caret just before the equation (logical): in right-to-left text the equation is
        // to the caret's left on screen, so Left goes into it.
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, 3]})).unwrap();
        h.state_mut().run("text.move", json!({"to": "left"})).unwrap();
        steps(&mut h, 4);
        assert!(h.state().equation.is_open(), "Left (on screen) next to the equation goes into it");
        // Entered at its right end on screen; Right there leaves on the same side, which in
        // right-to-left text is back to where the caret was (before the equation, logically).
        h.key_press(egui::Key::ArrowRight);
        steps(&mut h, 4);
        assert!(!h.state().equation.is_open());
        assert_eq!(text_caret(h.state()), Some((0, 3)), "Right on screen leaves on the right, the logical start");
        // And through its left end on screen: the caret goes after it, logically.
        h.state_mut().run("text.edit", json!({"id": id, "at": [0, end]})).unwrap();
        h.state_mut().run("text.move", json!({"to": "right"})).unwrap();
        steps(&mut h, 4);
        assert!(h.state().equation.is_open(), "Right (on screen) next to the equation's logical end goes into it");
        h.key_press(egui::Key::ArrowLeft);
        steps(&mut h, 4);
        assert!(!h.state().equation.is_open());
        assert_eq!(text_caret(h.state()), Some((0, end)), "Left on screen leaves on the left, the logical end");
    }

    #[test]
    fn double_click_edits_the_equation_in_place_and_esc_keeps_the_edit() {
        let mut h = harness();
        let r = h.state_mut().run("insert.equation", json!({"linear": "a/b", "rect": [100, 100, 300, 60]})).unwrap();
        let id = r["id"].as_u64().unwrap();
        edit_request(&mut h, id);
        assert!(h.state().equation.is_open());
        assert_eq!(h.state().equation.widget().map(|w| w.linear()), Some("a/b".to_string()));
        assert_eq!(h.state().equation.target(), Some(&json!({"id": id, "paragraph": 0, "run": 0})));

        // Esc without a change closes and changes nothing.
        let depth = undo_depth(h.state());
        h.key_press(egui::Key::Escape);
        steps(&mut h, 3);
        assert!(!h.state().equation.is_open() && h.state().dialog.is_none());
        assert_eq!(undo_depth(h.state()), depth, "nothing changed, no undo step");

        // Typing then Esc keeps the edit, applied to that run in one undo step.
        edit_request(&mut h, id);
        type_text(&mut h, "+c");
        h.key_press(egui::Key::Escape);
        steps(&mut h, 3);
        assert!(!h.state().equation.is_open());
        let eqs = equations(h.state());
        assert_eq!(eqs.len(), 1, "updated, not inserted");
        assert!(eqs[0].0.ends_with("+c"), "{eqs:?}");
        assert_eq!(undo_depth(h.state()), depth + 1);
    }

    #[test]
    fn an_empty_new_equation_is_dropped() {
        let mut h = harness();
        let depth = undo_depth(h.state());
        open_new(&mut h);
        h.key_press(egui::Key::Escape);
        steps(&mut h, 3);
        assert!(!h.state().equation.is_open() && h.state().dialog.is_none());
        assert!(equations(h.state()).is_empty());
        assert_eq!(undo_depth(h.state()), depth);
    }

    #[test]
    fn pasting_an_equation_on_the_slide_inserts_one() {
        let mut h = harness();
        h.event(egui::Event::Paste("$$\\frac{a}{b}$$".into()));
        steps(&mut h, 2);
        let eqs = equations(h.state());
        assert_eq!(eqs.len(), 1, "a delimited LaTeX paste becomes an equation");
        assert!(eqs[0].1.contains("<m:f>") && eqs[0].1.contains("oMathPara"), "{eqs:?}");
        // Text with dollar signs stays text.
        h.event(egui::Event::Paste("costs $5 and $6".into()));
        steps(&mut h, 2);
        assert_eq!(equations(h.state()).len(), 1);
    }
}
