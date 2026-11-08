//! Per-user undo for one document session (ADR 0010).
//!
//! Undo is not a rollback of the document: it is a **new operation** that restores the values the
//! actor's own step replaced, sent with `baseVersion = head` and checked like any other operation.
//! History belongs to an actor, so one participant's undo never takes back another's edit.
//!
//! What is remembered is the **before** and the **after** image of the objects a step touched:
//!
//! - the before image becomes the restoring payload;
//! - the after image is the expectation — if the object no longer holds it, someone else changed
//!   it and the undo is refused with `undo_conflict` instead of overwriting their edit.
//!
//! One step may touch several objects: `shape.fill` and `shape.line` on a group also format every
//! visible member, so the step records one image per member and the restore is a single
//! `shape.setFields` operation. A structural step (`slide.delete`, `slide.new`, `slide.duplicate`)
//! records a whole slide snapshot instead of a shape value.

use std::collections::HashMap;

use deckcraft_collab_proto::{Payload, ShapeField, ShapeFieldItem, ShapeKey, ShapeRestoreItem, SlideKey};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Fill, Line};

/// A value one step replaced. Coarse on purpose: the whole box for a move/resize, the whole
/// text body for a text edit or a format step, the whole fill or outline for an appearance step,
/// because a finer split would let an undo half-revert its own step.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    /// Position and size in slide points.
    Xfrm { x: f64, y: f64, w: f64, h: f64 },
    /// A text body (or one table cell) as plain text.
    Text(String),
    /// The shape's explicit fill; `None` is "inherited from the style".
    Fill(Option<Fill>),
    /// The shape's outline.
    Line(Option<Line>),
    /// The whole body formatting and text of one shape, as `format.*` leaves it. Formatting is
    /// per run and per paragraph, so one command's arguments cannot describe the value it
    /// replaced: the image is the body itself.
    Format(TextBody),
}

/// One shape a step touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub slide: SlideKey,
    pub shape: ShapeKey,
    /// `(row, column)` when the text lives in a table cell.
    pub cell: Option<(u32, u32)>,
    pub image: Image,
}

/// A whole slide, present or absent, for a structural step. `slide` is the model serialised as
/// JSON (the same value `shape.setField` carries: an exact model value, never a user edit), and
/// the membership lists put the slide back into the sections and custom shows it belonged to.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideImage {
    pub key: SlideKey,
    /// Position among the slides while the slide was present.
    pub index: u32,
    /// `None` = the slide was absent.
    pub slide: Option<serde_json::Value>,
    /// `(section index, position in that section)`.
    pub sections: Vec<(u32, u32)>,
    /// `(custom show index, position in that show)`.
    pub shows: Vec<(u32, u32)>,
}

/// A whole shape, present or absent, for a structural step: the model as JSON (its id, text,
/// geometry and every other field), the group that held it and its place among that group's
/// children, and the animations that named it or a descendant — including the effects of other
/// shapes that it triggered, which a delete clears.
///
/// A group's image carries its children, so one image restores a whole subtree; a step records
/// only the top-most shapes it removed.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeImage {
    pub slide: SlideKey,
    pub key: ShapeKey,
    /// The group that held the shape; `None` = the top level of the slide.
    pub parent: Option<ShapeKey>,
    /// Position among the parent's children (`0` = back) while the shape was present.
    pub index: u32,
    /// `None` = the shape was absent.
    pub shape: Option<serde_json::Value>,
    /// The animation entries a delete dropped: the shape's own effects and the effects of other
    /// shapes that it triggered.
    pub animations: Vec<serde_json::Value>,
}

/// One object a step touched: a shape value, a whole shape or a whole slide.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Shape(Snapshot),
    ShapeBody(ShapeImage),
    Slide(SlideImage),
}

impl Change {
    /// Whether `current` still holds the value this image recorded. A slide's or a shape's
    /// position may have moved because neighbours came and went; only its presence, owner and
    /// content are compared. Animations count as content: deleting a shape whose animation
    /// someone added would drop their effect.
    pub fn same_state(&self, current: &Change) -> bool {
        match (self, current) {
            (Change::Shape(a), Change::Shape(b)) => a == b,
            (Change::ShapeBody(a), Change::ShapeBody(b)) => match (&a.shape, &b.shape) {
                (None, None) => a.key == b.key,
                (Some(x), Some(y)) => a.key == b.key && a.parent == b.parent && x == y && a.animations == b.animations,
                _ => false,
            },
            (Change::Slide(a), Change::Slide(b)) => match (&a.slide, &b.slide) {
                (None, None) => a.key == b.key,
                (Some(x), Some(y)) => a.key == b.key && x == y,
                _ => false,
            },
            _ => false,
        }
    }
}

