//! Draw › Ink to Shape driven through the ribbon: pen a rough rectangle on the slide with the
//! mouse, press the button, get a rectangle shape.

use deckcraft_engine::Session;
use egui::{Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn pen_a_rectangle_then_ink_to_shape() {
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
    h.get_by_label("Black Pen").click();
    h.run_steps(2);
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let at = |x: f32, y: f32| slide.min + egui::vec2(x * k, y * k);
    let corners = [(200.0, 150.0), (600.0, 150.0), (600.0, 400.0), (200.0, 400.0), (204.0, 152.0)];
    h.hover_at(at(200.0, 150.0));
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at(200.0, 150.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    for w in corners.windows(2) {
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            let p: Pos2 = at(w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            h.event(egui::Event::PointerMoved(p));
            h.run_steps(1);
        }
    }
    h.event(egui::Event::PointerButton { pos: at(204.0, 152.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let has_ink = |h: &Harness<'_, SlideApp>| {
        h.state().session.active().unwrap().shapes().iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Ink { .. }))
    };
    assert!(has_ink(&h), "the pen drew a stroke");
    h.get_by_label("Ink to Shape").click();
    h.run_steps(3);
    assert!(!has_ink(&h), "the only stroke became a shape");
    let st = h.state().session.active().unwrap();
    let sh = st.shapes().last().unwrap();
    assert!(matches!(&sh.geom, deckcraft_model::Geom::Preset { name, .. } if name == "rect"), "{:?}", sh.geom);
    let x = sh.xfrm.unwrap();
    assert!((x.x - 200.0).abs() < 10.0 && (x.w - 400.0).abs() < 15.0 && (x.h - 250.0).abs() < 15.0, "{x:?}");
    assert_eq!(st.selection.shapes, vec![sh.id]);
    if let Ok(img) = h.render() {
        let _ = img.save("/tmp/ink_to_shape.png");
    }
}
