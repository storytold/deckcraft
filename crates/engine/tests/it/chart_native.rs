//! Native saves must not discard an imported chart's opaque package parts.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_engine::cmd::{base64_decode, base64_encode};
use deckcraft_geom::Xfrm;
use deckcraft_model::chart::{Chart, ChartKept, ChartPart, ChartRel, ChartType};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, defaults};
use serde_json::json;

const WORKBOOK: &[u8] = b"original-synthetic-workbook\0\xff\x80";
const DRAWING: &[u8] = br#"<c:userShapes xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#;
const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("chart") };
    c
}

fn source() -> Vec<u8> {
    let mut p = defaults::new_presentation(None);
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape {
        id,
        name: "Owned chart".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 400.0, 300.0)),
        kind: ShapeKind::Chart(Box::new(Chart::sample(ChartType::Pie))),
        ..Default::default()
    }];
    // Start with our own chart XML, then give it two original, synthetic package parts.
    let mut p = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("import");
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    c.raw = c
        .raw
        .as_ref()
        .map(|raw| raw.replace("</c:chartSpace>", r#"<c:externalData r:id="rWorkbook"/><c:userShapes r:id="rDrawing"/></c:chartSpace>"#));
    c.kept = ChartKept {
        rels: vec![
            ChartRel {
                id: "rWorkbook".into(),
                rel_type: format!("{RT}/package"),
                target: "../embeddings/owned.xlsx".into(),
                part: "ppt/embeddings/owned.xlsx".into(),
                external: false,
            },
            ChartRel {
                id: "rDrawing".into(),
                rel_type: format!("{RT}/chartUserShapes"),
                target: "../drawings/owned.xml".into(),
                part: "ppt/drawings/owned.xml".into(),
                external: false,
            },
        ],
        parts: vec![
            ChartPart {
                name: "ppt/embeddings/owned.xlsx".into(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
                bytes: WORKBOOK.to_vec(),
                native_entry: None,
            },
            ChartPart {
                name: "ppt/drawings/owned.xml".into(),
                content_type: "application/vnd.openxmlformats-officedocument.drawingml.chartshapes+xml".into(),
                bytes: DRAWING.to_vec(),
                native_entry: None,
            },
        ],
    };
    deckcraft_pptx::export(&p).expect("PPTX fixture")
}

fn open(bytes: &[u8], name: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.openBytes", &json!({"name":name,"data":base64_encode(bytes)})).expect("open command");
    s
}

fn save(s: &mut Session, format: &str) -> Vec<u8> {
    let out = s.execute("file.saveBytes", &json!({"format":format})).expect("save command");
    base64_decode(out["data"].as_str().expect("data")).expect("base64")
}

#[test]
fn pptx_native_edit_save_reopen_keeps_chart_parts() {
    let mut s = open(&source(), "owned.pptx");
    let kept = chart(&s.doc().expect("doc").doc).kept.clone();
    assert_eq!(kept.rels.len(), 2, "fixture relationships must have been imported");
    assert_eq!(kept.parts.len(), 2);
    s.execute("file.properties", &json!({"title":"Edited title"})).expect("edit command");
    for _ in 0..2 {
        s = open(&save(&mut s, "deckcraft"), "owned.deckcraft");
        let p = &s.doc().expect("doc").doc;
        assert_eq!(p.props.title, "Edited title");
        assert_eq!(chart(p).kept, kept);
    }
    let bytes = save(&mut s, "pptx");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut payloads = vec![];
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut data = vec![];
        f.read_to_end(&mut data).expect("read");
        payloads.push((f.name().to_string(), data));
    }
    assert!(payloads.iter().any(|(n, d)| n.starts_with("ppt/embeddings/") && d == WORKBOOK));
    assert!(payloads.iter().any(|(n, d)| n.starts_with("ppt/drawings/") && d == DRAWING));
    let back = deckcraft_pptx::import(&bytes).expect("reopen PPTX");
    assert_eq!(chart(&back).kept.rels.len(), 2);
    for part in &kept.parts {
        assert!(chart(&back).kept.parts.iter().any(|p| p.content_type == part.content_type && p.bytes == part.bytes));
    }
}

#[test]
fn editing_chart_after_native_reopen_does_not_export_a_stale_workbook() {
    let mut s = open(&source(), "owned.pptx");
    s = open(&save(&mut s, "deckcraft"), "owned.deckcraft");
    let doc = s.doc_mut().expect("doc");
    let p = Arc::make_mut(&mut doc.doc);
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    assert_eq!(c.kept.rels.len(), 2);
    c.series[0].values[0] = Some(99.0);
    let bytes = save(&mut s, "pptx");
    let back = deckcraft_pptx::import(&bytes).expect("PPTX");
    assert_eq!(chart(&back).series[0].values[0], Some(99.0));
    assert!(chart(&back).kept.parts.is_empty(), "stale source workbook must not be attached to edited chart data");
}
