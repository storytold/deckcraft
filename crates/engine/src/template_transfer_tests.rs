//! Independent contributor-original slide/template transfer regression fixtures.
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
        display_name: Some("漢 & < > \t\r\nب".into()),
        slots: [901, 902]
            .into_iter()
            .map(|id| PresentationTemplateSlot {
                id: PresentationTemplateSlotId(id),
                role: PresentationRole::Outline,
                bounds: PresentationTemplateBox::new(-5., 2., 80., 35.).unwrap(),
            })
            .collect(),
    })
}

fn source() -> Presentation {
    let mut p = Presentation { presentation_templates: vec![owner(700, "same"), owner(701, "other"), owner(702, "unused")], ..Default::default() };
    let original = p.slides[0].as_ref().clone();
    p.slides = [700, 700, 701]
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            Arc::new(Slide { id: SlideId(1000 + index as u32), presentation_template: Some(PresentationTemplateId(id)), ..original.clone() })
        })
        .collect();
    p.fix_next_id();
    p
}

fn slides(p: &Presentation) -> Vec<Slide> {
    p.slides.iter().map(|slide| slide.as_ref().clone()).collect()
}
fn selected<'a>(p: &'a Presentation, slide: &Slide) -> &'a Arc<PresentationTemplate> {
    p.presentation_templates.iter().find(|owner| Some(owner.id) == slide.presentation_template).unwrap()
}

#[test]
fn capture_and_remap_keep_a_a_b_owner_sharing_and_only_selected_closure() {
    let source = source();
    let mut moved = slides(&source);
    let closure = template_transfer::capture(&source, moved.iter()).unwrap();
    let original = moved.clone();
    let mut destination = Presentation::default();
    template_transfer::remap(&mut destination, &closure, &mut moved).unwrap();
    assert_eq!(destination.presentation_templates.len(), 2);
    assert_eq!(moved[0].presentation_template, moved[1].presentation_template);
    assert_ne!(moved[0].presentation_template, moved[2].presentation_template);
    for (old, new) in original.iter().zip(&moved) {
        let a = selected(&source, old);
        let b = selected(&destination, new);
        assert!(!Arc::ptr_eq(a, b));
        assert_ne!(a.id, b.id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.display_name, b.display_name);
        assert_eq!(a.slots.iter().map(|s| (s.role, s.bounds)).collect::<Vec<_>>(), b.slots.iter().map(|s| (s.role, s.bounds)).collect::<Vec<_>>());
        assert!(a.slots.iter().zip(&b.slots).all(|(a, b)| a.id != b.id));
        let mut expected = old.clone();
        expected.presentation_template = new.presentation_template;
        assert_eq!(new, &expected, "transfer must change no live shape/master/layout/content fields");
    }
    assert!(!destination.presentation_templates.iter().any(|owner| owner.name == "unused"));
    assert!(presentation_template::validate(&destination).is_ok());
}

#[test]
fn clipboard_style_closure_survives_source_deletion_and_document_replacement() {
    let mut source = source();
    let mut copied = slides(&source);
    let closure = template_transfer::capture(&source, copied.iter()).unwrap();
    let snapshot = source.presentation_templates[0].clone();
    source.slides.clear();
    source.presentation_templates.clear();
    drop(source);
    let mut destination = Presentation::default();
    template_transfer::remap(&mut destination, &closure, &mut copied).unwrap();
    assert_eq!(selected(&destination, &copied[0]).slots[0].bounds, snapshot.slots[0].bounds);
    assert_eq!(selected(&destination, &copied[0]).display_name, snapshot.display_name);
}

#[test]
fn exact_same_document_arc_reuse_needs_no_fresh_ids_even_when_exhausted() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = source.clone();
    destination.next_id = u32::MAX;
    let before = destination.clone();
    let mut copied = slides(&source);
    template_transfer::remap(&mut destination, &closure, &mut copied).unwrap();
    assert_eq!(destination, before);
    assert_eq!(copied, slides(&source));
    assert!(Arc::ptr_eq(&source.presentation_templates[0], &destination.presentation_templates[0]));
}

