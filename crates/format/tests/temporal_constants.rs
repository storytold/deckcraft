//! Native temporal constants preserve exact labels and reject corrupt authority.
use deckcraft_model::{
    Presentation, Shape, ShapeId, ShapeKind, Table, TextBody,
    table::{CellConstant, CellValue},
};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn presentation(value: CellValue) -> Presentation {
    let mut p = Presentation::default();
    let mut table = Table::new(1, 1, 100.0, 40.0);
    let cell = &mut table.rows[0].cells[0];
    cell.text = TextBody::from_text("cached 漢\nsecond label");
    cell.text.paragraphs[0].runs[0].props.bold = Some(true);
    cell.data = Some(Arc::new(CellConstant::from_display(value, &cell.text).unwrap()));
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(71), kind: ShapeKind::Table(table), ..Default::default() }];
    p
}
fn package(json: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [("mimetype", deckcraft_format::MIMETYPE), ("presentation.json", json)] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn cell(p: &Presentation) -> &deckcraft_model::Cell {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("table") };
    &table.rows[0].cells[0]
}
#[test]
fn temporal_native_roundtrip_keeps_independent_rich_display_and_exact_lexical() {
    for value in
        [CellValue::Date { lexical: " \t2024-02-29+00:00\r\n".into() }, CellValue::Duration { lexical: "-P0001Y02M03DT004H05M06.700S".into() }]
    {
        let p = presentation(value);
        let before = p.clone();
        let back = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert_eq!(cell(&back), cell(&p));
        assert_eq!(p, before);
    }
}
#[test]
fn malformed_temporal_native_constants_never_use_legacy_numeric_repair() {
    for value in [
        serde_json::json!({"kind":"date","lexical":"1900-02-29"}),
        serde_json::json!({"kind":"duration","lexical":"PT"}),
        serde_json::json!({"kind":"duration","lexical":null}),
        serde_json::json!({"kind":"duration","lexical":format!("P{}D","0".repeat(255))}),
    ] {
        let p = presentation(CellValue::Date { lexical: "2024-02-29".into() });
        let mut native = serde_json::json!({"format":"deckcraft","version":1,"generator":"original-test","presentation":p});
        native["presentation"]["slides"][0]["shapes"][0]["kind"]["rows"][0]["cells"][0]["data"]["value"] = value;
        native["presentation"]["slides"][0]["shapes"][0]["xfrm"]["x"] = serde_json::Value::Null;
        assert!(deckcraft_format::load(&package(&native.to_string())).is_err());
    }
}
