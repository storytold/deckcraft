//! Drawing guides driven with the mouse: View › Guides adds the centre guides, dragging one
//! moves it (with its distance from the centre shown), dropping it off the slide deletes it.

use deckcraft_engine::Session;
use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn turn_guides_on_drag_one_and_drop_it_off_the_slide() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().run("view.guides", json!({"on": true})).unwrap();
    h.run_steps(2);
    let guides = |h: &Harness<'_, SlideApp>| h.state().session.active().unwrap().doc.guides.clone();
    assert_eq!(guides(&h).len(), 2, "the two centre guides");
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let at = |x: f32, y: f32| slide.min + egui::vec2(x * k, y * k);

    // Grab the vertical guide away from the horizontal one and drag it left.
    let start = at(480.0, 100.0);
    h.hover_at(start);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=10 {
        h.event(egui::Event::PointerMoved(at(480.0 - 18.0 * i as f32, 100.0)));
        h.run_steps(1);
    }
    assert_eq!(h.state().session.tool.guide_drag, Some((0, 300.0)));
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/guides.png");
    }
    h.event(egui::Event::PointerButton { pos: at(300.0, 100.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(guides(&h)[0].pos, 300.0);

    // Off the top of the slide with the horizontal guide: deleted.
    let g = at(700.0, 270.0);
    h.hover_at(g);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: g, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=10 {
        h.event(egui::Event::PointerMoved(at(700.0, 270.0 - 30.0 * i as f32)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: at(700.0, -30.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(guides(&h).len(), 1);
    assert!(guides(&h)[0].vertical);

    // Hidden guides stay in the file but aren't drawn or grabbed.
    h.state_mut().run("view.guides", json!({"on": false})).unwrap();
    h.run_steps(2);
    assert!(!h.state().session.tool.guides_visible);
    assert_eq!(guides(&h).len(), 1);
}