#[test]
fn numeric_name_and_value_coincidence_never_rebind_to_an_unrelated_owner() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = source.clone();
    // Equal values and the same IDs/names, but independently materialized owners.
    destination.presentation_templates = source.presentation_templates.iter().map(|owner| Arc::new(owner.as_ref().clone())).collect();
    let existing = destination.presentation_templates.clone();
    let mut copied = slides(&source);
    template_transfer::remap(&mut destination, &closure, &mut copied).unwrap();
    assert_eq!(destination.presentation_templates.len(), 5);
    assert_ne!(copied[0].presentation_template, source.slides[0].presentation_template);
    assert_ne!(copied[2].presentation_template, source.slides[2].presentation_template);
    for (before, after) in existing.iter().zip(&destination.presentation_templates) {
        assert!(Arc::ptr_eq(before, after));
    }
    assert!(presentation_template::validate(&destination).is_ok());
}

#[test]
fn colliding_generated_names_are_checked_and_unicode_labels_remain_exact() {
    let source = source();
    let mut copied = vec![source.slides[0].as_ref().clone()];
    let closure = template_transfer::capture(&source, copied.iter()).unwrap();
    let mut destination = Presentation {
        next_id: 10_000,
        presentation_templates: vec![owner(700, "same"), owner(701, "template_10001"), owner(702, "template_10001_1")],
        ..Default::default()
    };
    template_transfer::remap(&mut destination, &closure, &mut copied).unwrap();
    let transferred = selected(&destination, &copied[0]);
    assert_eq!(transferred.name, "template_10001_2");
    assert!(presentation_template::valid_ncname(&transferred.name));
    assert_eq!(transferred.display_name, source.presentation_templates[0].display_name);
    assert_eq!(source.presentation_templates[0].name, "same");
}

#[test]
fn closure_is_restricted_again_to_the_actual_transfer_batch() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut copied = vec![source.slides[2].as_ref().clone()];
    let mut destination = Presentation::default();
    template_transfer::remap(&mut destination, &closure, &mut copied).unwrap();
    assert_eq!(destination.presentation_templates.len(), 1);
    assert_eq!(selected(&destination, &copied[0]).name, "other");
    let before = destination.clone();
    let mut plain = [Slide::default()];
    template_transfer::remap(&mut destination, &closure, &mut plain).unwrap();
    assert_eq!(destination, before);
}

#[test]
fn exhaustion_missing_closure_and_metadata_budget_fail_atomically() {
    for case in 0..4 {
        let source = source();
        let mut copied = slides(&source);
        let closure = template_transfer::capture(&source, copied.iter()).unwrap();
        let mut destination = Presentation::default();
        match case {
            0 => destination.next_id = u32::MAX,
            1 => destination.next_id = u32::MAX - 2,
            2 => copied[0].presentation_template = Some(PresentationTemplateId(999)),
            _ => {
                let mut large = owner(800, "large").as_ref().clone();
                large.name = "n".repeat(presentation_template::MAX_TEMPLATE_NAME_BYTES);
                large.display_name = Some("l".repeat(presentation_template::MAX_TEMPLATE_NAME_BYTES));
                let shared = Arc::new(large);
                destination.presentation_templates = vec![shared; presentation_template::MAX_TEMPLATES];
            }
        }
        let before = destination.clone();
        let originals = copied.clone();
        let mut receipt = TransferMap::default();
        assert!(template_transfer::remap_with(&mut destination, &closure, &mut copied, &mut receipt).is_err(), "case {case}");
        assert_eq!(destination, before);
        assert_eq!(copied, originals);
        assert!(!receipt.maps(&source.presentation_templates[0], &source.presentation_templates[0]));
    }
}

#[test]
fn capture_checks_full_source_metadata_and_not_only_selected_good_resources() {
    let mut source = source();
    let picked = [source.slides[0].as_ref().clone()];
    Arc::make_mut(&mut source.presentation_templates[2]).slots[0].bounds.w = 0.;
    assert!(template_transfer::capture(&source, picked.iter()).is_err());
}

