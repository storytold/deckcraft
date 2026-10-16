//! Independent hostile-input tests. Authored from source review; NOT RUN by reviewer.
use deckcraft_model::table::{CellBlock, CellTextBlock, MAX_NESTED_TABLE_DEPTH, NestedTable};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, Table, TextBody, Xfrm};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
fn package(json: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("presentation.json", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(json).unwrap();
    zip.finish().unwrap().into_inner()
}
fn manifest(shape: &str) -> String {
    format!(r#"{{"format":"deckcraft","version":1,"generator":"independent","presentation":{{"slides":[{{"shapes":[{shape}]}}]}}}}"#)
}
fn positional(groups: usize, depth: usize, mode: usize) -> Vec<u8> {
    let body = serde_json::to_string(&TextBody::from_text("positional leaf")).unwrap();
    let empty = serde_json::to_string(&TextBody::default()).unwrap();
    let mut rows = format!(r#"[{{"height":20,"cells":[{{"text":{body}}}]}}]"#);
    let mut table = format!(r#"{{"cols":[120],"rows":{rows}}}"#);
    for _ in 0..depth {
        let owner = if mode == 0 { format!(r#"{{"id":1,"table":{table}}}"#) } else { format!("[1,{table}]") };
        let block = if mode == 1 { format!(r#"{{"kind":"table","content":{owner}}}"#) } else { format!(r#"["table",{owner}]"#) };
        let cell = format!("[{empty},null,[null,null,null,null],null,null,null,null,1,1,false,false,null,null,null,[{block}]]");
        rows = format!("[[20,[{cell}]]]");
        table = format!("[[120],{rows}]");
    }
    let mut shape = format!(r#"[700,"leaf",null,{{"type":"table","cols":[120],"rows":{rows}}}]"#);
    let child = serde_json::to_string(&Xfrm::new(0., 0., 120., 60.)).unwrap();
    for n in 0..groups {
        shape = format!(r#"[{},"group",null,{{"type":"group","children":[{shape}],"child":{child}}}]"#, 701 + n);
    }
    package(manifest(&shape).as_bytes())
}
fn check_positional(mode: usize) {
    let back = deckcraft_format::load(&positional(64, MAX_NESTED_TABLE_DEPTH, mode)).unwrap();
    let mut shape = &back.slides[0].shapes[0];
    for _ in 0..64 {
        let ShapeKind::Group { children, .. } = &shape.kind else { panic!("group") };
        shape = &children[0];
    }
    let ShapeKind::Table(table) = &shape.kind else { panic!("table") };
    let mut table = table;
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        let CellBlock::Table(child) = &table.rows[0].cells[0].blocks[0] else { panic!("child") };
        table = &child.table;
    }
    assert_eq!(table.rows[0].cells[0].text.text(), "positional leaf");
    let reopened = deckcraft_format::load(&deckcraft_format::save(&back).unwrap()).unwrap();
    assert_eq!(reopened.slides[0].shapes, back.slides[0].shapes);
}
#[test]
fn adjacent_enum_sequence_with_map_owner_at_combined_maximum() {
    check_positional(0);
}
#[test]
fn map_enum_with_sequence_owner_at_combined_maximum() {
    check_positional(1);
}
#[test]
fn adjacent_enum_and_owner_sequences_at_combined_maximum() {
    check_positional(2);
}
#[test]
fn positional_nested_semantic_maximum_plus_one_refuses() {
    for mode in 0..3 {
        assert!(deckcraft_format::load(&positional(64, 17, mode)).is_err());
    }
}
fn document(table: Table) -> Presentation {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(70), kind: ShapeKind::Table(table), ..Default::default() }];
    p
}
fn conflicting_body() -> TextBody {
    let mut body = TextBody::from_text("conflicting");
    body.paragraphs[0].props.line_height = Some(deckcraft_model::text::LineHeight::Normal);
    body.paragraphs[0].props.line_spacing = Some(deckcraft_model::text::Spacing::Pts(12.));
    body
}
#[test]
fn nested_text_blocks_receive_line_height_validation() {
    let mut table = Table::new(1, 1, 120., 20.);
    table.rows[0].cells[0].blocks.push(CellBlock::Text(CellTextBlock { id: 1, body: conflicting_body() }));
    let p = document(table);
    assert!(deckcraft_model::line_height::validate(&p).is_err());
    assert!(deckcraft_format::save(&p).is_err());
}
#[test]
fn nested_table_canonical_text_receives_line_height_validation() {
    let mut child = Table::new(1, 1, 120., 20.);
    child.rows[0].cells[0].text = conflicting_body();
    let mut table = Table::new(1, 1, 120., 20.);
    table.rows[0].cells[0].blocks.push(CellBlock::Table(NestedTable { id: 1, table: Box::new(child) }));
    let p = document(table);
    assert!(deckcraft_model::line_height::validate(&p).is_err());
    assert!(deckcraft_format::save(&p).is_err());
}
#[test]
fn very_deep_reply_input_refuses_without_stack_overflow() {
    // Run alone in its own test process: this probes a separate recursion path,
    // rather than asserting a currently reproduced crash.
    let mut comment = "{}".to_string();
    for _ in 0..245 {
        comment = format!(r#"{{"replies":[{comment}]}}"#);
    }
    let json = format!(r#"{{"format":"deckcraft","version":1,"generator":"independent","presentation":{{"slides":[{{"comments":[{comment}]}}]}}}}"#);
    assert!(deckcraft_format::load(&package(json.as_bytes())).is_err());
}
