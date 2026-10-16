//! Independent contributor-original typed-cell command and ownership regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::{Session, Target};
use deckcraft_model::{
    Cell, Master, Shape, ShapeId, ShapeKind, Table, TextBody,
    notes::{NotesMaster, NotesPage},
    table::{CellConstant, CellValue},
};
use serde_json::json;
use std::sync::Arc;

fn session() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let id = s.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[100,100,240,100]})).unwrap()["id"].as_u64().unwrap();
    let id = ShapeId(u32::try_from(id).unwrap());
    s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":0,"value":{"kind":"string","value":"stored"},"display":"Shown"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[0,0]})).unwrap();
    (s, id)
}
fn shape_table(shape: &Shape) -> &Table {
    match &shape.kind {
        ShapeKind::Table(table) => Some(table),
        _ => None,
    }
    .expect("fixture table")
}
fn table(s: &Session, id: ShapeId) -> &Table {
    shape_table(s.doc().unwrap().shape(id).unwrap())
}
fn record(s: &Session, id: ShapeId) -> Arc<CellConstant> {
    table(s, id).cell(0, 0).unwrap().data.clone().unwrap()
}
fn pure_table() -> Table {
    let mut t = Table::new(2, 1, 120., 40.);
    for (row, text) in [(0, "A"), (1, "B")] {
        let cell = t.cell_mut(row, 0).unwrap();
        cell.text = TextBody::from_text(text);
        cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: format!("stored {text}") }, &cell.text).unwrap()));
    }
    t
}
fn document(t: Table) -> deckcraft_model::Presentation {
    let mut p = deckcraft_model::Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(9000), kind: ShapeKind::Table(t), ..Default::default() }];
    p
}

