//! Independent native boundary witnesses; all documents are contributor-original.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    Presentation, Shape, ShapeId, ShapeKind, Table, TextBody,
    table::{CellConstant, CellValue},
};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn fixture(kind: &str, lexical: String) -> Presentation {
    let scalar = if kind == "date" { CellValue::Date { lexical } } else { CellValue::Duration { lexical } };
    let mut table = Table::new(1, 2, 300.0, 40.0);
    for cell in &mut table.rows[0].cells {
        cell.text = TextBody::from_text("cached review label");
        cell.data = Some(Arc::new(CellConstant::from_display(scalar.clone(), &cell.text).unwrap()));
    }
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(722), kind: ShapeKind::Table(table), ..Default::default() }];
    p
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("table fixture") };
    table
}
fn package(json: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [("mimetype", deckcraft_format::MIMETYPE), ("presentation.json", json)] {
        archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(body.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
fn wire(p: &Presentation) -> serde_json::Value {
    serde_json::json!({"format":"deckcraft","version":1,"generator":"independent-review","presentation":p})
}

#[test]
fn native_escaped_temporal_length_is_decoded_and_never_downgraded_to_text() {
    for (kind, core) in [("date", "2004-02-29-00:00"), ("duration", "-P0007M")] {
        let exact = format!("{}\t{core}\r\n", " ".repeat(256 - core.len() - 3));
        assert_eq!(exact.len(), 256);
        let p = fixture(kind, exact.clone());
        let json = wire(&p).to_string().replace(' ', "\\u0020");
        let loaded = deckcraft_format::load(&package(&json)).unwrap();
        assert_eq!(table(&loaded), table(&p));
        let mut over = wire(&p);
        over["presentation"]["slides"][0]["shapes"][0]["kind"]["rows"][0]["cells"][0]["data"]["value"]["lexical"] = format!(" {exact}").into();
        assert!(deckcraft_format::load(&package(&over.to_string())).is_err());
        assert_eq!(table(&p).rows[0].cells[0].data.as_ref().unwrap().display_baseline(), &["cached review label"]);
    }
}

#[test]
fn native_reconciliation_drops_only_stale_temporal_authority_without_mutating_caller() {
    for (kind, lexical) in [("date", "2004-02-29+13:59"), ("duration", "P13MT89M")] {
        let mut p = fixture(kind, lexical.into());
        let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        let attached = t.rows[0].cells[0].data.clone().unwrap();
        t.rows[0].cells[0].text = TextBody::from_text("edited review label");
        let back = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert!(table(&back).rows[0].cells[0].data.is_none());
        assert_eq!(table(&back).rows[0].cells[0].text.text(), "edited review label");
        assert_eq!(table(&back).rows[0].cells[1].data, table(&p).rows[0].cells[1].data);
        assert!(Arc::ptr_eq(table(&p).rows[0].cells[0].data.as_ref().unwrap(), &attached));
        // The load boundary independently performs the same reconciliation.
        let loaded_direct = deckcraft_format::load(&package(&wire(&p).to_string())).unwrap();
        assert!(table(&loaded_direct).rows[0].cells[0].data.is_none());
        assert!(table(&loaded_direct).rows[0].cells[1].data.is_some());
    }
}

#[test]
fn native_save_charges_stale_reserved_temporal_metadata_before_reconciliation() {
    for (kind, core) in [("date", "2004-02-29"), ("duration", "P13M")] {
        let mut lexical = String::with_capacity(34 * 1024 * 1024);
        lexical.push_str(core);
        let scalar = if kind == "date" { CellValue::Date { lexical } } else { CellValue::Duration { lexical } };
        let mut p = fixture(kind, core.into());
        let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        t.rows[0].cells[0].data = Some(Arc::new(CellConstant::from_display(scalar, &t.rows[0].cells[0].text).unwrap()));
        t.rows[0].cells[0].text = TextBody::from_text("stale changed cache");
        let attached = t.rows[0].cells[0].data.clone().unwrap();
        assert!(deckcraft_format::save(&p).unwrap_err().to_string().contains("metadata"));
        assert!(Arc::ptr_eq(table(&p).rows[0].cells[0].data.as_ref().unwrap(), &attached));
        assert!(table(&p).rows[0].cells[1].data.is_some());
    }
}
