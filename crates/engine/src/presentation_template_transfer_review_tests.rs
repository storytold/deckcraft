//! Independent, contributor-original adversarial transfer regressions (no external assets).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use deckcraft_model::{presentation_template, *};
use serde_json::json;

use crate::{
    Session,
    template_transfer::{self, TransferMap},
};

fn owner(id: u32, name: &str) -> Arc<PresentationTemplate> {
    Arc::new(PresentationTemplate {
        id: PresentationTemplateId(id),
        name: name.into(),
        display_name: Some("Original label".into()),
        slots: vec![PresentationTemplateSlot {
            id: PresentationTemplateSlotId(id + 1),
            role: PresentationRole::Graphic,
            bounds: PresentationTemplateBox::new(-23.5, 17.25, 121.75, 59.5).unwrap(),
        }],
    })
}

fn deck() -> Presentation {
    let mut doc = Presentation { presentation_templates: vec![owner(700, "A"), owner(710, "B"), owner(720, "unused")], ..Default::default() };
    let original = doc.slides[0].as_ref().clone();
    doc.slides = [700, 710]
        .into_iter()
        .enumerate()
        .map(|(at, id)| Arc::new(Slide { id: SlideId(800 + at as u32), presentation_template: Some(PresentationTemplateId(id)), ..original.clone() }))
        .collect();
    doc.fix_next_id();
    doc
}

fn selected(doc: &Presentation, slide: &Slide) -> Arc<PresentationTemplate> {
    doc.presentation_templates.iter().find(|owner| Some(owner.id) == slide.presentation_template).unwrap().clone()
}

#[test]
fn captured_snapshot_ignores_later_source_edits_even_when_source_ids_stay_equal() {
    let mut source = deck();
    let source_before = source.clone();
    let mut moved = [source.slides[0].as_ref().clone()];
    let closure = template_transfer::capture(&source, moved.iter()).unwrap();
    let changed = Arc::make_mut(&mut source.presentation_templates[0]);
    changed.name = "EditedInSource".into();
    changed.slots[0].bounds.x = 900.;
    let mut destination = source.clone();
    let before = source.clone();
    template_transfer::remap(&mut destination, &closure, &mut moved).unwrap();
    let transferred = selected(&destination, &moved[0]);
    assert_eq!(transferred.name, "A");
    assert_eq!(transferred.slots[0].bounds, source_before.presentation_templates[0].slots[0].bounds);
    assert_ne!(moved[0].presentation_template, source.slides[0].presentation_template);
    assert_eq!(source, before, "reading a captured snapshot must not mutate the live source");
}

#[test]
fn mixed_live_and_stale_receipts_replace_only_the_stale_binding() {
    let source = deck();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = Presentation::default();
    let mut receipt = TransferMap::default();
    let mut first: Vec<_> = source.slides.iter().map(|slide| slide.as_ref().clone()).collect();
    template_transfer::remap_with(&mut destination, &closure, &mut first, &mut receipt).unwrap();
    let original_a = selected(&destination, &first[0]);
    let original_b = selected(&destination, &first[1]);
    destination.presentation_templates.retain(|owner| !Arc::ptr_eq(owner, &original_a));
    // Equal ID, name, payload and slot identities are still an unrelated owner.
    destination.presentation_templates.push(Arc::new(original_a.as_ref().clone()));
    let mut second: Vec<_> = source.slides.iter().map(|slide| slide.as_ref().clone()).collect();
    template_transfer::remap_with(&mut destination, &closure, &mut second, &mut receipt).unwrap();
    let replacement_a = selected(&destination, &second[0]);
    assert!(!Arc::ptr_eq(&original_a, &replacement_a));
    assert!(Arc::ptr_eq(&original_b, &selected(&destination, &second[1])));
    assert_eq!(receipt.pairs().count(), 2, "replacing provenance must not append duplicate source bindings");
    assert!(receipt.maps(&source.presentation_templates[0], &replacement_a));
    assert!(receipt.maps(&source.presentation_templates[1], &original_b));
    let mut third = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut third, &mut receipt).unwrap();
    assert!(Arc::ptr_eq(&replacement_a, &selected(&destination, &third[0])));
}

