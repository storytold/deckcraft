//! Synthetic, contributor-original public transfer guard regressions; no private documents.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use deckcraft_model::{
    notes::{NotesFields, NotesMaster, NotesPage},
    *,
};
use serde_json::json;

use crate::{
    Session,
    transfer_policy::{self, MAX_POLICY_PAGES},
};

fn warned() -> Presentation {
    let mut p = Presentation::default();
    p.tags.push(("odp:warning".into(), "Synthetic unattributed source loss".into()));
    p.tags.push(("odp:imported".into(), "1".into()));
    p
}

fn structured(footer: &str) -> Presentation {
    let mut p = Presentation::default();
    let body = Shape {
        id: ShapeId(910),
        ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
        xfrm: Some(Xfrm::new(40., 400., 480., 240.)),
        ..Default::default()
    };
    p.notes_masters = vec![NotesMaster {
        master: Arc::new(Master { id: MasterId(900), shapes: vec![body], ..Default::default() }),
        size: Size::new(612., 792.),
        fields: NotesFields { footer: Some(footer.into()), ..Default::default() },
    }];
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.notes = TextBody::from_text("Canonical notes sentinel");
    slide.notes_page = Some(NotesPage {
        master: Some(MasterId(900)),
        body_shape: Some(ShapeId(901)),
        shapes: vec![Shape { id: ShapeId(901), ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }), ..Default::default() }],
        ..Default::default()
    });
    assert!(p.notes_page_error(&p.slides[0]).is_none());
    p.fix_next_id();
    p
}

fn install(session: &mut Session, presentation: Presentation) {
    session
        .edit_resources(|doc, _| {
            *doc = presentation;
            Ok(())
        })
        .unwrap();
}

fn select_copy_slides(session: &mut Session) {
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
}

fn unchanged_after_error(session: &mut Session, command: &str, args: serde_json::Value, phrase: &str) {
    let state = session.doc().unwrap();
    let before = state.doc.clone();
    let selection = state.selection.clone();
    let history = (state.history.undo.len(), state.history.redo.len());
    let clipboard = format!("{:?}", session.clipboard);
    let journal = session.journal.len();
    let error = session.execute(command, &args).unwrap_err().to_string();
    assert!(error.contains(phrase), "{error}");
    let after = session.doc().unwrap();
    assert!(Arc::ptr_eq(&before, &after.doc), "failed {command} published a document");
    assert_eq!(after.selection, selection);
    assert_eq!((after.history.undo.len(), after.history.redo.len()), history);
    assert_eq!(format!("{:?}", session.clipboard), clipboard);
    assert_eq!(session.journal.len(), journal);
}

#[test]
fn helper_loss_signal_needs_no_source_archive_and_survives_clear_new_document() {
    let source = warned();
    assert!(source.media.is_empty());
    let origin = transfer_policy::capture(&source, 7, source.slides.iter().map(Arc::as_ref)).unwrap();
    let fresh = Presentation::default();
    let error = transfer_policy::check_clipboard(&origin, &fresh, 8, source.slides.iter().map(Arc::as_ref)).unwrap_err().to_string();
    assert!(error.contains("unattributed ODP loss"));
    assert!(transfer_policy::check_external(&source, source.slides.iter().map(Arc::as_ref)).is_err());
    assert!(transfer_policy::check_clipboard(&origin, &source, 7, source.slides.iter().map(Arc::as_ref)).is_ok());
    let mut cleared = source.clone();
    cleared.tags.clear();
    assert!(
        transfer_policy::check_clipboard(&origin, &cleared, 7, source.slides.iter().map(Arc::as_ref))
            .unwrap_err()
            .to_string()
            .contains("guard was cleared")
    );
    // A real UID zero is still distinct from the uncaptured default origin.
    let origin = transfer_policy::capture(&source, 0, source.slides.iter().map(Arc::as_ref)).unwrap();
    assert!(transfer_policy::check_clipboard(&origin, &source, 0, source.slides.iter().map(Arc::as_ref)).is_ok());
}

