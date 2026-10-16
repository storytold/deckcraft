//! Contributor-original template review/ownership cases; no external assets or IO.
use std::sync::Arc;

use deckcraft_model::{Presentation, PresentationTemplateId, presentation_template};
use serde_json::{Value, json};

use crate::{
    Session,
    compare::{self, Compare, ReviewTransfers},
};

fn add_template(s: &mut Session, name: &str) -> u32 {
    s.execute("presentation.template.create", &json!({"name":name,"slots":[{"role":"text","bounds":{"x":10,"y":10,"w":100,"h":50}}]})).unwrap()["id"]
        .as_u64()
        .unwrap() as u32
}
fn selected() -> Session {
    let mut s = Session::with_new();
    let id = add_template(&mut s, "A");
    s.execute("slide.presentationTemplate", &json!({"template":id})).unwrap();
    s
}
fn begin(s: &mut Session, revised: &Presentation) -> Value {
    let data = crate::cmd::base64_encode(&deckcraft_format::save(revised).unwrap());
    s.execute("review.compare", &json!({"name":"revision.deckcraft","data":data})).unwrap()
}
fn changes(s: &mut Session) -> Vec<Value> {
    s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().clone()
}
fn revised_box(s: &Session) -> Presentation {
    let mut revised = s.doc().unwrap().doc.as_ref().clone();
    Arc::make_mut(&mut revised.presentation_templates[0]).slots[0].bounds.w = 150.0;
    revised
}

#[test]
fn public_diff_reports_changed_selected_resource_and_legacy_accept_refuses_without_receipt() {
    let s = selected();
    let revised = revised_box(&s);
    let mut current = s.doc().unwrap().doc.as_ref().clone();
    let diffs = compare::diff(&current, &revised, false);
    assert_eq!(diffs.len(), 1);
    assert!(diffs[0].details.iter().any(|detail| detail == "presentation template resource"));
    let before = current.clone();
    assert!(compare::accept(&mut current, &revised, &diffs[0]).unwrap_err().to_string().contains("ownership receipt"));
    assert_eq!(current, before);
    let adopted = compare::adopt(&mut current, &revised, &revised.slides[0], None).unwrap();
    let owner = current.presentation_templates.iter().find(|owner| Some(owner.id) == adopted.presentation_template).unwrap();
    assert_eq!(owner.slots[0].bounds.w, 150.0);
    assert_ne!(owner.id, revised.presentation_templates[0].id);
}

#[test]
fn equal_but_distinct_selected_resources_are_not_collapsed_by_payload() {
    let mut s = selected();
    let b = add_template(&mut s, "B");
    let a = s.doc().unwrap().doc.presentation_templates[0].id;
    let mut revised = s.doc().unwrap().doc.as_ref().clone();
    Arc::make_mut(&mut revised.slides[0]).presentation_template = Some(PresentationTemplateId(b));
    assert_eq!(begin(&mut s, &revised)["changes"].as_array().unwrap().len(), 1);
    s.execute("review.accept", &json!({"all":true})).unwrap();
    let p = &s.doc().unwrap().doc;
    assert_ne!(p.slides[0].presentation_template, Some(a));
    let target = p.presentation_templates.iter().find(|owner| Some(owner.id) == p.slides[0].presentation_template).unwrap();
    assert_eq!(target.slots[0].bounds, revised.presentation_templates[1].slots[0].bounds);
    assert!(changes(&mut s).is_empty());
}

#[test]
fn accept_all_transfers_shared_a_a_b_once_terminates_and_undo_redo_rechecks_live_owners() {
    let mut s = selected();
    let a = s.doc().unwrap().doc.presentation_templates[0].id;
    let b = PresentationTemplateId(add_template(&mut s, "B"));
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.presentationTemplate", &json!({"template":a})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.presentationTemplate", &json!({"template":b})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let mut revised = revised_box(&s);
    Arc::make_mut(&mut revised.presentation_templates[1]).display_name = Some("Revised B".into());
    begin(&mut s, &revised);
    assert_eq!(changes(&mut s).len(), 3);
    assert_eq!(s.execute("review.accept", &json!({"all":true})).unwrap()["accepted"], 3);
    assert!(changes(&mut s).is_empty());
    let after = s.doc().unwrap().doc.clone();
    assert_eq!(after.presentation_templates.len(), 4);
    assert_eq!(after.slides[0].presentation_template, after.slides[1].presentation_template);
    assert_ne!(after.slides[1].presentation_template, after.slides[2].presentation_template);
    assert!(s.execute("review.accept", &json!({"all":true})).is_err());
    assert!(Arc::ptr_eq(&after, &s.doc().unwrap().doc));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    assert_eq!(changes(&mut s).len(), 3);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *after);
    assert!(changes(&mut s).is_empty());
}

