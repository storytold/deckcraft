//! Integration regressions for stable nested ownership and typed display edits.
use crate::Session;
use deckcraft_model::table::{CellBlock, Table, TablePath};
use deckcraft_model::{ShapeId, ShapeKind};
use serde_json::json;

fn session() -> (Session, ShapeId) {
    let mut session = Session::with_new();
    let result = session.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[80,60,400,120]})).unwrap();
    (session, serde_json::from_value(result["id"].clone()).unwrap())
}
fn table(session: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(table) = &session.doc().unwrap().shape(id).unwrap().kind else { panic!("table missing") };
    table
}
fn nested(session: &Session, id: ShapeId) -> (&TablePath, &Table) {
    let path = &session.doc().unwrap().selection.table_path;
    (path, table(session, id).at_path(path).unwrap())
}
#[test]
fn nested_text_edits_use_stable_child_owner_and_undo_restores_parent_and_child() {
    let (mut s, id) = session();
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"before after"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[0,0],"at":[0,7]})).unwrap();
    let inserted = s.execute("table.insertNested", &json!({"id":id,"rows":2,"cols":2})).unwrap();
    let path = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.edit", &json!({"id":id,"path":path,"cell":[0,0],"at":[0,0]})).unwrap();
    s.execute("text.insert", &json!({"text":"child"})).unwrap();
    assert_eq!(nested(&s, id).1.cell(0, 0).unwrap().text.text(), "child");
    let parent = table(&s, id).cell(0, 0).unwrap();
    assert_eq!(parent.text.text(), "before ");
    assert!(matches!(parent.blocks.first(), Some(CellBlock::Table(_))));
    assert_eq!(parent.text_blocks().last().unwrap().text(), "after");
    let current = s.doc().unwrap().doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(nested(&s, id).1.cell(0, 0).unwrap().text.text(), "");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *current);
    s.execute("table.leaveNested", &json!({"id":id})).unwrap();
    s.execute("text.insert", &json!({"text":"tail "})).unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text_blocks().last().unwrap().text(), "tail after");
    assert!(inserted["child"].as_u64().is_some());
}
#[test]
fn nested_formula_values_recalculate_only_the_correct_owner() {
    let (mut s, id) = session();
    s.execute("table.insertNested", &json!({"id":id,"row":0,"col":0,"rows":1,"cols":2})).unwrap();
    let path = s.doc().unwrap().selection.table_path.clone();
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"path":path,"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"12.5"},"display":"12.5"}),
    )
    .unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":path,"row":0,"col":1,"expression":"=SUM([.A1];2.5)/3"})).unwrap();
    assert_eq!(nested(&s, id).1.cell(0, 1).unwrap().text.text(), "5");
    assert!(table(&s, id).cell(0, 1).unwrap().formula.is_none());
    let before = s.doc().unwrap().doc.clone();
    let mut missing = path.clone();
    missing.0.first_mut().unwrap().child += 100;
    assert!(s.execute("text.set", &json!({"id":id,"path":missing,"cell":[0,0],"text":"wrong"})).is_err());
    assert_eq!(*s.doc().unwrap().doc, *before);
}
#[test]
fn value_formatting_retains_legacy_math_and_native_authority() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":0,"expression":"=1+2","profile":"exact-decimal-v1"})).unwrap();
    let format = json!({"locale":"en-US","style":{"kind":"number","decimalPlaces":2,"minDecimalPlaces":2,"minIntegerDigits":1,"grouping":false}});
    s.execute("table.setCellNumberFormat", &json!({"id":id,"row":0,"col":0,"format":format})).unwrap();
    let cell = table(&s, id).cell(0, 0).unwrap();
    assert_eq!(cell.text.text(), "3.00");
    assert_eq!(cell.formula.as_ref().unwrap().profile(), "exact-decimal-v1-formatted");
    assert!(!cell.formula.as_ref().unwrap().is_v2());
    let saved = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let loaded = deckcraft_format::load(&saved).unwrap();
    assert_eq!(loaded, *s.doc().unwrap().doc);
    s.execute("table.convertToLiteral", &json!({"id":id,"row":0,"col":0})).unwrap();
    let cell = table(&s, id).cell(0, 0).unwrap();
    assert_eq!(cell.text.text(), "3.00");
    assert!(cell.formula.is_none());
    assert!(cell.number_format.is_none());
}
#[test]
fn nested_ime_owner_switch_settles_original_child_once() {
    let (mut s, id) = session();
    s.execute("table.insertNested", &json!({"id":id,"row":0,"col":0,"rows":1,"cols":1})).unwrap();
    let path = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.edit", &json!({"id":id,"path":path,"cell":[0,0]})).unwrap();
    s.execute("text.compose", &json!({"text":"composed"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"path":[],"cell":[1,1],"block":0})).unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    assert_eq!(table(&s, id).at_path(&path).unwrap().cell(0, 0).unwrap().text.text(), "composed");
    assert_eq!(table(&s, id).cell(1, 1).unwrap().text.text(), "");
}

