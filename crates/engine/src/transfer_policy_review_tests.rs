//! Independent R11 admission regressions. All document content is contributor-original.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use deckcraft_model::{notes::NotesPage, *};
use serde_json::{Value, json};

use crate::{Session, transfer_policy};

fn artwork(warning: bool) -> Presentation {
    let mut doc = Presentation::default();
    Arc::make_mut(&mut doc.slides[0]).shapes = vec![Shape {
        id: ShapeId(30_000),
        name: "Independent transfer artwork".into(),
        text: Some(TextBody::from_text("Independent transfer words")),
        xfrm: Some(Xfrm::new(20., 30., 150., 50.)),
        ..Default::default()
    }];
    if warning {
        // No imported archive: warning ownership alone must be sufficient.
        doc.tags.push(("odp:warning".into(), "Independent unsupported semantic property".into()));
    }
    doc.fix_next_id();
    doc
}

fn notes(implicit: bool) -> Presentation {
    let mut doc = artwork(false);
    let master = Arc::new(Master { id: MasterId(30_010), ..Default::default() });
    if implicit {
        doc.notes_master = Some(master);
    } else {
        doc.notes_masters.push(notes::NotesMaster { master, ..Default::default() });
    }
    let slide = Arc::make_mut(&mut doc.slides[0]);
    slide.notes = TextBody::from_text("Canonical independent notes");
    slide.notes_page = Some(NotesPage {
        master: (!implicit).then_some(MasterId(30_010)),
        body_shape: Some(ShapeId(30_011)),
        shapes: vec![Shape { id: ShapeId(30_011), ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }), ..Default::default() }],
        ..Default::default()
    });
    assert!(doc.notes_page_error(&doc.slides[0]).is_none());
    doc.fix_next_id();
    doc
}

fn install(session: &mut Session, source: Presentation) {
    session
        .edit_resources(|doc, _| {
            *doc = source;
            Ok(())
        })
        .unwrap();
}

fn copy(session: &mut Session, whole_slide: bool) {
    let scope = if whole_slide { json!({"scope":"slides"}) } else { json!({}) };
    session.execute("edit.selectAll", &scope).unwrap();
    session.execute("edit.copy", &scope).unwrap();
}

fn refused_without_mutation(session: &mut Session, command: &str, params: Value, reason: &str) {
    let state = session.doc().unwrap();
    let doc = state.doc.clone();
    let selection = state.selection.clone();
    let history = (state.history.undo.len(), state.history.redo.len());
    let revision = state.revision;
    let clipboard = format!("{:?}", session.clipboard);
    let journal = session.journal.len();
    let requests = session.ui_requests.len();
    let error = session.execute(command, &params).unwrap_err().to_string();
    assert!(error.contains(reason), "{command}: {error}");
    let state = session.doc().unwrap();
    assert!(Arc::ptr_eq(&state.doc, &doc), "{command} published a document");
    assert_eq!(state.selection, selection);
    assert_eq!((state.history.undo.len(), state.history.redo.len()), history);
    assert_eq!(state.revision, revision);
    assert_eq!(format!("{:?}", session.clipboard), clipboard, "{command} changed clipboard or paste offset");
    assert_eq!(session.journal.len(), journal);
    assert_eq!(session.ui_requests.len(), requests);
}

#[test]
fn independent_every_normal_paste_special_lane_refuses_warned_objects_and_slides_atomically() {
    for whole_slide in [false, true] {
        for mode in ["destinationTheme", "useDestinationTheme", "keepSource", "keepSourceFormatting"] {
            let mut session = Session::with_new();
            install(&mut session, artwork(true));
            copy(&mut session, whole_slide);
            session.execute("file.new", &json!({})).unwrap();
            refused_without_mutation(&mut session, "edit.pasteSpecial", json!({"as":mode}), "unattributed ODP loss");
        }
    }
}

#[test]
fn independent_clipboard_loss_receipt_survives_source_changes_and_close() {
    let mut session = Session::with_new();
    install(&mut session, artwork(true));
    copy(&mut session, false);
    session
        .edit_resources(|doc, _| {
            doc.tags.clear();
            Ok(())
        })
        .unwrap();
    session.execute("file.close", &json!({})).unwrap();
    session.execute("file.new", &json!({})).unwrap();
    refused_without_mutation(&mut session, "edit.paste", json!({}), "unattributed ODP loss");
}

#[test]
fn independent_reopening_identical_native_document_has_a_different_clipboard_origin() {
    let mut session = Session::with_new();
    let source = artwork(true);
    let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
    install(&mut session, source);
    let original_uid = session.doc().unwrap().uid;
    copy(&mut session, true);
    session.execute("file.close", &json!({})).unwrap();
    session.execute("file.openBytes", &json!({"name":"independent.deckcraft", "data":data})).unwrap();
    assert_ne!(session.doc().unwrap().uid, original_uid);
    refused_without_mutation(&mut session, "edit.paste", json!({}), "unattributed ODP loss");
}

#[test]
fn independent_warning_bearing_destination_does_not_authorize_foreign_clipboard() {
    let mut session = Session::with_new();
    install(&mut session, artwork(true));
    copy(&mut session, false);
    session.execute("file.new", &json!({})).unwrap();
    install(&mut session, artwork(true));
    refused_without_mutation(&mut session, "edit.paste", json!({}), "unattributed ODP loss");
}

#[test]
fn independent_failed_notes_copy_and_cut_preserve_previous_clipboard_and_document() {
    for command in ["edit.copy", "edit.cut"] {
        let mut session = Session::with_new();
        install(&mut session, artwork(false));
        copy(&mut session, false);
        let mut invalid = notes(false);
        Arc::make_mut(&mut invalid.slides[0]).notes_page.as_mut().unwrap().shapes.clear();
        install(&mut session, invalid);
        session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
        refused_without_mutation(&mut session, command, json!({"scope":"slides"}), "notes body frame is missing");
    }
}

