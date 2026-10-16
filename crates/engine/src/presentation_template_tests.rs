//! Contributor-original resource/command cases. Template selection does not apply
//! geometry, instantiate placeholders or reinterpret equal resources as one owner.
use std::sync::Arc;

use deckcraft_model::{
    Presentation, PresentationTemplate, PresentationTemplateId, PresentationTemplateSlotId, Shape, ShapeId, ShapeKind, Xfrm,
    presentation_template::{self as templates, PresentationRole, PresentationTemplateBox, PresentationTemplateSlot},
};
use serde_json::{Value, json};

use crate::Session;

fn slot(role: &str, x: f64) -> Value {
    json!({"role": role, "bounds": {"x": x, "y": -10.0, "w": 100.0, "h": 50.0}})
}
fn create(s: &mut Session, name: &str, slots: Vec<Value>) -> (u32, Vec<u32>) {
    let answer = s.execute("presentation.template.create", &json!({"name": name, "displayName": "Shared label", "slots": slots})).unwrap();
    (answer["id"].as_u64().unwrap() as u32, answer["slots"].as_array().unwrap().iter().map(|value| value.as_u64().unwrap() as u32).collect())
}
fn resource(s: &Session, index: usize) -> &PresentationTemplate {
    &s.doc().unwrap().doc.presentation_templates[index]
}
fn assert_atomic_error(s: &mut Session, command: &str, params: Value) {
    let st = s.doc().unwrap();
    let before = st.doc.clone();
    let selection = st.selection.clone();
    let revision = st.revision;
    let history = (st.history.undo.len(), st.history.redo.len());
    assert!(s.execute(command, &params).is_err(), "{command} accepted {params}");
    let st = s.doc().unwrap();
    assert!(Arc::ptr_eq(&before, &st.doc), "{command} detached or published state on failure");
    assert_eq!(*before, *st.doc);
    assert_eq!(st.selection, selection);
    assert_eq!(st.revision, revision);
    assert_eq!((st.history.undo.len(), st.history.redo.len()), history);
}
fn source_template(id: u32, name: &str, slot_id: u32) -> Arc<PresentationTemplate> {
    Arc::new(PresentationTemplate {
        id: PresentationTemplateId(id),
        name: name.into(),
        display_name: None,
        slots: vec![PresentationTemplateSlot {
            id: PresentationTemplateSlotId(slot_id),
            role: PresentationRole::Outline,
            bounds: PresentationTemplateBox::new(10.0, 20.0, 100.0, 50.0).unwrap(),
        }],
    })
}

#[test]
fn create_zero_one_many_and_all_exact_roles_as_distinct_unused_owners() {
    let mut s = Session::with_new();
    let (empty, none) = create(&mut s, "Empty", vec![]);
    assert!(none.is_empty());
    assert_eq!(resource(&s, 0).id.0, empty);
    let (one, first) = create(&mut s, "One", vec![slot("outline", 10.0)]);
    let (other, second) = create(&mut s, "EqualButDistinct", vec![slot("outline", 10.0)]);
    assert_ne!(one, other);
    assert_ne!(first, second);
    assert_eq!(resource(&s, 1).slots[0].bounds, resource(&s, 2).slots[0].bounds);
    assert_eq!(resource(&s, 1).slots[0].role, resource(&s, 2).slots[0].role);
    let roles = [
        "title",
        "outline",
        "subtitle",
        "text",
        "graphic",
        "object",
        "chart",
        "table",
        "orgchart",
        "page",
        "notes",
        "handout",
        "header",
        "footer",
        "date-time",
        "page-number",
    ];
    let (_, all) = create(&mut s, "AllRoles", roles.iter().map(|role| slot(role, 10.0)).collect());
    assert_eq!(all.len(), 16);
    assert_eq!(resource(&s, 3).slots.iter().map(|slot| slot.role.odf_token()).collect::<Vec<_>>(), roles);
    assert!(s.doc().unwrap().doc.slides.iter().all(|slide| slide.presentation_template.is_none()));
    templates::validate(&s.doc().unwrap().doc).unwrap();
}

