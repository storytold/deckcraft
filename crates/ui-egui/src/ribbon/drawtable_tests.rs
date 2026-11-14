//! Draw Table driven like a person: Insert › Table › Draw Table, drag out a table, draw a border
//! across it, then the Table Eraser merges the cells back.

use deckcraft_engine::Session;
use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn drag(h: &mut Harness<'_, SlideApp>, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=10 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (i as f32 / 10.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn table(h: &Harness<'_, SlideApp>) -> Option<(deckcraft_model::Table, deckcraft_geom::Xfrm)> {
    h.state().session.active()?.shapes().iter().find_map(|s| match &s.kind {
        deckcraft_model::ShapeKind::Table(t) => Some((t.clone(), s.xfrm?)),
        _ => None,
    })
}

#[test]
fn draw_a_table_split_it_and_erase_the_border() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Table").click();
    h.run_steps(2);
    h.get_by_label("Draw Table").click();
    h.run_steps(2);
    let (slide, k) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
    let at = |x: f32, y: f32| slide.min + egui::vec2(x * k, y * k);

    drag(&mut h, at(200.0, 100.0), at(600.0, 300.0));
    let (t, x) = table(&h).expect("the pencil drew a table");
    assert_eq!((t.rows.len(), t.cols.len()), (1, 1));
    assert!((x.w - 400.0).abs() < 3.0 && (x.h - 200.0).abs() < 3.0, "{x:?}");

    // Still the pencil: a border across, then one down the bottom half only.
    drag(&mut h, at(205.0, 180.0), at(610.0, 180.0));
    drag(&mut h, at(350.0, 190.0), at(350.0, 299.0));
    let (t, _) = table(&h).unwrap();
    assert_eq!((t.rows.len(), t.cols.len()), (2, 2), "{t:?}");
    assert_eq!(t.cell(0, 0).unwrap().grid_span, 2, "the top row stays one cell");

    // The eraser on the vertical border merges the bottom cells again.
    let ids: Vec<u32> = h.state().session.active().unwrap().shapes().iter().map(|s| s.id.0).collect();
    h.state_mut().session.execute("edit.select", &json!({"ids": ids})).unwrap();
    h.state_mut().ui.tab = "Table Design".into();
    h.run_steps(3);
    h.get_by_label("Table Eraser").click();
    h.run_steps(2);
    h.hover_at(at(350.0, 250.0));
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at(350.0, 250.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at(350.0, 250.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let (t, _) = table(&h).unwrap();
    assert_eq!(t.cell(1, 0).unwrap().grid_span, 2, "{t:?}");
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(table(&h).unwrap().0.cell(1, 0).unwrap().grid_span, 1);
}