/// The payload that puts one image back, and the object it addresses. `None` when the value
/// cannot be described on the wire (a text image of a table cell keeps using `text.set`).
pub struct Restore {
    pub slide: SlideKey,
    pub shape: Option<ShapeKey>,
    /// The objects a restored or removed group of shapes addresses: several shapes come back in
    /// one `ShapeRestore`, and several are removed in one `ShapeDelete`.
    pub shapes: Vec<ShapeKey>,
    pub payload: Payload,
}

impl Image {
    pub fn payload(&self, cell: Option<(u32, u32)>) -> Option<Payload> {
        Some(match self {
            Image::Xfrm { x, y, w, h } => Payload::ShapeSetBounds { x: *x, y: *y, w: *w, h: *h },
            Image::Text(text) => Payload::TextSet { text: text.clone(), cell },
            // The exact value: the user-facing fill, line and format commands accept only a
            // subset of the model, so a lossless restore carries the model value itself.
            Image::Fill(fill) => Payload::ShapeField { field: ShapeField::Fill, value: serde_json::to_value(fill).ok()? },
            Image::Line(line) => Payload::ShapeField { field: ShapeField::Line, value: serde_json::to_value(line).ok()? },
            Image::Format(body) => Payload::ShapeField { field: ShapeField::Text, value: serde_json::to_value(body).ok()? },
        })
    }
}

/// The operation that puts back one step's before or after image.
///
/// One shape value keeps the single-object `ShapeField` payload; a group's appearance step is one
/// `ShapeFields` merge so its members are restored atomically; a whole shape (or several) comes
/// back in one `ShapeRestore` or goes in one `ShapeDelete`; a slide is a `SlideRestore` or a
/// `SlideDelete`.
pub fn restores(changes: &[Change]) -> Option<Restore> {
    match changes {
        [Change::Shape(at)] => {
            Some(Restore { slide: at.slide.clone(), shape: Some(at.shape.clone()), shapes: Vec::new(), payload: at.image.payload(at.cell)? })
        }
        [Change::ShapeBody(at)] => body_restore(&[at]),
        [Change::Slide(at)] => {
            let payload = match &at.slide {
                Some(slide) => {
                    Payload::SlideRestore { index: at.index, slide: slide.clone(), sections: at.sections.clone(), shows: at.shows.clone() }
                }
                None => Payload::SlideDelete {},
            };
            Some(Restore { slide: at.key.clone(), shape: None, shapes: Vec::new(), payload })
        }
        [] => None,
        many if many.iter().all(|change| matches!(change, Change::ShapeBody(_))) => {
            let bodies: Vec<&ShapeImage> = many
                .iter()
                .filter_map(|change| match change {
                    Change::ShapeBody(at) => Some(at),
                    _ => None,
                })
                .collect();
            body_restore(&bodies)
        }
        many => {
            // A group's fill/outline is one step over its members: every image is an exact model
            // value of the same kind, and the restore is one atomic operation.
            let mut items = Vec::with_capacity(many.len());
            let mut slide: Option<&SlideKey> = None;
            for change in many {
                let Change::Shape(at) = change else { return None };
                if slide.is_some_and(|s| s != &at.slide) {
                    return None;
                }
                slide = Some(&at.slide);
                match at.image.payload(at.cell)? {
                    Payload::ShapeField { field, value } => items.push(ShapeFieldItem { shape: at.shape.clone(), field, value }),
                    _ => return None,
                }
            }
            Some(Restore { slide: slide?.clone(), shape: None, shapes: Vec::new(), payload: Payload::ShapeFields { items } })
        }
    }
}

