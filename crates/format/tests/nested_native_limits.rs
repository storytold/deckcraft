//! Regression for stack growth in recursive internally tagged serde enums.
//! These fixtures are contributor-original and exercise the actual ZIP reader.
use deckcraft_model::{
    Presentation, Shape, ShapeId, ShapeKind, Table, TextBody, Xfrm,
    table::{CellBlock, MAX_NESTED_TABLE_DEPTH},
};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn nested_table(depth: usize) -> Table {
    let mut table = Table::new(1, 1, 120., 20.);
    table.cell_mut(0, 0).unwrap().text = TextBody::from_text("deep original");
    for level in 0..depth {
        let mut parent = Table::new(1, 1, 120., 20.);
        let cell = parent.cell_mut(0, 0).unwrap();
        cell.text = TextBody::from_text(&format!("before {level}"));
        cell.append_text_block(TextBody::from_text(&format!("middle {level}"))).unwrap();
        cell.append_nested_table(table).unwrap();
        cell.append_text_block(TextBody::from_text(&format!("after {level}"))).unwrap();
        table = parent;
    }
    table
}
fn document(groups: usize, tables: usize) -> Presentation {
    let mut shape = Shape { id: ShapeId(700), kind: ShapeKind::Table(nested_table(tables)), ..Default::default() };
    for index in 0..groups {
        shape = Shape {
            id: ShapeId(701 + index as u32),
            name: format!("group {index}"),
            xfrm: Some(Xfrm::new(0., 0., 120., 60.)),
            kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0., 0., 120., 60.) },
            ..Default::default()
        };
    }
    let mut doc = Presentation::default();
    Arc::make_mut(&mut doc.slides[0]).shapes = vec![shape];
    doc.fix_next_id();
    doc
}
fn raw_zip(json: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("presentation.json", options).unwrap();
    zip.write_all(json).unwrap();
    zip.finish().unwrap().into_inner()
}
fn manifest(doc: &Presentation) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Manifest<'a> {
        format: &'static str,
        version: u32,
        generator: &'static str,
        presentation: &'a Presentation,
    }
    serde_json::to_vec(&Manifest { format: "deckcraft", version: 1, generator: "synthetic test", presentation: doc }).unwrap()
}
fn assert_roundtrip(groups: usize, tables: usize) {
    let doc = document(groups, tables);
    let bytes = deckcraft_format::save(&doc).unwrap();
    let (back, warnings) = deckcraft_format::load_checked(&bytes).unwrap();
    assert!(warnings.is_empty());
    // Check the complete graph: names, geometry, cell-local identities, block
    // ordering and bodies must survive, not merely the deepest displayed text.
    assert_eq!(back.slides[0].shapes, doc.slides[0].shapes);
    let mut shape = &back.slides[0].shapes[0];
    for _ in 0..groups {
        let ShapeKind::Group { children, .. } = &shape.kind else { panic!("group missing") };
        assert_eq!(children.len(), 1);
        shape = &children[0];
    }
    let ShapeKind::Table(table) = &shape.kind else { panic!("table missing") };
    let mut table = table;
    for level in (0..tables).rev() {
        let cell = table.cell(0, 0).unwrap();
        assert_eq!(cell.text.text(), format!("before {level}"));
        assert_eq!(cell.blocks.len(), 3);
        let CellBlock::Text(before) = &cell.blocks[0] else { panic!("middle text missing") };
        assert_eq!(before.id, 1);
        assert_eq!(before.body.text(), format!("middle {level}"));
        let CellBlock::Table(child) = &cell.blocks[1] else { panic!("nested table missing") };
        assert_eq!(child.id, 2);
        let CellBlock::Text(after) = &cell.blocks[2] else { panic!("after text missing") };
        assert_eq!(after.id, 3);
        assert_eq!(after.body.text(), format!("after {level}"));
        table = &child.table;
    }
    assert_eq!(table.cell(0, 0).unwrap().text.text(), "deep original");
}
#[test]
fn maximum_group_and_nested_table_composition_roundtrips_without_serde_default_limit() {
    assert_roundtrip(64, MAX_NESTED_TABLE_DEPTH);
}
#[test]
fn maximum_group_depth_alone_keeps_every_owner() {
    assert_roundtrip(64, 0);
}
#[test]
fn maximum_nested_table_depth_alone_keeps_order_and_identities() {
    assert_roundtrip(0, MAX_NESTED_TABLE_DEPTH);
}
#[test]
fn semantic_depth_plus_one_refuses_raw_native_input_and_save() {
    for (groups, tables) in [(65, 0), (0, MAX_NESTED_TABLE_DEPTH + 1), (64, MAX_NESTED_TABLE_DEPTH + 1)] {
        let mut doc = document(0, 0);
        // The seventeenth edge is deliberately constructed without the safe
        // authoring API, to exercise a hostile native wire boundary.
        let mut table = nested_table(tables.min(MAX_NESTED_TABLE_DEPTH));
        if tables > MAX_NESTED_TABLE_DEPTH {
            let mut parent = Table::new(1, 1, 120., 20.);
            parent.cell_mut(0, 0).unwrap().blocks.push(CellBlock::Table(deckcraft_model::table::NestedTable { id: 1, table: Box::new(table) }));
            table = parent;
        }
        let mut shape = Shape { id: ShapeId(700), kind: ShapeKind::Table(table), ..Default::default() };
        for index in 0..groups {
            shape = Shape {
                id: ShapeId(701 + index as u32),
                kind: ShapeKind::Group { children: vec![shape], child: Xfrm::default() },
                ..Default::default()
            };
        }
        Arc::make_mut(&mut doc.slides[0]).shapes = vec![shape];
        assert!(deckcraft_format::save(&doc).is_err());
        assert!(deckcraft_format::load(&raw_zip(&manifest(&doc))).is_err());
    }
}
#[test]
fn raw_encoded_depth_cap_plus_one_refuses_before_model_decode() {
    let json = format!("{{\"format\":\"deckcraft\",\"presentation\":{{}},\"unknown\":{}0{}}}", "[".repeat(512), "]".repeat(512));
    let error = deckcraft_format::load(&raw_zip(json.as_bytes())).unwrap_err().to_string();
    assert!(error.contains("depth limit"), "{error}");
}
#[test]
fn escaped_one_mebibyte_string_obeys_decoded_size_limit() {
    let mut doc = document(0, 0);
    Arc::make_mut(&mut doc.slides[0]).shapes[0].kind =
        ShapeKind::Opaque { xml: "".into(), preview: None, label: "ESCAPED_LABEL".into(), parts: vec![] };
    let source = String::from_utf8(manifest(&doc)).unwrap();
    for extra in [0, 1] {
        let json = source.replace("ESCAPED_LABEL", &r"\u0041".repeat(1024 * 1024 + extra));
        let back = deckcraft_format::load(&raw_zip(json.as_bytes()));
        if extra == 0 {
            let back = back.unwrap();
            let ShapeKind::Opaque { label, .. } = &back.slides[0].shapes[0].kind else { panic!("opaque shape missing") };
            assert_eq!(label.len(), 1024 * 1024);
            assert!(label.bytes().all(|b| b == b'A'));
        } else {
            assert!(back.is_err());
        }
    }
}

