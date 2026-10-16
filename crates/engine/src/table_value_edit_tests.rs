//! Typed constants retain ownership through formatting/copies and become literal after content edits.
use std::sync::Arc;

use deckcraft_model::table::{CellConstant, CellNumberKind, CellValue};
use deckcraft_model::{Cell, ShapeId, ShapeKind, Table, TextBody};
use serde_json::json;

use crate::{Session, cmd, shapes_mut};

fn session(display: &str) -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let id = s.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[100,100,240,100]})).unwrap()["id"].as_u64().unwrap();
    let id = ShapeId(id as u32);
    s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":0,"value":{"kind":"string","value":"authoritative"},"display":display})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[0,0],"end":true})).unwrap();
    (s, id)
}
fn table(s: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(t) = &s.doc().unwrap().shape(id).unwrap().kind else { panic!("table missing") };
    t
}
fn cell(s: &Session, id: ShapeId) -> &Cell {
    table(s, id).cell(0, 0).unwrap()
}
fn record(s: &Session, id: ShapeId) -> Arc<CellConstant> {
    cell(s, id).data.as_ref().unwrap().clone()
}
fn all(s: &mut Session) {
    s.execute("text.selectAll", &json!({})).unwrap();
}

#[test]
fn explicit_constants_keep_stored_value_separate_from_the_required_display() {
    let (mut s, id) = session("shown");
    for (value, expected) in [
        (json!({"kind":"string","value":"not shown"}), CellValue::String { value: "not shown".into() }),
        (json!({"kind":"boolean","value":false}), CellValue::Boolean { value: false }),
        (
            json!({"kind":"number","numberKind":{"kind":"float"},"lexical":"9007199254740993"}),
            CellValue::Number { kind: CellNumberKind::Float, lexical: "9007199254740993".into() },
        ),
        (
            json!({"kind":"number","numberKind":{"kind":"percentage"},"lexical":"0.5"}),
            CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5".into() },
        ),
        (
            json!({"kind":"number","numberKind":{"kind":"currency","currency":""},"lexical":"-0.00e+0"}),
            CellValue::Number { kind: CellNumberKind::Currency { currency: Some("".into()) }, lexical: "-0.00e+0".into() },
        ),
    ] {
        s.execute("table.setCellValue", &json!({"value":value,"display":"cached 漢\n50%"})).unwrap();
        assert_eq!(record(&s, id).value(), &expected);
        assert_eq!(cell(&s, id).text.text(), "cached 漢\n50%");
        assert_eq!(record(&s, id).display_baseline(), &["cached 漢", "50%"]);
    }
}

#[test]
fn formatting_run_splits_and_semantic_noops_keep_the_same_value_arc() {
    let (mut s, id) = session("shown");
    let before = record(&s, id);
    s.execute("text.select", &json!({"anchor":[0,1],"caret":[0,3]})).unwrap();
    s.execute("format.bold", &json!({"on":true})).unwrap();
    assert!(cell(&s, id).text.paragraphs[0].runs.len() > 1);
    assert!(Arc::ptr_eq(&before, &record(&s, id)));
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"shown"})).unwrap();
    s.execute("text.replace", &json!({"text":"shown","with":"shown","matchCase":true})).unwrap();
    all(&mut s);
    s.execute("edit.paste", &json!({"text":"shown","html":"<b>shown</b>"})).unwrap();
    assert!(Arc::ptr_eq(&before, &record(&s, id)));
}

#[test]
fn content_mutations_become_literal_and_undo_restores_the_original_arc() {
    for (command, params) in [
        ("text.insert", json!({"text":"!"})),
        ("text.delete", json!({"dir":"backward"})),
        ("edit.paste", json!({"text":"other","html":"<i>other</i>"})),
        ("text.set", json!({"text":"other","cell":[0,0]})),
        ("text.replace", json!({"text":"teh","with":"the"})),
        ("review.spellingChange", json!({"word":"teh","to":"the","para":0,"start":0,"row":0,"col":0})),
    ] {
        let (mut s, id) = session("teh");
        let before = record(&s, id);
        let mut params = params;
        if command == "text.set" {
            params["id"] = json!(id);
        }
        if command == "review.spellingChange" {
            params["shape"] = json!(id);
        }
        s.execute(command, &params).unwrap();
        assert!(cell(&s, id).data.is_none(), "{command}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(cell(&s, id).text.text(), "teh", "{command}");
        assert!(Arc::ptr_eq(&before, &record(&s, id)), "{command}");
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(cell(&s, id).data.is_none(), "{command}");
    }
}

#[test]
fn autocorrect_reconciles_even_when_the_insert_itself_did_not_change_display() {
    let (mut s, id) = session("teh ");
    s.execute("text.autocorrect", &json!({"on":true,"sentenceCaps":false})).unwrap();
    let before = record(&s, id);
    s.execute("text.insert", &json!({"text":""})).unwrap();
    assert_eq!(cell(&s, id).text.text(), "the ");
    assert!(cell(&s, id).data.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&before, &record(&s, id)));
}

