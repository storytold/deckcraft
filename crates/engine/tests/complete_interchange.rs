//! Contributor-original author/edit/reopen workflow across all three formats.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_engine::{DocState, Session};
use deckcraft_model::{
    Presentation, ShapeId, ShapeKind, Table,
    table::{CellBlock, TablePath},
};
use serde_json::json;
fn table(p: &Presentation) -> (ShapeId, &Table) {
    p.slides[0].shapes.iter().find_map(|shape| if let ShapeKind::Table(t) = &shape.kind { Some((shape.id, t)) } else { None }).unwrap()
}
fn same_table(actual: &Table, expected: &Table) {
    assert_eq!(actual.cols, expected.cols);
    assert_eq!(actual.rows.len(), expected.rows.len());
    for (actual, expected) in actual.rows.iter().zip(&expected.rows) {
        assert_eq!(actual.height, expected.height);
        assert_eq!(actual.cells.len(), expected.cells.len());
        for (actual, expected) in actual.cells.iter().zip(&expected.cells) {
            assert_eq!(actual.text.text(), expected.text.text());
            assert_eq!(actual.data, expected.data);
            assert_eq!(actual.formula, expected.formula);
            assert_eq!(actual.number_format, expected.number_format);
            assert_eq!(
                (actual.grid_span, actual.row_span, actual.h_merge, actual.v_merge),
                (expected.grid_span, expected.row_span, expected.h_merge, expected.v_merge)
            );
            assert_eq!(actual.blocks.len(), expected.blocks.len());
            for (actual, expected) in actual.blocks.iter().zip(&expected.blocks) {
                assert_eq!(actual.id(), expected.id());
                match (actual, expected) {
                    (CellBlock::Text(a), CellBlock::Text(b)) => assert_eq!(a.body.text(), b.body.text()),
                    (CellBlock::Table(a), CellBlock::Table(b)) => same_table(&a.table, &b.table),
                    _ => panic!("content order changed"),
                }
            }
        }
    }
}
fn same_comments(actual: &Presentation, expected: &Presentation) {
    let mut normalized = actual.slides[0].comments.clone();
    fn walk(actual: &mut [deckcraft_model::Comment], expected: &[deckcraft_model::Comment], a: &Presentation, e: &Presentation) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter_mut().zip(expected) {
            assert_eq!(actual.attachments.len(), expected.attachments.len());
            for (actual, expected) in actual.attachments.iter_mut().zip(&expected.attachments) {
                let am = a.media(actual.media).unwrap();
                let em = e.media(expected.media).unwrap();
                assert_eq!(am.content_type, em.content_type);
                assert_eq!(am.data, em.data);
                actual.media = expected.media;
            }
            walk(&mut actual.replies, &expected.replies, a, e);
        }
    }
    walk(&mut normalized, &expected.slides[0].comments, actual, expected);
    assert_eq!(normalized, expected.slides[0].comments);
}
fn source() -> Presentation {
    let mut s = Session::with_new();
    let id = s.execute("insert.table", &json!({"rows":2,"cols":3,"rect":[50,50,600,150]})).unwrap()["id"].clone();
    s.execute(
        "table.setCellValue",
        &json!({"id":id,"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"12.5"},"display":"12.5"}),
    )
    .unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":0,"col":1,"expression":"=IF([.A1]>0;SUM([.A1];2.5)/3;0)"})).unwrap();
    s.execute("table.setCellNumberFormat",&json!({"id":id,"row":0,"col":1,"format":{"locale":"fr-BE","style":{"kind":"number","decimalPlaces":2,"minDecimalPlaces":2,"minIntegerDigits":1,"grouping":true}}})).unwrap();
    s.execute("text.set", &json!({"id":id,"cell":[1,0],"text":"Before after"})).unwrap();
    s.execute("text.edit", &json!({"id":id,"cell":[1,0],"at":[0,7]})).unwrap();
    s.execute("table.insertNested", &json!({"id":id,"rows":1,"cols":1})).unwrap();
    s.execute("text.insert", &json!({"text":"Nested content"})).unwrap();
    let root = s.execute("comment.add", &json!({"text":"Original thread","anchor":{"kind":"slide"}})).unwrap()["commentId"].clone();
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"Reply"})).unwrap()["commentId"].clone();
    s.execute("comment.reply", &json!({"commentId":root,"replyTo":reply,"text":"Nested reply"})).unwrap();
    s.execute("comment.task.set", &json!({"commentId":root,"task":{"title":"Original task","complete":"25%"}})).unwrap();
    s.execute(
        "comment.attachment.import",
        &json!({"commentId":root,"name":"original.txt","contentType":"text/plain","data":"Y29udHJpYnV0b3Igb3JpZ2luYWw="}),
    )
    .unwrap();
    s.execute("comment.close", &json!({"commentId":root})).unwrap();
    s.execute("slide.notes", &json!({"text":"Original speaker notes"})).unwrap();
    s.doc().unwrap().doc.as_ref().clone()
}
fn encode(p: &Presentation, format: &str) -> Vec<u8> {
    deckcraft_engine::cmd::file::save_bytes(p, format).unwrap()
}
fn decode(bytes: &[u8], format: &str) -> Presentation {
    match format {
        "odp" => deckcraft_odp::import(bytes).unwrap(),
        "pptx" => deckcraft_pptx::import(bytes).unwrap(),
        _ => deckcraft_format::load(bytes).unwrap(),
    }
}
#[test]
fn real_edits_after_native_odp_and_pptx_reopen_keep_nested_owner_formula_comments_files_and_undo() {
    let source = source();
    for format in ["deckcraft", "odp", "pptx"] {
        let bytes = encode(&source, format);
        let reopened = decode(&bytes, format);
        assert!(!reopened.tags.iter().any(|(k, _)| k == "odp:warning" || k == "pptx:comment-warning"), "{format}: {:?}", reopened.tags);
        same_table(table(&reopened).1, table(&source).1);
        if let Ok(dir) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
            let opts = deckcraft_render::RenderOpts { scale: 0.75, ..Default::default() };
            let actual = deckcraft_render::render_slide(&reopened, 0, &opts);
            let expected = deckcraft_render::render_slide(&source, 0, &opts);
            for (name, image) in [(format!("reopened-{format}.png"), &actual), ("source.png".into(), &expected)] {
                image::save_buffer(std::path::Path::new(&dir).join(name), &image.to_straight(), image.width, image.height, image::ColorType::Rgba8)
                    .unwrap();
            }
            let changed = actual.pixels.iter().zip(&expected.pixels).filter(|(a, b)| a != b).count();
            eprintln!("Original workflow {format}: changed RGBA channels {changed}/{}", actual.pixels.len());
        }

        same_comments(&reopened, &source);
        assert_eq!(reopened.slides[0].notes.text(), source.slides[0].notes.text(), "{format}");
        let mut s = Session::new();
        s.add_document(DocState::new(reopened, None, "Original workflow".into()));
        let (id, _) = table(&s.doc().unwrap().doc);
        let before = s.doc().unwrap().doc.clone();
        s.execute(
            "table.setCellValue",
            &json!({"id":id,"path":[],"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"27.5"},"display":"27.5"}),
        )
        .unwrap();
        assert_eq!(table(&s.doc().unwrap().doc).1.cell(0, 1).unwrap().text.text(), "10,00");
        let edited = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, edited);
        let back = decode(&encode(&edited, format), format);
        same_table(table(&back).1, table(&edited).1);
        same_comments(&back, &edited);
        let CellBlock::Table(child) = &table(&back).1.cell(1, 0).unwrap().blocks[0] else { panic!("nested table") };
        assert_eq!(child.table.at_path(&TablePath::default()).unwrap().cell(0, 0).unwrap().text.text(), "Nested content");
        let attachment = &back.slides[0].comments[0].attachments[0];
        assert_eq!(back.media(attachment.media).unwrap().data.as_slice(), b"contributor original");
        if let Ok(dir) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("edited-workflow.{format}")), encode(&edited, format)).unwrap();
        }
    }
}

