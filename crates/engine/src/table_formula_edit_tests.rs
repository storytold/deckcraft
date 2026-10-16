//! Real command routes keep formula outcomes and edits in one history transaction.
use std::sync::Arc;

use deckcraft_model::table::{CellFormula, FormulaError, FormulaOutcome};
use deckcraft_model::{Cell, ShapeId, ShapeKind, Table};
use serde_json::{Value, json};

use crate::Session;

fn session() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let value = s.execute("insert.table", &json!({"rows":2,"cols":4,"rect":[100,100,320,100]})).unwrap();
    let id = ShapeId(value["id"].as_u64().unwrap() as u32);
    (s, id)
}

fn table(s: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(table) = &s.doc().unwrap().shape(id).unwrap().kind else { panic!("table missing") };
    table
}

fn cell(s: &Session, id: ShapeId, col: usize) -> &Cell {
    table(s, id).cell(0, col).unwrap()
}

fn formula(s: &Session, id: ShapeId, col: usize) -> Arc<CellFormula> {
    cell(s, id, col).formula.clone().unwrap()
}

fn number(s: &mut Session, id: ShapeId, col: usize, lexical: &str, display: &str) {
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"row":0,"col":col,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":lexical},"display":display}),
    )
    .unwrap();
}

fn set_formula(s: &mut Session, id: ShapeId, col: usize, expression: &str) {
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":col,"expression":expression,"profile":"exact-decimal-v1"})).unwrap();
}

fn chain() -> (Session, ShapeId) {
    let (mut s, id) = session();
    number(&mut s, id, 0, "2", "2");
    set_formula(&mut s, id, 1, "=[.A1]*3");
    set_formula(&mut s, id, 2, "=[.B1]+1");
    (s, id)
}

fn edit_all(s: &mut Session, id: ShapeId, col: usize) {
    s.execute("text.edit", &json!({"id":id,"cell":[0,col],"end":true})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
}

fn assert_error(s: &Session, id: ShapeId, col: usize, error: FormulaError) {
    assert_eq!(formula(s, id, col).outcome(), &FormulaOutcome::Error { code: error });
    assert!(cell(s, id, col).data.is_none(), "an error cannot expose an old scalar");
    assert!(cell(s, id, col).text.text().starts_with('#'));
}

#[test]
fn formula_commands_are_registered_and_exact_decimal_assignment_is_semantic() {
    let (mut s, id) = session();
    for (expression, expected) in [("=0.1+0.2", "0.3"), ("=9007199254740993+1", "9007199254740994"), ("=-(2+3)*-4", "20")] {
        set_formula(&mut s, id, 0, expression);
        assert_eq!(formula(&s, id, 0).expression(), expression);
        assert_eq!(formula(&s, id, 0).outcome(), &FormulaOutcome::Number { lexical: expected.into() });
        assert_eq!(cell(&s, id, 0).text.text(), expected);
        assert!(cell(&s, id, 0).data.is_none());
    }
    s.execute("table.recalculate", &json!({"id":id})).unwrap();
}

#[test]
fn scalar_command_recalculates_chain_and_one_undo_restores_original_arcs() {
    let (mut s, id) = chain();
    let (b, c) = (formula(&s, id, 1), formula(&s, id, 2));
    let count = s.doc().unwrap().history.undo.len();
    number(&mut s, id, 0, "4", "4");
    assert_eq!(cell(&s, id, 1).text.text(), "12");
    assert_eq!(cell(&s, id, 2).text.text(), "13");
    assert_eq!(s.doc().unwrap().history.undo.len(), count + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(cell(&s, id, 0).text.text(), "2");
    assert!(Arc::ptr_eq(&b, &formula(&s, id, 1)));
    assert!(Arc::ptr_eq(&c, &formula(&s, id, 2)));
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(cell(&s, id, 2).text.text(), "13");
}

#[test]
fn scalar_equal_lexical_assignment_keeps_dependent_formula_identity() {
    let (mut s, id) = chain();
    let before = formula(&s, id, 1);
    number(&mut s, id, 0, "02.00", "two");
    assert!(Arc::ptr_eq(&before, &formula(&s, id, 1)));
    assert_eq!(cell(&s, id, 1).text.text(), "6");
}

#[test]
fn literal_numeric_edit_is_not_a_typed_operand_and_typed_assignment_recovers() {
    let (mut s, id) = chain();
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"4"})).unwrap();
    assert!(cell(&s, id, 0).data.is_none());
    assert_error(&s, id, 1, FormulaError::InputNotNumeric);
    assert_error(&s, id, 2, FormulaError::DependencyError);
    number(&mut s, id, 0, "4", "4");
    assert_eq!(cell(&s, id, 1).text.text(), "12");
    assert_eq!(cell(&s, id, 2).text.text(), "13");
}