#[test]
fn ambiguous_or_out_of_bounds_typed_anchors_refuse_before_native_or_command_repair() {
    let mut cases = Vec::new();
    let mut t = pure_table();
    t.cell_mut(0, 0).unwrap().row_span = 2;
    cases.push(t);
    let mut t = pure_table();
    t.cell_mut(0, 0).unwrap().row_span = 3;
    cases.push(t);
    let mut t = pure_table();
    t.cell_mut(0, 0).unwrap().grid_span = 2;
    cases.push(t);
    let mut t = pure_table();
    let orphan = t.cell_mut(1, 0).unwrap();
    orphan.data = None;
    orphan.v_merge = true;
    cases.push(t);
    let mut horizontal = Table::new(1, 2, 120., 40.);
    let values = pure_table();
    *horizontal.cell_mut(0, 0).unwrap() = values.cell(0, 0).unwrap().clone();
    *horizontal.cell_mut(0, 1).unwrap() = values.cell(1, 0).unwrap().clone();
    horizontal.cell_mut(0, 0).unwrap().grid_span = 2;
    cases.push(horizontal);
    for t in cases {
        let p = document(t.clone());
        assert!(deckcraft_model::table::validate_constants(&p).is_err());
        assert!(deckcraft_format::save(&p).is_err(), "native save cannot bless malformed typed ownership");
        let mut drawn = t.clone();
        assert!(!crate::drawtable::draw_line(&mut drawn, (0., 20.), (120., 20.)));
        assert_eq!(drawn, t);
        assert!(!crate::drawtable::erase_at(&mut drawn, (60., 40.), 3.));
        assert_eq!(drawn, t);
        // Public model callers must receive the same protection as engine/native
        // boundaries: mutation cannot erase evidence of ambiguous typed ownership.
        let mut direct = t.clone();
        direct.insert_row(0);
        assert_eq!(direct, t);
        direct.insert_col(0);
        assert_eq!(direct, t);
        assert!(!direct.delete_row(0));
        assert_eq!(direct, t);
        assert!(!direct.delete_col(0));
        assert_eq!(direct, t);
        assert!(!direct.merge(0, 0, t.n_rows() - 1, t.n_cols() - 1));
        assert_eq!(direct, t);
        assert!(!direct.split(0, 0));
        assert_eq!(direct, t);
        direct.normalize();
        assert_eq!(direct, t);
        let (mut s, id) = session();
        s.edit(|doc, sel| {
            let shape = deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), id).unwrap();
            shape.kind = ShapeKind::Table(t.clone());
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("table.deleteRow", &json!({"id":id,"row":0})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert_eq!(table(&s, id), &t);
    }
    let mut valid = pure_table();
    valid.cell_mut(0, 0).unwrap().row_span = 2;
    *valid.cell_mut(1, 0).unwrap() = Cell { v_merge: true, grid_span: 1, row_span: 1, ..Cell::new("") };
    let p = document(valid);
    deckcraft_model::table::validate_constants(&p).unwrap();
    assert!(deckcraft_format::save(&p).is_ok());
}

#[test]
fn indexed_eraser_preserves_every_unrelated_cell_in_a_large_mixed_grid() {
    for across_columns in [false, true] {
        let mut table = Table::new(40, 40, 800., 20.);
        for (row_index, row) in table.rows.iter_mut().enumerate() {
            for (column_index, cell) in row.cells.iter_mut().enumerate() {
                cell.text = TextBody::from_text(&format!("r{row_index}c{column_index}"));
                if (row_index + column_index) % 3 == 0 {
                    cell.data = Some(Arc::new(
                        CellConstant::from_display(CellValue::String { value: format!("stored-{row_index}-{column_index}") }, &cell.text).unwrap(),
                    ));
                }
            }
        }
        let before = table.clone();
        let (row, column) = (17, 18);
        let neighbour = if across_columns { (row, column + 1) } else { (row + 1, column) };
        assert!(before.cell(row, column).unwrap().data.is_none());
        assert!(before.cell(neighbour.0, neighbour.1).unwrap().data.is_some());
        let point = if across_columns { (380., 350.) } else { (370., 360.) };
        assert!(crate::drawtable::erase_at(&mut table, point, 1.));
        assert_eq!((table.n_rows(), table.n_cols()), (40, 40));
        let anchor = table.cell(row, column).unwrap();
        assert_eq!((anchor.row_span, anchor.grid_span), if across_columns { (1, 2) } else { (2, 1) });
        assert!(anchor.data.is_none());
        assert_eq!(
            anchor.text.text(),
            format!("{}\n{}", before.cell(row, column).unwrap().text.text(), before.cell(neighbour.0, neighbour.1).unwrap().text.text())
        );
        let covered = table.cell(neighbour.0, neighbour.1).unwrap();
        assert_eq!((covered.h_merge, covered.v_merge), (across_columns, !across_columns));
        assert!(covered.text.is_empty() && covered.data.is_none());
        for r in 0..40 {
            for c in 0..40 {
                if (r, c) == (row, column) || (r, c) == neighbour {
                    continue;
                }
                let actual = table.cell(r, c).unwrap();
                let original = before.cell(r, c).unwrap();
                assert_eq!(actual, original, "unrelated cell ({r},{c}) changed");
                if let Some(data) = &original.data {
                    assert!(Arc::ptr_eq(data, actual.data.as_ref().unwrap()), "unrelated stored value ({r},{c}) was rebound");
                }
            }
        }
        deckcraft_model::table::validate_constants(&document(table)).unwrap();
    }
}

#[test]
fn prospective_typed_grid_growth_checks_addition_product_and_exact_cell_limit_without_expansion() {
    use deckcraft_model::table::validate_constant_growth;
    let table = pure_table();
    let before = table.clone();
    for (rows, cols) in [(0, 0), (999_998, 0), (0, 499_999), (998, 999)] {
        validate_constant_growth(&table, rows, cols).unwrap();
        assert_eq!(table, before);
    }
    for (rows, cols) in [(999_999, 0), (0, 500_000), (998, 1000), (usize::MAX, 0), (0, usize::MAX), (usize::MAX, usize::MAX)] {
        assert!(validate_constant_growth(&table, rows, cols).is_err(), "unbounded prospective growth ({rows},{cols})");
        assert_eq!(table, before);
    }
    let mut malformed = table.clone();
    malformed.cell_mut(0, 0).unwrap().row_span = 2;
    let unchanged = malformed.clone();
    assert!(validate_constant_growth(&malformed, 0, 0).is_err(), "prospective checks must validate existing ownership first");
    assert_eq!(malformed, unchanged);
    assert_eq!((table.n_rows(), table.n_cols()), (2, 1), "boundary checks must never allocate the prospective grid");
}

#[test]
fn master_owned_composition_settles_its_original_owner_after_view_change() {
    let (mut s, id) = session();
    let slide_value = record(&s, id);
    let master_id = ShapeId(9001);
    s.edit(|doc, _| {
        let mut shape = doc.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().clone();
        shape.id = master_id;
        Arc::make_mut(&mut doc.masters[0]).shapes.push(shape);
        Ok(())
    })
    .unwrap();
    s.select(|_, sel| {
        sel.target = Target::Master { master: 0 };
        sel.shapes = vec![master_id];
        sel.text = None;
    })
    .unwrap();
    s.execute("table.setCellValue", &json!({"id":master_id,"row":0,"col":0,"value":{"kind":"boolean","value":false},"display":"Master"})).unwrap();
    let original = record(&s, master_id);
    s.execute("text.edit", &json!({"id":master_id,"cell":[0,0]})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("text.compose", &json!({"text":"Changed"})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, master_id)));
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    s.select(|_, sel| {
        sel.target = Target::Slides;
        sel.shapes = vec![id];
        sel.text = None;
    })
    .unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    let master_table = shape_table(s.doc().unwrap().doc.masters[0].shapes.iter().find(|shape| shape.id == master_id).unwrap());
    assert!(master_table.cell(0, 0).unwrap().data.is_none());
    assert_eq!(master_table.cell(0, 0).unwrap().text.text(), "Changed");
    assert!(Arc::ptr_eq(&slide_value, &record(&s, id)));
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Master { master: 0 });
    assert!(Arc::ptr_eq(&original, &record(&s, master_id)));
}

