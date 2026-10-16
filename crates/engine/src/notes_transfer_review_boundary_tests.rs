//! Clipboard support is not review support: rich/master/page-only changes must refuse.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{
    Session,
    compare::{self, Change, ChangeKind},
    notes_transfer_tests::{fixture, install, refused},
};
use deckcraft_model::{Background, Fill, MediaId, MediaItem, ShapeId, Size, style::PictureFill};
use serde_json::json;
use std::sync::Arc;

#[test]
fn rich_master_size_field_and_media_only_notes_changes_never_compare_equal_or_adopt() {
    for mode in 0..7 {
        let mut source = fixture();
        if mode == 5 {
            source.media.push(MediaItem {
                id: MediaId(995),
                name: "original.bin".into(),
                content_type: "application/octet-stream".into(),
                data: Arc::new(vec![1, 2, 3]),
                link: None,
            });
            Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().background =
                Some(Background::Fill { fill: Fill::Picture(PictureFill { media: MediaId(995), ..Default::default() }) });
        }
        let mut revised = source.clone();
        match mode {
            0 => Arc::make_mut(&mut revised.slides[0]).notes.paragraphs[0].runs[0].props.italic = Some(true),
            1 => Arc::make_mut(&mut revised.notes_masters[0].master).name = "Edited notes master".into(),
            2 => Arc::make_mut(&mut revised.slides[0]).notes_page.as_mut().unwrap().size = Some(Size::new(420., 620.)),
            3 => revised.notes_masters[0].fields.footer = Some("Edited footer".into()),
            4 => Arc::make_mut(&mut revised.slides[0]).notes_page.as_mut().unwrap().shapes[1].xfrm.as_mut().unwrap().x += 10.,
            5 => revised.media[0].data = Arc::new(vec![4, 5, 6]),
            _ => revised.notes_masters[0].size = Size::new(405., 650.),
        }
        assert_eq!(source.slides[0].notes.text(), revised.slides[0].notes.text());
        let error = compare::diff_with(&source, &revised, false, &Default::default()).unwrap_err().to_string();
        assert!(error.contains("structured notes"), "{error}");
        assert!(!compare::diff(&source, &revised, false).is_empty());
        let mut destination = source.clone();
        let before = destination.clone();
        let id = destination.slides[0].id;
        let error = compare::adopt(&mut destination, &revised, &revised.slides[0], Some(id)).unwrap_err().to_string();
        assert!(error.contains("structured notes"));
        assert_eq!(destination, before);
        let change = Change { kind: ChangeKind::Changed, slide: 0, revised: Some(0), details: vec!["notes".into()] };
        let error = compare::accept(&mut destination, &revised, &change).unwrap_err().to_string();
        assert!(error.contains("structured notes"));
        assert_eq!(destination, before);
        let mut session = Session::with_new();
        install(&mut session, source);
        let bytes = deckcraft_format::save(&revised).unwrap();
        refused(&mut session, "review.compare", json!({"data":crate::cmd::base64_encode(&bytes),"name":"notes.deckcraft"}), "structured notes");
    }
}
#[test]
fn deletion_only_review_cannot_bypass_structured_notes_admission() {
    let source = fixture();
    let mut destination = source.clone();
    let before = destination.clone();
    let change = Change { kind: ChangeKind::Deleted, slide: 0, revised: None, details: vec!["deleted".into()] };
    let error = compare::accept(&mut destination, &source, &change).unwrap_err().to_string();
    assert!(error.contains("structured notes"), "{error}");
    assert_eq!(destination, before);
}
#[test]
fn copy_primary_body_object_and_same_document_owner_checks_remain_in_force() {
    let mut session = Session::with_new();
    install(&mut session, fixture());
    session.execute("view.notesMaster", &json!({"master":0})).unwrap();
    session.doc_mut().unwrap().selection.shapes = vec![ShapeId(910)];
    refused(&mut session, "edit.copy", json!({}), "primary notes body");
    session.execute("view.closeMaster", &json!({})).unwrap();
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.notes_masters[0].fields.footer = Some("Live edit".into());
            Ok(())
        })
        .unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.notes_masters.len(), 3);
    assert_eq!(doc.notes_fields(&doc.slides[1]).footer.as_deref(), Some("Live edit"));
    assert_eq!(doc.slides[0].notes_page.as_ref().unwrap().master, doc.slides[1].notes_page.as_ref().unwrap().master);
    assert_ne!(doc.slides[0].id, doc.slides[1].id);
    assert_eq!(doc.slides[1].notes, fixture().slides[0].notes);
}
