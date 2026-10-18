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
    let hint = if run.len() < 2 {
        "Add a state, then change the copy: move, resize, recolour or delete layers. Morph animates from one state to the next."
    } else {
        "The states are in the strip under the slide: click one to edit it, + to add one after it."
    };
    ui.label(egui::RichText::new(hint).font(theme::font(12.0)).color(t.text_dim));
    ui.add_space(6.0);
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
    build_section(app, ui);
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

// ----- Working with states -----------------------------------------------------------------

/// Add a state after slide `i`: a copy of it that enters with Morph.
pub fn add_state_after(app: &mut SlideApp, i: usize) {
    let _ = app.run("slide.go", json!({"index": i}));
    add_state(app);
}

/// `transition.set` parameters giving slide `index` the transition `tr` a run started with (no
/// transition when it had Morph or none).
fn head_transition(tr: Option<&deckcraft_model::Transition>, index: usize) -> serde_json::Value {
    match tr.filter(|t| t.kind != "morph" && t.kind != "none") {
        Some(t) => json!({"kind": t.kind, "option": t.option, "duration": t.duration_ms, "index": index}),
        None => json!({"kind": "none", "index": index}),
    }
}

fn morph_ms(doc: &Presentation, i: usize) -> u32 {
    doc.slides.get(i).and_then(|s| s.transition.as_ref()).filter(|t| t.kind == "morph").map(|t| t.duration_ms).unwrap_or(1000)
}

/// Delete state `i`. When it's the first, the next state takes its place and its transition, so
/// it doesn't join the slide before the run.
pub fn delete_state(app: &mut SlideApp, i: usize) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let run = run_of(&doc, i);
    if run.len() < 2 {
        return;
    }
    let mut cmds = vec![("slide.delete", json!({"index": i}))];
    if i == run.start {
        cmds.push(("transition.set", head_transition(doc.slides.get(i).and_then(|s| s.transition.as_ref()), i)));
    }
    app.run_as_one("Delete State", cmds);
    let _ = app.run("slide.go", json!({"index": i.min(run.end - 2)}));
}

/// Swap state `i` with the one before it (or after it, `forth`). The run keeps its first slide's
/// transition at its start and Morph after it.
pub fn move_state(app: &mut SlideApp, i: usize, forth: bool) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let run = run_of(&doc, i);
    let j = if forth { i + 1 } else { i.wrapping_sub(1) };
    if !run.contains(&i) || !run.contains(&j) {
        return;
    }
    let mut cmds = vec![("slide.move", json!({"from": i, "to": j}))];
    let s = run.start;
    if i == s || j == s {
        // The state that becomes first takes the run's transition; the old first one is reached
        // with the Morph that state had.
        let head = doc.slides.get(s).and_then(|x| x.transition.as_ref());
        cmds.push(("transition.set", head_transition(head, s)));
        cmds.push(("transition.set", json!({"kind": "morph", "duration": morph_ms(&doc, s + 1), "index": s + 1})));
    }
    app.run_as_one("Move State", cmds);
    let _ = app.run("slide.go", json!({"index": j}));
}

// ----- The strip under the slide -------------------------------------------------------------

/// Height of the strip's thumbnails.
pub const STRIP_THUMB_H: f32 = 54.0;

/// Should the strip show: the slide has states, or it was turned on.
pub fn strip_shown(app: &SlideApp) -> bool {
    let Some(st) = app.session.active() else { return false };
    st.selection.target == deckcraft_engine::Target::Slides
        && !st.doc.slides.is_empty()
        && (app.ui.states_strip || run_of(&st.doc, st.selection.slide).len() > 1)
}

