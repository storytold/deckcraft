//! States: the canvas shell's Animate tab. A state is a slide; consecutive slides joined by the
//! Morph transition are the states of one animation, and Morph animates every layer it can pair
//! between them (by `!!name`, name, text or shape, see `deckcraft_anim::morph_pairs`) from
//! one state to the next: position, size, rotation, colour and transparency, and fades layers in
//! or out that only one state has. Adding a state duplicates the slide and sets Morph on the copy,
//! so a deck of states is a plain PPTX that PowerPoint plays the same way.

use std::collections::HashSet;
use std::ops::Range;

use deckcraft_model::{Presentation, Shape, ShapeId, Slide};
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Is slide `i` a later state of the slide before it (it enters with Morph)?
pub fn is_state_step(doc: &Presentation, i: usize) -> bool {
    i > 0 && doc.slides.get(i).and_then(|s| s.transition.as_ref()).is_some_and(|t| t.kind == "morph")
}

/// The slides that are states of the same animation as slide `i` (just `i` when it has none).
pub fn run_of(doc: &Presentation, i: usize) -> Range<usize> {
    if doc.slides.is_empty() {
        return 0..0;
    }
    let i = i.min(doc.slides.len() - 1);
    let mut start = i;
    while is_state_step(doc, start) {
        start -= 1;
    }
    let mut end = i + 1;
    while end < doc.slides.len() && is_state_step(doc, end) {
        end += 1;
    }
    start..end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Moved,
    Resized,
    Rotated,
    /// Fill, outline or their transparency.
    Restyled,
    Text,
    /// Hidden or shown with the eye: Morph doesn't fade these (see [`LayerChange::warning`]).
    Hidden,
    Shown,
    /// Only in the later state: fades in.
    Enters,
    /// Only in the earlier state: fades out.
    Leaves,
}

impl Change {
    pub fn label(self) -> &'static str {
        match self {
            Change::Moved => "moves",
            Change::Resized => "resizes",
            Change::Rotated => "rotates",
            Change::Restyled => "restyles",
            Change::Text => "text changes",
            Change::Hidden => "hidden",
            Change::Shown => "shown",
            Change::Enters => "fades in",
            Change::Leaves => "fades out",
        }
    }
}

/// One layer that changes from a state to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerChange {
    /// The layer on the later state (on the earlier one for [`Change::Leaves`]).
    pub id: ShapeId,
    pub name: String,
    pub changes: Vec<Change>,
}

impl LayerChange {
    /// A note when the change won't animate the way it looks like it should.
    pub fn warning(&self) -> Option<&'static str> {
        self.changes
            .iter()
            .any(|c| matches!(c, Change::Hidden | Change::Shown))
            .then_some("Hiding with the eye isn't animated: delete the layer in the state where it should be gone, and Morph fades it.")
    }
}

fn text_of(s: &Shape) -> String {
    s.text.as_ref().map(|t| t.text()).unwrap_or_default()
}

/// What Morph changes from slide `a` to slide `b`, in `b`'s layer order (then the layers that leave).
pub fn changes(a: &Slide, b: &Slide) -> Vec<LayerChange> {
    let pairs = deckcraft_anim::morph_pairs(a, b);
    let find = |shapes: &[Shape], id: ShapeId| shapes.iter().find(|s| s.id == id).cloned();
    let mut out = vec![];
    for sb in &b.shapes {
        let Some((ia, _)) = pairs.iter().find(|(_, ib)| *ib == sb.id) else {
            out.push(LayerChange { id: sb.id, name: sb.name.clone(), changes: vec![Change::Enters] });
            continue;
        };
        let Some(sa) = find(&a.shapes, *ia) else { continue };
        let mut c = vec![];
        let near = |x: f64, y: f64| (x - y).abs() < 0.01;
        match (sa.xfrm, sb.xfrm) {
            (Some(xa), Some(xb)) => {
                if !near(xa.x, xb.x) || !near(xa.y, xb.y) {
                    c.push(Change::Moved);
                }
                if !near(xa.w, xb.w) || !near(xa.h, xb.h) {
                    c.push(Change::Resized);
                }
                if !near(xa.rot, xb.rot) {
                    c.push(Change::Rotated);
                }
            }
            (None, None) => {}
            _ => c.push(Change::Moved),
        }
        if sa.fill != sb.fill || sa.line != sb.line || sa.geom != sb.geom {
            c.push(Change::Restyled);
        }
        if text_of(&sa) != text_of(sb) {
            c.push(Change::Text);
        }
        match (sa.hidden, sb.hidden) {
            (false, true) => c.push(Change::Hidden),
            (true, false) => c.push(Change::Shown),
            _ => {}
        }
        if !c.is_empty() {
            out.push(LayerChange { id: sb.id, name: sb.name.clone(), changes: c });
        }
    }
    for sa in &a.shapes {
        if !pairs.iter().any(|(ia, _)| *ia == sa.id) {
            out.push(LayerChange { id: sa.id, name: sa.name.clone(), changes: vec![Change::Leaves] });
        }
    }
    out
}