#[test]
fn field_and_math_runs_invalidate_even_when_their_visible_text_is_equal() {
    let (mut s, id) = session("shown");
    all(&mut s);
    cmd::text::insert_field(&mut s, "custom", "shown").unwrap();
    assert_eq!(cell(&s, id).text.text(), "shown");
    assert!(cell(&s, id).data.is_none());
    let (mut s, id) = session("shown");
    all(&mut s);
    let mut body = TextBody::from_text("shown");
    body.paragraphs[0].runs[0].kind = deckcraft_model::text::RunKind::Math { omml: "<m:oMath/>".into() };
    cmd::text::paste_body(&mut s, &body).unwrap();
    assert_eq!(cell(&s, id).text.text(), "shown");
    assert!(cell(&s, id).data.is_none());
}

#[test]
fn ime_cancel_restores_the_entire_document_and_value_without_history() {
    let (mut s, id) = session("漢字");
    all(&mut s);
    let before = s.doc().unwrap().doc.clone();
    let value = record(&s, id);
    let undo = s.doc().unwrap().history.undo.len();
    for text in ["か", "仮"] {
        s.execute("text.compose", &json!({"text":text})).unwrap();
        assert!(Arc::ptr_eq(&value, &record(&s, id)), "preedit defers reconciliation");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
    s.execute("text.compose", &json!({"text":""})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert!(Arc::ptr_eq(&value, &record(&s, id)));
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
}

#[test]
fn ime_final_equal_display_retains_value_changed_commit_clears_and_undo_restores() {
    for final_text in ["漢字", "別字"] {
        let (mut s, id) = session("漢字");
        all(&mut s);
        let before = record(&s, id);
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("text.compose", &json!({"text":"か"})).unwrap();
        s.execute("text.commit", &json!({"text":final_text})).unwrap();
        assert_eq!(cell(&s, id).text.text(), final_text);
        if final_text == "漢字" {
            assert!(Arc::ptr_eq(&before, &record(&s, id)));
        } else {
            assert!(cell(&s, id).data.is_none());
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(cell(&s, id).text.text(), "漢字");
        assert!(Arc::ptr_eq(&before, &record(&s, id)));
    }
}

#[test]
fn ime_settle_after_selection_moves_reconciles_its_original_cell_only() {
    let (mut s, id) = session("old");
    let original = record(&s, id);
    s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":1,"value":{"kind":"boolean","value":false},"display":"other"})).unwrap();
    let other = table(&s, id).cell(0, 1).unwrap().data.clone().unwrap();
    s.execute("text.compose", &json!({"text":"new"})).unwrap();
    s.select(|_, sel| {
        sel.text.as_mut().unwrap().cell = Some((0, 1));
    })
    .unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    assert!(cell(&s, id).data.is_none());
    assert!(Arc::ptr_eq(&other, table(&s, id).cell(0, 1).unwrap().data.as_ref().unwrap()));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
}

#[test]
fn merge_converts_every_cell_to_literal_undo_restores_and_split_preserves_anchor_only() {
    let (mut s, id) = session("first");
    s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":1,"value":{"kind":"boolean","value":true},"display":"second"})).unwrap();
    let first = record(&s, id);
    let second = table(&s, id).cell(0, 1).unwrap().data.clone().unwrap();
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[0,1]})).unwrap();
    assert!(table(&s, id).rows[0].cells.iter().all(|c| c.data.is_none()));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&first, &record(&s, id)));
    assert!(Arc::ptr_eq(&second, table(&s, id).cell(0, 1).unwrap().data.as_ref().unwrap()));
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[0,1]})).unwrap();
    s.edit(|doc, sel| {
        let ShapeKind::Table(t) = &mut deckcraft_model::find_shape_mut(shapes_mut(doc, sel).unwrap(), id).unwrap().kind else { panic!() };
        let c = t.cell_mut(0, 0).unwrap();
        c.data = Some(Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &c.text)?));
        Ok(())
    })
    .unwrap();
    let merged = record(&s, id);
    s.execute("table.split", &json!({"id":id,"cell":[0,0]})).unwrap();
    assert!(Arc::ptr_eq(&merged, &record(&s, id)));
    assert!(table(&s, id).cell(0, 1).unwrap().data.is_none());
}

