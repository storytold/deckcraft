//! Authoritative delta of one operation (ADR 0001 §3).
//!
//! A delta names what changed, not how to patch it: the client resolves the key, refetches the
//! object and re-renders. Coarse fields are deliberate — a delta is not a patch format, and a
//! wrong field list would be worse than a broad one.
//!
//! **Derived state is not in a delta.** A group's frame and child space (`refit_groups`), a
//! connector's route and a text box's autofit are functions of the model: each replica computes
//! them from the authoritative inputs, otherwise two clients that moved different children of one
//! group would apply each other's computed frames (ADR 0001 §4, ADR 0004).

use std::sync::Arc;

use deckcraft_collab_proto::{Change, ChangeKind, Delta, Field, SlideKey};
use deckcraft_model::{Presentation, Shape, ShapeId, Slide};

use crate::keys::KeyMap;

/// Deepest group nesting walked.
const MAX_DEPTH: usize = 64;

/// Everything that changed between two states of one document.
pub fn delta(keys: &KeyMap, before: &Presentation, after: &Presentation) -> Delta {
    let mut changes = Vec::new();
    for slide in &after.slides {
        let Some(key) = keys.slide_key(slide.id) else { continue };
        match find_slide(before, slide.id) {
            None => changes.push(Change { slide: key.clone(), shape: None, kind: ChangeKind::Inserted }),
            Some(old) => {
                if Arc::ptr_eq(old, slide) {
                    continue;
                }
                slide_changes(keys, old, slide, key, &mut changes);
            }
        }
    }
    for slide in &before.slides {
        if find_slide(after, slide.id).is_none()
            && let Some(key) = keys.slide_key(slide.id)
        {
            changes.push(Change { slide: key.clone(), shape: None, kind: ChangeKind::Deleted });
        }
    }
    Delta { changes }
}

fn find_slide(doc: &Presentation, id: deckcraft_model::SlideId) -> Option<&Arc<Slide>> {
    doc.slides.iter().find(|s| s.id == id)
}

fn slide_changes(keys: &KeyMap, before: &Slide, after: &Slide, key: &SlideKey, out: &mut Vec<Change>) {
    let old = flatten(&before.shapes);
    let new = flatten(&after.shapes);

    for (id, shape) in &new {
        let shape_key = match keys.shape_key(after.id, *id) {
            Some(k) => k.clone(),
            None => continue,
        };
        match old.iter().find(|(old_id, _)| old_id == id) {
            None => out.push(Change { slide: key.clone(), shape: Some(shape_key), kind: ChangeKind::Inserted }),
            Some((_, old_shape)) => {
                let fields = fields_of(old_shape, shape);
                if !fields.is_empty() {
                    out.push(Change { slide: key.clone(), shape: Some(shape_key), kind: ChangeKind::Modified { fields } });
                }
            }
        }
    }
    for (id, _) in &old {
        if new.iter().any(|(new_id, _)| new_id == id) {
            continue;
        }
        if let Some(shape_key) = keys.shape_key(after.id, *id) {
            out.push(Change { slide: key.clone(), shape: Some(shape_key.clone()), kind: ChangeKind::Deleted });
        }
    }

    // z-order: the order of the top-level shapes. Reported on the slide, not per shape.
    let old_order: Vec<ShapeId> = before.shapes.iter().map(|s| s.id).collect();
    let new_order: Vec<ShapeId> = after.shapes.iter().map(|s| s.id).collect();
    let mut slide_fields = if old_order != new_order { vec![Field::ZOrder] } else { Vec::new() };
    if slide_props_differ(before, after) {
        slide_fields.push(Field::Slide);
    }
    if !slide_fields.is_empty() {
        let kind =
            if old_order != new_order { ChangeKind::Reordered { fields: slide_fields } } else { ChangeKind::Modified { fields: slide_fields } };
        out.push(Change { slide: key.clone(), shape: None, kind });
    }
}

/// Slide-level properties that are not a shape: background, transition, notes, comments…
fn slide_props_differ(a: &Slide, b: &Slide) -> bool {
    a.background != b.background
        || a.transition != b.transition
        || a.notes != b.notes
        || a.comments != b.comments
        || a.hidden != b.hidden
        || a.name != b.name
        || a.layout != b.layout
        || a.color_map != b.color_map
        || a.tags != b.tags
        || a.animations != b.animations
        || a.show_master_shapes != b.show_master_shapes
}