#[test]
fn helper_structured_notes_refuses_numeric_alias_and_implicit_fallback_cross_document() {
    let source = structured("SOURCE");
    let destination = structured("DESTINATION");
    let origin = transfer_policy::capture(&source, 1, source.slides.iter().map(Arc::as_ref)).unwrap();
    assert_ne!(source.notes_fields(&source.slides[0]), destination.notes_fields(&destination.slides[0]));
    assert!(destination.notes_page_error(&source.slides[0]).is_none(), "numeric alias otherwise looks valid");
    let error = transfer_policy::check_clipboard(&origin, &destination, 2, source.slides.iter().map(Arc::as_ref)).unwrap_err().to_string();
    assert!(error.contains("structured notes"));
    let mut implicit = source.clone();
    implicit.notes_master = Some(implicit.notes_masters[0].master.clone());
    Arc::make_mut(&mut implicit.slides[0]).notes_page.as_mut().unwrap().master = None;
    assert!(
        transfer_policy::check_external(&implicit, implicit.slides.iter().map(Arc::as_ref)).unwrap_err().to_string().contains("structured notes")
    );
}

#[test]
fn same_document_notes_admission_revalidates_current_master_and_body() {
    let source = structured("SOURCE");
    let origin = transfer_policy::capture(&source, 1, source.slides.iter().map(Arc::as_ref)).unwrap();
    assert!(transfer_policy::check_clipboard(&origin, &source, 1, source.slides.iter().map(Arc::as_ref)).is_ok());
    let mut missing = source.clone();
    missing.notes_masters.clear();
    assert!(
        transfer_policy::check_clipboard(&origin, &missing, 1, source.slides.iter().map(Arc::as_ref))
            .unwrap_err()
            .to_string()
            .contains("missing master")
    );
    let mut invalid_body = source.clone();
    Arc::make_mut(&mut invalid_body.slides[0]).notes_page.as_mut().unwrap().shapes[0].text = Some(TextBody::from_text("duplicate"));
    assert!(
        transfer_policy::check_clipboard(&origin, &source, 1, invalid_body.slides.iter().map(Arc::as_ref))
            .unwrap_err()
            .to_string()
            .contains("duplicates canonical")
    );
}

#[test]
fn policy_page_tag_and_repeated_notes_work_limits_have_cheap_bounded_fixtures() {
    let plain = Presentation::default();
    let page = plain.slides[0].as_ref();
    assert!(transfer_policy::check_external(&plain, std::iter::repeat_n(page, MAX_POLICY_PAGES)).is_ok());
    assert!(
        transfer_policy::check_external(&plain, std::iter::repeat_n(page, MAX_POLICY_PAGES + 1))
            .unwrap_err()
            .to_string()
            .contains("too many selected pages")
    );
    let mut tags = plain.clone();
    tags.tags = vec![(String::new(), String::new()); 100_001];
    assert!(transfer_policy::check_external(&tags, std::iter::empty()).unwrap_err().to_string().contains("too many source tags"));
    let mut notes = structured("SOURCE");
    // One relevant master plus 9,999 irrelevant immutable owner references. Repeated
    // expensive model lookups are charged before being attempted; no million objects.
    let unrelated = NotesMaster::default();
    notes.notes_masters.extend(std::iter::repeat_n(unrelated, 9_999));
    assert!(transfer_policy::capture(&notes, 9, std::iter::once(notes.slides[0].as_ref())).is_ok());
    let error = transfer_policy::capture(&notes, 9, std::iter::repeat_n(notes.slides[0].as_ref(), 500)).unwrap_err().to_string();
    assert!(error.contains("policy work limit"), "{error}");
}

#[test]
fn public_warned_whole_slide_and_object_clipboards_refuse_cross_document_atomically() {
    for objects in [false, true] {
        let mut session = Session::with_new();
        install(&mut session, warned());
        if objects {
            session.execute("edit.selectAll", &json!({})).unwrap();
            session.execute("edit.copy", &json!({})).unwrap();
        } else {
            select_copy_slides(&mut session);
        }
        session.execute("file.new", &json!({})).unwrap();
        unchanged_after_error(&mut session, "edit.paste", json!({}), "unattributed ODP loss");
    }
}

#[test]
fn public_same_document_warned_slide_paste_retains_export_guard_and_undo() {
    let mut session = Session::with_new();
    install(&mut session, warned());
    select_copy_slides(&mut session);
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.slides.len(), 2);
    assert!(doc.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(crate::cmd::file::save_bytes(doc, "odp").is_err());
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc().unwrap().doc.slides.len(), 1);
    assert!(session.doc().unwrap().doc.tags.iter().any(|(key, _)| key == "odp:warning"));
}

