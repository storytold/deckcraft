//! Original command-driven ODP formula, native persistence and rendering witnesses.
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

use deckcraft_engine::{DocState, Session};
use deckcraft_model::{Cell, Presentation, ShapeId, ShapeKind, Table, table::FormulaOutcome};
use serde_json::json;

fn table(p: &Presentation) -> &Table {
    p.slides
        .iter()
        .flat_map(|slide| &slide.shapes)
        .find_map(|shape| match &shape.kind {
            ShapeKind::Table(table) => Some(table),
            _ => None,
        })
        .unwrap()
}
fn shape_id(p: &Presentation) -> ShapeId {
    p.slides.iter().flat_map(|slide| &slide.shapes).find(|shape| matches!(shape.kind, ShapeKind::Table(_))).unwrap().id
}
fn assign(s: &mut Session, id: ShapeId, lexical: &str) {
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"row":0,"col":0,
        "value":{"kind":"number","numberKind":{"kind":"float"},"lexical":lexical},"display":lexical}),
    )
    .unwrap();
}
fn native_source() -> Presentation {
    let mut s = Session::with_new();
    let id = ShapeId(s.execute("insert.table", &json!({"rows":1,"cols":3,"rect":[80,80,480,70]})).unwrap()["id"].as_u64().unwrap() as u32);
    s.execute("table.style", &json!({"id":id,"style":"none"})).unwrap();
    s.execute("table.options", &json!({"id":id,"headerRow":false,"bandedRows":false})).unwrap();
    assign(&mut s, id, "2");
    for (col, expr) in [(1, "=[.A1]*3"), (2, "=[.B1]+1")] {
        s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":col,"expression":expr,"profile":"exact-decimal-v1"})).unwrap();
    }
    s.doc().unwrap().doc.as_ref().clone()
}
fn outcome(cell: &Cell) -> &str {
    let FormulaOutcome::Number { lexical } = cell.formula.as_ref().unwrap().outcome() else { panic!("numeric formula expected") };
    lexical
}
fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().to_owned();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}
fn version(bytes: &[u8], value: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in parts(bytes) {
        let data = if name.ends_with(".xml") {
            String::from_utf8(data).unwrap().replace("version=\"1.3\"", &format!("version=\"{value}\"")).into_bytes()
        } else {
            data
        };
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn emit(label: &str, bytes: &[u8]) {
    let Some(dir) = std::env::var_os("DECKCRAFT_FORMULA_XML_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, data) in parts(bytes).into_iter().filter(|(name, _)| name.ends_with(".xml")) {
        std::fs::write(dir.join(format!("{label}-{}", name.replace('/', "-"))), data).unwrap();
    }
}

#[test]
fn actual_odp_edit_recalculates_then_native_reopen_and_odp_reimport_retain_current_formulas() {
    let native = native_source();
    let source = deckcraft_odp::export(&native).unwrap();
    for v in ["1.2", "1.3"] {
        let source = version(&source, v);
        let original = source.clone();
        emit(&format!("{v}-source"), &source);
        let imported = deckcraft_odp::import(&source).unwrap();
        assert!(!imported.tags.iter().any(|(key, _)| key == "odp:warning"));
        assert_eq!(outcome(table(&imported).cell(0, 1).unwrap()), "6");
        assert_eq!(outcome(table(&imported).cell(0, 2).unwrap()), "7");
        let id = shape_id(&imported);
        let mut s = Session::new();
        s.add_document(DocState::new(imported, None, "Original formulas".into()));
        let before = s.doc().unwrap().doc.clone();
        let opts = deckcraft_render::RenderOpts { scale: 0.5, ..Default::default() };
        let image = deckcraft_render::render_slide(&before, 0, &opts);
        assign(&mut s, id, "4");
        let edited = s.doc().unwrap().doc.clone();
        assert_eq!(outcome(table(&edited).cell(0, 1).unwrap()), "12");
        assert_eq!(outcome(table(&edited).cell(0, 2).unwrap()), "13");
        assert_ne!(image.pixels, deckcraft_render::render_slide(&edited, 0, &opts).pixels);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        s.execute("edit.redo", &json!({})).unwrap();
        let reopened = deckcraft_format::load(&deckcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        assert_eq!(table(&reopened), table(&edited));
        assert_eq!(deckcraft_render::render_slide(&reopened, 0, &opts).pixels, deckcraft_render::render_slide(&edited, 0, &opts).pixels);
        let output = deckcraft_odp::export(&reopened).unwrap();
        emit(&format!("{v}-edited-output"), &output);
        let back = deckcraft_odp::import(&output).unwrap();
        assert!(!back.tags.iter().any(|(key, _)| key == "odp:warning"));
        assert_eq!(outcome(table(&back).cell(0, 1).unwrap()), "12");
        assert_eq!(table(&back).cell(0, 1).unwrap().formula.as_ref().unwrap().expression(), "=[.A1]*3");
        assert_eq!(outcome(table(&back).cell(0, 2).unwrap()), "13");
        let retained = reopened.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
        assert_eq!(retained.data.as_slice(), original);
        assert_eq!(source, original);
        for format in ["pptx", "potx", "ppsx"] {
            let bytes = deckcraft_engine::cmd::file::save_bytes(&reopened, format).unwrap();
            let back = deckcraft_pptx::import(&bytes).unwrap();
            assert_eq!(table(&back), table(&reopened));
        }
    }
}

#[test]
fn errors_persist_through_native_odp_and_pptx_mappings() {
    let mut s = Session::new();
    s.add_document(DocState::new(native_source(), None, "Original error".into()));
    let id = shape_id(&s.doc().unwrap().doc);
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"literal"})).unwrap();
    let native = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let reopened = deckcraft_format::load(&native).unwrap();
    assert!(matches!(table(&reopened).cell(0, 1).unwrap().formula.as_ref().unwrap().outcome(), FormulaOutcome::Error { .. }));
    let back = deckcraft_odp::import(&deckcraft_odp::export(&reopened).unwrap()).unwrap();
    for col in [1, 2] {
        assert_eq!(table(&back).cell(0, col).unwrap().formula, table(&reopened).cell(0, col).unwrap().formula);
    }
    for format in ["pptx", "potx", "ppsx"] {
        let bytes = deckcraft_engine::cmd::file::save_bytes(&reopened, format).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(table(&back), table(&reopened));
    }
}