#[test]
fn same_role_slots_keep_identity_through_add_edit_reorder_remove_and_history() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "Repeated", vec![slot("text", 10.0), slot("text", 10.0)]);
    let before = s.doc().unwrap().doc.clone();
    let added = s
        .execute(
            "presentation.template.addSlot",
            &json!({"id": id, "role": "text", "bounds": {"x": 20.0, "y": 30.0, "w": 40.0, "h": 50.0}, "before": slots[1]}),
        )
        .unwrap()["slot"]
        .as_u64()
        .unwrap() as u32;
    assert_eq!(resource(&s, 0).slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), vec![slots[0], added, slots[1]]);
    s.execute(
        "presentation.template.setSlot",
        &json!({"id": id, "slot": added, "role": "chart", "bounds": {"x": -15.0, "y": 5.0, "w": 75.0, "h": 20.0}}),
    )
    .unwrap();
    assert_eq!(resource(&s, 0).slots[1].role, PresentationRole::Chart);
    s.execute("presentation.template.reorderSlot", &json!({"id": id, "slot": added, "index": 2})).unwrap();
    assert_eq!(resource(&s, 0).slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), vec![slots[0], slots[1], added]);
    assert_eq!(resource(&s, 0).slots[2].bounds.x, -15.0);
    s.execute("presentation.template.removeSlot", &json!({"id": id, "slot": slots[0]})).unwrap();
    let after = s.doc().unwrap().doc.clone();
    assert_eq!(resource(&s, 0).slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), vec![slots[1], added]);
    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(*s.doc().unwrap().doc, *before);
    for _ in 0..4 {
        s.execute("edit.redo", &json!({})).unwrap();
    }
    assert_eq!(*s.doc().unwrap().doc, *after);
}

#[test]
fn create_and_rename_undo_redo_restore_exact_owners_names_labels_and_ids() {
    let mut s = Session::with_new();
    let before = s.doc().unwrap().doc.clone();
    let (id, _) = create(&mut s, "Original", vec![slot("text", 0.0)]);
    s.execute("presentation.template.rename", &json!({"id":id,"name":"Renamed","displayName":null})).unwrap();
    let after = s.doc().unwrap().doc.clone();
    assert_eq!(resource(&s, 0).display_name, None);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(resource(&s, 0).name, "Original");
    assert_eq!(resource(&s, 0).display_name.as_deref(), Some("Shared label"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *after);
}

#[test]
fn adding_beyond_the_per_owner_slot_limit_refuses_before_allocation_or_cow() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "Full", vec![slot("text", 0.0); 256]);
    assert_eq!(slots.len(), 256);
    assert_atomic_error(&mut s, "presentation.template.addSlot", json!({"id":id,"role":"text","bounds":{"x":0,"y":0,"w":1,"h":1}}));
}