#[test]
fn public_same_document_paste_fails_closed_if_source_warning_was_removed() {
    let mut session = Session::with_new();
    install(&mut session, warned());
    select_copy_slides(&mut session);
    session
        .edit_resources(|doc, _| {
            doc.tags.clear();
            Ok(())
        })
        .unwrap();
    unchanged_after_error(&mut session, "edit.paste", json!({}), "guard was cleared");
}

#[test]
fn public_reuse_refuses_warned_sources_and_transfers_native_structured_notes_in_both_modes() {
    for keep in [false, true] {
        let mut session = Session::with_new();
        let data = crate::cmd::base64_encode(&deckcraft_format::save(&warned()).unwrap());
        unchanged_after_error(
            &mut session,
            "slide.reuse",
            json!({"data":data,"name":"source.deckcraft","slides":[0],"keepFormatting":keep}),
            "unattributed ODP loss",
        );
        let source = structured("SOURCE");
        let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
        session.execute("slide.reuse", &json!({"data":data,"name":"notes.deckcraft","slides":[0],"keepFormatting":keep})).unwrap();
        let doc = &session.doc().unwrap().doc;
        let slide = doc.slides.last().unwrap();
        assert!(doc.notes_page_error(slide).is_none());
        assert_eq!(doc.notes_fields(slide).footer.as_deref(), Some("SOURCE"));
        assert_eq!(slide.notes, source.slides[0].notes);
    }
}

#[test]
fn public_compare_start_refuses_before_replacing_prior_comparison() {
    let mut session = Session::with_new();
    let ordinary = session.doc().unwrap().doc.as_ref().clone();
    let bytes = crate::cmd::base64_encode(&deckcraft_format::save(&ordinary).unwrap());
    session.execute("review.compare", &json!({"name":"prior.deckcraft","data":bytes})).unwrap();
    let previous = session.doc().unwrap().compare.as_ref().unwrap().revised.clone();
    for source in [warned(), structured("SOURCE")] {
        let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
        let phrase = if source.tags.is_empty() { "structured notes" } else { "unattributed ODP loss" };
        unchanged_after_error(&mut session, "review.compare", json!({"name":"guarded.deckcraft","data":data}), phrase);
        let compare = session.doc().unwrap().compare.as_ref().unwrap();
        assert_eq!(compare.name, "prior.deckcraft");
        assert!(Arc::ptr_eq(&compare.revised, &previous));
    }
}

#[test]
fn direct_compare_adoption_refuses_before_mutation_even_with_matching_notes_ids() {
    for source in [warned(), structured("SOURCE")] {
        let mut destination = structured("DESTINATION");
        let before = destination.clone();
        let id = destination.slides[0].id;
        let error = crate::compare::adopt(&mut destination, &source, &source.slides[0], Some(id)).unwrap_err().to_string();
        assert!(error.contains(if source.tags.is_empty() { "structured notes" } else { "unattributed ODP loss" }), "{error}");
        assert_eq!(destination, before);
    }
}

#[test]
fn public_structured_notes_cross_document_paste_transfers_instead_of_aliasing_numeric_master_id() {
    let mut session = Session::with_new();
    install(&mut session, structured("SOURCE"));
    select_copy_slides(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    install(&mut session, structured("DESTINATION"));
    let old_master = session.doc().unwrap().doc.notes_masters[0].master.clone();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.slides.len(), 2);
    assert_eq!(doc.notes_masters.len(), 2);
    assert!(Arc::ptr_eq(&old_master, &doc.notes_masters[0].master));
    assert_eq!(doc.notes_fields(&doc.slides[0]).footer.as_deref(), Some("DESTINATION"));
    assert_eq!(doc.notes_fields(&doc.slides[1]).footer.as_deref(), Some("SOURCE"));
    assert_ne!(doc.slides[0].notes_page.as_ref().unwrap().master, doc.slides[1].notes_page.as_ref().unwrap().master);
    assert!(doc.notes_page_error(&doc.slides[1]).is_none());
}