#[test]
fn shallow_lowering_keeps_strict_metadata_and_owner_checks() {
    use deckcraft_model::table::{CellConstant, CellValue};
    let mut doc = document(1, 1);
    let ShapeKind::Group { children, .. } = &mut Arc::make_mut(&mut doc.slides[0]).shapes[0].kind else { panic!("group") };
    let ShapeKind::Table(table) = &mut children[0].kind else { panic!("table") };
    let CellBlock::Table(child) = &mut table.rows[0].cells[0].blocks[1] else { panic!("child") };
    let cell = child.table.cell_mut(0, 0).unwrap();
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "stored scalar".into() }, &cell.text).unwrap()));
    let source = String::from_utf8(manifest(&doc)).unwrap();
    let back = deckcraft_format::load(&raw_zip(source.as_bytes())).unwrap();
    assert_eq!(back.slides[0].shapes, doc.slides[0].shapes);
    for (from, to) in [
        ("\"id\":2,\"table\"", "\"id\":2,\"id\":2,\"table\""),
        ("\"display\":[\"deep original\"]", "\"display\":[\"deep original\"],\"display\":[\"deep original\"]"),
        ("\"display\":[\"deep original\"]", "\"display\":[\"deep original\"],\"trusted\":true"),
        ("\"id\":2,\"table\"", "\"id\":0,\"table\""),
        ("\"height\":20.0", "\"height\":null"),
        ("\"hMerge\":false", "\"hMerge\":true"),
        ("\"table\":{", "\"table\":0,\"foreignTable\":{"),
    ] {
        assert!(source.contains(from), "missing synthetic edit target {from}");
        let damaged = source.replacen(from, to, 1);
        assert!(deckcraft_format::load(&raw_zip(damaged.as_bytes())).is_err(), "accepted malformed owner/metadata {to}");
    }
    // A genuine display edit is still ordinary text after stale-authority
    // reconciliation, with no resurrected typed scalar or detached child.
    let edited = source.replacen("deep original", "edited literal", 1);
    let (back, warnings) = deckcraft_format::load_checked(&raw_zip(edited.as_bytes())).unwrap();
    assert_eq!(warnings.len(), 1);
    let ShapeKind::Group { children, .. } = &back.slides[0].shapes[0].kind else { panic!("group") };
    let ShapeKind::Table(table) = &children[0].kind else { panic!("table") };
    let CellBlock::Table(child) = &table.rows[0].cells[0].blocks[1] else { panic!("child") };
    assert_eq!(child.table.rows[0].cells[0].text.text(), "edited literal");
    assert!(child.table.rows[0].cells[0].data.is_none());
}

