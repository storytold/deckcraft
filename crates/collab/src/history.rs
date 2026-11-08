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

use std::collections::HashMap;

use deckcraft_collab_proto::{Payload, ShapeField, ShapeKey, SlideKey};
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

/// One object a step touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub slide: SlideKey,
    pub shape: ShapeKey,
    /// `(row, column)` when the text lives in a table cell.
    pub cell: Option<(u32, u32)>,
    pub image: Image,
}

/// The payload that puts one image back. `None` when the value cannot be described on the wire
/// (a text image of a table cell keeps using `text.set`).
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
/// One undoable step of one actor.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The operation that made the change.
    pub op_id: String,
    /// The document version it produced.
    pub version: u64,
    pub before: Snapshot,
    pub after: Snapshot,
}

impl Step {
    /// The operation that takes this step back. `undo_of` marks it so the adapter moves the step
    /// to redo instead of recording a new one.
    pub fn restoring_payload(&self) -> Option<(SlideKey, ShapeKey, Payload)> {
        let payload = self.before.image.payload(self.before.cell)?;
        Some((self.before.slide.clone(), self.before.shape.clone(), payload))
    }

    /// The operation that re-applies this step after an undo.
    pub fn reapplying_payload(&self) -> Option<(SlideKey, ShapeKey, Payload)> {
        let payload = self.after.image.payload(self.after.cell)?;
        Some((self.after.slide.clone(), self.after.shape.clone(), payload))
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
            before: Snapshot { slide: "s1".into(), shape: "h1".into(), cell: None, image: Image::Xfrm { x: 1.0, y: 2.0, w: 3.0, h: 4.0 } },
            after: Snapshot { slide: "s1".into(), shape: "h1".into(), cell: None, image: Image::Xfrm { x: 10.0, y: 20.0, w: 3.0, h: 4.0 } },
        }
    }

    #[test]
    fn a_restoring_payload_uses_the_before_image() {
        let (slide, shape, payload) = step("op-1", 5).restoring_payload().expect("payload");
        assert_eq!(slide.0, "s1");
        assert_eq!(shape.0, "h1");
        assert_eq!(payload, Payload::ShapeSetBounds { x: 1.0, y: 2.0, w: 3.0, h: 4.0 });
    }

    #[test]
    fn a_fill_or_line_image_restores_the_exact_model_value() {
        let mut s = step("op-1", 1);
        let fill = deckcraft_model::Fill::Background;
        s.before.image = Image::Fill(Some(fill.clone()));
        s.after.image = Image::Fill(None);
        let (_, _, payload) = s.restoring_payload().expect("fill payload");
        let Payload::ShapeField { field, value } = payload else { panic!("a shape field restore") };
        assert_eq!(field, ShapeField::Fill);
        assert_eq!(value, serde_json::to_value(Some(fill)).expect("json"));

        s.before.image = Image::Line(None);
        let (_, _, payload) = s.restoring_payload().expect("line payload");
        let Payload::ShapeField { field, value } = payload else { panic!("a shape field restore") };
        assert_eq!((field, value), (ShapeField::Line, serde_json::Value::Null), "no outline is `null`");
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
