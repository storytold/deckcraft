//! Independent command-boundary and resource-only transaction regressions.
use crate::{Session, undo_cost};
use deckcraft_model::{PresentationTemplate, PresentationTemplateId, Shape, ShapeId, ShapeKind, SlideId, Xfrm};
use serde_json::{Value, json};
use std::sync::Arc;

fn slot() -> Value {
    json!({"role":"text", "bounds":{"x":-4,"y":8,"w":16,"h":32}})
}
fn make(s: &mut Session, name: &str) -> (u32, u32) {
    let v = s.execute("presentation.template.create", &json!({"name":name,"slots":[slot(),slot()]})).unwrap();
    (v["id"].as_u64().unwrap() as u32, v["slots"][0].as_u64().unwrap() as u32)
}
fn assert_atomic(s: &mut Session, command: &str, params: Value) {
    let old = s.doc().unwrap().doc.clone();
    let revision = s.doc().unwrap().revision;
    let selection = s.doc().unwrap().selection.clone();
    let history = (s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len());
    assert!(s.execute(command, &params).is_err(), "{command} accepted {params}");
    let st = s.doc().unwrap();
    assert!(Arc::ptr_eq(&old, &st.doc));
    assert_eq!(st.revision, revision);
    assert_eq!(st.selection, selection);
    assert_eq!((st.history.undo.len(), st.history.redo.len()), history);
}

#[test]
fn every_resource_command_preserves_slide_arcs_stale_connector_geometry_and_exact_history() {
    let mut s = Session::with_new();
    {
        let p = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let slide = Arc::make_mut(&mut p.slides[0]);
        slide.shapes = vec![
            Shape { id: ShapeId(500), xfrm: Some(Xfrm::new(20., 30., 40., 50.)), ..Default::default() },
            Shape {
                id: ShapeId(501),
                kind: ShapeKind::Connector { start: Some((ShapeId(500), 0)), end: Some((ShapeId(999), 0)) },
                xfrm: Some(Xfrm::new(200., 300., 400., 500.)),
                ..Default::default()
            },
        ];
        let mut peer = slide.clone();
        peer.id = SlideId(502);
        p.slides.push(Arc::new(peer));
        p.next_id = 1000;
    }
    let initial = s.doc().unwrap().doc.clone();
    let (id, first) = make(&mut s, "T");
    assert!(Arc::ptr_eq(&initial.slides[0], &s.doc().unwrap().doc.slides[0]));
    assert!(Arc::ptr_eq(&initial.slides[1], &s.doc().unwrap().doc.slides[1]));
    let mut changes = vec![
        ("presentation.template.rename", json!({"id":id,"name":"Renamed","displayName":"Label"})),
        ("presentation.template.addSlot", json!({"id":id,"role":"chart","bounds":{"x":1,"y":2,"w":3,"h":4},"before":first})),
        ("presentation.template.setSlot", json!({"id":id,"slot":first,"role":"graphic"})),
        ("presentation.template.reorderSlot", json!({"id":id,"slot":first,"index":2})),
        ("presentation.template.removeSlot", json!({"id":id,"slot":first})),
    ];
    for (command, params) in changes.drain(..) {
        let before = s.doc().unwrap().doc.clone();
        s.execute(command, &params).unwrap();
        let after = s.doc().unwrap().doc.clone();
        for i in 0..2 {
            assert!(Arc::ptr_eq(&initial.slides[i], &after.slides[i]), "{command} detached slide {i}");
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*s.doc().unwrap().doc, *before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(*s.doc().unwrap().doc, *after);
    }
    s.execute("slide.presentationTemplate", &json!({"slide":502,"template":id})).unwrap();
    let selected = s.doc().unwrap().doc.clone();
    assert!(Arc::ptr_eq(&initial.slides[0], &selected.slides[0]));
    assert_eq!(selected.slides[0].shapes, initial.slides[0].shapes);
    assert_eq!(selected.slides[1].shapes, initial.slides[1].shapes);
    assert_eq!(selected.slides[1].presentation_template, Some(PresentationTemplateId(id)));
    s.execute("slide.presentationTemplate", &json!({"slide":502,"template":null})).unwrap();
    let cleared = s.doc().unwrap().doc.clone();
    s.execute("presentation.template.delete", &json!({"id":id})).unwrap();
    assert!(Arc::ptr_eq(&cleared.slides[0], &s.doc().unwrap().doc.slides[0]));
    assert!(Arc::ptr_eq(&cleared.slides[1], &s.doc().unwrap().doc.slides[1]));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *cleared);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *selected);
}

#[test]
fn every_command_rejects_unknown_params_before_publishing_and_nested_bounds_are_strict() {
    let mut s = Session::with_new();
    let (id, first) = make(&mut s, "T");
    for (command, params) in [
        ("presentation.template.create", json!({"name":"Other","slots":[],"unknown":false})),
        ("presentation.template.rename", json!({"id":id,"name":"Other","unknown":false})),
        ("presentation.template.addSlot", json!({"id":id,"role":"text","bounds":{"x":1,"y":2,"w":3,"h":4},"unknown":false})),
        ("presentation.template.setSlot", json!({"id":id,"slot":first,"role":"title","unknown":false})),
        ("presentation.template.reorderSlot", json!({"id":id,"slot":first,"index":1,"unknown":false})),
        ("presentation.template.removeSlot", json!({"id":id,"slot":first,"unknown":false})),
        ("presentation.template.delete", json!({"id":id,"unknown":false})),
        ("slide.presentationTemplate", json!({"template":id,"unknown":false})),
        ("presentation.template.setSlot", json!({"id":id,"slot":first,"bounds":{"x":1,"y":2,"w":3,"h":4,"rotation":1}})),
        ("presentation.template.create", json!({"name":"Other","slots":[{"role":"text","bounds":{"x":1,"y":2,"w":3,"h":4},"id":7}]})),
    ] {
        assert_atomic(&mut s, command, params);
    }
}

#[test]
fn detached_template_payload_counts_toward_undo_budget_and_history_prunes() {
    let mut s = Session::with_new();
    let (id, _) = make(&mut s, "T");
    s.execute("presentation.template.rename", &json!({"id":id,"displayName":"a".repeat(8192)})).unwrap();
    let old = s.doc().unwrap().doc.clone();
    let shared = old.as_ref().clone();
    let mut detached = shared.clone();
    let copy: PresentationTemplate = detached.presentation_templates[0].as_ref().clone();
    detached.presentation_templates[0] = Arc::new(copy);
    assert!(undo_cost(&old, &detached) >= undo_cost(&old, &shared).saturating_add(8192));
    s.doc_mut().unwrap().history.byte_limit = 1;
    for c in ['b', 'c', 'd', 'e'] {
        s.execute("presentation.template.rename", &json!({"id":id,"displayName":c.to_string().repeat(8192)})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), 1);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.presentation_templates[0].display_name.as_deref(), Some("d".repeat(8192).as_str()));
}