#[test]
fn committed_text_routes_recalculate_before_the_single_history_step() {
    for (command, params) in [
        ("text.insert", json!({"text":"literal"})),
        ("text.delete", json!({"dir":"backward"})),
        ("edit.paste", json!({"text":"literal","html":"<i>literal</i>"})),
        ("edit.cut", json!({})),
        ("text.replace", json!({"text":"teh","with":"the"})),
        ("review.spellingChange", json!({"word":"teh","to":"the","para":0,"start":0,"row":0,"col":0})),
    ] {
        let (mut s, id) = chain();
        number(&mut s, id, 0, "2", "teh");
        edit_all(&mut s, id, 0);
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        let mut params = params;
        if command == "review.spellingChange" {
            params["shape"] = json!(id);
        }
        s.execute(command, &params).unwrap();
        assert_error(&s, id, 1, FormulaError::InputNotNumeric);
        assert_error(&s, id, 2, FormulaError::DependencyError);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "{command}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc), "{command}");
    }
}

#[test]
fn autocorrect_updates_formula_errors_in_its_original_transaction() {
    let (mut s, id) = chain();
    number(&mut s, id, 0, "2", "teh ");
    s.execute("text.edit", &json!({"id":id,"cell":[0,0],"end":true})).unwrap();
    s.execute("text.autocorrect", &json!({"on":true,"sentenceCaps":false})).unwrap();
    s.execute("text.insert", &json!({"text":""})).unwrap();
    assert_eq!(cell(&s, id, 0).text.text(), "the ");
    assert_error(&s, id, 1, FormulaError::InputNotNumeric);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(cell(&s, id, 1).text.text(), "6");
}

#[test]
fn formula_display_formatting_and_noops_preserve_formula_arc() {
    let (mut s, id) = session();
    set_formula(&mut s, id, 0, "=1234");
    let original = formula(&s, id, 0);
    edit_all(&mut s, id, 0);
    s.execute("text.select", &json!({"anchor":[0,1],"caret":[0,3]})).unwrap();
    s.execute("format.bold", &json!({"on":true})).unwrap();
    assert!(cell(&s, id, 0).text.paragraphs[0].runs.len() > 1);
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"1234"})).unwrap();
    s.execute("text.replace", &json!({"text":"1234","with":"1234"})).unwrap();
    edit_all(&mut s, id, 0);
    s.execute("edit.paste", &json!({"text":"1234","html":"<b>1234</b>"})).unwrap();
    assert!(Arc::ptr_eq(&original, &formula(&s, id, 0)));
}

#[test]
fn formula_display_edit_clears_source_and_propagates_dependency_error() {
    let (mut s, id) = chain();
    let original = formula(&s, id, 1);
    s.execute("text.set", &json!({"id":id,"cell":[0,1],"text":"7"})).unwrap();
    assert!(cell(&s, id, 1).formula.is_none());
    assert_error(&s, id, 2, FormulaError::InputNotNumeric);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&original, &formula(&s, id, 1)));
}

#[test]
fn explicit_formula_clear_becomes_literal_and_explicit_scalar_replaces_formula() {
    let (mut s, id) = chain();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":null})).unwrap();
    assert!(cell(&s, id, 1).formula.is_none());
    assert!(cell(&s, id, 1).data.is_none());
    assert_eq!(cell(&s, id, 1).text.text(), "6");
    assert_error(&s, id, 2, FormulaError::InputNotNumeric);
    number(&mut s, id, 1, "8", "8");
    assert_eq!(cell(&s, id, 2).text.text(), "9");
    set_formula(&mut s, id, 1, "=9");
    assert!(cell(&s, id, 1).data.is_none());
    number(&mut s, id, 1, "10", "10");
    assert!(cell(&s, id, 1).formula.is_none());
    assert_eq!(cell(&s, id, 2).text.text(), "11");
}

