//! Draw ▸ Stencils ▸ Ruler driven like a person: the ribbon shows it, the wheel over it turns
//! it, the degree readout takes a typed angle, a pen stroke along it is committed straight, and
//! the ribbon hides it again.

use deckcraft_engine::Session;
use deckcraft_engine::ruler;
use egui::{Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, TouchPhase};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn slide_at(h: &Harness<'_, SlideApp>, x: f64, y: f64) -> Pos2 {
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    slide.min + egui::vec2(x as f32 * k, y as f32 * k)
}

fn click(h: &mut Harness<'_, SlideApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn wheel(h: &mut Harness<'_, SlideApp>, at: Pos2, notches: f32) {
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: egui::vec2(0.0, notches),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

fn ink_line(h: &Harness<'_, SlideApp>) -> Vec<(f64, f64, f32)> {
    let st = h.state().session.active().unwrap();
    let Some(sh) = st.shapes().iter().rev().find(|s| matches!(s.kind, deckcraft_model::ShapeKind::Ink { .. })) else { return vec![] };
    let deckcraft_model::ShapeKind::Ink { strokes } = &sh.kind else { return vec![] };
    strokes.last().map(|s| s.points.clone()).unwrap_or_default()
}

#[test]
fn a_trackpad_turns_the_ruler_once_its_small_deltas_add_up() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("draw.ruler", &json!({"on": true})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Draw".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let r = h.state().session.view.ruler;
    assert!(r.visible, "{r:?}");
    let a = r.angle.to_radians();
    let along = egui::vec2(a.cos() as f32, a.sin() as f32);
    let body_at = slide_at(&h, r.x + 300.0 * f64::from(along.x), r.y + 300.0 * f64::from(along.y));
    // A trackpad sends a fraction of a notch per frame. Four frames of a quarter notch each make
    // one degree, though no single frame comes near half a notch.
    for _ in 0..4 {
        h.hover_at(body_at);
        h.run_steps(1);
        h.event(egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 12.5),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Trackpad,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    let turned = h.state().session.view.ruler;
    assert!((turned.angle - (r.angle - 1.0)).abs() < 1e-6, "the fractions added up: {r:?} -> {turned:?}");
}

#[test]
fn show_turn_and_type_the_ruler_then_draw_a_straight_line_along_it() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Draw".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);

    // The Stencils group's Ruler button shows the ruler, centred and at PowerPoint's first angle.
    assert!(!h.state().session.view.ruler.visible);
    h.get_by_label("Ruler").click();
    h.run_steps(2);
    let r = h.state().session.view.ruler;
    assert!(r.visible, "the ribbon showed the ruler");
    assert_eq!((r.x, r.y, r.angle), (480.0, 270.0, 45.0), "{r:?}");

    // The wheel over the body turns it 1° a notch around the pointer.
    let along = egui::vec2((45.0_f64.to_radians().cos()) as f32, (45.0_f64.to_radians().sin()) as f32);
    let body_at = slide_at(&h, 480.0 + 300.0 * f64::from(along.x), 270.0 + 300.0 * f64::from(along.y));
    wheel(&mut h, body_at, -1.0);
    let turned = h.state().session.view.ruler;
    assert!((turned.angle - 46.0).abs() < 1e-6, "one notch, one degree: {turned:?}");
    // Clicking the degree readout types an exact angle.
    let dial = slide_at(&h, h.state().session.view.ruler.x, h.state().session.view.ruler.y);
    click(&mut h, dial);
    h.run_steps(2);
    // The field opens empty: type the exact angle.
    h.event(egui::Event::Text("30".into()));
    h.run_steps(1);
    h.event(egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!((h.state().session.view.ruler.angle - 30.0).abs() < 1e-6, "typed angle: {:?}", h.state().session.view.ruler);

    // Dragging the body moves it.
    let before = h.state().session.view.ruler;
    let grab = slide_at(&h, before.x + 100.0 * (30.0_f64.to_radians().cos()), before.y + 100.0 * (30.0_f64.to_radians().sin()));
    h.hover_at(grab);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: grab, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(grab + egui::vec2(0.0, -6.0 * i as f32)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton {
        pos: grab + egui::vec2(0.0, -36.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let moved = h.state().session.view.ruler;
    let k = f64::from(h.state().canvas_scale);
    assert!((moved.y - (before.y - 36.0 / k)).abs() < 1.0, "the drag moved it: {before:?} -> {moved:?}");
    assert!((moved.angle - 30.0).abs() < 1e-6, "moving keeps the angle: {moved:?}");

    // A pen stroke drawn just outside an edge is committed as one straight line along it.
    h.get_by_label("Black Pen").click();
    h.run_steps(2);
    let r = h.state().session.view.ruler;
    let edges = ruler::edges(&r);
    let edge = edges[1];
    let outward = edge.normal;
    let a = deckcraft_geom::Point::new(edge.from.x + outward.x * 3.0, edge.from.y + outward.y * 3.0) + edge.dir * 40.0;
    let b = deckcraft_geom::Point::new(edge.from.x + outward.x * 3.0, edge.from.y + outward.y * 3.0) + edge.dir * 380.0;
    let (pa, pb) = (slide_at(&h, a.x, a.y), slide_at(&h, b.x, b.y));
    h.hover_at(pa);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: pa, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=12 {
        h.event(egui::Event::PointerMoved(pa + (pb - pa) * (i as f32 / 12.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: pb, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let line = ink_line(&h);
    assert_eq!(line.len(), 2, "one straight line, not the freehand samples: {line:?}");
    for (x, y, _) in &line {
        assert!(edge.distance(deckcraft_geom::Point::new(*x, *y)) < 1e-6, "on the edge: {line:?}");
    }
    let t0 = (deckcraft_geom::Point::new(line[0].0, line[0].1) - edge.from).dot(edge.dir);
    let t1 = (deckcraft_geom::Point::new(line[1].0, line[1].1) - edge.from).dot(edge.dir);
    assert!((t1 - t0 - 340.0).abs() < 2.0, "the stroke's along-edge extent is kept: {t0}..{t1}");

    // The off-screen renders, for the pull request (outside the repository).
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/deckcraft-draw-ruler.png");
    }
    if let Ok(v) = h.state_mut().session.execute("file.render", &json!({"scale": 2.0}))
        && let Some(b64) = v["png"].as_str()
        && let Some(bytes) = deckcraft_engine::cmd::base64_decode(b64)
    {
        let _ = std::fs::write("/tmp/deckcraft-draw-ruler-slide.png", bytes);
    }

    // Hiding it again restores normal interaction: the same stroke is freehand.
    h.get_by_label("Ruler").click();
    h.run_steps(2);
    assert!(!h.state().session.view.ruler.visible);
    let freehand = [slide_at(&h, a.x, a.y), slide_at(&h, a.x + 40.0, a.y + 30.0), slide_at(&h, a.x + 80.0, a.y + 10.0)];
    h.hover_at(freehand[0]);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: freehand[0], button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for p in &freehand[1..] {
        h.event(egui::Event::PointerMoved(*p));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: freehand[2], button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let line = ink_line(&h);
    assert!(line.len() > 2, "freehand again once the ruler is hidden: {line:?}");
}