#[test]
fn row_insertion_promotion_and_shape_clipboard_move_whole_value_display_pairs() {
    let (mut s, id) = session("cached");
    let original = record(&s, id);
    s.execute("table.insertRowAbove", &json!({"id":id,"row":0})).unwrap();
    assert!(table(&s, id).cell(0, 0).unwrap().data.is_none());
    assert!(Arc::ptr_eq(&original, table(&s, id).cell(1, 0).unwrap().data.as_ref().unwrap()));
    s.execute("table.deleteRow", &json!({"id":id,"row":0})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("edit.select", &json!({"ids":[id]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let copied = s.doc().unwrap().selection.shapes[0];
    assert_ne!(copied, id);
    assert_eq!(cell(&s, copied).text.text(), "cached");
    assert!(Arc::ptr_eq(&original, &record(&s, copied)));
}

#[test]
fn merged_anchor_row_deletion_preserves_the_entire_record() {
    let (mut s, id) = session("cached");
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[1,0]})).unwrap();
    s.edit(|doc, sel| {
        let ShapeKind::Table(t) = &mut deckcraft_model::find_shape_mut(shapes_mut(doc, sel).unwrap(), id).unwrap().kind else { panic!() };
        let c = t.cell_mut(0, 0).unwrap();
        c.data = Some(Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &c.text)?));
        Ok(())
    })
    .unwrap();
    let original = record(&s, id);
    s.execute("table.deleteRow", &json!({"id":id,"row":0})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    assert_eq!(cell(&s, id).text.text(), "cached");
    assert!(!cell(&s, id).is_covered());
}

#[test]
fn invalid_typed_parameters_and_merged_targets_are_atomic_errors() {
    let (mut s, id) = session("cached");
    let valid = json!({"kind":"boolean","value":false});
    for params in [
        json!({"display":"x"}),
        json!({"value":valid}),
        json!({"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"NaN"},"display":"x"}),
        json!({"value":{"kind":"string","value":"\u{0000}"},"display":"x"}),
        json!({"value":{"kind":"boolean","value":false,"extra":true},"display":"x"}),
        json!({"value":valid,"display":"x","row":-1}),
        json!({"value":valid,"display":"x","col":99}),
        json!({"value":valid,"display":"x".repeat(64*1024+1)}),
        json!({"value":valid,"display":"\n".repeat(1024)}),
    ] {
        let before = s.doc().unwrap().doc.clone();
        let history = s.doc().unwrap().history.undo.len();
        assert!(s.execute("table.setCellValue", &params).is_err(), "{params}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
    }
    s.execute("text.exit", &json!({})).unwrap();
    assert!(
        s.execute("table.setCellValue", &json!({"id":id,"value":valid,"display":"x"})).is_err(),
        "selection of a table does not imply coordinate zero"
    );
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[0,1]})).unwrap();
    for col in [0, 1] {
        assert!(s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":col,"value":valid,"display":"x"})).is_err());
    }
}

#[test]
fn explicit_assignment_metadata_budget_failure_rolls_back_document_and_history() {
    let (mut s, id) = session("cached");
    let data = Arc::new(CellConstant::from_display(CellValue::String { value: "x".repeat(64 * 1024) }, &TextBody::from_text("cached")).unwrap());
    let count = 32 * 1024 * 1024 / data.metadata_bytes();
    s.edit(|doc, sel| {
        let ShapeKind::Table(t) = &mut deckcraft_model::find_shape_mut(shapes_mut(doc, sel).unwrap(), id).unwrap().kind else { panic!() };
        *t = Table::new(1, count + 1, 600., 40.);
        for c in t.rows[0].cells.iter_mut().take(count) {
            c.text = TextBody::from_text("cached");
            c.data = Some(data.clone());
        }
        Ok(())
    })
    .unwrap();
    deckcraft_model::table::validate_constants(&s.doc().unwrap().doc).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let history = s.doc().unwrap().history.undo.len();
    let error = s
        .execute("table.setCellValue", &json!({"id":id,"row":0,"col":count,"value":{"kind":"string","value":"x".repeat(64*1024)},"display":"cached"}))
        .unwrap_err();
    assert!(error.to_string().contains("metadata"));
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), history);
    assert!(table(&s, id).cell(0, count).unwrap().data.is_none());
}