#[test]
fn find_case_soft_break_and_formatting_follow_exact_display_content() {
    let (mut s, id) = session();
    s.execute(
        "table.setCellValue",
        &json!({"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"9007199254740993"},"display":"A\u{b}B"}),
    )
    .unwrap();
    let original = record(&s, id);
    let document = s.doc().unwrap().doc.clone();
    s.execute("text.find", &json!({"text":"A"})).unwrap();
    assert!(Arc::ptr_eq(&document, &s.doc().unwrap().doc));
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("format.case", &json!({"mode":"upper"})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    s.execute("format.bold", &json!({"on":true})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"A\nB"})).unwrap();
    assert!(table(&s, id).cell(0, 0).unwrap().data.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("format.case", &json!({"mode":"lower"})).unwrap();
    assert!(table(&s, id).cell(0, 0).unwrap().data.is_none());
}

#[test]
fn explicit_target_and_hostile_coordinates_never_fall_back_to_the_edited_cell() {
    let (mut s, id) = session();
    let record = record(&s, id);
    for bad in [
        json!({"row":null}),
        json!({"row":"0"}),
        json!({"row":0.5}),
        json!({"row":u64::MAX}),
        json!({"col":-1}),
        json!({"id":u64::MAX}),
        json!({"row":false}),
    ] {
        let mut params = json!({"value":{"kind":"boolean","value":true},"display":"new"});
        for (key, value) in bad.as_object().unwrap() {
            params[key] = value.clone();
        }
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("table.setCellValue", &params).is_err(), "{bad}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert!(Arc::ptr_eq(&record, table(&s, id).cell(0, 0).unwrap().data.as_ref().unwrap()));
    }
    let other = s.execute("insert.table", &json!({"rows":1,"cols":1})).unwrap()["id"].clone();
    s.execute("text.edit", &json!({"id":id,"cell":[0,0]})).unwrap();
    assert!(s.execute("table.setCellValue", &json!({"id":other,"value":{"kind":"boolean","value":true},"display":"wrong target"})).is_err());
    assert!(Arc::ptr_eq(&record, table(&s, id).cell(0, 0).unwrap().data.as_ref().unwrap()));
}

#[test]
fn history_counts_shared_metadata_per_attachment_across_every_native_owner() {
    let data = Arc::new(CellConstant::from_display(CellValue::String { value: "x".repeat(64 * 1024) }, &TextBody::from_text("cached")).unwrap());
    let make_shape = || {
        let mut t = Table::new(1, 1, 100., 40.);
        let c = t.cell_mut(0, 0).unwrap();
        c.text = TextBody::from_text("cached");
        c.data = Some(data.clone());
        Shape { kind: ShapeKind::Table(t), ..Default::default() }
    };
    let make_master = || {
        Arc::new(Master {
            shapes: vec![make_shape()],
            layouts: vec![deckcraft_model::Layout { shapes: vec![make_shape()], ..Default::default() }],
            ..Default::default()
        })
    };
    let mut old = deckcraft_model::Presentation::default();
    Arc::make_mut(&mut old.slides[0]).shapes = vec![make_shape()];
    Arc::make_mut(&mut old.slides[0]).notes_page = Some(NotesPage { shapes: vec![make_shape()], ..Default::default() });
    old.masters = vec![make_master()];
    old.handout_master = Some(make_master());
    old.notes_master = Some(make_master());
    old.notes_masters = vec![NotesMaster { master: make_master(), ..Default::default() }];
    let mut newer = old.clone();
    Arc::make_mut(&mut newer.slides[0]);
    Arc::make_mut(&mut newer.masters[0]);
    Arc::make_mut(newer.handout_master.as_mut().unwrap());
    Arc::make_mut(newer.notes_master.as_mut().unwrap());
    Arc::make_mut(&mut newer.notes_masters[0].master);
    assert!(
        crate::undo_cost(&old, &newer) >= 10 * data.metadata_bytes(),
        "each changed owner retains its table metadata even if all records share one Arc"
    );
    let (mut s, id) = session();
    s.doc_mut().unwrap().history.byte_limit = 20 * 1024;
    for _ in 0..3 {
        s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":0,"value":{"kind":"string","value":"x".repeat(64*1024)},"display":"Shown"}))
            .unwrap();
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), 1, "only latest over-budget history step is retained");
}