#[test]
fn owner_qualified_source_slot_ids_do_not_cross_equal_resources() {
    let mut s = Session::with_new();
    {
        let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        doc.presentation_templates = vec![source_template(10_000, "A", 333), source_template(20_000, "B", 333)];
        doc.next_id = 1; // Direct source model: checked resource IDs still take precedence.
    }
    let b = s.doc().unwrap().doc.presentation_templates[1].clone();
    s.execute("presentation.template.setSlot", &json!({"id": 10_000, "slot": 333, "role": "table"})).unwrap();
    assert_eq!(resource(&s, 0).slots[0].role, PresentationRole::Table);
    assert_eq!(resource(&s, 1).slots[0].role, PresentationRole::Outline);
    assert!(Arc::ptr_eq(&b, &s.doc().unwrap().doc.presentation_templates[1]));
    let added = s
        .execute("presentation.template.addSlot", &json!({"id": 10_000, "role": "text", "bounds": {"x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}}))
        .unwrap();
    assert!(added["slot"].as_u64().unwrap() > 20_000);
    let (created, slots) = create(&mut s, "Fresh", vec![slot("title", 0.0)]);
    assert!(created > 20_000);
    assert!(slots[0] > created);
    assert_atomic_error(&mut s, "presentation.template.removeSlot", json!({"id": 20_000, "slot": slots[0]}));
}

#[test]
fn equal_value_commands_preserve_document_resource_slide_arcs_revision_and_history() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "Stable", vec![slot("text", 10.0)]);
    s.execute("slide.presentationTemplate", &json!({"template": id})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let revision = s.doc().unwrap().revision;
    let history = s.doc().unwrap().history.undo.len();
    for (command, params) in [
        ("presentation.template.rename", json!({"id": id, "name": "Stable", "displayName": "Shared label"})),
        ("presentation.template.rename", json!({"id": id})),
        ("presentation.template.setSlot", json!({"id": id, "slot": slots[0], "role": "text", "bounds": {"x":10.0,"y":-10.0,"w":100.0,"h":50.0}})),
        ("presentation.template.setSlot", json!({"id": id, "slot": slots[0]})),
        ("presentation.template.reorderSlot", json!({"id": id, "slot": slots[0], "index": 0})),
        ("slide.presentationTemplate", json!({"template": id})),
    ] {
        s.execute(command, &params).unwrap();
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc), "{command} detached a no-op");
        assert_eq!(s.doc().unwrap().revision, revision);
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
    }
}

fn content_fixture(s: &mut Session) -> Presentation {
    let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    let slide = Arc::make_mut(&mut doc.slides[0]);
    slide.shapes = vec![
        Shape {
            id: ShapeId(1000),
            xfrm: Some(Xfrm::new(10.0, 10.0, 50.0, 50.0)),
            fill: Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(255, 0, 0)))),
            line: Some(deckcraft_model::Line::none()),
            ..Default::default()
        },
        Shape {
            id: ShapeId(1001),
            kind: ShapeKind::Connector { start: Some((ShapeId(1000), 0)), end: None },
            xfrm: Some(Xfrm::new(150.0, 100.0, 10.0, 10.0)),
            ..Default::default()
        },
    ];
    doc.next_id = 2000;
    doc.clone()
}

#[test]
fn template_edits_and_selection_leave_every_live_object_and_actual_pixels_unchanged() {
    let mut s = Session::with_new();
    let original = content_fixture(&mut s);
    let opts = deckcraft_render::RenderOpts { size: Some((200, 150)), ..Default::default() };
    let before = deckcraft_render::render_slide(&original, 0, &opts);
    assert_eq!(before.pixel(20, 20), [255, 0, 0, 255]);
    let (id, slots) = create(&mut s, "OnlyAResource", vec![slot("text", -100.0), slot("text", 100.0)]);
    s.execute("presentation.template.setSlot", &json!({"id":id,"slot":slots[0],"bounds":{"x":500.0,"y":500.0,"w":1000.0,"h":1000.0}})).unwrap();
    assert!(Arc::ptr_eq(&original.slides[0], &s.doc().unwrap().doc.slides[0]), "resource edit detached a live slide");
    s.execute("slide.presentationTemplate", &json!({"template":id})).unwrap();
    s.execute("presentation.template.rename", &json!({"id":id,"name":"Renamed","displayName":null})).unwrap();
    let current = &s.doc().unwrap().doc;
    let mut actual_slide = current.slides[0].as_ref().clone();
    actual_slide.presentation_template = None;
    assert_eq!(actual_slide, *original.slides[0]);
    assert_eq!(current.masters, original.masters);
    assert_eq!(current.notes_masters, original.notes_masters);
    assert_eq!(current.notes_master, original.notes_master);
    assert_eq!(current.handout_master, original.handout_master);
    for (before, after) in original.masters.iter().zip(&current.masters) {
        assert!(Arc::ptr_eq(before, after));
    }
    let after = deckcraft_render::render_slide(current, 0, &opts);
    assert_eq!(after.pixels, before.pixels);
    assert_eq!(resource(&s, 0).display_name, None);
}