/// One operation over a step's whole-shape images: they are all present (put them back) or all
/// absent (remove them again). A mix has no single operation.
fn body_restore(bodies: &[&ShapeImage]) -> Option<Restore> {
    let slide = bodies.first()?.slide.clone();
    if bodies.iter().any(|b| b.slide != slide) {
        return None;
    }
    if bodies.iter().all(|b| b.shape.is_none()) {
        let shapes: Vec<ShapeKey> = bodies.iter().map(|b| b.key.clone()).collect();
        return Some(Restore { slide, shape: None, shapes, payload: Payload::ShapeDelete {} });
    }
    if bodies.iter().any(|b| b.shape.is_none()) {
        return None;
    }
    let mut items = Vec::with_capacity(bodies.len());
    for b in bodies {
        items.push(ShapeRestoreItem {
            shape: b.key.clone(),
            parent: b.parent.clone(),
            index: b.index,
            value: b.shape.clone()?,
            animations: b.animations.clone(),
        });
    }
    Some(Restore { slide, shape: None, shapes: Vec::new(), payload: Payload::ShapeRestore { items } })
}

/// One undoable step of one actor. A step may touch several objects (a group's members) or one
/// slide (a structural step), so both images are lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The operation that made the change.
    pub op_id: String,
    /// The document version it produced.
    pub version: u64,
    pub before: Vec<Change>,
    pub after: Vec<Change>,
}

impl Step {
    /// The operation that takes this step back. `undo_of` marks it so the adapter moves the step
    /// to redo instead of recording a new one.
    pub fn restoring_payload(&self) -> Option<Restore> {
        restores(&self.before)
    }

    /// The operation that re-applies this step after an undo.
    pub fn reapplying_payload(&self) -> Option<Restore> {
        restores(&self.after)
    }
}

/// Undo and redo stacks of one actor.
#[derive(Clone, Debug)]
pub struct ActorHistory {
    undo: Vec<Step>,
    redo: Vec<Step>,
    limit: usize,
}

impl Default for ActorHistory {
    fn default() -> Self {
        ActorHistory { undo: Vec::new(), redo: Vec::new(), limit: 200 }
    }
}

