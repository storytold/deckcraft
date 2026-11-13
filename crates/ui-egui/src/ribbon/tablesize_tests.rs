//! Table Layout ▸ Cell Size: the Column Width and Row Height fields run the same
//! `table.columnWidth` / `table.rowHeight` commands a border drag does, so a live session
//! queues them like any other edit.

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> (Harness<'static, SlideApp>, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = ShapeId(
        s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100.0, 100.0, 300.0, 100.0]})).unwrap()["id"].as_u64().unwrap() as u32,
    );
    s.execute("selection.select", &json!({"ids": [id.0]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Layout".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, id)
}

fn table(h: &Harness<'_, SlideApp>, id: ShapeId) -> deckcraft_model::Table {
    match &h.state().session.active().unwrap().shape(id).unwrap().kind {
        deckcraft_model::ShapeKind::Table(t) => t.clone(),
        other => panic!("not a table: {other:?}"),
    }
}

#[test]
fn the_cell_size_fields_run_the_sizing_commands() {
    let (mut h, id) = harness();
    assert_eq!(table(&h, id).cols[0], 150.0, "the inserted table starts with two equal columns");

    // The field is a number input named by the label beside it; a screen reader sets its value.
    h.get_by_label("Column Width").set_value_accesskit(200.0);
    h.run_steps(3);
    assert_eq!(table(&h, id).cols[0], 200.0, "the column width command ran");
    assert_eq!(table(&h, id).width(), 350.0, "the table grew by the difference");

    h.get_by_label("Row Height").set_value_accesskit(60.0);
    h.run_steps(3);
    assert_eq!(table(&h, id).rows[0].height, 60.0, "the row height command ran");
    assert_eq!(table(&h, id).height(), 110.0);
    let boxed = h.state().session.active().unwrap().shape(id).unwrap().xfrm.unwrap();
    assert_eq!((boxed.w, boxed.h), (350.0, 110.0), "the shape box follows the table's size");
}