#[test]
fn exact_remaining_id_space_is_usable_and_one_less_refuses_atomically() {
    let source = deck();
    let mut moved = [source.slides[0].as_ref().clone()];
    let closure = template_transfer::capture(&source, moved.iter()).unwrap();
    let mut destination = Presentation { next_id: u32::MAX - 2, ..Default::default() };
    template_transfer::remap(&mut destination, &closure, &mut moved).unwrap();
    let transferred = selected(&destination, &moved[0]);
    assert_eq!(transferred.id.0, u32::MAX - 1);
    assert_eq!(transferred.slots[0].id.0, u32::MAX);
    assert_eq!(destination.next_id, u32::MAX);
    let mut destination = Presentation { next_id: u32::MAX - 1, ..Default::default() };
    let before = destination.clone();
    let mut moved = [source.slides[0].as_ref().clone()];
    let original = moved.clone();
    assert!(template_transfer::remap(&mut destination, &closure, &mut moved).is_err());
    assert_eq!(destination, before);
    assert_eq!(moved, original);
}

#[test]
fn maximal_unicode_ncname_collision_generates_bounded_valid_name_without_truncating_label() {
    let mut source = deck();
    let name = format!("{}a", "漢".repeat(2730)); // 8191 bytes; no partial UTF-8 truncation.
    let label = "ب".repeat(4096);
    let source_owner = Arc::make_mut(&mut source.presentation_templates[0]);
    source_owner.name = name.clone();
    source_owner.display_name = Some(label.clone());
    let mut moved = [source.slides[0].as_ref().clone()];
    let closure = template_transfer::capture(&source, moved.iter()).unwrap();
    let mut destination = source.clone();
    destination.presentation_templates = source.presentation_templates.iter().map(|owner| Arc::new(owner.as_ref().clone())).collect();
    template_transfer::remap(&mut destination, &closure, &mut moved).unwrap();
    let copied = selected(&destination, &moved[0]);
    assert!(presentation_template::valid_ncname(&copied.name));
    assert!(copied.name.len() < 64);
    assert_eq!(copied.display_name.as_deref(), Some(label.as_str()));
    assert_eq!(source.presentation_templates[0].name, name);
}

#[test]
fn late_byte_budget_failure_preserves_existing_receipt_and_every_caller_owned_value() {
    let mut source = deck();
    let initial_closure = template_transfer::capture(&source, [source.slides[0].as_ref()]).unwrap();
    let mut destination = Presentation::default();
    let mut receipt = TransferMap::default();
    let mut initial = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &initial_closure, &mut initial, &mut receipt).unwrap();
    let original_binding = selected(&destination, &initial[0]);
    // Force a replacement to be staged before later, large owners exhaust the budget.
    destination.presentation_templates.clear();
    let large_owner = |id, prefix: &str| {
        let mut value = owner(id, &format!("{prefix}{id:08}{}", "x".repeat(8180))).as_ref().clone();
        value.display_name = Some("y".repeat(8192));
        Arc::new(value)
    };
    destination.presentation_templates = (0..500).map(|at| large_owner(10_000 + at * 2, "D")).collect();
    // Page-reference scratch consumes bytes with little string-scan work. This
    // reaches the byte guard after resource/receipt staging, not the work guard.
    let plain = destination.slides[0].clone();
    destination.slides.resize(1_500_000, plain);
    destination.fix_next_id();
    source.presentation_templates.extend((0..200).map(|at| large_owner(100_000 + at * 2, "S")));
    let mut moved = vec![source.slides[0].as_ref().clone()];
    moved.extend(source.presentation_templates.iter().skip(3).map(|owner| Slide { presentation_template: Some(owner.id), ..Slide::default() }));
    let closure = template_transfer::capture(&source, moved.iter()).unwrap();
    assert!(presentation_template::validate(&destination).is_ok(), "destination itself is admissible");
    let before = destination.clone();
    let originals = moved.clone();
    let error = template_transfer::remap_with(&mut destination, &closure, &mut moved, &mut receipt).unwrap_err();
    assert!(error.to_string().contains("byte budget"), "{error}");
    assert_eq!(destination, before);
    assert_eq!(moved, originals);
    assert_eq!(receipt.pairs().count(), 1);
    assert!(receipt.maps(&source.presentation_templates[0], &original_binding));
}

fn connector_deck() -> Presentation {
    let mut source = deck();
    source.slides.truncate(1);
    Arc::make_mut(&mut source.slides[0]).shapes = vec![
        Shape { id: ShapeId(850), name: "Transfer target".into(), xfrm: Some(Xfrm::new(20., 30., 80., 60.)), ..Default::default() },
        Shape {
            id: ShapeId(851),
            name: "Transfer connector".into(),
            kind: ShapeKind::Connector { start: Some((ShapeId(850), 0)), end: Some((ShapeId(850), 2)) },
            xfrm: Some(Xfrm::new(200., 300., 40., 50.)),
            ..Default::default()
        },
    ];
    source.fix_next_id();
    source
}