#[test]
fn ordinary_content_edits_reconcile_only_the_touched_cell_and_keep_other_owner_arcs() {
    let (mut s, id) = session("cached");
    let mut untouched = cell(&s, id).clone();
    untouched.h_merge = true; // Deliberately invalid metadata in an unrelated direct-model owner.
    s.edit(|doc, _| {
        doc.handout_master = Some(Arc::new(deckcraft_model::Master {
            shapes: vec![deckcraft_model::Shape {
                kind: ShapeKind::Table(Table {
                    cols: vec![10.],
                    rows: vec![deckcraft_model::table::Row { height: 10., cells: vec![untouched] }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        Ok(())
    })
    .unwrap();
    let owner = s.doc().unwrap().doc.handout_master.clone().unwrap();
    s.execute("text.insert", &json!({"text":"!"})).unwrap();
    assert!(cell(&s, id).data.is_none());
    assert!(Arc::ptr_eq(&owner, s.doc().unwrap().doc.handout_master.as_ref().unwrap()));
    let ShapeKind::Table(t) = &owner.shapes[0].kind else { panic!() };
    assert!(t.cell(0, 0).unwrap().data.is_some(), "content edits do not run a global reconciliation/validation");
}

#[test]
fn pencil_splits_both_axes_without_copying_the_anchor_value_into_new_cells() {
    for vertical in [false, true] {
        let mut t = Table::new(1, 1, 200., 80.);
        let body = TextBody::from_text("cached 漢");
        let value = Arc::new(CellConstant::from_display(CellValue::String { value: "authoritative".into() }, &body).unwrap());
        let anchor = t.cell_mut(0, 0).unwrap();
        anchor.text = body.clone();
        anchor.data = Some(value.clone());
        let (a, b) = if vertical { ((100., 0.), (100., 80.)) } else { ((0., 40.), (200., 40.)) };
        assert!(crate::drawtable::draw_line(&mut t, a, b));
        assert_eq!((t.n_rows(), t.n_cols()), if vertical { (1, 2) } else { (2, 1) });
        let kept = t.cell(0, 0).unwrap();
        assert_eq!(kept.text, body);
        assert!(Arc::ptr_eq(&value, kept.data.as_ref().unwrap()));
        assert!(matches!(kept.constant_state().unwrap(), deckcraft_model::table::CellConstantState::Current(_)));
        let manufactured = if vertical { t.cell(0, 1).unwrap() } else { t.cell(1, 0).unwrap() };
        assert!(manufactured.text.is_empty());
        assert!(manufactured.data.is_none(), "manufactured blank cells cannot inherit a stored value");
        assert!(!manufactured.is_covered());
    }
}

#[test]
fn pencil_command_preserves_untouched_spanning_anchor_and_undo_restores_original_grid() {
    let (mut s, id) = session("cached");
    s.execute("table.setCellValue", &json!({"id":id,"row":0,"col":1,"value":{"kind":"boolean","value":false},"display":"untouched"})).unwrap();
    let first = record(&s, id);
    let untouched = table(&s, id).cell(0, 1).unwrap().data.clone().unwrap();
    let original = table(&s, id).clone();
    s.execute("table.drawLine", &json!({"id":id,"from":[100,125],"to":[220,125]})).unwrap();
    let t = table(&s, id);
    assert_eq!((t.n_rows(), t.n_cols()), (3, 2));
    assert!(Arc::ptr_eq(&first, t.cell(0, 0).unwrap().data.as_ref().unwrap()));
    assert!(t.cell(1, 0).unwrap().data.is_none());
    assert!(t.cell(1, 0).unwrap().text.is_empty());
    assert_eq!(t.cell(0, 1).unwrap().row_span, 2);
    assert!(Arc::ptr_eq(&untouched, t.cell(0, 1).unwrap().data.as_ref().unwrap()));
    assert!(t.cell(1, 1).unwrap().is_covered());
    assert!(t.cell(1, 1).unwrap().data.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(table(&s, id), &original);
    assert!(Arc::ptr_eq(&first, &record(&s, id)));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(table(&s, id).cell(1, 0).unwrap().data.is_none());
}

#[test]
fn eraser_command_concatenates_display_clears_both_values_and_keeps_unrelated_value() {
    let (mut s, id) = session("cached");
    for (row, col, text) in [(0, 1, "second"), (1, 1, "unrelated")] {
        s.execute("table.setCellValue", &json!({"id":id,"row":row,"col":col,"value":{"kind":"boolean","value":false},"display":text})).unwrap();
    }
    let original = record(&s, id);
    let second = table(&s, id).cell(0, 1).unwrap().data.clone().unwrap();
    let unrelated = table(&s, id).cell(1, 1).unwrap().data.clone().unwrap();
    s.execute("table.eraseLine", &json!({"id":id,"at":[220,125],"tol":3})).unwrap();
    let t = table(&s, id);
    assert_eq!(t.cell(0, 0).unwrap().text.text(), "cached\nsecond");
    assert_eq!(t.cell(0, 0).unwrap().grid_span, 2);
    assert!(t.cell(0, 0).unwrap().data.is_none());
    assert!(t.cell(0, 1).unwrap().is_covered());
    assert!(t.cell(0, 1).unwrap().data.is_none());
    assert!(Arc::ptr_eq(&unrelated, t.cell(1, 1).unwrap().data.as_ref().unwrap()));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&original, &record(&s, id)));
    assert!(Arc::ptr_eq(&second, table(&s, id).cell(0, 1).unwrap().data.as_ref().unwrap()));
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("table.split", &json!({"id":id,"cell":[0,0]})).unwrap();
    assert!(table(&s, id).rows[0].cells.iter().all(|cell| cell.data.is_none()), "splitting cannot reactivate an erased stored value");
}

#[test]
fn erasing_an_empty_cached_display_still_converts_its_stored_value_to_literal() {
    let mut t = Table::new(1, 2, 200., 80.);
    let data = Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &TextBody::from_text("")).unwrap());
    t.cell_mut(0, 0).unwrap().data = Some(data.clone());
    t.cell_mut(0, 1).unwrap().data = Some(data);
    assert!(crate::drawtable::erase_at(&mut t, (100., 40.), 3.));
    assert!(t.cell(0, 0).unwrap().text.is_empty());
    assert!(t.rows[0].cells.iter().all(|cell| cell.data.is_none()));
}

#[test]
fn unsupported_gesture_grids_refuse_without_normalizing_or_losing_attached_values() {
    let mut base = Table::new(2, 2, 200., 80.);
    let body = TextBody::from_text("cached");
    let value = Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &body).unwrap());
    base.cell_mut(0, 0).unwrap().text = body;
    base.cell_mut(0, 0).unwrap().data = Some(value);
    let mut grids = vec![];
    let mut t = base.clone();
    t.cols.clear();
    grids.push(t);
    let mut t = base.clone();
    t.rows.clear();
    grids.push(t);
    let mut t = base.clone();
    t.rows[1].cells.pop();
    grids.push(t);
    let mut t = base.clone();
    t.rows[1].cells.push(Cell::new("extra"));
    grids.push(t);
    let mut t = base.clone();
    t.cols = vec![10.; 1001];
    grids.push(t);
    for horizontal in [false, true] {
        let mut t = base.clone();
        let covered = t.cell_mut(0, 0).unwrap();
        covered.h_merge = horizontal;
        covered.v_merge = !horizontal;
        grids.push(t);
    }
    for before in grids {
        let mut t = before.clone();
        assert!(!crate::drawtable::draw_line(&mut t, (0., 20.), (200., 20.)));
        assert_eq!(t, before, "pencil must refuse before mutation");
        assert!(!crate::drawtable::draw_line(&mut t, (50., 0.), (50., 80.)));
        assert_eq!(t, before, "vertical transpose must not run on a rejected grid");
        assert!(!crate::drawtable::erase_at(&mut t, (100., 20.), 3.));
        assert_eq!(t, before, "eraser must refuse before mutation");
    }
}

#[test]
fn rejected_ragged_or_covered_gesture_commands_preserve_document_arc_and_history() {
    for covered in [false, true] {
        let (mut s, id) = session("cached");
        s.edit(|doc, sel| {
            let ShapeKind::Table(t) = &mut deckcraft_model::find_shape_mut(shapes_mut(doc, sel).unwrap(), id).unwrap().kind else { panic!() };
            if covered {
                t.cell_mut(0, 0).unwrap().h_merge = true;
            } else {
                t.rows[1].cells.pop();
            }
            Ok(())
        })
        .unwrap();
        for (command, params) in
            [("table.drawLine", json!({"id":id,"from":[100,125],"to":[220,125]})), ("table.eraseLine", json!({"id":id,"at":[220,125],"tol":3}))]
        {
            let before = s.doc().unwrap().doc.clone();
            let undo = s.doc().unwrap().history.undo.len();
            assert!(s.execute(command, &params).is_err());
            assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        }
    }
}