#[test]
fn structural_edits_rewrite_references_without_migrating_legacy_math_and_undo_is_exact() {
    let (mut s, id) = session();
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"7"},"display":"7"}),
    )
    .unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":"=[.A1]*2","profile":"exact-decimal-v1"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.insertColumnLeft", &json!({"id":id,"col":0})).unwrap();
    let formula = table(&s, id).cell(0, 2).unwrap().formula.as_ref().unwrap();
    assert_eq!(formula.expression(), "=[.B1]*2");
    assert_eq!(formula.profile(), "exact-decimal-v1");
    assert_eq!(table(&s, id).cell(0, 2).unwrap().text.text(), "14");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    s.execute("table.deleteColumn", &json!({"id":id,"col":0})).unwrap();
    let formula = table(&s, id).cell(0, 0).unwrap().formula.as_ref().unwrap();
    assert_eq!(formula.expression(), "=[#REF!]*2");
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text.text(), "#REF!");
}

#[test]
fn actual_broader_formula_command_uses_one_cumulative_numeric_budget() {
    let (mut s, id) = session();
    let started = std::time::Instant::now();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":0,"expression":"=1.000000000000000000000000001^1000000000000000000000000000"}))
        .unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text.text(), "2.71828182845904523536028747");
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let expression = format!("=SUM({})", std::iter::repeat_n("IFERROR(POWER(2;0.1234567890123456789012345678);0)", 32).collect::<Vec<_>>().join(";"));
    assert!(s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":expression})).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(undo, s.doc().unwrap().history.undo.len());
    eprintln!("Actual extended formula command and bounded hostile rejection: {:?}", started.elapsed());
}
#[test]
fn find_replace_addresses_nested_and_continuation_text_without_touching_parent_cache() {
    let (mut s, id) = session();
    s.execute("table.insertNested", &json!({"id":id,"row":0,"col":0,"rows":1,"cols":1})).unwrap();
    let path = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.set", &json!({"id":id,"path":path,"cell":[0,0],"text":"nested token"})).unwrap();
    s.execute("table.leaveNested", &json!({"id":id})).unwrap();
    s.execute("text.insert", &json!({"text":"trailing token"})).unwrap();
    let hits = s.execute("text.find", &json!({"text":"token"})).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 2);
    let nested_hit = hits.as_array().unwrap().iter().find(|hit| hit.get("path").is_some()).unwrap().clone();
    s.execute("text.replace", &json!({"text":"token","with":"match","at":nested_hit})).unwrap();
    assert_eq!(table(&s, id).at_path(&path).unwrap().cell(0, 0).unwrap().text.text(), "nested match");
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text_blocks().last().unwrap().text(), "trailing token");
}

#[test]
fn merge_preserves_ordered_children_and_bakes_values_while_outside_formulas_recalculate() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":0,"expression":"=2+3"})).unwrap();
    s.execute("table.setCellNumberFormat", &json!({"id":id,"row":0,"col":0,"format":{"style":{"kind":"number","decimalPlaces":2,"minDecimalPlaces":2,"minIntegerDigits":1,"grouping":false}}})).unwrap();
    s.execute("text.set", &json!({"id":id,"cell":[0,1],"text":"before"})).unwrap();
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":1,"rows":1,"cols":1})).unwrap();
    let child_path = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.set", &json!({"id":id,"path":child_path,"cell":[0,0],"text":"inner"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":1,"expression":"=10+2"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    let top = table(&s, id).cell(0, 0).unwrap();
    assert_eq!(top.text.text(), "5.00");
    assert!(top.formula.is_none() && top.number_format.is_none() && top.data.is_none());
    assert!(matches!(&top.blocks[0],CellBlock::Text(block) if block.body.text()=="before"));
    assert!(matches!(&top.blocks[1],CellBlock::Table(child) if child.table.cell(0,0).unwrap().text.text()=="inner"));
    assert!(table(&s, id).cell(0, 1).unwrap().blocks.is_empty());
    assert_eq!(table(&s, id).cell(1, 1).unwrap().text.text(), "12");
    let merged = s.doc().unwrap().doc.clone();
    s.execute("table.split", &json!({"id":id,"path":[],"cell":[0,0]})).unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().blocks, top_blocks(&merged, id));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *merged);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}
fn top_blocks(doc: &deckcraft_model::Presentation, id: ShapeId) -> Vec<CellBlock> {
    let ShapeKind::Table(table) = &doc.slides[0].shapes.iter().find(|shape| shape.id == id).unwrap().kind else { panic!("table") };
    table.cell(0, 0).unwrap().blocks.clone()
}
#[test]
fn changing_cell_and_tabbing_leave_continuation_block_at_its_original_owner() {
    let (mut s, id) = session();
    s.execute("table.insertNested", &json!({"id":id,"row":0,"col":0,"rows":1,"cols":1})).unwrap();
    s.execute("table.leaveNested", &json!({"id":id})).unwrap();
    assert!(s.doc().unwrap().selection.cell_block.is_some());
    s.execute("text.edit", &json!({"id":id,"cell":[0,1]})).unwrap();
    s.execute("text.insert", &json!({"text":"right"})).unwrap();
    assert_eq!(table(&s, id).cell(0, 1).unwrap().text.text(), "right");
    s.execute("text.edit", &json!({"id":id,"cell":[0,0],"block":2})).unwrap();
    s.key("Tab", crate::Mods::default()).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.as_ref().unwrap().cell, Some((0, 1)));
    assert_eq!(s.doc().unwrap().selection.cell_block, Some(0));
}
#[test]
fn explicit_canonical_block_cannot_split_a_formula_display() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":0,"expression":"=123"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[0,0],"block":0,"at":[0,1]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("table.insertNested", &json!({"id":id,"rows":1,"cols":1})).is_err());
    assert_eq!(*s.doc().unwrap().doc, *before);
}