#[test]
fn public_slide_paste_keeps_template_and_stored_connector_geometry() {
    let source = connector_deck();
    let mut session = Session::with_new();
    session.doc_mut().unwrap().doc = Arc::new(source.clone());
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let pasted = &session.doc().unwrap().doc.slides[1];
    assert_eq!(pasted.shapes[1].xfrm, source.slides[0].shapes[1].xfrm, "slide transfer must not recompute stored connector geometry");
    assert!(pasted.presentation_template.is_some());
}

#[test]
fn public_slide_reuse_keeps_template_and_stored_connector_geometry_in_both_modes() {
    let source = connector_deck();
    let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
    for keep in [false, true] {
        let mut session = Session::with_new();
        session.execute("slide.reuse", &json!({"name":"original.deckcraft","data":data,"keepFormatting":keep})).unwrap();
        let reused = &session.doc().unwrap().doc.slides[1];
        assert_eq!(reused.shapes[1].xfrm, source.slides[0].shapes[1].xfrm, "keepFormatting={keep} must preserve stored connector geometry");
        assert!(reused.presentation_template.is_some());
    }
}

fn begin_review(session: &mut Session, revised: &Presentation) -> serde_json::Value {
    let data = crate::cmd::base64_encode(&deckcraft_format::save(revised).unwrap());
    session.execute("review.compare", &json!({"name":"synthetic-revision.deckcraft","data":data})).unwrap()
}

fn review_count(session: &mut Session) -> usize {
    session.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().len()
}

#[test]
fn compare_unused_raw_id_collision_refuses_after_accept_even_with_another_live_selected_owner() {
    let current = deck();
    let mut revised = current.clone();
    Arc::make_mut(&mut revised.presentation_templates[0]).slots[0].bounds.w = 150.;
    let mut session = Session::with_new();
    session.doc_mut().unwrap().doc = Arc::new(current);
    assert_eq!(begin_review(&mut session, &revised)["changes"].as_array().unwrap().len(), 1);
    session.execute("review.accept", &json!({"all":true})).unwrap();
    assert_eq!(review_count(&mut session), 0);
    session.execute("presentation.template.rename", &json!({"id":700,"displayName":"Edited unused original"})).unwrap();
    let before = session.doc().unwrap().doc.clone();
    let revision = session.doc().unwrap().revision;
    let history = (session.doc().unwrap().history.undo.len(), session.doc().unwrap().history.redo.len());
    let metadata = format!("{:?}", session.doc().unwrap().compare);
    let error = session.execute("review.revisions", &json!({})).unwrap_err();
    assert!(error.to_string().contains("unused template revisions"), "{error}");
    assert!(session.execute("review.accept", &json!({"all":true})).is_err());
    assert!(Arc::ptr_eq(&before, &session.doc().unwrap().doc));
    assert_eq!(session.doc().unwrap().revision, revision);
    assert_eq!((session.doc().unwrap().history.undo.len(), session.doc().unwrap().history.redo.len()), history);
    assert_eq!(format!("{:?}", session.doc().unwrap().compare), metadata);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(review_count(&mut session), 0);
}

#[test]
fn compare_selection_only_accept_history_recognizes_exact_old_receipt_after_two_undos() {
    let revised = deck();
    let mut current = revised.clone();
    Arc::make_mut(&mut current.slides[0]).presentation_template = None;
    let mut session = Session::with_new();
    session.doc_mut().unwrap().doc = Arc::new(current);
    assert_eq!(begin_review(&mut session, &revised)["changes"].as_array().unwrap().len(), 1);
    assert_eq!(session.execute("review.accept", &json!({"all":true})).unwrap()["accepted"], 1);
    let first = session.doc().unwrap().doc.clone();
    let adopted = selected(&first, &first.slides[0]);
    session.execute("presentation.template.setSlot", &json!({"id":adopted.id,"slot":adopted.slots[0].id,"role":"chart"})).unwrap();
    assert_eq!(review_count(&mut session), 1);
    session.execute("review.accept", &json!({"all":true})).unwrap();
    assert_eq!(review_count(&mut session), 0);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(review_count(&mut session), 1);
    session.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&adopted, &selected(&session.doc().unwrap().doc, &session.doc().unwrap().doc.slides[0])));
    assert_eq!(review_count(&mut session), 0);
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(review_count(&mut session), 1);
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(review_count(&mut session), 0);
    assert!(session.execute("review.accept", &json!({"all":true})).is_err(), "accept-all must terminate without revisiting accepted pages");
}