#[test]
fn merged_grid_keeps_outside_formulas_through_all_formats_and_checked_axis_edits() {
    let mut s = Session::with_new();
    let id = s.execute("insert.table", &json!({"rows":2,"cols":3,"rect":[50,50,600,150]})).unwrap()["id"].clone();
    s.execute("text.set", &json!({"id":id,"cell":[0,0],"text":"first"})).unwrap();
    s.execute("text.set", &json!({"id":id,"cell":[0,1],"text":"second"})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":1,"col":2,"expression":"=1+2","profile":"exact-decimal-v1"})).unwrap();
    s.execute("table.merge", &json!({"id":id,"from":[0,0],"to":[0,1]})).unwrap();
    s.execute("table.setCellFormula", &json!({"id":id,"row":1,"col":1,"expression":"=SUM([.B1];4)"})).unwrap();
    let original = s.doc().unwrap().doc.clone();
    for format in ["deckcraft", "odp", "pptx"] {
        let reopened = decode(&encode(&original, format), format);
        same_table(table(&reopened).1, table(&original).1);
        assert!(!reopened.tags.iter().any(|(key, _)| key == "odp:warning"), "merged formula graph must stay authoritative");
        let (id, _) = table(&reopened);
        let mut s = Session::new();
        s.add_document(DocState::new(reopened, None, "Merged workflow".into()));
        let before = s.doc().unwrap().doc.clone();
        s.execute("table.insertRowAbove", &json!({"id":id,"row":0})).unwrap();
        assert_eq!(table(&s.doc().unwrap().doc).1.cell(2, 1).unwrap().formula.as_ref().unwrap().expression(), "=SUM([.B2];4)");
        assert_eq!(table(&s.doc().unwrap().doc).1.cell(2, 1).unwrap().text.text(), "4");
        assert_eq!(table(&s.doc().unwrap().doc).1.cell(2, 2).unwrap().formula.as_ref().unwrap().profile(), "exact-decimal-v1");
        let edited = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, edited);
        let back = decode(&encode(&edited, format), format);
        same_table(table(&back).1, table(&edited).1);
        if let Ok(dir) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("merged-workflow.{format}")), encode(&edited, format)).unwrap();
        }
    }
}