#[test]
fn later_numeric_limit_becomes_current_error_and_native_roundtrip_keeps_it() {
    let (mut s, id) = session();
    number(&mut s, id, 0, "1", "1");
    set_formula(&mut s, id, 1, "=[.A1]*10");
    number(&mut s, id, 0, "1e100", "large");
    assert_error(&s, id, 1, FormulaError::NumericLimit);
    let bytes = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let reopened = deckcraft_format::load(&bytes).unwrap();
    let ShapeKind::Table(t) = &reopened.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().kind else { panic!() };
    assert_eq!(t.cell(0, 1).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::Error { code: FormulaError::NumericLimit });
    assert_eq!(t.cell(0, 1).unwrap().text.text(), cell(&s, id, 1).text.text());
    number(&mut s, id, 0, "2", "2");
    assert_eq!(cell(&s, id, 1).text.text(), "20");
}

#[test]
fn invalid_assignment_refuses_without_document_history_or_selection_change() {
    let (mut s, id) = chain();
    for expression in ["=[.C1]", "=[.Z999]", "=[.D1]", "=1/3", "=1+", "", "=1e100"] {
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(
            s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":expression,"profile":"exact-decimal-v1"})).is_err(),
            "{expression}"
        );
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc), "{expression}");
        assert_eq!(selection, s.doc().unwrap().selection);
        assert_eq!(undo, s.doc().unwrap().history.undo.len());
    }
}

#[test]
fn formula_params_are_strict_and_merged_assignment_refuses() {
    let (mut s, id) = session();
    for params in [
        json!({"id":id,"row":-1,"col":0,"expression":"=1"}),
        json!({"id":id,"row":0.5,"col":0,"expression":"=1"}),
        json!({"id":id,"row":0,"col":9999,"expression":"=1"}),
        json!({"id":id,"row":0,"col":0,"expression":1}),
        json!({"id":id,"row":0,"col":0,"expression":"=1","extra":true}),
        json!({"id":null,"row":0,"col":0,"expression":"=1"}),
        json!({"id":id,"expression":"=1"}),
        Value::Null,
    ] {
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("table.setCellFormula", &params).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[0,1]})).unwrap();
    for col in [0, 1] {
        assert!(s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":col,"expression":"=1"})).is_err());
    }
}

#[test]
fn unmerged_split_refuses_atomically_and_formula_style_size_remain_allowed() {
    let (mut s, id) = chain();
    let before = s.doc().unwrap().doc.clone();
    let count = s.doc().unwrap().history.undo.len();
    assert!(s.execute("table.split", &json!({"id":id,"cell":[0,0]})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), count);
    let original = formula(&s, id, 1);
    for (command, params) in [
        ("table.style", json!({"style":"grid"})),
        ("table.columnWidth", json!({"col":0,"width":100})),
        ("table.rowHeight", json!({"row":0,"height":70})),
        ("table.cellAlign", json!({"anchor":"bottom"})),
    ] {
        let mut params = params;
        params["id"] = json!(id);
        s.execute(command, &params).unwrap();
        assert!(Arc::ptr_eq(&original, &formula(&s, id, 1)), "{command}");
    }
}

