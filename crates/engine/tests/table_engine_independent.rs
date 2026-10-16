//! Contributor-original independent command and shared-layout regressions. UNRUN.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_engine::{Mods, Session};
use deckcraft_model::{table::*, text::TextBody, *};
use serde_json::json;
use std::sync::Arc;
fn session() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let result = s.execute("insert.table", &json!({"rows":3,"cols":3,"rect":[20,30,360,120]})).unwrap();
    let id = serde_json::from_value(result["id"].clone()).unwrap();
    (s, id)
}
fn table(s: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(t) = &s.doc().unwrap().shape(id).unwrap().kind else { panic!("table") };
    t
}
fn body(s: &Session, id: ShapeId, path: &TablePath, row: usize, col: usize) -> String {
    table(s, id).at_path(path).unwrap().cell(row, col).unwrap().text.text()
}
fn nested(s: &mut Session, id: ShapeId, row: usize, col: usize, text: &str) -> TablePath {
    let inserted = s.execute("table.insertNested", &json!({"id":id,"path":[],"row":row,"col":col,"rows":1,"cols":2})).unwrap();
    let path: TablePath = serde_json::from_value(inserted["path"].clone()).unwrap();
    s.execute("text.set", &json!({"id":id,"path":path,"cell":[0,0],"text":text})).unwrap();
    path
}
fn number(s: &mut Session, id: ShapeId, row: usize, col: usize, value: &str) {
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"path":[],"row":row,"col":col,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":value},"display":value}),
    )
    .unwrap();
}
#[test]
fn explicit_parent_insert_cannot_reuse_a_nested_selected_text_range() {
    let (mut s, id) = session();
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"ROOT SAFE BODY"})).unwrap();
    let child = nested(&mut s, id, 0, 0, "CHILD selected");
    s.execute("text.edit", &json!({"id":id,"path":child,"cell":[0,0]})).unwrap();
    s.execute("text.select", &json!({"anchor":[0,0],"caret":[0,5]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":0,"rows":1,"cols":1})).unwrap();
    assert_eq!(body(&s, id, &TablePath::default(), 0, 0), "ROOT SAFE BODY");
    assert_eq!(body(&s, id, &child, 0, 0), "CHILD selected");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}
#[test]
fn axis_insertions_and_prior_deletions_keep_direct_caret_on_its_original_cell() {
    for (command, coordinate, expected) in [
        ("table.insertRowAbove", json!({"row":0}), (2, 1)),
        ("table.insertColumnLeft", json!({"col":0}), (1, 2)),
        ("table.deleteRow", json!({"row":0}), (0, 1)),
        ("table.deleteColumn", json!({"col":0}), (1, 0)),
    ] {
        let (mut s, id) = session();
        s.execute("text.set", &json!({"id":id,"cell":[1,1],"text":"ORIGINAL OWNER"})).unwrap();
        s.execute("text.edit", &json!({"id":id,"cell":[1,1]})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let mut params = coordinate;
        params["id"] = json!(id);
        params["path"] = json!([]);
        s.execute(command, &params).unwrap();
        assert_eq!(s.doc().unwrap().selection.text.as_ref().unwrap().cell, Some(expected), "{command}");
        s.execute("text.insert", &json!({"text":"!"})).unwrap();
        assert_eq!(body(&s, id, &TablePath::default(), expected.0, expected.1), "ORIGINAL OWNER!");
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*s.doc().unwrap().doc, *before);
    }
}
#[test]
fn deleting_the_direct_edited_owner_exits_editing_before_neighbour_can_be_modified() {
    for (command, params) in [("table.deleteRow", json!({"row":1})), ("table.deleteColumn", json!({"col":1}))] {
        let (mut s, id) = session();
        s.execute("text.set", &json!({"id":id,"cell":[1,1],"text":"REMOVE THIS"})).unwrap();
        s.execute("text.set", &json!({"id":id,"cell":[2,2],"text":"KEEP NEIGHBOUR"})).unwrap();
        s.execute("text.edit", &json!({"id":id,"cell":[1,1]})).unwrap();
        let mut params = params;
        params["id"] = json!(id);
        s.execute(command, &params).unwrap();
        assert!(s.doc().unwrap().selection.text.is_none());
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("text.insert", &json!({"text":"wrong neighbour"})).is_err());
        assert_eq!(*s.doc().unwrap().doc, *before);
    }
}
#[test]
fn ancestor_axis_edits_move_descendant_path_without_retargeting_child_contents() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 1, 1, "nested original");
    s.execute("text.edit", &json!({"id":id,"path":child,"cell":[0,0]})).unwrap();
    s.execute("table.insertRowAbove", &json!({"id":id,"path":[],"row":0})).unwrap();
    let after = s.doc().unwrap().selection.table_path.clone();
    assert_eq!(after.0[0].row, 2);
    assert_eq!(after.0[0].col, 1);
    assert_eq!(after.0[0].child, child.0[0].child);
    s.execute("text.insert", &json!({"text":"!"})).unwrap();
    assert_eq!(body(&s, id, &after, 0, 0), "nested original!");
    s.execute("table.deleteColumn", &json!({"id":id,"path":[],"col":0})).unwrap();
    let after = s.doc().unwrap().selection.table_path.clone();
    assert_eq!(after.0[0].col, 0);
    assert_eq!(body(&s, id, &after, 0, 0), "nested original!");
}
#[test]
fn merging_reordered_anchor_blocks_never_leaves_a_valid_path_to_the_wrong_child() {
    let (mut s, id) = session();
    let first = nested(&mut s, id, 0, 0, "FIRST CHILD");
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":0,"after":0,"rows":1,"cols":1})).unwrap();
    let second = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.set", &json!({"id":id,"path":second,"cell":[0,0],"text":"SECOND CHILD"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"path":first,"cell":[0,0]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    if let Some(text) = &s.doc().unwrap().selection.text {
        assert_eq!(
            body(&s, id, &s.doc().unwrap().selection.table_path, text.cell.unwrap().0, text.cell.unwrap().1),
            "FIRST CHILD",
            "a retained path must still name the same original owner"
        );
    }
    let mut contents = vec![];
    table(&s, id).cell(0, 0).unwrap().blocks.iter().for_each(|block| {
        if let CellBlock::Table(child) = block {
            contents.push(child.table.cell(0, 0).unwrap().text.text());
        }
    });
    assert_eq!(contents, vec!["SECOND CHILD", "FIRST CHILD"]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    assert_eq!(s.doc().unwrap().selection.table_path, first);
}
#[test]
fn typed_value_and_number_format_changes_retain_paragraph_defaults_and_outline_level() {
    let (mut s, id) = session();
    s.edit(|doc, _| {
        let shape = doc.slides.iter_mut().flat_map(|slide| &mut Arc::make_mut(slide).shapes).find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(t) = &mut shape.kind else { panic!("table") };
        let p = &mut t.cell_mut(0, 0).unwrap().text.paragraphs[0];
        p.level = 3;
        p.def_props.size = Some(31.);
        p.def_props.bold = Some(true);
        p.end_props.italic = Some(true);
        Ok(())
    })
    .unwrap();
    number(&mut s, id, 0, 0, "12.5");
    s.execute("table.setCellNumberFormat",&json!({"id":id,"path":[],"row":0,"col":0,"format":{"style":{"kind":"number","decimalPlaces":2,"minDecimalPlaces":2,"minIntegerDigits":1,"grouping":false}}})).unwrap();
    let p = &table(&s, id).cell(0, 0).unwrap().text.paragraphs[0];
    assert_eq!(p.level, 3);
    assert_eq!(p.def_props.size, Some(31.));
    assert_eq!(p.def_props.bold, Some(true));
    assert_eq!(p.end_props.italic, Some(true));
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text.text(), "12.50");
}
#[test]
fn structural_references_keep_absolute_range_markers_and_formula_profile() {
    let (mut s, id) = session();
    number(&mut s, id, 0, 0, "2");
    number(&mut s, id, 1, 0, "5");
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":2,"col":2,"expression":"=SUM([.$A$1:.$A$2])"})).unwrap();
    let profile = table(&s, id).cell(2, 2).unwrap().formula.as_ref().unwrap().profile().to_string();
    s.execute("table.insertRowAbove", &json!({"id":id,"path":[],"row":0})).unwrap();
    let f = table(&s, id).cell(3, 2).unwrap().formula.as_ref().unwrap();
    assert_eq!(f.expression(), "=SUM([.$A$2:.$A$3])");
    assert_eq!(f.profile(), profile);
    s.execute("table.deleteRow", &json!({"id":id,"path":[],"row":1})).unwrap();
    let c = table(&s, id).cell(2, 2).unwrap();
    assert_eq!(c.formula.as_ref().unwrap().expression(), "=SUM([.$A$2:.$A$2])");
    assert_eq!(c.formula.as_ref().unwrap().profile(), profile);
    assert_eq!(c.text.text(), "5");
}
#[test]
fn nested_tab_grows_its_own_grid_and_resets_continuation_block() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 0, 0, "child");
    s.execute("text.edit", &json!({"id":id,"path":child,"cell":[0,1]})).unwrap();
    s.key("Tab", Mods::default()).unwrap();
    assert_eq!(s.doc().unwrap().selection.table_path, child);
    assert_eq!(s.doc().unwrap().selection.text.as_ref().unwrap().cell, Some((1, 0)));
    assert_eq!(s.doc().unwrap().selection.cell_block, Some(0));
    assert_eq!(table(&s, id).at_path(&child).unwrap().n_rows(), 2);
    assert_eq!(table(&s, id).n_rows(), 3);
}
#[test]
fn nested_composition_settles_before_parent_structure_edit_and_undo_keeps_ownership() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 1, 0, "before");
    s.execute("text.edit", &json!({"id":id,"path":child,"cell":[0,0]})).unwrap();
    s.execute("text.compose", &json!({"text":" composed"})).unwrap();
    s.execute("table.insertRowAbove", &json!({"id":id,"path":[],"row":0})).unwrap();
    let shifted = s.doc().unwrap().selection.table_path.clone();
    assert!(s.doc().unwrap().composition.is_none());
    assert_eq!(shifted.0[0].row, 2);
    assert_eq!(body(&s, id, &shifted, 0, 0), "before composed");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(body(&s, id, &child, 0, 0), "before composed");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(body(&s, id, &child, 0, 0), "before");
}
#[test]
fn find_replace_all_retains_ordered_children_and_recalculates_only_current_canonical_owner() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 0, 0, "child needle");
    s.execute("table.leaveNested", &json!({"id":id})).unwrap();
    s.execute("text.insert", &json!({"text":"tail needle"})).unwrap();
    let changed = s.execute("text.replace", &json!({"text":"needle","with":"match"})).unwrap();
    assert_eq!(changed["replaced"], 2);
    assert_eq!(body(&s, id, &child, 0, 0), "child match");
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text_blocks().last().unwrap().text(), "tail match");
    let saved = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    assert_eq!(deckcraft_format::load(&saved).unwrap(), *s.doc().unwrap().doc);
}
#[test]
fn explicit_cell_range_path_changes_context_before_following_implicit_command() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 0, 0, "child");
    s.execute("table.selectCells", &json!({"id":id,"path":[],"from":[0,0],"to":[0,0]})).unwrap();
    assert!(s.doc().unwrap().selection.table_path.is_empty());
    assert_eq!(s.doc().unwrap().selection.table_owner, Some(id));
    s.execute("table.rowHeight", &json!({"row":0,"height":99.})).unwrap();
    assert_eq!(table(&s, id).rows[0].height, 99.);
    assert_eq!(table(&s, id).at_path(&child).unwrap().rows[0].height, 24.);
}
#[test]
fn selected_cell_range_moves_with_inserted_axis_and_undo_restores_its_coordinates() {
    let (mut s, id) = session();
    s.execute("table.selectCells", &json!({"id":id,"path":[],"from":[1,1],"to":[2,2]})).unwrap();
    let before = s.doc().unwrap().selection.cells;
    s.execute("table.insertRowAbove", &json!({"id":id,"path":[],"row":0})).unwrap();
    assert_eq!(s.doc().unwrap().selection.cells, Some((id, (2, 1), (3, 2))));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection.cells, before);
}
#[test]
fn cell_range_rejects_invalid_leaf_endpoints_without_changing_selection_or_history() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 0, 0, "child");
    let selected = s.doc().unwrap().selection.clone();
    let doc = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    for from in [json!([99, 0]), json!([0, 99]), json!(["bad", 0]), json!([-1, 0])] {
        assert!(s.execute("table.selectCells", &json!({"id":id,"path":child,"from":from,"to":[0,0]})).is_err());
        assert_eq!(s.doc().unwrap().selection, selected);
        assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
}
#[test]
fn merge_redo_restores_the_same_safe_canonical_owner_state() {
    let (mut s, id) = session();
    let first = nested(&mut s, id, 0, 0, "first");
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":0,"after":0,"rows":1,"cols":1})).unwrap();
    s.execute("text.edit", &json!({"id":id,"path":first,"cell":[0,0]})).unwrap();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    let selection = s.doc().unwrap().selection.clone();
    let doc = s.doc().unwrap().doc.clone();
    assert!(selection.text.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(*s.doc().unwrap().doc, *doc);
}
#[test]
fn parent_axis_edit_keeps_existing_nested_cell_range_in_leaf_coordinates() {
    let (mut s, id) = session();
    let child = nested(&mut s, id, 1, 0, "child");
    s.execute("table.selectCells", &json!({"id":id,"path":child,"from":[0,0],"to":[0,1]})).unwrap();
    let range = s.doc().unwrap().selection.cells;
    s.execute("table.insertRowAbove", &json!({"id":id,"path":[],"row":0})).unwrap();
    assert_eq!(s.doc().unwrap().selection.cells, range);
    assert_eq!(s.doc().unwrap().selection.table_path.0[0].row, 2);
}
#[test]
fn merge_and_split_preserve_outside_legacy_arithmetic_and_profile_in_a_merged_grid() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":0,"col":0,"expression":"=2+3","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":2,"col":2,"expression":"=7*8","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().text.text(), "5");
    assert!(table(&s, id).cell(0, 0).unwrap().formula.is_none());
    let outside = table(&s, id).cell(2, 2).unwrap();
    assert_eq!(outside.text.text(), "56");
    assert_eq!(outside.formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
    s.execute("table.split", &json!({"id":id,"path":[],"cell":[0,0]})).unwrap();
    assert_eq!(table(&s, id).cell(2, 2).unwrap().text.text(), "56");
    assert_eq!(table(&s, id).cell(2, 2).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
}
#[test]
fn covered_cells_are_empty_in_v2_while_legacy_empty_numeric_semantics_stay_strict() {
    let (mut s, id) = session();
    number(&mut s, id, 0, 0, "2");
    number(&mut s, id, 0, 1, "3");
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":0,"expression":"=LEN([.A1])"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":1,"expression":"=[.B1]+1"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":2,"expression":"=[.B1]+1","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    assert_eq!(table(&s, id).cell(1, 0).unwrap().text.text(), "#VALUE!");
    assert_eq!(table(&s, id).cell(1, 1).unwrap().text.text(), "1");
    assert_eq!(table(&s, id).cell(1, 2).unwrap().text.text(), "#VALUE!");
    assert_eq!(table(&s, id).cell(1, 2).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
    s.edit(|doc, _| {
        let shape = doc.slides.iter_mut().flat_map(|slide| &mut Arc::make_mut(slide).shapes).find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(t) = &mut shape.kind else { panic!("table") };
        t.cell_mut(0, 1).unwrap().text = TextBody::from_text("HOSTILE HIDDEN TEXT");
        Ok(())
    })
    .unwrap();
    assert_eq!(table(&s, id).cell(1, 1).unwrap().text.text(), "1");
    assert_eq!(table(&s, id).cell(1, 2).unwrap().text.text(), "#VALUE!");
}
#[test]
fn calculation_plan_cannot_publish_after_merge_ownership_changes_without_scalar_edits() {
    let mut t = Table::new(2, 3, 300., 30.);
    t.set_cell_formula_v2(1, 2, "=1+2").unwrap();
    let plan = t.plan_recalculation().unwrap();
    let cached = t.cell(1, 2).unwrap().clone();
    t.cell_mut(0, 0).unwrap().grid_span = 2;
    t.cell_mut(0, 1).unwrap().h_merge = true;
    let before = t.clone();
    assert!(plan.apply(&mut t).is_err(), "a plan must bind physical span/coverage ownership even if scalar caches did not change");
    assert_eq!(t, before);
    assert_eq!(t.cell(1, 2).unwrap(), &cached);
}
#[test]
fn merged_grid_formula_shapes_copy_paste_native_and_axis_edits_keep_semantics() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"row":2,"col":2,"expression":"=6*7","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[0,0],"to":[0,1]})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let copied = s.doc().unwrap().selection.shapes[0];
    assert_ne!(copied, id);
    assert_eq!(table(&s, copied), table(&s, id));
    s.execute("table.insertRowAbove", &json!({"id":copied,"path":[],"row":0})).unwrap();
    assert_eq!(table(&s, copied).cell(3, 2).unwrap().text.text(), "42");
    assert_eq!(table(&s, copied).cell(3, 2).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
    let saved = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    assert_eq!(deckcraft_format::load(&saved).unwrap(), *s.doc().unwrap().doc);
}
#[test]
fn merged_nested_block_hit_and_placed_text_share_independently_derived_geometry() {
    let (mut s, id) = session();
    let path = nested(&mut s, id, 1, 0, "Geometry child");
    s.execute("table.merge", &json!({"id":id,"path":[],"from":[1,0],"to":[1,1]})).unwrap();
    s.edit(|doc, _| {
        let shape = doc.slides.iter_mut().flat_map(|slide| &mut Arc::make_mut(slide).shapes).find(|shape| shape.id == id).unwrap();
        let ShapeKind::Table(t) = &mut shape.kind else { panic!("table") };
        for row in &mut t.rows {
            row.height = 150.;
        }
        let c = t.cell_mut(1, 0).unwrap();
        c.text = TextBody::default();
        c.margins = Some([10., 20., 15., 25.]);
        c.anchor = Some(text::Anchor::Bottom);
        c.blocks.retain(|b| matches!(b, CellBlock::Table(_)));
        let CellBlock::Table(child) = &mut c.blocks[0] else { panic!("child") };
        child.table.cols = vec![420.];
        child.table.rows.truncate(1);
        child.table.rows[0].cells.truncate(1);
        child.table.rows[0].height = 40.;
        child.table.cell_mut(0, 0).unwrap().text = TextBody::from_text("Geometry child");
        Ok(())
    })
    .unwrap();
    let p = &s.doc().unwrap().doc;
    let ctx = resolve::Ctx::for_slide(p, &p.slides[0]).unwrap();
    let plan = deckcraft_text::table_content::layout(&ctx, table(&s, id), &deckcraft_text::NoFields).unwrap();
    let c = plan.cells.iter().find(|c| c.row == 1 && c.col == 0).unwrap();
    let deckcraft_text::table_content::BlockLayout::Table { id: child, transform, .. } = &c.blocks[0] else { panic!("table block") };
    assert_eq!(transform.as_coeffs(), [0.5, 0., 0., 0.5, 10., 255.]);
    let hit = plan.hit_text(&ctx, &deckcraft_text::NoFields, *transform * deckcraft_geom::Point::new(10., 10.)).unwrap();
    assert_eq!(hit.path.0, vec![TablePathStep { row: 1, col: 0, child: *child }]);
    assert_eq!((hit.row, hit.col, hit.block), (0, 0, None));
    let placed = deckcraft_render::place_slide(p, 0);
    let text = placed.texts.iter().find(|t| t.body.text() == "Geometry child").unwrap();
    assert_eq!(text.transform.as_coeffs(), [0.5, 0., 0., 0.5, 30., 285.]);
    assert_eq!(path.0[0].row, 1);
}