/// Layers of the current slide that change from the state before it (for the Layers list).
pub fn changed_layers(app: &SlideApp) -> HashSet<u32> {
    let Some(st) = app.session.active() else { return HashSet::new() };
    let i = st.selection.slide;
    if !is_state_step(&st.doc, i) {
        return HashSet::new();
    }
    match (st.doc.slides.get(i - 1), st.doc.slides.get(i)) {
        (Some(a), Some(b)) => changes(a, b).into_iter().filter(|c| c.changes != [Change::Leaves]).map(|c| c.id.0).collect(),
        _ => HashSet::new(),
    }
}

// ----- The Animate tab ----------------------------------------------------------------------

const MORPH_OPTIONS: [(&str, &str); 3] = [("objects", "Objects"), ("words", "Words"), ("characters", "Characters")];

pub fn animate(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    if st.selection.target != deckcraft_engine::Target::Slides || st.doc.slides.is_empty() {
        ui.label(egui::RichText::new("States are made of slides.").color(t.text_dim));
        return;
    }
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let run = run_of(&doc, cur);
    ui.label(egui::RichText::new("States").font(theme::bold(13.0)).color(t.text));
    ui.add_space(2.0);
    if run.len() < 2 {
        ui.label(
            egui::RichText::new(
                "Add a state, then change the copy: move, resize, recolour or delete layers. Morph animates from one state to the next.",
            )
            .font(theme::font(12.0))
            .color(t.text_dim),
        );
        ui.add_space(6.0);
    } else {
        strip(app, ui, &doc, run.clone(), cur);
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new("+ Add state").stroke(Stroke::new(1.0, t.border)))
            .on_hover_text("Duplicate this slide as the next state")
            .clicked()
        {
            add_state(app);
        }
        if run.len() > 1 && ui.add(egui::Button::new("▶ Play").stroke(Stroke::new(1.0, t.border))).on_hover_text("Present the states").clicked() {
            app.start_show(run.start, false);
        }
    });
    if is_state_step(&doc, cur) {
        transition(app, ui, &doc, cur);
        layer_changes(app, ui, &doc, cur);
    }
    section(ui, "Effects on this slide");
    let id = ui.id().with("states-effects");
    let mut open: bool = ui.data_mut(|d| d.get_temp(id).unwrap_or(false));
    if ui.button(if open { "Hide entrance, emphasis and exit effects" } else { "Entrance, emphasis and exit effects…" }).clicked() {
        open = !open;
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    if open {
        crate::panes::animation(app, ui);
    }
}

fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).font(theme::bold(13.0)).color(t.text));
    ui.add_space(4.0);
}

/// Duplicate the current slide as the next state: the copy enters with Morph.
pub fn add_state(app: &mut SlideApp) {
    let duration =
        app.session.active().and_then(|d| d.current_slide().and_then(|s| s.transition.clone())).filter(|t| t.kind == "morph").map(|t| t.duration_ms);
    let cmds = vec![("slide.duplicate", json!({})), ("transition.set", json!({"kind": "morph", "duration": duration.unwrap_or(1000)}))];
    app.run_as_one("Add State", cmds);
}