/// What changed on one shape, coarsely.
pub fn fields_of(a: &Shape, b: &Shape) -> Vec<Field> {
    let mut out = Vec::new();
    if a.xfrm != b.xfrm {
        out.push(Field::Xfrm);
    }
    if a.text != b.text {
        out.push(Field::Text);
    }
    if a.fill != b.fill {
        out.push(Field::Fill);
    }
    if a.line != b.line {
        out.push(Field::Line);
    }
    if a.effects != b.effects {
        out.push(Field::Effects);
    }
    if a.hidden != b.hidden {
        out.push(Field::Hidden);
    }
    if a.locked != b.locked {
        out.push(Field::Locked);
    }
    if a.name != b.name {
        out.push(Field::Name);
    }
    if out.is_empty() && a != b {
        // Kind, geometry, diagram, ph, tags… something the model stores that the list above
        // does not name. Better broad than missing.
        out.push(Field::Other);
    }
    out
}

/// Every shape in a tree, depth-first, with its id.
fn flatten(shapes: &[Shape]) -> Vec<(ShapeId, &Shape)> {
    fn walk<'a>(shapes: &'a [Shape], out: &mut Vec<(ShapeId, &'a Shape)>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for s in shapes {
            out.push((s.id, s));
            let children = s.children();
            if !children.is_empty() {
                walk(children, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    walk(shapes, &mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_engine::Session;
    use serde_json::json;

    /// A deck with one blank slide and two shapes, built through the same commands on every
    /// replica so ids and keys line up.
    fn deck() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank slide");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop the title slide");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("insert");
        s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("insert");
        s
    }

    #[test]
    fn moving_a_shape_reports_xfrm_and_nothing_else() {
        let mut s = deck();
        let id = s.doc().expect("doc").doc.as_ref().slides[0].shapes[0].id.0;
        let mut issuer = crate::keys::KeyIssuer::new("t");
        let mut keys = KeyMap::new();
        let before = s.doc().expect("doc").doc.as_ref().clone();
        keys.sync(&before, &mut issuer);

        s.apply_remote("shape.move", &json!({"ids": [id], "x": 300.0, "y": 200.0})).expect("move");
        let after = s.doc().expect("doc").doc.as_ref().clone();
        let d = delta(&keys, &before, &after);

        assert_eq!(d.changes.len(), 1, "{d:?}");
        let c = &d.changes[0];
        assert!(c.shape.is_some());
        // Moving a child of no group changes the shape only, not the slide order.
        assert_eq!(c.kind, ChangeKind::Modified { fields: vec![Field::Xfrm] });
    }

    #[test]
    fn deleting_a_shape_is_a_deleted_change() {
        let mut s = deck();
        let id = s.doc().expect("doc").doc.as_ref().slides[0].shapes[0].id.0;
        let before = s.doc().expect("doc").doc.as_ref().clone();
        let mut issuer = crate::keys::KeyIssuer::new("t");
        let mut keys = KeyMap::new();
        keys.sync(&before, &mut issuer);

        s.apply_remote("edit.delete", &json!({"ids": [id]})).expect("delete");
        let after = s.doc().expect("doc").doc.as_ref().clone();
        let d = delta(&keys, &before, &after);
        assert!(d.changes.iter().any(|c| c.kind == ChangeKind::Deleted && c.shape.is_some()), "{d:?}");
    }

    #[test]
    fn reordering_is_reported_on_the_slide() {
        let mut s = deck();
        let ids: Vec<u32> = s.doc().expect("doc").doc.as_ref().slides[0].shapes.iter().map(|x| x.id.0).collect();
        assert_eq!(ids.len(), 2);
        let before = s.doc().expect("doc").doc.as_ref().clone();
        let mut issuer = crate::keys::KeyIssuer::new("t");
        let mut keys = KeyMap::new();
        keys.sync(&before, &mut issuer);

        s.apply_remote("arrange.reorder", &json!({"id": ids[1], "index": 0})).expect("reorder");
        let after = s.doc().expect("doc").doc.as_ref().clone();
        let d = delta(&keys, &before, &after);
        assert!(d.changes.iter().any(|c| matches!(c.kind, ChangeKind::Reordered { .. }) && c.shape.is_none()), "{d:?}");
    }
}