impl ActorHistory {
    pub fn with_limit(limit: usize) -> Self {
        ActorHistory { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1) }
    }

    /// Record a new step: any redo branch is dropped, as in every editor.
    pub fn record(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    pub fn peek_undo(&self) -> Option<&Step> {
        self.undo.last()
    }

    pub fn peek_redo(&self) -> Option<&Step> {
        self.redo.last()
    }

    /// The step was undone and applied: move it from undo to redo.
    pub fn commit_undo(&mut self) {
        if let Some(step) = self.undo.pop() {
            self.redo.push(step);
            if self.redo.len() > self.limit {
                self.redo.remove(0);
            }
        }
    }

    /// The step was redone and applied: move it back to undo.
    pub fn commit_redo(&mut self) {
        if let Some(step) = self.redo.pop() {
            self.undo.push(step);
            if self.undo.len() > self.limit {
                self.undo.remove(0);
            }
        }
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

/// Every actor's history in this session.
#[derive(Clone, Debug, Default)]
pub struct Histories {
    actors: HashMap<String, ActorHistory>,
    limit: usize,
}

impl Histories {
    pub fn with_limit(limit: usize) -> Self {
        Histories { actors: HashMap::new(), limit }
    }

    pub fn of(&mut self, actor: &str) -> &mut ActorHistory {
        let limit = if self.limit == 0 { 200 } else { self.limit };
        self.actors.entry(actor.to_string()).or_insert_with(|| ActorHistory::with_limit(limit))
    }

    pub fn get(&self, actor: &str) -> Option<&ActorHistory> {
        self.actors.get(actor)
    }

    pub fn actors(&self) -> usize {
        self.actors.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(op: &str, version: u64) -> Step {
        Step {
            op_id: op.into(),
            version,
            before: vec![Change::Shape(Snapshot {
                slide: "s1".into(),
                shape: "h1".into(),
                cell: None,
                image: Image::Xfrm { x: 1.0, y: 2.0, w: 3.0, h: 4.0 },
            })],
            after: vec![Change::Shape(Snapshot {
                slide: "s1".into(),
                shape: "h1".into(),
                cell: None,
                image: Image::Xfrm { x: 10.0, y: 20.0, w: 3.0, h: 4.0 },
            })],
        }
    }

    #[test]
    fn a_restoring_payload_uses_the_before_image() {
        let restore = step("op-1", 5).restoring_payload().expect("payload");
        assert_eq!(restore.slide.0, "s1");
        assert_eq!(restore.shape.as_ref().map(|k| k.0.as_str()), Some("h1"));
        assert_eq!(restore.payload, Payload::ShapeSetBounds { x: 1.0, y: 2.0, w: 3.0, h: 4.0 });
    }

    #[test]
    fn a_fill_or_line_image_restores_the_exact_model_value() {
        let mut s = step("op-1", 1);
        let fill = deckcraft_model::Fill::Background;
        let Change::Shape(at) = &mut s.before[0] else { panic!("a shape") };
        at.image = Image::Fill(Some(fill.clone()));
        let Change::Shape(at) = &mut s.after[0] else { panic!("a shape") };
        at.image = Image::Fill(None);
        let restore = s.restoring_payload().expect("fill payload");
        let Payload::ShapeField { field, value } = restore.payload else { panic!("a shape field restore") };
        assert_eq!(field, ShapeField::Fill);
        assert_eq!(value, serde_json::to_value(Some(fill)).expect("json"));

        let Change::Shape(at) = &mut s.before[0] else { panic!("a shape") };
        at.image = Image::Line(None);
        let restore = s.restoring_payload().expect("line payload");
        let Payload::ShapeField { field, value } = restore.payload else { panic!("a shape field restore") };
        assert_eq!((field, value), (ShapeField::Line, serde_json::Value::Null), "no outline is `null`");
    }

    #[test]
    fn a_group_step_restores_every_member_in_one_operation() {
        let image =
            |shape: &str, fill| Change::Shape(Snapshot { slide: "s1".into(), shape: shape.into(), cell: None, image: Image::Fill(Some(fill)) });
        let s = Step {
            op_id: "op".into(),
            version: 1,
            before: vec![image("g", deckcraft_model::Fill::Background), image("h1", deckcraft_model::Fill::Background)],
            after: vec![image("g", deckcraft_model::Fill::None), image("h1", deckcraft_model::Fill::None)],
        };
        let restore = s.restoring_payload().expect("group payload");
        assert_eq!(restore.slide.0, "s1");
        assert!(restore.shape.is_none(), "the restore addresses the slide and every member");
        let Payload::ShapeFields { items } = restore.payload else { panic!("one merge payload") };
        let keys: Vec<&str> = items.iter().map(|i| i.shape.0.as_str()).collect();
        assert_eq!(keys, ["g", "h1"]);
        assert!(items.iter().all(|i| i.field == ShapeField::Fill));
    }

    #[test]
    fn a_slide_step_restores_or_deletes_the_whole_slide() {
        let s = Step {
            op_id: "op".into(),
            version: 1,
            before: vec![Change::Slide(SlideImage {
                key: "s1".into(),
                index: 2,
                slide: Some(serde_json::json!({"id": 7, "name": "Gone"})),
                sections: vec![(0, 1)],
                shows: vec![],
            })],
            after: vec![Change::Slide(SlideImage { key: "s1".into(), index: 0, slide: None, sections: vec![], shows: vec![] })],
        };
        let restore = s.restoring_payload().expect("slide restore");
        assert_eq!(restore.slide.0, "s1");
        assert!(restore.shape.is_none());
        let Payload::SlideRestore { index, sections, .. } = restore.payload else { panic!("a slide restore") };
        assert_eq!((index, sections), (2, vec![(0, 1)]));
        let redo = s.reapplying_payload().expect("slide delete");
        assert_eq!(redo.payload, Payload::SlideDelete {});
    }

    #[test]
    fn a_slide_image_compares_presence_and_content_not_position() {
        let at = Change::Slide(SlideImage { key: "s1".into(), index: 2, slide: Some(serde_json::json!({"id": 7})), sections: vec![], shows: vec![] });
        let moved = Change::Slide(SlideImage {
            key: "s1".into(),
            index: 5,
            slide: Some(serde_json::json!({"id": 7})),
            sections: vec![(1, 0)],
            shows: vec![],
        });
        assert!(at.same_state(&moved), "another slide was inserted before it");
        let edited = Change::Slide(SlideImage {
            key: "s1".into(),
            index: 2,
            slide: Some(serde_json::json!({"id": 7, "name": "Edited"})),
            sections: vec![],
            shows: vec![],
        });
        assert!(!at.same_state(&edited), "the slide itself changed");
    }

    #[test]
    fn a_whole_shape_step_restores_or_deletes_it() {
        let value = serde_json::json!({"id": 7, "name": "Gone"});
        let step = Step {
            op_id: "op".into(),
            version: 1,
            before: vec![Change::ShapeBody(ShapeImage {
                slide: "s1".into(),
                key: "h1".into(),
                parent: Some("h9".into()),
                index: 2,
                shape: Some(value.clone()),
                animations: vec![serde_json::json!({"shape": 7, "effect": "fade"})],
            })],
            after: vec![Change::ShapeBody(ShapeImage {
                slide: "s1".into(),
                key: "h1".into(),
                parent: Some("h9".into()),
                index: 2,
                shape: None,
                animations: Vec::new(),
            })],
        };
        let restore = step.restoring_payload().expect("a shape restore");
        assert_eq!(restore.slide.0, "s1");
        assert!(restore.shape.is_none(), "the payload names the shape");
        assert!(restore.shapes.is_empty());
        let Payload::ShapeRestore { items } = restore.payload else { panic!("one shape restore") };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].shape.0, "h1");
        assert_eq!(items[0].parent.as_ref().map(|k| k.0.as_str()), Some("h9"));
        assert_eq!((items[0].index, &items[0].value), (2, &value));
        assert_eq!(items[0].animations.len(), 1);

        let redo = step.reapplying_payload().expect("a shape delete");
        assert_eq!(redo.payload, Payload::ShapeDelete {});
        assert_eq!(redo.shapes, vec![ShapeKey("h1".into())], "the delete names the shapes it removes");
    }

    #[test]
    fn a_shape_image_compares_content_and_owner_but_not_position() {
        let at = ShapeImage {
            slide: "s1".into(),
            key: "h1".into(),
            parent: None,
            index: 0,
            shape: Some(serde_json::json!({"id": 7})),
            animations: Vec::new(),
        };
        let shifted = Change::ShapeBody(ShapeImage { index: 3, ..at.clone() });
        assert!(Change::ShapeBody(at.clone()).same_state(&shifted), "a neighbour was inserted or removed before it");
        let edited = Change::ShapeBody(ShapeImage { shape: Some(serde_json::json!({"id": 7, "name": "Edited"})), ..at.clone() });
        assert!(!Change::ShapeBody(at.clone()).same_state(&edited), "the shape itself changed");
        let regrouped = Change::ShapeBody(ShapeImage { parent: Some("h2".into()), ..at.clone() });
        assert!(!Change::ShapeBody(at.clone()).same_state(&regrouped), "the shape moved into another group");
        let animated = Change::ShapeBody(ShapeImage { animations: vec![serde_json::json!({"shape": 7})], ..at.clone() });
        assert!(!Change::ShapeBody(at.clone()).same_state(&animated), "someone added an effect that a delete would drop");
        let gone = Change::ShapeBody(ShapeImage { shape: None, animations: Vec::new(), ..at.clone() });
        assert!(!Change::ShapeBody(at).same_state(&gone), "presence is compared");
        assert!(gone.same_state(&gone));
    }

    #[test]
    fn recording_drops_the_redo_branch() {
        let mut h = ActorHistory::default();
        h.record(step("op-1", 1));
        h.record(step("op-2", 2));
        h.commit_undo();
        assert_eq!((h.undo_len(), h.redo_len()), (1, 1));
        h.record(step("op-3", 3));
        assert_eq!((h.undo_len(), h.redo_len()), (2, 0), "a new step clears redo");
    }

    #[test]
    fn the_limit_drops_the_oldest_step() {
        let mut h = ActorHistory::with_limit(2);
        for i in 0..4 {
            h.record(step(&format!("op-{i}"), i));
        }
        assert_eq!(h.undo_len(), 2);
        assert_eq!(h.peek_undo().map(|s| s.op_id.as_str()), Some("op-3"));
    }

    #[test]
    fn histories_are_per_actor() {
        let mut all = Histories::default();
        all.of("a").record(step("a-1", 1));
        all.of("b").record(step("b-1", 1));
        assert_eq!(all.get("a").map(ActorHistory::undo_len), Some(1));
        assert_eq!(all.get("b").map(ActorHistory::undo_len), Some(1));
        assert_eq!(all.actors(), 2);
    }

    #[test]
    fn a_refused_undo_leaves_the_history_untouched() {
        // `undo` only peeks: an operation that is never applied moves nothing.
        let mut h = ActorHistory::default();
        h.record(step("op-1", 1));
        let _peeked = h.peek_undo().cloned();
        assert_eq!((h.undo_len(), h.redo_len()), (1, 0));
    }
}
