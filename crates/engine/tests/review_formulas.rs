//! Independent transaction, native authority and interchange preservation fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_model::{
    table::{CellConstant, CellFormula, CellNumberKind, CellValue, FormulaOutcome},
    *,
};
use serde_json::json;

fn session() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let r = s.execute("insert.table", &json!({"rows":1,"cols":3,"rect":[10,10,240,40]})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"2"},"display":"two"}),
    )
    .unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":"=[.A1]*3"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":2,"expression":"=[.B1]+1"})).unwrap();
    (s, id)
}

fn table(s: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(t) = &s.doc().unwrap().shape(id).unwrap().kind else { panic!() };
    t
}

#[test]
fn direct_session_edit_settles_formula_graph_before_publication() {
    let (mut s, id) = session();
    s.edit(|p, _| {
        let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(t) = &mut shape.kind else { panic!() };
        let c = t.cell_mut(0, 0).unwrap();
        c.text = TextBody::from_text("four");
        c.data = Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: "4".into() }, &c.text)?));
        Ok(())
    })
    .unwrap();
    assert_eq!(table(&s, id).cell(0, 1).unwrap().text.text(), "12");
    assert_eq!(table(&s, id).cell(0, 2).unwrap().text.text(), "13");
    deckcraft_model::table::validate_constants(&s.doc().unwrap().doc).unwrap();
}

#[test]
fn unrelated_hidden_owner_failure_rolls_back_text_edit_history_selection_and_redo() {
    let (mut s, id) = session();
    s.execute("table.rowHeight", &json!({"id":id,"row":0,"height":60})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let mut damaged = Table::new(1, 1, 20., 20.);
    let c = damaged.cell_mut(0, 0).unwrap();
    c.text = TextBody::from_text("1");
    c.formula = Some(Arc::new(CellFormula::number("=[.ZZ999]".into(), "1".into(), &c.text).unwrap()));
    // Deliberate in-memory damage: ordinary readers refuse this before admission.
    Arc::make_mut(&mut s.doc_mut().unwrap().doc).masters.push(Arc::new(Master {
        id: MasterId(9000),
        shapes: vec![Shape { id: ShapeId(9001), kind: ShapeKind::Table(damaged), ..Default::default() }],
        ..Default::default()
    }));
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let history = (s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len());
    let requests = s.ui_requests.len();
    assert!(s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"changed"})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!((s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len()), history);
    assert_eq!(s.ui_requests.len(), requests);
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text.text(), "two");
}

#[test]
fn native_save_rejects_forged_cache_even_with_stale_neighbor_and_preserves_input() {
    let (mut s, id) = session();
    let p = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|shape| shape.id == id).unwrap();
    let ShapeKind::Table(t) = &mut shape.kind else { panic!() };
    t.cell_mut(0, 1).unwrap().text = TextBody::from_text("literal");
    let c = t.cell_mut(0, 2).unwrap();
    c.text = TextBody::from_text("999");
    c.formula = Some(Arc::new(CellFormula::number("=[.B1]+1".into(), "999".into(), &c.text).unwrap()));
    let before = s.doc().unwrap().doc.clone();
    assert!(deckcraft_format::save(&before).unwrap_err().to_string().contains("cached outcome"));
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn native_stale_display_roundtrip_lowers_source_and_retains_current_error_without_old_scalar() {
    let (mut s, id) = session();
    let p = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|shape| shape.id == id).unwrap();
    let ShapeKind::Table(t) = &mut shape.kind else { panic!() };
    t.cell_mut(0, 1).unwrap().text = TextBody::from_text("edited display");
    let reopened = deckcraft_format::load(&deckcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    let ShapeKind::Table(t) = &reopened.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().kind else { panic!() };
    assert!(t.cell(0, 1).unwrap().formula.is_none());
    assert_eq!(t.cell(0, 1).unwrap().text.text(), "edited display");
    assert!(matches!(t.cell(0, 2).unwrap().formula.as_ref().unwrap().outcome(), FormulaOutcome::Error { .. }));
    assert!(t.cell(0, 2).unwrap().data.is_none());
    let before = reopened.clone();
    for format in ["odp", "pptx", "potx", "ppsx"] {
        let bytes = deckcraft_engine::cmd::file::save_bytes(&reopened, format).unwrap();
        let back = if format == "odp" { deckcraft_odp::import(&bytes).unwrap() } else { deckcraft_pptx::import(&bytes).unwrap() };
        let actual = back.slides[0]
            .shapes
            .iter()
            .find_map(|shape| match &shape.kind {
                ShapeKind::Table(table) => Some(table),
                _ => None,
            })
            .unwrap();
        for col in 0..3 {
            let expected = t.cell(0, col).unwrap();
            let actual = actual.cell(0, col).unwrap();
            assert_eq!(actual.text.text(), expected.text.text(), "{format}: current display");
            assert_eq!(actual.data, expected.data, "{format}: stored scalar authority");
            assert_eq!(actual.formula, expected.formula, "{format}: current formula/error authority");
        }
        assert_eq!(reopened, before, "export must not mutate its source");
    }
}