#[test]
fn compare_receipt_reuses_only_live_exact_owner_snapshots_across_accepts() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = Presentation::default();
    let mut receipt = TransferMap::default();
    let mut first = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut first, &mut receipt).unwrap();
    let adopted = selected(&destination, &first[0]).clone();
    assert!(receipt.maps(&source.presentation_templates[0], &adopted));
    let mut second = [source.slides[1].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut second, &mut receipt).unwrap();
    assert_eq!(first[0].presentation_template, second[0].presentation_template);
    assert_eq!(destination.presentation_templates.len(), 1);
    // Editing the target detaches its Arc even if every effective value stays equal.
    Arc::make_mut(&mut destination.presentation_templates[0]).slots.swap(0, 1);
    assert!(!receipt.maps(&source.presentation_templates[0], &destination.presentation_templates[0]));
    let mut third = [source.slides[1].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut third, &mut receipt).unwrap();
    assert_ne!(third[0].presentation_template, first[0].presentation_template);
    assert_eq!(destination.presentation_templates.len(), 2);
    assert!(receipt.maps(&source.presentation_templates[0], selected(&destination, &third[0])));
    assert!(!Arc::ptr_eq(&adopted, selected(&destination, &third[0])));
}

#[test]
fn stale_receipt_target_cannot_reuse_a_numerically_equal_replacement_or_undo_state() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = Presentation::default();
    let empty = destination.clone();
    let mut receipt = TransferMap::default();
    let mut moved = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut moved, &mut receipt).unwrap();
    let target = destination.presentation_templates[0].clone();
    destination.presentation_templates[0] = Arc::new(target.as_ref().clone());
    let mut again = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut again, &mut receipt).unwrap();
    assert_ne!(again[0].presentation_template, moved[0].presentation_template);
    // A stale receipt also cannot prove an owner exists after undo removed it.
    destination = empty;
    let mut after_undo = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut after_undo, &mut receipt).unwrap();
    assert_eq!(destination.presentation_templates.len(), 1);
    assert!(!Arc::ptr_eq(&target, selected(&destination, &after_undo[0])));
}

#[test]
fn failed_later_transfer_does_not_publish_receipt_changes() {
    let source = source();
    let closure = template_transfer::capture(&source, source.slides.iter().map(Arc::as_ref)).unwrap();
    let mut destination = Presentation::default();
    let mut receipt = TransferMap::default();
    let mut first = [source.slides[0].as_ref().clone()];
    template_transfer::remap_with(&mut destination, &closure, &mut first, &mut receipt).unwrap();
    let target = selected(&destination, &first[0]).clone();
    destination.next_id = u32::MAX;
    let mut other = [source.slides[2].as_ref().clone()];
    let before = destination.clone();
    let original = other.clone();
    assert!(template_transfer::remap_with(&mut destination, &closure, &mut other, &mut receipt).is_err());
    assert_eq!(destination, before);
    assert_eq!(other, original);
    assert!(receipt.maps(&source.presentation_templates[0], &target));
    assert!(!receipt.maps(&source.presentation_templates[1], &target));
}

#[test]
fn public_clipboard_survives_source_selection_clearing_and_new_document() {
    let source = source();
    let mut session = Session::with_new();
    session
        .edit_resources(|doc, _| {
            *doc = source.clone();
            Ok(())
        })
        .unwrap();
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    session
        .edit_resources(|doc, _| {
            for slide in &mut doc.slides {
                Arc::make_mut(slide).presentation_template = None;
            }
            doc.presentation_templates.clear();
            Ok(())
        })
        .unwrap();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.presentation_templates.len(), 2);
    assert_eq!(doc.slides.len(), 4);
    let moved = &doc.slides[1..];
    assert_eq!(moved[0].presentation_template, moved[1].presentation_template);
    assert_ne!(moved[0].presentation_template, moved[2].presentation_template);
    assert_eq!(selected(doc, &moved[0]).display_name, source.presentation_templates[0].display_name);
    assert!(presentation_template::validate(doc).is_ok());
}

#[test]
fn public_reuse_preserves_template_closure_for_both_formatting_modes() {
    let source = source();
    let data = crate::cmd::base64_encode(&deckcraft_format::save(&source).unwrap());
    for keep in [false, true] {
        let mut session = Session::with_new();
        session.execute("slide.reuse", &json!({"name":"independent.deckcraft","data":data,"keepFormatting":keep})).unwrap();
        let doc = &session.doc().unwrap().doc;
        assert_eq!(doc.presentation_templates.len(), 2);
        assert_eq!(doc.slides.len(), 4);
        assert_eq!(doc.slides[1].presentation_template, doc.slides[2].presentation_template);
        assert_ne!(doc.slides[1].presentation_template, doc.slides[3].presentation_template);
        assert!(presentation_template::validate(doc).is_ok());
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.doc().unwrap().doc.presentation_templates.is_empty());
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.presentation_templates.len(), 2);
    }
}