#[test]
fn compare_accept_remaps_connector_attachment_ids_when_another_slide_owns_source_ids() {
    let mut revised = connector_deck();
    Arc::make_mut(&mut revised.slides[0]).shapes.push(Shape {
        id: ShapeId(852),
        name: "Alternative target".into(),
        xfrm: Some(Xfrm::new(120., 130., 80., 60.)),
        ..Default::default()
    });
    let mut current = revised.clone();
    Arc::make_mut(&mut current.slides[0]).shapes.clear();
    // The source rectangle's ID is already owned by a different current slide.
    let mut other = current.slides[0].as_ref().clone();
    other.id = SlideId(900);
    other.shapes = vec![Shape { id: ShapeId(850), ..Default::default() }];
    current.slides.push(Arc::new(other));
    current.fix_next_id();
    let mut session = Session::with_new();
    session.doc_mut().unwrap().doc = Arc::new(current);
    begin_review(&mut session, &revised);
    // Accept only the changed source page; the other page remains in place.
    session.execute("review.accept", &json!({"index":0})).unwrap();
    let doc = &session.doc().unwrap().doc;
    let adopted = &doc.slides[0];
    assert_ne!(adopted.shapes[0].id, ShapeId(850));
    assert_eq!(adopted.shapes[1].xfrm, revised.slides[0].shapes[1].xfrm);
    assert_eq!(
        adopted.shapes[1].kind,
        ShapeKind::Connector { start: Some((adopted.shapes[0].id, 0)), end: Some((adopted.shapes[0].id, 2)) },
        "renumbering the adopted rectangle must also retarget the connector's ownership references"
    );
    assert_eq!(doc.slides[1].shapes[0].id, ShapeId(850));
    let remaining = session.execute("review.revisions", &json!({})).unwrap();
    let changes = remaining["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "correctly remapped connector references must not create a changed-page revision: {remaining}");
    assert_eq!(changes[0]["kind"], "deleted");
    session
        .edit_resources(|doc, _| {
            let shapes = &mut Arc::make_mut(&mut doc.slides[0]).shapes;
            let alternative = shapes[2].id;
            let ShapeKind::Connector { end, .. } = &mut shapes[1].kind else { panic!("missing adopted connector") };
            *end = Some((alternative, 2));
            Ok(())
        })
        .unwrap();
    let retargeted = session.execute("review.revisions", &json!({})).unwrap();
    let changes = retargeted["changes"].as_array().unwrap();
    let changed = changes.iter().find(|change| change["kind"] == "changed").expect("retargeting must remain a semantic difference");
    assert_eq!(changed["slide"], 0);
    assert!(
        changed["details"].as_array().unwrap().iter().any(|detail| detail.as_str().is_some_and(|detail| detail.contains("properties"))),
        "{retargeted}"
    );
}

#[test]
fn compare_connector_targets_nested_shape_owner_after_collision_and_has_no_residual_change() {
    let mut revised = connector_deck();
    let slide = Arc::make_mut(&mut revised.slides[0]);
    let target = slide.shapes.remove(0);
    let connector = slide.shapes.remove(0);
    slide.shapes = vec![
        Shape {
            id: ShapeId(860),
            name: "Transfer group".into(),
            xfrm: Some(Xfrm::new(0., 0., 300., 200.)),
            kind: ShapeKind::Group { children: vec![target], child: Xfrm::new(0., 0., 300., 200.) },
            ..Default::default()
        },
        connector,
    ];
    let mut current = revised.clone();
    Arc::make_mut(&mut current.slides[0]).shapes.clear();
    let mut other = current.slides[0].as_ref().clone();
    other.id = SlideId(900);
    other.shapes = vec![Shape { id: ShapeId(850), ..Default::default() }];
    current.slides.push(Arc::new(other));
    current.fix_next_id();
    let mut session = Session::with_new();
    session.doc_mut().unwrap().doc = Arc::new(current);
    begin_review(&mut session, &revised);
    session.execute("review.accept", &json!({"index":0})).unwrap();
    let adopted = &session.doc().unwrap().doc.slides[0];
    let target = adopted.shapes[0].children()[0].id;
    assert_ne!(target, ShapeId(850));
    assert_eq!(adopted.shapes[1].kind, ShapeKind::Connector { start: Some((target, 0)), end: Some((target, 2)) });
    assert_eq!(adopted.shapes[1].xfrm, revised.slides[0].shapes[1].xfrm);
    let remaining = session.execute("review.revisions", &json!({})).unwrap();
    let changes = remaining["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "nested target identity remapping must compare equal: {remaining}");
    assert_eq!(changes[0]["kind"], "deleted");
}