/// The states as small numbered thumbnails; click one to go to it.
fn strip(app: &mut SlideApp, ui: &mut Ui, doc: &std::sync::Arc<Presentation>, run: Range<usize>, cur: usize) {
    let t = Tokens::get(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let tw = 76.0;
    let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
    let mut budget = 3u32;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (k, i) in run.enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(tw, th + 16.0), Sense::click());
            let img = Rect::from_min_size(r.min, vec2(tw, th));
            match app.textures.thumb(ui.ctx(), doc, i, (tw * ppp).round() as u32, &mut budget) {
                Some(tex) => {
                    ui.painter().image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(img, CornerRadius::ZERO, Color32::WHITE);
                    ui.ctx().request_repaint();
                }
            }
            let on = i == cur;
            let stroke = if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, if resp.hovered() { t.text_faint } else { t.border }) };
            ui.painter().rect_stroke(img, CornerRadius::same(2), stroke, egui::StrokeKind::Outside);
            let label = format!("State {}", k + 1);
            ui.painter().text(pos2(r.min.x, r.max.y - 1.0), Align2::LEFT_BOTTOM, &label, theme::font(11.0), if on { t.text } else { t.text_dim });
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
            if resp.clicked() {
                let _ = app.run("slide.go", json!({"index": i}));
            }
        }
    });
}

/// How this state is reached from the one before: Morph's duration and what it pairs.
fn transition(app: &mut SlideApp, ui: &mut Ui, doc: &Presentation, cur: usize) {
    let Some(tr) = doc.slides.get(cur).and_then(|s| s.transition.clone()) else { return };
    section(ui, "Transition from the previous state");
    ui.horizontal(|ui| {
        ui.label("Duration");
        let secs = tr.duration_ms as f64 / 1000.0;
        if let Some(v) = crate::widgets::spinner(ui, "state-duration", secs, 0.25, " s", 60.0) {
            let ms = (v.clamp(0.01, 59.0) * 1000.0).round();
            let _ = app.run("transition.timing", json!({"duration": ms}));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Animate");
        let cur_opt = if tr.option.is_empty() { "objects" } else { tr.option.as_str() };
        for (id, label) in MORPH_OPTIONS {
            if ui.selectable_label(cur_opt == id, label).clicked() {
                let _ = app.run("transition.options", json!({"option": id}));
            }
        }
    });
    let t = Tokens::get(ui.ctx());
    if ui.add(egui::Button::new("Stop being a state").stroke(Stroke::new(1.0, t.border))).on_hover_text("Turn Morph off for this slide").clicked() {
        let _ = app.run("transition.set", json!({"kind": "none"}));
    }
}

/// The layers Morph changes from the previous state; click one to select it.
fn layer_changes(app: &mut SlideApp, ui: &mut Ui, doc: &Presentation, cur: usize) {
    let t = Tokens::get(ui.ctx());
    let (Some(a), Some(b)) = (doc.slides.get(cur - 1), doc.slides.get(cur)) else { return };
    let list = changes(a, b);
    section(ui, "What changes");
    if list.is_empty() {
        ui.label(egui::RichText::new("Nothing yet: change this state's layers and Morph animates them.").font(theme::font(12.0)).color(t.text_dim));
        return;
    }
    for c in &list {
        let words: Vec<&str> = c.changes.iter().map(|x| x.label()).collect();
        let leaves = c.changes == [Change::Leaves];
        let r = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{}  —  {}", if c.name.is_empty() { "Layer" } else { &c.name }, words.join(", ")))
                    .font(theme::font(12.0)),
            )
            .frame(false),
        );
        if r.clicked() && !leaves {
            let _ = app.run("selection.select", json!({"ids": [c.id.0]}));
        }
        if let Some(w) = c.warning() {
            ui.label(egui::RichText::new(w).font(theme::font(11.0)).color(t.text_dim));
        }
    }
}

#[cfg(test)]
#[path = "states_tests.rs"]
mod tests;