/// The current slide's states in a row under the slide (canvas shell): click one to edit it, + after
/// one adds a state there, the chip between two states holds Morph's timing, and a state's menu
/// moves or deletes it.
pub fn strip_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let run = run_of(&doc, cur);
    let th = STRIP_THUMB_H;
    let tw = th * (doc.slide_size.width / doc.slide_size.height.max(1.0)) as f32;
    let ppp = ui.ctx().pixels_per_point();
    let mut budget = 3u32;
    egui::ScrollArea::horizontal().id_salt("canvas_states_strip").auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (lr, _) = ui.allocate_exact_size(vec2(44.0, th), Sense::hover());
            ui.painter().text(pos2(lr.min.x, lr.center().y), Align2::LEFT_CENTER, "States", theme::bold(12.0), t.text_dim);
            for (k, i) in run.clone().enumerate() {
                if k > 0 {
                    timing_chip(app, ui, &doc, i, k);
                }
                let (r, resp) = ui.allocate_exact_size(vec2(tw, th), Sense::click());
                match app.textures.thumb(ui.ctx(), &doc, i, (tw * ppp).round() as u32, &mut budget) {
                    Some(tex) => {
                        ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::WHITE);
                        ui.ctx().request_repaint();
                    }
                }
                let on = i == cur;
                let stroke = if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, if resp.hovered() { t.text_faint } else { t.border }) };
                ui.painter().rect_stroke(r, CornerRadius::same(3), stroke, egui::StrokeKind::Outside);
                // The state's number in a corner badge.
                let badge = Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(16.0, 14.0));
                ui.painter().rect_filled(badge, CornerRadius::same(4), if on { t.accent } else { t.card });
                ui.painter().text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    format!("{}", k + 1),
                    theme::bold(10.0),
                    if on { t.accent_text } else { t.text },
                );
                builds_ticks(ui, &doc, i, r, &t);
                let label = format!("State {}", k + 1);
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
                if resp.clicked() && !on {
                    let _ = app.run("slide.go", json!({"index": i}));
                }
                let n = run.len();
                resp.on_hover_text(format!("{label}: click to edit it")).context_menu(|ui| {
                    if ui.add_enabled(k > 0, egui::Button::new("Move Left")).clicked() {
                        move_state(app, i, false);
                        ui.close();
                    }
                    if ui.add_enabled(k + 1 < n, egui::Button::new("Move Right")).clicked() {
                        move_state(app, i, true);
                        ui.close();
                    }
                    if ui.button("Add State After").clicked() {
                        add_state_after(app, i);
                        ui.close();
                    }
                    ui.separator();
                    if ui.add_enabled(n > 1, egui::Button::new("Delete State")).clicked() {
                        delete_state(app, i);
                        ui.close();
                    }
                });
                // + after each state adds one there.
                let (pr, presp) = ui.allocate_exact_size(vec2(20.0, th), Sense::click());
                let c = pr.center();
                ui.painter().circle(c, 9.0, if presp.hovered() { t.hover } else { Color32::TRANSPARENT }, Stroke::new(1.0, t.border));
                ui.painter().line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], Stroke::new(1.5, t.text_dim));
                ui.painter().line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], Stroke::new(1.5, t.text_dim));
                let plus = format!("Add a state after state {}", k + 1);
                presp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &plus));
                if presp.on_hover_text(&plus).clicked() {
                    add_state_after(app, i);
                }
            }
        });
    });
}

/// Between two states: how long Morph takes to the later one; click for its options.
fn timing_chip(app: &mut SlideApp, ui: &mut Ui, doc: &Presentation, i: usize, k: usize) {
    let t = Tokens::get(ui.ctx());
    let Some(tr) = doc.slides.get(i).and_then(|s| s.transition.clone()) else { return };
    let secs = tr.duration_ms as f64 / 1000.0;
    let text = format!("{}s", (secs * 100.0).round() / 100.0);
    let galley = ui.painter().layout_no_wrap(text, theme::font(11.0), t.text_dim);
    let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0, STRIP_THUMB_H), Sense::click());
    let pill = Rect::from_center_size(r.center(), vec2(r.width(), 20.0));
    ui.painter().rect_filled(pill, CornerRadius::same(10), if resp.hovered() { t.selected } else { t.hover });
    // An arrow pointing on to the next state, then the time.
    let y = pill.center().y;
    ui.painter().line_segment([pos2(pill.min.x + 5.0, y), pos2(pill.min.x + 12.0, y)], Stroke::new(1.2, t.text_dim));
    ui.painter().line_segment([pos2(pill.min.x + 9.5, y - 2.5), pos2(pill.min.x + 12.0, y)], Stroke::new(1.2, t.text_dim));
    ui.painter().line_segment([pos2(pill.min.x + 9.5, y + 2.5), pos2(pill.min.x + 12.0, y)], Stroke::new(1.2, t.text_dim));
    ui.painter().galley(pos2(pill.min.x + 15.0, y - galley.size().y / 2.0), galley, t.text_dim);
    let label = format!("Morph into state {}", k + 1);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let resp = resp.on_hover_text(format!("{label}: timing"));
    egui::Popup::menu(&resp).width(220.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.label(egui::RichText::new(&label).font(theme::bold(12.5)));
        ui.horizontal(|ui| {
            ui.label("Duration");
            if let Some(v) = crate::widgets::spinner(ui, &format!("strip-duration-{i}"), secs, 0.25, " s", 60.0) {
                let ms = (v.clamp(0.01, 59.0) * 1000.0).round();
                let _ = app.run("transition.timing", json!({"duration": ms, "index": i}));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Animate");
            let cur_opt = if tr.option.is_empty() { "objects" } else { tr.option.as_str() };
            for (id, l) in MORPH_OPTIONS {
                if ui.selectable_label(cur_opt == id, l).clicked() {
                    let _ = app.run("transition.options", json!({"option": id, "index": i}));
                }
            }
        });
        if ui.button("Stop being a state").on_hover_text("Turn Morph off: this slide starts its own run").clicked() {
            let _ = app.run("transition.set", json!({"kind": "none", "index": i}));
            ui.close();
        }
    });
}

