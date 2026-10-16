//! Contributor-original checked pencil/eraser authoring regressions. UNRUN.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_engine::{Session, drawtable};
use deckcraft_model::{table::*, text::TextBody, *};
use serde_json::json;
use std::sync::Arc;
fn session() -> (Session, ShapeId) {
    let mut s = Session::with_new();
    let result = s.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[100,100,240,100]})).unwrap();
    (s, serde_json::from_value(result["id"].clone()).unwrap())
}
fn table(s: &Session, id: ShapeId) -> &Table {
    let ShapeKind::Table(t) = &s.doc().unwrap().shape(id).unwrap().kind else { panic!("table") };
    t
}
fn number(s: &mut Session, id: ShapeId, row: usize, col: usize, lexical: &str) {
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"path":[],"row":row,"col":col,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":lexical},"display":lexical}),
    )
    .unwrap();
}
fn rich_table() -> Table {
    let mut t = Table::new(1, 1, 200., 80.);
    t.set_cell_formula_v2(0, 0, "=1+2").unwrap();
    let mut child = Table::new(1, 1, 80., 24.);
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("single child authority");
    let c = t.cell_mut(0, 0).unwrap();
    c.append_nested_table(child).unwrap();
    c.append_text_block(TextBody::from_text("ordered tail")).unwrap();
    c.number_format = Some(Arc::new(NumberFormat::default()));
    t
}
#[test]
fn horizontal_and_vertical_splits_preserve_formula_and_children_once_and_make_empty_cells() {
    for vertical in [false, true] {
        let mut t = rich_table();
        let original = t.cell(0, 0).unwrap().clone();
        let (a, b) = if vertical { ((100., 0.), (100., 80.)) } else { ((0., 40.), (200., 40.)) };
        assert!(drawtable::draw_line(&mut t, a, b));
        let kept = t.cell(0, 0).unwrap();
        assert_eq!(kept.text, original.text);
        assert_eq!(kept.blocks, original.blocks);
        assert_eq!(kept.formula.as_ref().unwrap().profile(), FORMULA_PROFILE_V2);
        assert_eq!(kept.number_format, original.number_format);
        let fresh = if vertical { t.cell(0, 1).unwrap() } else { t.cell(1, 0).unwrap() };
        assert!(fresh.text.is_empty());
        assert!(fresh.blocks.is_empty());
        assert!(fresh.data.is_none() && fresh.formula.is_none() && fresh.number_format.is_none());
        t.validate_content().unwrap();
        t.validate_formulas().unwrap();
    }
}
#[test]
fn actual_pencil_command_rewrites_row_references_and_keeps_untouched_formula_owner() {
    let (mut s, id) = session();
    number(&mut s, id, 0, 0, "7");
    number(&mut s, id, 1, 0, "5");
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":0,"col":1,"expression":"=7*2","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":1,"expression":"=[.A1]+[.A2]","profile":"exact-decimal-v1"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[1,1]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.drawLine", &json!({"id":id,"from":[100,125],"to":[220,125]})).unwrap();
    let t = table(&s, id);
    assert_eq!((t.n_rows(), t.n_cols()), (3, 2));
    assert_eq!(t.cell(0, 1).unwrap().row_span, 2);
    assert_eq!(t.cell(0, 1).unwrap().text.text(), "14");
    assert!(t.cell(1, 1).unwrap().formula.is_none());
    let f = t.cell(2, 1).unwrap().formula.as_ref().unwrap();
    assert_eq!(f.expression(), "=[.A1]+[.A3]");
    assert_eq!(f.profile(), FORMULA_PROFILE);
    assert_eq!(t.cell(2, 1).unwrap().text.text(), "12");
    assert!(s.doc().unwrap().selection.text.is_none());
    let after = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *after);
    assert_eq!(s.doc().unwrap().selection, selection);
}
#[test]
fn new_column_references_keep_absolute_reversed_ranges_and_their_profile() {
    let mut t = Table::new(2, 3, 300., 80.);
    for (col, value) in [(0, "2"), (1, "3")] {
        let c = t.cell_mut(0, col).unwrap();
        c.text = TextBody::from_text(value);
        c.data =
            Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: value.into() }, &c.text).unwrap()));
    }
    t.set_cell_formula_v2(1, 2, "=SUM([.$B$1:.$A$1])").unwrap();
    assert!(drawtable::draw_line(&mut t, (50., 0.), (50., 80.)));
    let f = t.cell(1, 3).unwrap().formula.as_ref().unwrap();
    assert_eq!(f.expression(), "=SUM([.$C$1:.$A$1])");
    assert_eq!(f.profile(), FORMULA_PROFILE_V2);
    assert_eq!(t.cell(1, 3).unwrap().text.text(), "5");
    assert!(t.cell(0, 1).unwrap().data.is_none());
}
#[test]
fn splitting_an_existing_merged_boundary_does_not_rewrite_or_duplicate_authority() {
    let mut t = Table::new(2, 2, 200., 40.);
    t.merge_with_budget((0, 0), (0, 1), &mut FormulaBudget::default()).unwrap();
    t.set_cell_formula(1, 1, "=3*4").unwrap();
    let mut child = Table::new(1, 1, 80., 24.);
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("kept child");
    t.cell_mut(0, 0).unwrap().append_nested_table(child).unwrap();
    assert!(drawtable::draw_line(&mut t, (100., 0.), (100., 40.)));
    assert_eq!((t.n_rows(), t.n_cols()), (2, 2));
    assert_eq!(t.cell(0, 0).unwrap().blocks.len(), 1);
    assert!(t.cell(0, 1).unwrap().blocks.is_empty());
    assert_eq!(t.cell(1, 1).unwrap().formula.as_ref().unwrap().expression(), "=3*4");
    assert_eq!(t.cell(1, 1).unwrap().text.text(), "12");
}
#[test]
fn actual_eraser_preserves_ordered_nested_content_and_bakes_formula_display() {
    let (mut s, id) = session();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":0,"col":0,"expression":"=2+3"})).unwrap();
    s.execute("table.setCellNumberFormat",&json!({"id":id,"path":[],"row":0,"col":0,"format":{"style":{"kind":"number","decimalPlaces":2,"minDecimalPlaces":2,"minIntegerDigits":1,"grouping":false}}})).unwrap();
    s.execute("text.set", &json!({"id":id,"path":[],"cell":[0,1],"text":"second canonical"})).unwrap();
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":1,"after":0,"rows":1,"cols":1})).unwrap();
    let child = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.set", &json!({"id":id,"path":child,"cell":[0,0],"text":"kept nested text"})).unwrap();
    s.execute("table.leaveNested", &json!({"id":id})).unwrap();
    s.execute("text.insert", &json!({"text":"tail"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":1,"expression":"=6*7","profile":"exact-decimal-v1"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.eraseLine", &json!({"id":id,"at":[220,125],"tol":3})).unwrap();
    let t = table(&s, id);
    let c = t.cell(0, 0).unwrap();
    assert_eq!(c.text.text(), "5.00");
    assert!(c.formula.is_none() && c.data.is_none() && c.number_format.is_none());
    assert!(matches!(&c.blocks[0],CellBlock::Text(text)if text.body.text()=="second canonical"));
    assert!(matches!(&c.blocks[1],CellBlock::Table(child)if child.table.cell(0,0).unwrap().text.text()=="kept nested text"));
    assert!(matches!(&c.blocks[2],CellBlock::Text(text)if text.body.text()=="tail"));
    assert!(t.cell(0, 1).unwrap().blocks.is_empty());
    assert_eq!(t.cell(1, 1).unwrap().text.text(), "42");
    assert_eq!(t.cell(1, 1).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
    let saved = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    assert_eq!(deckcraft_format::load(&saved).unwrap(), *s.doc().unwrap().doc);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}
#[test]
fn missed_line_nonfinite_input_invalid_geometry_and_no_border_leave_exact_source() {
    let before = rich_table();
    for (a, b) in [((400., 40.), (500., 40.)), ((f64::NAN, 40.), (200., 40.)), ((0., 1.), (200., 1.))] {
        let mut t = before.clone();
        assert!(!drawtable::draw_line(&mut t, a, b));
        assert_eq!(t, before);
    }
    for tolerance in [f64::NAN, f64::INFINITY, -1.] {
        let mut t = before.clone();
        assert!(!drawtable::erase_at(&mut t, (100., 40.), tolerance));
        assert_eq!(t, before);
    }
    let mut malformed = before.clone();
    malformed.cell_mut(0, 0).unwrap().grid_span = 3;
    let before = malformed.clone();
    assert!(!drawtable::draw_line(&mut malformed, (0., 40.), (200., 40.)));
    assert_eq!(malformed, before);
}
#[test]
fn stale_formula_display_refuses_pencil_and_eraser_without_repair_or_data_restore() {
    let mut t = Table::new(2, 2, 200., 80.);
    t.set_cell_formula_v2(1, 1, "=1+2").unwrap();
    t.cell_mut(1, 1).unwrap().text = TextBody::from_text("foreign edited display");
    let before = t.clone();
    assert!(!drawtable::draw_line(&mut t, (0., 40.), (200., 40.)));
    assert_eq!(t, before);
    assert!(!drawtable::erase_at(&mut t, (100., 40.), 3.));
    assert_eq!(t, before);
}
#[test]
fn command_gesture_from_nested_edit_context_targets_root_grid_and_exits_old_owner() {
    let (mut s, id) = session();
    s.execute("table.insertNested", &json!({"id":id,"path":[],"row":0,"col":0,"after":0,"rows":1,"cols":1})).unwrap();
    let path = s.doc().unwrap().selection.table_path.clone();
    s.execute("text.set", &json!({"id":id,"path":path,"cell":[0,0],"text":"kept child"})).unwrap();
    s.execute("table.drawLine", &json!({"id":id,"from":[100,125],"to":[220,125]})).unwrap();
    assert_eq!(table(&s, id).n_rows(), 3);
    assert_eq!(table(&s, id).at_path(&path).unwrap().n_rows(), 1);
    assert_eq!(table(&s, id).at_path(&path).unwrap().cell(0, 0).unwrap().text.text(), "kept child");
    assert!(s.doc().unwrap().selection.table_path.is_empty());
    assert!(s.doc().unwrap().selection.text.is_none());
}
#[test]
fn repeated_gestures_share_cumulative_fuel_and_exhaustion_is_atomic() {
    let mut t = Table::new(12, 12, 1200., 80.);
    t.set_cell_formula_v2(11, 11, "=1+2").unwrap();
    let mut budget = FormulaBudget::default();
    let mut stopped = false;
    for _ in 0..10_000 {
        let before = t.clone();
        if t.cell(0, 0).unwrap().grid_span > 1 {
            if !drawtable::draw_line_with_budget(&mut t, (100., 0.), (100., 80.), &mut budget) {
                assert_eq!(t, before);
                stopped = true;
                break;
            }
        } else if !drawtable::erase_at_with_budget(&mut t, (100., 40.), 3., &mut budget) {
            assert_eq!(t, before);
            stopped = true;
            break;
        }
    }
    assert!(stopped);
    assert!(budget.check_limits().is_err());
}
fn point_from_source(x: Xfrm, point: (f64, f64)) -> [f64; 2] {
    let mut dx = point.0 - x.w / 2.;
    let mut dy = point.1 - x.h / 2.;
    if x.flip_h {
        dx = -dx;
    }
    if x.flip_v {
        dy = -dy;
    }
    if x.rot == 90. { [x.x + x.w / 2. - dy, x.y + x.h / 2. + dx] } else { [x.x + x.w / 2. + dx, x.y + x.h / 2. + dy] }
}
#[test]
fn actual_rotated_flipped_and_mismatched_box_commands_use_intrinsic_coordinates_and_original_pivot() {
    for (case, (rotation, flip_h, flip_v, mismatch)) in
        [(90., false, false, false), (0., true, false, false), (0., false, true, false), (90., true, true, true)].into_iter().enumerate()
    {
        let (mut s, id) = session();
        number(&mut s, id, 0, 0, "7");
        s.execute("table.setCellFormula", &json!({"id":id,"path":[],"row":1,"col":1,"expression":"=[.A1]*2","profile":"exact-decimal-v1"})).unwrap();
        let x = Xfrm { rot: rotation, flip_h, flip_v, ..Xfrm::new(100., 100., if mismatch { 40. } else { 240. }, if mismatch { 20. } else { 100. }) };
        s.edit(|doc, _| {
            let sh = doc.slides.iter_mut().flat_map(|slide| &mut Arc::make_mut(slide).shapes).find(|shape| shape.id == id).unwrap();
            sh.xfrm = Some(x);
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let from = point_from_source(x, (0., 25.));
        let to = point_from_source(x, (120., 25.));
        s.execute("table.drawLine", &json!({"id":id,"from":from,"to":to})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm, Some(x));
        assert_eq!(table(&s, id).n_rows(), 3);
        assert!((table(&s, id).rows[0].height - 25.).abs() < 1e-7);
        assert_eq!(table(&s, id).cell(2, 1).unwrap().text.text(), "14");
        qa_png(&s, &format!("drawing-affine-{case}-split.png"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*s.doc().unwrap().doc, *before);
        let at = point_from_source(x, (120., 25.));
        s.execute("table.eraseLine", &json!({"id":id,"at":at,"tol":3.})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm, Some(x));
        assert_eq!(table(&s, id).cell(0, 0).unwrap().grid_span, 2);
        assert_eq!(table(&s, id).cell(1, 1).unwrap().text.text(), "#VALUE!");
        qa_png(&s, &format!("drawing-affine-{case}-erased.png"));
    }
}
#[test]
fn automatic_hit_finds_painted_row_overflow_and_materializes_drawn_heights_before_split() {
    let (mut s, id) = session();
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"Painted overflowing row text ".repeat(30)})).unwrap();
    s.edit(|doc, _| {
        let sh = doc.slides.iter_mut().flat_map(|slide| &mut Arc::make_mut(slide).shapes).find(|shape| shape.id == id).unwrap();
        sh.xfrm.as_mut().unwrap().h = 16.;
        let ShapeKind::Table(t) = &mut sh.kind else { panic!("table") };
        for row in &mut t.rows {
            row.height = 8.;
        }
        Ok(())
    })
    .unwrap();
    let frame = deckcraft_engine::cmd::table::table_frame(s.doc().unwrap(), s.doc().unwrap().shape(id).unwrap()).unwrap();
    let cut = frame.heights[0] / 2.;
    assert!(cut > 16.);
    let before = s.doc().unwrap().doc.clone();
    s.execute("table.drawLine", &json!({"from":[100.,100.+cut],"to":[340.,100.+cut]})).unwrap();
    let t = table(&s, id);
    assert_eq!(t.n_rows(), 3);
    assert!((t.rows[0].height - cut).abs() < 1e-7);
    assert!((t.rows[1].height - cut).abs() < 1e-7);
    assert!((t.rows[2].height - frame.heights[1]).abs() < 1e-7);
    assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm.as_ref().unwrap().h, 16.);
    assert_eq!(t.cell(0, 0).unwrap().text.text(), "Painted overflowing row text ".repeat(30));
    qa_png(&s, "drawing-overflow-split.png");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}
fn scale_group(s: &mut Session, id: ShapeId, scale: f64) {
    s.edit(|doc, _| {
        let group_id = ShapeId(doc.alloc_id().unwrap());
        let shapes = &mut Arc::make_mut(&mut doc.slides[0]).shapes;
        let at = shapes.iter().position(|shape| shape.id == id).unwrap();
        let mut child = shapes.remove(at);
        child.xfrm = Some(Xfrm::new(0.0, 0.0, 240.0, 100.0));
        shapes.push(Shape {
            id: group_id,
            xfrm: Some(Xfrm::new(100.0, 100.0, 240.0 * scale, 100.0 * scale)),
            kind: ShapeKind::Group { children: vec![child], child: Xfrm::new(0.0, 0.0, 240.0, 100.0) },
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
}
#[test]
fn eraser_tolerance_is_measured_in_slide_points_through_scaled_groups() {
    let (mut s, id) = session();
    scale_group(&mut s, id, 2.0);
    let before = s.doc().unwrap().doc.clone();
    // Intrinsic border x=120 -> slide x=340. Eight slide points exceeds tol=6.
    assert!(s.execute("table.eraseLine", &json!({"id":id,"at":[348.0,150.0],"tol":6.0})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);

    let (mut s, id) = session();
    scale_group(&mut s, id, 0.5);
    // Same intrinsic border -> slide x=160. Four slide points lies within tol=6.
    s.execute("table.eraseLine", &json!({"id":id,"at":[164.0,112.5],"tol":6.0})).unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().grid_span, 2);
}

fn nonuniform_group(s: &mut Session, id: ShapeId, sx: f64, sy: f64) {
    s.edit(|doc, _| {
        let group_id = ShapeId(doc.alloc_id().unwrap());
        let shapes = &mut Arc::make_mut(&mut doc.slides[0]).shapes;
        let at = shapes.iter().position(|shape| shape.id == id).unwrap();
        let mut child = shapes.remove(at);
        child.xfrm = Some(Xfrm::new(0., 0., 240., 100.));
        shapes.push(Shape {
            id: group_id,
            xfrm: Some(Xfrm::new(100., 100., 240. * sx, 100. * sy)),
            kind: ShapeKind::Group { children: vec![child], child: Xfrm::new(0., 0., 240., 100.) },
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
}
#[test]
fn anisotropic_eraser_uses_slide_tolerance_and_ranks_both_axes_in_slide_space() {
    let (mut s, id) = session();
    nonuniform_group(&mut s, id, 4., 0.5);
    let before = s.doc().unwrap().doc.clone();
    // Border x=120 maps to x=580; eight slide points must miss tol=6.
    assert!(s.execute("table.eraseLine", &json!({"id":id,"at":[588.,112.5],"tol":6.})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    // Four slide points must hit that same finite border.
    s.execute("table.eraseLine", &json!({"id":id,"at":[584.,112.5],"tol":6.})).unwrap();
    assert_eq!(table(&s, id).cell(0, 0).unwrap().grid_span, 2);
    let (mut s, id) = session();
    nonuniform_group(&mut s, id, 4., 0.2);
    // At local (117,40), vertical border distance is 3 local/12 slide,
    // horizontal is 10 local/2 slide. Rank by slide metric, not local units.
    s.execute("table.eraseLine", &json!({"id":id,"at":[568.,108.],"tol":15.})).unwrap();
    let owner = table(&s, id).cell(0, 0).unwrap();
    assert_eq!((owner.row_span, owner.grid_span), (2, 1));
}
fn qa_png(s: &Session, name: &str) {
    if let Ok(dir) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let opts = deckcraft_render::RenderOpts { scale: 0.75, ..Default::default() };
        let image = deckcraft_render::render_slide(&s.doc().unwrap().doc, 0, &opts);
        image::save_buffer(std::path::Path::new(&dir).join(name), &image.to_straight(), image.width, image.height, image::ColorType::Rgba8).unwrap();
    }
}