#[test]
fn selection_targets_exact_slide_id_and_selected_resource_deletion_is_atomic() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let slides = s.doc().unwrap().doc.slides.clone();
    let (a, _) = create(&mut s, "A", vec![]);
    let (b, _) = create(&mut s, "B", vec![]);
    s.execute("slide.presentationTemplate", &json!({"slide":slides[0].id,"template":a})).unwrap();
    s.execute("slide.presentationTemplate", &json!({"template":b})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides[0].presentation_template, Some(PresentationTemplateId(a)));
    assert_eq!(s.doc().unwrap().doc.slides[1].presentation_template, Some(PresentationTemplateId(b)));
    assert_atomic_error(&mut s, "presentation.template.delete", json!({"id":a}));
    assert_atomic_error(&mut s, "presentation.template.delete", json!({"id":b}));
    let before = s.doc().unwrap().doc.clone();
    s.execute("slide.presentationTemplate", &json!({"slide":slides[0].id,"template":null})).unwrap();
    s.execute("presentation.template.delete", &json!({"id":a})).unwrap();
    assert_eq!(s.doc().unwrap().doc.presentation_templates.len(), 1);
    assert_eq!(resource(&s, 0).id.0, b);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}

#[test]
fn hostile_types_missing_fields_bounds_names_and_owner_ids_never_publish_or_consume_ids() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "Valid", vec![slot("text", 0.0)]);
    let (other, _) = create(&mut s, "Other", vec![]);
    for params in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"name":"A"}),
        json!({"name":false,"slots":[]}),
        json!({"name":"A","slots":null}),
        json!({"name":"A","slots":[null]}),
        json!({"name":"A","slots":[{"role":"body","bounds":{"x":0,"y":0,"w":10,"h":10}}]}),
        json!({"name":"A","slots":[{"role":"text","bounds":{"x":0,"y":0,"w":10}}]}),
        json!({"name":"A","slots":[],"displayName":null}),
        json!({"name":"A","slots":[],"extra":0}),
    ] {
        assert_atomic_error(&mut s, "presentation.template.create", params);
    }
    for bad_name in ["", "two words", "bad:name", "1name", &"x".repeat(8193)] {
        assert_atomic_error(&mut s, "presentation.template.rename", json!({"id":id,"name":bad_name}));
    }
    for wrong in [Value::Null, json!(-1), json!(1.5), json!("1"), json!(true), json!(u64::MAX)] {
        assert_atomic_error(&mut s, "presentation.template.delete", json!({"id":wrong}));
        assert_atomic_error(&mut s, "presentation.template.setSlot", json!({"id":id,"slot":wrong,"role":"title"}));
        assert_atomic_error(&mut s, "slide.presentationTemplate", json!({"slide":wrong,"template":null}));
        if !wrong.is_null() {
            assert_atomic_error(&mut s, "slide.presentationTemplate", json!({"template":wrong}));
        }
    }
    for bounds in [
        json!({"x":null,"y":0,"w":10,"h":10}),
        json!({"x":"0","y":0,"w":10,"h":10}),
        json!({"x":false,"y":0,"w":10,"h":10}),
        json!({"x":0,"y":0,"w":0,"h":10}),
        json!({"x":0,"y":0,"w":10,"h":-1}),
        json!({"x":10_000_000,"y":0,"w":1,"h":1}),
        json!({"x":0,"y":0,"w":1e300,"h":1}),
        json!({"x":0,"y":0,"w":1,"h":1,"rotation":1}),
    ] {
        assert_atomic_error(&mut s, "presentation.template.setSlot", json!({"id":id,"slot":slots[0],"bounds":bounds}));
    }
    for (command, params) in [
        ("presentation.template.rename", json!({"id":other,"name":"Valid"})),
        ("presentation.template.rename", json!({"id":id,"displayName":"bad\u{1}"})),
        ("presentation.template.rename", json!({"id":id,"displayName":"x".repeat(8193)})),
        ("presentation.template.addSlot", json!({"id":id,"role":"text","bounds":{"x":0,"y":0,"w":1,"h":1},"before":null})),
        ("presentation.template.addSlot", json!({"id":other,"role":"text","bounds":{"x":0,"y":0,"w":1,"h":1},"before":slots[0]})),
        ("presentation.template.reorderSlot", json!({"id":id,"slot":slots[0],"index":1})),
        ("presentation.template.reorderSlot", json!({"id":id,"slot":slots[0],"index":-1})),
        ("presentation.template.removeSlot", json!({"id":other,"slot":slots[0]})),
        ("slide.presentationTemplate", json!({})),
        ("slide.presentationTemplate", json!({"template":u32::MAX})),
    ] {
        assert_atomic_error(&mut s, command, params);
    }
    assert_atomic_error(&mut s, "presentation.template.create", json!({"name":"Many","slots":vec![slot("text",0.0);257]}));
}