#[test]
fn ime_preedit_cancel_and_equal_commit_keep_original_formula_and_dependents() {
    let (mut s, id) = chain();
    edit_all(&mut s, id, 1);
    let before = s.doc().unwrap().doc.clone();
    let original = formula(&s, id, 1);
    let dependent = formula(&s, id, 2);
    let count = s.doc().unwrap().history.undo.len();
    for text in ["候", "候補"] {
        s.execute("text.compose", &json!({"text":text})).unwrap();
        assert!(Arc::ptr_eq(&original, &formula(&s, id, 1)));
        assert!(Arc::ptr_eq(&dependent, &formula(&s, id, 2)));
        assert_eq!(s.doc().unwrap().history.undo.len(), count);
    }
    s.execute("text.compose", &json!({"text":""})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.execute("text.compose", &json!({"text":"候"})).unwrap();
    s.execute("text.commit", &json!({"text":"6"})).unwrap();
    assert!(Arc::ptr_eq(&original, &formula(&s, id, 1)));
    assert!(Arc::ptr_eq(&dependent, &formula(&s, id, 2)));
    assert_eq!(s.doc().unwrap().history.undo.len(), count + 1);
}

#[test]
fn ime_changed_commit_recalculates_and_undo_restores_original_owner() {
    let (mut s, id) = chain();
    edit_all(&mut s, id, 0);
    let before = s.doc().unwrap().doc.clone();
    let count = s.doc().unwrap().history.undo.len();
    s.execute("text.compose", &json!({"text":"4"})).unwrap();
    assert_eq!(cell(&s, id, 1).text.text(), "6", "preedit must not calculate");
    s.execute("text.commit", &json!({"text":"4"})).unwrap();
    assert_error(&s, id, 1, FormulaError::InputNotNumeric);
    assert_error(&s, id, 2, FormulaError::DependencyError);
    assert_eq!(s.doc().unwrap().history.undo.len(), count + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn ime_selection_move_settles_the_original_cell_and_keeps_destination_formula() {
    let (mut s, id) = chain();
    edit_all(&mut s, id, 0);
    let before = s.doc().unwrap().doc.clone();
    let source = formula(&s, id, 1).expression().to_string();
    s.execute("text.compose", &json!({"text":"different"})).unwrap();
    s.select(|_, selection| selection.text.as_mut().unwrap().cell = Some((0, 1))).unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    assert!(cell(&s, id, 0).data.is_none());
    assert_eq!(formula(&s, id, 1).expression(), source);
    assert_error(&s, id, 1, FormulaError::InputNotNumeric);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn same_coordinates_in_different_tables_have_independent_formula_graphs() {
    let (mut s, first) = chain();
    let value = s.execute("insert.table", &json!({"rows":1,"cols":2,"rect":[100,250,200,50]})).unwrap();
    let second = ShapeId(value["id"].as_u64().unwrap() as u32);
    number(&mut s, second, 0, "10", "10");
    set_formula(&mut s, second, 1, "=[.A1]*3");
    let original = formula(&s, second, 1);
    number(&mut s, first, 0, "4", "4");
    assert_eq!(cell(&s, first, 1).text.text(), "12");
    assert_eq!(cell(&s, second, 1).text.text(), "30");
    assert!(Arc::ptr_eq(&original, &formula(&s, second, 1)));
}

#[test]
fn whole_table_copy_preserves_relative_formula_scope_independently() {
    let (mut s, id) = chain();
    s.execute("edit.select", &json!({"ids":[id]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let pasted = s.doc().unwrap().selection.shapes[0];
    assert_ne!(pasted, id);
    assert_eq!(cell(&s, pasted, 2).text.text(), "7");
    number(&mut s, pasted, 0, "5", "5");
    assert_eq!(cell(&s, pasted, 2).text.text(), "16");
    assert_eq!(cell(&s, id, 2).text.text(), "7");
}

#[test]
fn successful_formula_native_save_reopen_preserves_source_and_exact_outcome() {
    let (mut s, id) = chain();
    number(&mut s, id, 0, "4", "4");
    let reopened = deckcraft_format::load(&deckcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
    let ShapeKind::Table(table) = &reopened.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().kind else { panic!() };
    assert_eq!(table.cell(0, 1).unwrap().formula.as_ref().unwrap().expression(), "=[.A1]*3");
    assert_eq!(table.cell(0, 2).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::Number { lexical: "13".into() });
}

#[test]
fn failed_ime_settlement_restores_saved_document_selection_and_history() {
    let mut s = Session::with_new();
    // insert.table caps each dimension at 75; use a grid whose actual physical
    // cell count exceeds the 128-formula limit without hitting that UI cap.
    let value = s.execute("insert.table", &json!({"rows":3,"cols":43,"rect":[100,100,500,150]})).unwrap();
    let id = ShapeId(value["id"].as_u64().unwrap() as u32);
    set_formula(&mut s, id, 0, "=1");
    let record = formula(&s, id, 0);
    // An over-budget in-memory owner exercises the fallible settlement path without
    // admitting it through an ordinary edit or untrusted document reader.
    {
        let state = s.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut state.doc);
        let slide = Arc::make_mut(doc.slides.first_mut().unwrap());
        let shape = slide.shapes.iter_mut().find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(table) = &mut shape.kind else { panic!() };
        for cell in table.rows.iter_mut().flat_map(|row| row.cells.iter_mut()).skip(1) {
            cell.text = deckcraft_model::TextBody::from_text("1");
            cell.formula = Some(record.clone());
        }
    }
    assert_eq!(table(&s, id).rows.iter().flat_map(|row| row.cells.iter()).filter(|cell| cell.formula.is_some()).count(), 129);
    assert!(table(&s, id).plan_recalculation().is_err(), "fixture must fail formula preflight before testing settlement rollback");
    edit_all(&mut s, id, 0);
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("text.compose", &json!({"text":"different"})).unwrap();
    let _ = s.execute("text.commit", &json!({"text":"different"}));
    assert!(s.doc().unwrap().composition.is_none());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(selection, s.doc().unwrap().selection);
    assert_eq!(undo, s.doc().unwrap().history.undo.len());
}