#[test]
fn independent_same_document_notes_paste_rechecks_deleted_named_master_atomically() {
    let mut session = Session::with_new();
    install(&mut session, notes(false));
    copy(&mut session, true);
    session
        .edit_resources(|doc, _| {
            doc.notes_masters.clear();
            Ok(())
        })
        .unwrap();
    refused_without_mutation(&mut session, "edit.paste", json!({}), "missing master");
}

#[test]
fn independent_same_document_object_paste_rechecks_the_captured_warning() {
    let mut session = Session::with_new();
    install(&mut session, artwork(true));
    copy(&mut session, false);
    session
        .edit_resources(|doc, _| {
            doc.tags.clear();
            Ok(())
        })
        .unwrap();
    refused_without_mutation(&mut session, "edit.pasteSpecial", json!({"as":"keepSource"}), "guard was cleared");
}

#[test]
fn independent_clipboard_notes_body_reference_is_validated_before_publishing() {
    let source = notes(false);
    let origin = transfer_policy::capture(&source, 123, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut stale = source.slides[0].as_ref().clone();
    stale.notes_page.as_mut().unwrap().body_shape = Some(ShapeId(99_999));
    let error = transfer_policy::check_clipboard(&origin, &source, 123, std::iter::once(&stale)).unwrap_err().to_string();
    assert!(error.contains("notes body frame is missing"), "{error}");
}

#[test]
fn independent_same_document_implicit_master_uses_the_current_live_fallback() {
    let mut session = Session::with_new();
    install(&mut session, notes(true));
    copy(&mut session, true);
    // Implicit inheritance changes for original and pasted pages together. No
    // explicit missing-owner reference is created by deleting this fallback.
    session
        .edit_resources(|doc, _| {
            doc.notes_master = None;
            Ok(())
        })
        .unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.slides.len(), 2);
    assert!(doc.slides.iter().all(|slide| doc.notes_page_error(slide).is_none()));
}

#[test]
fn independent_legacy_fallback_notes_transfer_without_aliasing_equal_destination_master() {
    let mut session = Session::with_new();
    install(&mut session, notes(true));
    copy(&mut session, true);
    session.execute("file.new", &json!({})).unwrap();
    install(&mut session, notes(true));
    let legacy = session.doc().unwrap().doc.notes_master.as_ref().unwrap().clone();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.slides.len(), 2);
    assert!(Arc::ptr_eq(doc.notes_master.as_ref().unwrap(), &legacy));
    assert!(doc.slides[0].notes_page.as_ref().unwrap().master.is_none());
    let copied = doc.slides[1].notes_page.as_ref().unwrap().master.unwrap();
    assert_ne!(copied, legacy.id);
    assert_eq!(doc.notes_masters.len(), 1);
    assert_eq!(doc.notes_masters[0].master.id, copied);
    assert!(!Arc::ptr_eq(&doc.notes_masters[0].master, &legacy));
    assert!(doc.notes_page_error(&doc.slides[1]).is_none());
}

#[test]
fn independent_explicit_text_only_extraction_remains_available_for_guarded_object_clipboard() {
    for (command, params) in
        [("edit.pasteSpecial", json!({"as":"text"})), ("edit.pasteSpecial", json!({"as":"keepTextOnly"})), ("edit.pasteText", json!({}))]
    {
        let mut session = Session::with_new();
        install(&mut session, artwork(true));
        copy(&mut session, false);
        session.execute("file.new", &json!({})).unwrap();
        session.execute(command, &params).unwrap();
        let doc = &session.doc().unwrap().doc;
        assert!(doc.slides[0].shapes.iter().any(|shape| shape.text.as_ref().is_some_and(|text| text.text() == "Independent transfer words")));
        assert!(!doc.tags.iter().any(|(key, _)| key == "odp:warning"));
    }
}

#[test]
fn independent_artwork_from_structured_notes_document_can_transfer_without_the_notes_page() {
    let mut session = Session::with_new();
    install(&mut session, notes(false));
    copy(&mut session, false);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    assert!(session.doc().unwrap().doc.slides[0].shapes.iter().any(|shape| shape.name == "Independent transfer artwork"));
    assert!(session.doc().unwrap().doc.slides[0].notes_page.is_none());
}

#[test]
fn independent_policy_rejects_deep_notes_before_recursive_model_validation() {
    let mut doc = notes(false);
    let mut shape = Shape { id: ShapeId(40_000), ..Default::default() };
    for depth in 0..66 {
        shape = Shape {
            id: ShapeId(40_001 + depth),
            kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0., 0., 1., 1.) },
            ..Default::default()
        };
    }
    Arc::make_mut(&mut doc.slides[0]).notes_page.as_mut().unwrap().shapes.push(shape);
    let error = transfer_policy::capture(&doc, 99, doc.slides.iter().map(Arc::as_ref)).unwrap_err().to_string();
    assert!(error.contains("notes dependency nesting exceeds limit"), "{error}");
}

#[test]
fn independent_direct_accept_cannot_adopt_warned_revisions() {
    let mut target = artwork(false);
    let mut revised = artwork(true);
    Arc::make_mut(&mut revised.slides[0]).notes = TextBody::from_text("Independent revised notes");
    let before = target.clone();
    let changes = crate::compare::diff(&target, &revised, false);
    assert!(!changes.is_empty());
    let error = crate::compare::accept(&mut target, &revised, &changes[0]).unwrap_err().to_string();
    assert!(error.contains("unattributed ODP loss"), "{error}");
    assert_eq!(target, before);
}