#[test]
fn selected_owner_deleted_and_selection_cleared_is_accepted_once_and_undoes() {
    let mut s = selected();
    let before = s.doc().unwrap().doc.clone();
    let mut revised = before.as_ref().clone();
    revised.presentation_templates.clear();
    Arc::make_mut(&mut revised.slides[0]).presentation_template = None;
    begin(&mut s, &revised);
    assert_eq!(s.execute("review.accept", &json!({"all":true})).unwrap()["accepted"], 1);
    assert!(s.doc().unwrap().doc.slides[0].presentation_template.is_none());
    assert!(changes(&mut s).is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    assert_eq!(changes(&mut s).len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(changes(&mut s).is_empty());
}

#[test]
fn historical_accepted_pair_survives_resource_edit_second_accept_and_two_undos() {
    let mut s = selected();
    let revised = revised_box(&s);
    begin(&mut s, &revised);
    s.execute("review.accept", &json!({"all":true})).unwrap();
    let accepted = s.doc().unwrap().doc.clone();
    let target = accepted.slides[0].presentation_template.unwrap();
    let slot = accepted.presentation_templates.iter().find(|owner| owner.id == target).unwrap().slots[0].id;
    s.execute("presentation.template.setSlot", &json!({"id":target,"slot":slot,"role":"chart"})).unwrap();
    assert_eq!(changes(&mut s).len(), 1);
    s.execute("review.accept", &json!({"all":true})).unwrap();
    assert!(changes(&mut s).is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(changes(&mut s).len(), 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *accepted);
    assert!(changes(&mut s).is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(changes(&mut s).len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(changes(&mut s).is_empty());
}

#[test]
fn edited_unused_original_with_colliding_revised_selected_id_is_not_ignored() {
    let mut s = selected();
    let original = s.doc().unwrap().doc.presentation_templates[0].clone();
    let revised = revised_box(&s);
    begin(&mut s, &revised);
    s.execute("review.accept", &json!({"all":true})).unwrap();
    s.execute("presentation.template.setSlot", &json!({"id":original.id,"slot":original.slots[0].id,"role":"chart"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("review.revisions", &json!({})).unwrap_err().to_string().contains("unused template revisions"));
    assert!(s.execute("review.accept", &json!({"all":true})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(changes(&mut s).is_empty());
}

#[test]
fn changed_added_and_deleted_unused_definitions_refuse_comparison_atomically() {
    for kind in 0..3 {
        let mut s = selected();
        add_template(&mut s, "Unused");
        let mut revised = revised_box(&s); // Mixed selected-resource change must not hide unused changes.
        match kind {
            0 => Arc::make_mut(&mut revised.presentation_templates[1]).display_name = Some("changed".into()),
            1 => {
                let mut owner = revised.presentation_templates[1].as_ref().clone();
                owner.id = PresentationTemplateId(9000);
                owner.name = "NewUnused".into();
                revised.presentation_templates.push(Arc::new(owner));
            }
            _ => {
                revised.presentation_templates.remove(1);
            }
        }
        let data = crate::cmd::base64_encode(&deckcraft_format::save(&revised).unwrap());
        let before = s.doc().unwrap().doc.clone();
        assert!(
            s.execute("review.compare", &json!({"name":"revision.deckcraft","data":data}))
                .unwrap_err()
                .to_string()
                .contains("unused template revisions")
        );
        assert!(s.doc().unwrap().compare.is_none());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
}

#[test]
fn malformed_staged_template_reference_refuses_without_publishing_receipt_or_document() {
    let mut s = selected();
    let mut revised = s.doc().unwrap().doc.as_ref().clone();
    Arc::make_mut(&mut revised.slides[0]).presentation_template = Some(PresentationTemplateId(u32::MAX));
    s.doc_mut().unwrap().compare =
        Some(Compare { name: "staged".into(), revised: Arc::new(revised), by_position: false, transfers: ReviewTransfers::default() });
    let before = s.doc().unwrap().doc.clone();
    let metadata = format!("{:?}", s.doc().unwrap().compare);
    assert!(s.execute("review.accept", &json!({"all":true})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(metadata, format!("{:?}", s.doc().unwrap().compare));
    presentation_template::validate(&s.doc().unwrap().doc).unwrap();
}