#[test]
fn exhausted_checked_allocation_rejects_before_cow_but_nonallocating_edits_still_work() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "Existing", vec![slot("text", 0.0)]);
    Arc::make_mut(&mut s.doc_mut().unwrap().doc).next_id = u32::MAX - 1;
    assert_atomic_error(&mut s, "presentation.template.create", json!({"name":"NeedsTwoIDs","slots":[slot("text",0.0)]}));
    let (last, none) = create(&mut s, "LastID", vec![]);
    assert_eq!(last, u32::MAX);
    assert!(none.is_empty());
    assert_atomic_error(&mut s, "presentation.template.create", json!({"name":"Exhausted","slots":[]}));
    assert_atomic_error(&mut s, "presentation.template.addSlot", json!({"id":id,"role":"text","bounds":{"x":0,"y":0,"w":1,"h":1}}));
    s.execute("presentation.template.setSlot", &json!({"id":id,"slot":slots[0],"role":"title"})).unwrap();
    s.execute("presentation.template.rename", &json!({"id":id,"name":"StillEditable"})).unwrap();
    assert_eq!(resource(&s, 0).name, "StillEditable");
    assert_eq!(s.doc().unwrap().doc.next_id, u32::MAX);
}

#[test]
fn invalid_existing_template_owners_are_not_repaired_by_new_commands() {
    for kind in 0..4 {
        let mut s = Session::with_new();
        let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        doc.presentation_templates = vec![source_template(100, "A", 101)];
        match kind {
            0 => doc.presentation_templates.push(source_template(100, "B", 102)),
            1 => doc.presentation_templates.push(source_template(200, "A", 201)),
            2 => Arc::make_mut(&mut doc.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
            _ => {
                let resource = Arc::make_mut(&mut doc.presentation_templates[0]);
                resource.slots.push(resource.slots[0].clone());
            }
        }
        assert_atomic_error(&mut s, "presentation.template.create", json!({"name":"Fresh","slots":[]}));
        assert_atomic_error(&mut s, "presentation.template.rename", json!({"id":100,"name":"Renamed"}));
    }
}

#[test]
fn native_save_and_reopen_preserve_resource_and_slot_identities_after_commands() {
    let mut s = Session::with_new();
    let (id, slots) = create(&mut s, "保存", vec![slot("outline", -50.0), slot("text", 50.0)]);
    s.execute("slide.presentationTemplate", &json!({"template":id})).unwrap();
    s.execute("presentation.template.rename", &json!({"id":id,"displayName":"Visible label\t\n"})).unwrap();
    let p = &s.doc().unwrap().doc;
    let reopened = deckcraft_format::load(&deckcraft_format::save(p).unwrap()).unwrap();
    assert_eq!(reopened.presentation_templates, p.presentation_templates);
    assert_eq!(reopened.presentation_templates[0].slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), slots);
    assert_eq!(reopened.slides[0].presentation_template, Some(PresentationTemplateId(id)));
}