#[test]
fn explicit_text_range_and_external_plain_text_extraction_remain_available() {
    let mut source = warned();
    Arc::make_mut(&mut source.slides[0]).shapes = vec![Shape {
        id: ShapeId(500),
        text: Some(TextBody::from_text("Allowed explicit text")),
        text_box: true,
        xfrm: Some(Xfrm::new(10., 10., 200., 60.)),
        ..Default::default()
    }];
    source.fix_next_id();
    let mut session = Session::with_new();
    install(&mut session, source);
    session.execute("text.edit", &json!({"id":500})).unwrap();
    session.execute("edit.selectAll", &json!({})).unwrap();
    session.execute("edit.copy", &json!({})).unwrap();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    assert!(
        session
            .doc()
            .unwrap()
            .doc
            .slides
            .iter()
            .flat_map(|slide| &slide.shapes)
            .any(|shape| shape.text.as_ref().is_some_and(|text| text.text() == "Allowed explicit text"))
    );
    // This is explicit new plain text, even if a clipboard has guarded object provenance.
    install(&mut session, warned());
    select_copy_slides(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({"text":"Explicit external plain text"})).unwrap();
    assert!(
        session
            .doc()
            .unwrap()
            .doc
            .slides
            .iter()
            .flat_map(|slide| &slide.shapes)
            .any(|shape| shape.text.as_ref().is_some_and(|text| text.text() == "Explicit external plain text"))
    );
}

#[test]
fn explicit_picture_extraction_remains_available_from_guarded_shape_clipboard() {
    let mut source = warned();
    Arc::make_mut(&mut source.slides[0]).shapes = vec![Shape {
        id: ShapeId(500),
        xfrm: Some(Xfrm::new(10., 10., 80., 40.)),
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
        ..Default::default()
    }];
    source.fix_next_id();
    let mut session = Session::with_new();
    install(&mut session, source);
    session.execute("edit.selectAll", &json!({})).unwrap();
    session.execute("edit.copy", &json!({})).unwrap();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.pasteSpecial", &json!({"as":"picture"})).unwrap();
    assert!(session.doc().unwrap().doc.slides[0].shapes.iter().any(|shape| matches!(shape.kind, ShapeKind::Picture { .. })));
    assert!(!session.doc().unwrap().doc.tags.iter().any(|(key, _)| key == "odp:warning"));
}

#[test]
fn plain_canonical_notes_and_template_resources_stay_supported_without_structured_notes() {
    let mut source = Presentation::default();
    Arc::make_mut(&mut source.slides[0]).notes = TextBody::from_text("Plain notes supported");
    source.presentation_templates =
        vec![Arc::new(PresentationTemplate { id: PresentationTemplateId(700), name: "template".into(), display_name: None, slots: vec![] })];
    Arc::make_mut(&mut source.slides[0]).presentation_template = Some(PresentationTemplateId(700));
    source.fix_next_id();
    assert!(transfer_policy::check_external(&source, source.slides.iter().map(Arc::as_ref)).is_ok());
    let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
    let mut session = Session::with_new();
    session.execute("slide.reuse", &json!({"name":"ordinary.deckcraft","data":data})).unwrap();
    assert_eq!(session.doc().unwrap().doc.slides[1].notes.text(), "Plain notes supported");
    assert_eq!(session.doc().unwrap().doc.presentation_templates.len(), 1);
}

fn semantic_loss_odp() -> Vec<u8> {
    let namespaces = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
    let content = format!(
        "<office:document-content {namespaces} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page><draw:frame svg:x='10pt' svg:y='10pt' svg:width='144pt' svg:height='72pt'><draw:text-box><text:p text:style-name='P'>Transfer sentinel</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", deckcraft_odp::MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn real_synthetic_odp_unrepresented_paragraph_property_cannot_be_laundered_by_reuse() {
    let bytes = semantic_loss_odp();
    let source = deckcraft_odp::import(&bytes).unwrap();
    assert!(source.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit")));
    assert!(deckcraft_odp::export(&source).is_err());
    assert_eq!(source.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    let data = crate::cmd::base64_encode(&bytes);
    let mut session = Session::with_new();
    unchanged_after_error(&mut session, "slide.reuse", json!({"name":"synthetic.odp","data":data}), "unattributed ODP loss");
}
