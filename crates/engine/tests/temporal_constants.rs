//! Public command, history and rendering witnesses for temporal table constants.
use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_model::{Cell, ShapeId, ShapeKind, table::CellConstant};
use serde_json::{Value, json};

fn setup(value: &Value) -> (Session, ShapeId) {
    let mut session = Session::with_new();
    let id = ShapeId(session.execute("insert.table", &json!({"rows":1,"cols":2,"rect":[50,50,300,70]})).unwrap()["id"].as_u64().unwrap() as u32);
    for col in 0..2 {
        let display = if col == 0 { "cached label" } else { "untouched label" };
        session.execute("table.setCellValue", &json!({"id":id,"row":0,"col":col,"value":value,"display":display})).unwrap();
    }
    session.execute("text.edit", &json!({"id":id,"cell":[0,0],"end":true})).unwrap();
    (session, id)
}
fn cell(session: &Session, id: ShapeId, col: usize) -> &Cell {
    let ShapeKind::Table(table) = &session.doc().unwrap().shape(id).unwrap().kind else { panic!("table") };
    table.cell(0, col).unwrap()
}
fn record(session: &Session, id: ShapeId, col: usize) -> Arc<CellConstant> {
    cell(session, id, col).data.as_ref().unwrap().clone()
}
fn values() -> [Value; 2] {
    [json!({"kind":"date","lexical":" \t2024-02-29T24:00:00.000+00:00\r\n"}), json!({"kind":"duration","lexical":"-P001Y02M03DT004H05M06.700S"})]
}

#[test]
fn formatting_and_native_roundtrip_keep_temporal_metadata_and_cached_pixels() {
    for value in values() {
        let (mut session, id) = setup(&value);
        let saved = record(&session, id, 0);
        session.execute("text.selectAll", &json!({})).unwrap();
        session.execute("format.bold", &json!({"on":true})).unwrap();
        assert!(Arc::ptr_eq(&saved, &record(&session, id, 0)));
        let original = session.doc().unwrap().doc.clone();
        let opts = deckcraft_render::RenderOpts { scale: 0.25, ..Default::default() };
        let before = deckcraft_render::render_slide(&original, 0, &opts);
        let native = deckcraft_format::save(&original).unwrap();
        let reopened = deckcraft_format::load(&native).unwrap();
        assert_eq!(deckcraft_render::render_slide(&reopened, 0, &opts).pixels, before.pixels);
        let mut other = (*original).clone();
        let shape = Arc::make_mut(&mut other.slides[0]).shapes.iter_mut().find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(table) = &mut shape.kind else { panic!("table") };
        let cell = &mut table.rows[0].cells[0];
        cell.data =
            Some(Arc::new(CellConstant::from_display(deckcraft_model::table::CellValue::Duration { lexical: "P999Y".into() }, &cell.text).unwrap()));
        assert_eq!(deckcraft_render::render_slide(&other, 0, &opts).pixels, before.pixels, "stored metadata must not replace rich cached display");
        let ShapeKind::Table(expected) = &original.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().kind else { panic!("source table") };
        let snapshot = (*original).clone();
        for format in ["pptx", "potx", "ppsx"] {
            let bytes = deckcraft_engine::cmd::file::save_bytes(&original, format).unwrap();
            let back = deckcraft_pptx::import(&bytes).unwrap();
            let actual = back.slides[0]
                .shapes
                .iter()
                .find_map(|shape| match &shape.kind {
                    ShapeKind::Table(table) => Some(table),
                    _ => None,
                })
                .unwrap();
            assert_eq!(actual, expected, "{format}: typed metadata and rich cached display");
            assert_eq!(serde_json::to_value(actual.cell(0, 0).unwrap().data.as_ref().unwrap().value()).unwrap(), value);
            assert_eq!(actual.cell(0, 0).unwrap().text.text(), "cached label");
            assert_eq!(actual.cell(0, 1).unwrap().text.text(), "untouched label");
            let twice = deckcraft_pptx::import(&deckcraft_engine::cmd::file::save_bytes(&back, format).unwrap()).unwrap();
            let repeated = twice.slides[0]
                .shapes
                .iter()
                .find_map(|shape| match &shape.kind {
                    ShapeKind::Table(table) => Some(table),
                    _ => None,
                })
                .unwrap();
            assert_eq!(repeated, actual, "{format}: second interchange roundtrip");
            assert_eq!(*original, snapshot, "{format}: export mutated source");
            assert!(Arc::ptr_eq(&saved, &record(&session, id, 0)));
        }
    }
}

#[test]
fn actual_content_commands_clear_only_touched_temporal_values_and_undo_restores_authority() {
    for value in values() {
        for (command, mut params) in [
            ("text.insert", json!({"text":"!"})),
            ("text.set", json!({"cell":[0,0],"text":"new label"})),
            ("text.replace", json!({"text":"cached","with":"changed"})),
            ("text.commit", json!({"text":"committed"})),
        ] {
            let (mut session, id) = setup(&value);
            let saved = record(&session, id, 0);
            let untouched = record(&session, id, 1);
            if command == "text.set" {
                params["id"] = json!(id);
            }
            if command == "text.commit" {
                session.execute("text.selectAll", &json!({})).unwrap();
                session.execute("text.compose", &json!({"text":"preedit"})).unwrap();
            }
            session.execute(command, &params).unwrap();
            assert!(cell(&session, id, 0).data.is_none(), "{command}");
            assert!(Arc::ptr_eq(&untouched, &record(&session, id, 1)));
            session.execute("edit.undo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&saved, &record(&session, id, 0)), "{command}");
            session.execute("edit.redo", &json!({})).unwrap();
            assert!(cell(&session, id, 0).data.is_none());
        }
    }
}

#[test]
fn canceled_composition_and_invalid_temporal_assignment_preserve_document_and_history() {
    for value in values() {
        let (mut session, id) = setup(&value);
        let before = session.doc().unwrap().doc.clone();
        let saved = record(&session, id, 0);
        let undo = session.doc().unwrap().history.undo.len();
        session.execute("text.selectAll", &json!({})).unwrap();
        session.execute("text.compose", &json!({"text":"temporary"})).unwrap();
        session.execute("text.compose", &json!({"text":""})).unwrap();
        assert!(Arc::ptr_eq(&before, &session.doc().unwrap().doc));
        assert!(Arc::ptr_eq(&saved, &record(&session, id, 0)));
        for value in [
            json!({"kind":"date","lexical":"1900-02-29"}),
            json!({"kind":"duration","lexical":"P1DT"}),
            json!({"kind":"duration","lexical":format!("P{}D","9".repeat(255))}),
            json!({"kind":"date","lexical":"2024-02-29","value":"bad"}),
        ] {
            assert!(session.execute("table.setCellValue", &json!({"id":id,"row":0,"col":0,"value":value,"display":"changed"})).is_err());
            assert!(Arc::ptr_eq(&before, &session.doc().unwrap().doc));
            assert_eq!(session.doc().unwrap().history.undo.len(), undo);
        }
    }
}