/// Build effects for text, as `animation.build` takes them.
const BUILDS: [(&str, &str); 6] = [("none", "None"), ("appear", "Appear"), ("fade", "Fade"), ("fly", "Fly in"), ("wipe", "Wipe"), ("zoom", "Zoom")];

/// The clicks slide `sl` takes to play: one per on-click effect, and one per paragraph of a
/// text build.
pub fn build_steps(sl: &Slide) -> usize {
    use deckcraft_model::anim::AnimStart;
    sl.animations
        .iter()
        .filter(|a| a.trigger.is_none() && a.start == AnimStart::OnClick)
        .map(|a| {
            if !deckcraft_engine::cmd::animation::is_build(a) {
                return 1;
            }
            let Some(t) = deckcraft_model::find_shape(&sl.shapes, a.shape).and_then(|s| s.text.as_ref()) else { return 1 };
            let filled: Vec<u8> = t.paragraphs.iter().filter(|p| p.runs.iter().any(|r| !r.text.trim().is_empty())).map(|p| p.level).collect();
            let top = filled.iter().copied().min().unwrap_or(0);
            filled.iter().filter(|l| **l == top).count().max(1)
        })
        .sum()
}

/// Build the selected text box's paragraphs one per click (bullets coming in one at a time).
fn build_section(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let shapes = st.selected_shapes();
    let [sh] = shapes.as_slice() else { return };
    if !sh.text.as_ref().is_some_and(|x| !x.is_empty()) || !matches!(sh.kind, deckcraft_model::ShapeKind::Shape) {
        return;
    }
    let id = sh.id;
    let now = st
        .current_slide()
        .and_then(|sl| sl.animations.iter().find(|a| a.shape == id && deckcraft_engine::cmd::animation::is_build(a)).map(|a| a.effect.clone()))
        .unwrap_or_else(|| "none".into());
    section(ui, "Build text");
    ui.label(egui::RichText::new("Paragraphs come in one per click, as the slide plays.").font(theme::font(12.0)).color(t.text_dim));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (effect, label) in BUILDS {
            if ui.selectable_label(now == effect, label).clicked() && now != effect {
                let _ = app.run("animation.build", json!({"effect": effect, "ids": [id.0]}));
            }
        }
    });
}

/// Under a state's thumbnail, a tick for each step of the text builds on that slide: one per
/// click, as the slide plays.
fn builds_ticks(ui: &Ui, doc: &Presentation, i: usize, r: Rect, t: &Tokens) {
    let n = doc.slides.get(i).map(|s| build_steps(s)).unwrap_or(0);
    if n == 0 {
        return;
    }
    let y = r.max.y - 5.0;
    let shown = n.min(12);
    for k in 0..shown {
        let x = r.max.x - 6.0 - (shown - 1 - k) as f32 * 5.0;
        ui.painter().circle_filled(pos2(x, y), 1.8, t.accent);
    }
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