#[test]
fn historical_positional_structs_use_the_same_bounded_owner_lowering() {
    let body = serde_json::to_string(&TextBody::from_text("positional deep original")).unwrap();
    let empty = serde_json::to_string(&TextBody::default()).unwrap();
    let mut cell = format!("[{body},null,[null,null,null,null],null,null,null,null,1,1,false,false,null]");
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        // Table and Row use their historical sequence representation. Cell's
        // new blocks follow data/formula, leaving all old indexes unchanged.
        let table = format!("[[120],[[20,[{cell}]]]]");
        let block = format!(r#"{{"kind":"table","content":{{"id":1,"table":{table}}}}}"#);
        cell = format!("[{empty},null,[null,null,null,null],null,null,null,null,1,1,false,false,null,null,null,[{block}]]");
    }
    let kind = format!(r#"{{"type":"table","cols":[120],"rows":[[20,[{cell}]]]}}"#);
    let mut shape = format!("[700,\"deep leaf\",null,{kind}]");
    let child = serde_json::to_string(&Xfrm::new(0., 0., 120., 60.)).unwrap();
    for index in 0..64 {
        let kind = format!(r#"{{"type":"group","children":[{shape}],"child":{child}}}"#);
        shape = format!("[{},\"positional group\",null,{kind}]", 701 + index);
    }
    let json = format!(r#"["deckcraft",1,"synthetic positional",{{"slides":[{{"shapes":[{shape}]}}]}}]"#);
    let back = deckcraft_format::load(&raw_zip(json.as_bytes())).unwrap();
    let mut shape = &back.slides[0].shapes[0];
    for _ in 0..64 {
        let ShapeKind::Group { children, .. } = &shape.kind else { panic!("missing positional group") };
        assert_eq!(children.len(), 1);
        shape = &children[0];
    }
    let ShapeKind::Table(table) = &shape.kind else { panic!("missing positional table") };
    let mut table = table;
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        let CellBlock::Table(child) = &table.rows[0].cells[0].blocks[0] else { panic!("missing positional child") };
        assert_eq!(child.id, 1);
        table = &child.table;
    }
    assert_eq!(table.rows[0].cells[0].text.text(), "positional deep original");
    let reopened = deckcraft_format::load(&deckcraft_format::save(&back).unwrap()).unwrap();
    assert_eq!(reopened.slides[0].shapes, back.slides[0].shapes);
}
