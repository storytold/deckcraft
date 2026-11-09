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

use deckcraft_collab_proto::{Payload, ShapeBounds, ShapeField, ShapeFieldItem, ShapeKey, ShapeRestoreItem, SlideKey};
use deckcraft_model::anim::{Animation, Transition};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Fill, Line, Table};

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
    /// The whole body formatting and text of one shape or of one table cell, as `format.*` leaves
    /// it. Formatting is per run and per paragraph, so one command's arguments cannot describe
    /// the value it replaced: the image is the body itself, addressed by the step's cell.
    Format(TextBody),
    /// A whole table, for a structural row/column edit: the cells' indices and values both
    /// change, so only the whole table is an exact image.
    Table(Table),
    /// A whole SmartArt group — its diagram value and the shapes generated from it — for an
    /// outline, layout, style or colour edit. An outline edit regenerates the children with fresh
    /// ids and replaces any drawing an imported file brought, so the diagram value alone is not an
    /// exact image of the step.
    SmartArt(serde_json::Value),
    /// One shape's animation steps, in order, as `animation.add`/`animation.remove` leave them.
    /// A file's animations carry preset ids, timings, colours and motion paths that the
    /// `animation.add` arguments cannot repeat, so the step image is the model list itself.
    Animation(Vec<Animation>),
    /// One slide's **whole** animation sequence, in order, as `animation.set`, `animation.timing`
    /// and `animation.move` leave it. A step may move past another shape's step, so a per-shape
    /// image cannot put the sequence back.
    SlideAnimations(Vec<Animation>),
    /// One slide's speaker notes as a whole body. Notes belong to a slide, not to a shape, and a
    /// notes edit replaces text the typed string cannot describe (runs, levels, bullets), so the
    /// image is the body itself.
    Notes(TextBody),
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

/// One slide's transition, present or absent, for a `transition.set` step. The value is the whole
/// model transition: the command keeps advance timings, the embedded sound and kept XML of the
/// value it replaced, so a restore must put all of it back.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionImage {
    pub slide: SlideKey,
    /// `None` = the slide had no transition.
    pub transition: Option<Transition>,
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

/// A whole group, present or absent, for `arrange.group` and `arrange.ungroup` (RFC §6).
///
/// While the group is present its model value carries the children, so one image puts the whole
/// subtree back; `children` keeps the keys needed to take the group's members out again before
/// that image is inserted, and `index` is the group's place at the top level. While the group is
/// absent, `members` holds the whole image of every former member at the top level, with the
/// value it had before grouping (undo) or after ungrouping (redo).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupImage {
    pub slide: SlideKey,
    /// The group's key, stable for the session.
    pub key: ShapeKey,
    /// Position among the top-level shapes while the group was present.
    pub index: u32,
    /// `None` = the group was absent.
    pub group: Option<serde_json::Value>,
    /// The group's direct children while it was present (used to remove them before a restore).
    pub children: Vec<ShapeKey>,
    /// Whole images of the former members while the group was absent.
    pub members: Vec<ShapeImage>,
    /// Animations that named the group or a descendant while it was present.
    pub animations: Vec<serde_json::Value>,
}

/// One object a step touched: a shape value, a whole shape, a whole group, a whole slide or one
/// slide's transition.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Shape(Snapshot),
    ShapeBody(ShapeImage),
    GroupSubtree(GroupImage),
    Slide(SlideImage),
    SlideTransition(TransitionImage),
    Notes(NotesImage),
    SlideAnimations(SlideAnimationsImage),
    Font(FontImage),
}

/// One embedded font entry, present or absent, for a `font.create`, `font.set` or `font.remove`
/// step. A document font is presentation-level (it belongs to no slide) and a font step changes
/// no slide, shape or media address, so it is its own image kind. The whole entry is kept: its
/// descriptors and the media ids of its four face slots, because the arguments of the user
/// command cannot describe the entry it replaced.
#[derive(Clone, Debug, PartialEq)]
pub struct FontImage {
    pub typeface: String,
    /// `None` = the document had no entry for this typeface.
    pub font: Option<deckcraft_model::EmbeddedFont>,
}

/// One slide's speaker notes, present as a whole body, for a `notes.*` step. The address is the
/// slide (`target.slide`), not a shape: notes are part of the slide model.
#[derive(Clone, Debug, PartialEq)]
pub struct NotesImage {
    pub slide: SlideKey,
    pub notes: TextBody,
}

/// One slide's whole animation sequence, for an `animation.set`/`animation.timing`/
/// `animation.move` step. Steps of different shapes may interleave, so the image is the ordered
/// list of the slide itself, not one shape's sub-list.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideAnimationsImage {
    pub slide: SlideKey,
    pub animations: Vec<Animation>,
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
            (Change::GroupSubtree(a), Change::GroupSubtree(b)) => match (&a.group, &b.group) {
                (None, None) => {
                    a.key == b.key
                        && a.members.len() == b.members.len()
                        && a.members.iter().zip(&b.members).all(|(x, y)| Change::ShapeBody(x.clone()).same_state(&Change::ShapeBody(y.clone())))
                }
                (Some(x), Some(y)) => a.key == b.key && x == y && a.animations == b.animations,
                _ => false,
            },
            (Change::Slide(a), Change::Slide(b)) => match (&a.slide, &b.slide) {
                (None, None) => a.key == b.key,
                (Some(x), Some(y)) => a.key == b.key && x == y,
                _ => false,
            },
            (Change::SlideTransition(a), Change::SlideTransition(b)) => a.slide == b.slide && a.transition == b.transition,
            (Change::Notes(a), Change::Notes(b)) => a.slide == b.slide && a.notes == b.notes,
            (Change::SlideAnimations(a), Change::SlideAnimations(b)) => a.slide == b.slide && a.animations == b.animations,
            (Change::Font(a), Change::Font(b)) => a.typeface.eq_ignore_ascii_case(&b.typeface) && a.font == b.font,
            _ => false,
        }
    }
}

/// The payload that puts one image back, and the object it addresses. `None` when the value
/// cannot be described on the wire (a text image of a table cell keeps using `text.set`).
pub struct Restore {
    /// The slide the restored object lives on; `None` for a presentation-level value (a document
    /// font), which no slide addresses.
    pub slide: Option<SlideKey>,
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
            // A body inside a table cell is its own object: the restore names the cell and
            // carries that body, so a neighbour's edit is neither overwritten nor touched.
            Image::Format(body) => match cell {
                Some((r, c)) => Payload::CellText { cell: (r, c), body: serde_json::to_value(body).ok()? },
                None => Payload::ShapeField { field: ShapeField::Text, value: serde_json::to_value(body).ok()? },
            },
            Image::Table(table) => Payload::ShapeField { field: ShapeField::Table, value: serde_json::to_value(table).ok()? },
            // A SmartArt group is one value: the diagram and the children generated from it. The
            // whole group keeps the child ids and any imported drawing, which the diagram value
            // alone cannot reproduce.
            Image::SmartArt(value) => Payload::ShapeField { field: ShapeField::SmartArt, value: value.clone() },
            Image::Animation(steps) => Payload::ShapeAnimations { animations: steps.iter().map(crate::animation::spec).collect::<Option<Vec<_>>>()? },
            Image::SlideAnimations(steps) => {
                Payload::SlideAnimations { animations: steps.iter().map(crate::animation::spec).collect::<Option<Vec<_>>>()? }
            }
            // Notes are a slide's own body: the payload names the slide and carries the whole body.
            Image::Notes(body) => Payload::NotesSet { notes: serde_json::to_value(body).ok()? },
        })
    }
}

/// The operation that puts back one step's before or after image.
///
/// One shape value keeps the single-object `ShapeField` payload; a group's appearance step is one
/// `ShapeFields` merge so its members are restored atomically; a whole shape (or several) comes
/// back in one `ShapeRestore` or goes in one `ShapeDelete`; a slide is a `SlideRestore` or a
/// `SlideDelete`; a transition step is one `SlideTransition`, the same payload the edit carried,
/// with the whole model value the step replaced.
pub fn restores(changes: &[Change]) -> Option<Restore> {
    match changes {
        [Change::Shape(at)] => {
            Some(Restore { slide: Some(at.slide.clone()), shape: Some(at.shape.clone()), shapes: Vec::new(), payload: at.image.payload(at.cell)? })
        }
        [Change::ShapeBody(at)] => body_restore(&[at]),
        [Change::GroupSubtree(at)] => Some(group_restore(at)),
        [Change::Slide(at)] => {
            let payload = match &at.slide {
                Some(slide) => {
                    Payload::SlideRestore { index: at.index, slide: slide.clone(), sections: at.sections.clone(), shows: at.shows.clone() }
                }
                None => Payload::SlideDelete {},
            };
            Some(Restore { slide: Some(at.key.clone()), shape: None, shapes: Vec::new(), payload })
        }
        [Change::SlideTransition(at)] => {
            let transition = match &at.transition {
                Some(value) => Some(crate::transition::spec(value)?),
                None => None,
            };
            Some(Restore { slide: Some(at.slide.clone()), shape: None, shapes: Vec::new(), payload: Payload::SlideTransition { transition } })
        }
        [Change::Notes(at)] => Some(Restore {
            slide: Some(at.slide.clone()),
            shape: None,
            shapes: Vec::new(),
            payload: Payload::NotesSet { notes: serde_json::to_value(&at.notes).ok()? },
        }),
        [Change::SlideAnimations(at)] => Some(Restore {
            slide: Some(at.slide.clone()),
            shape: None,
            shapes: Vec::new(),
            payload: Payload::SlideAnimations { animations: at.animations.iter().map(crate::animation::spec).collect::<Option<Vec<_>>>()? },
        }),
        [Change::Font(at)] => Some(Restore {
            slide: None,
            shape: None,
            shapes: Vec::new(),
            payload: Payload::FontRestore {
                typeface: at.typeface.clone(),
                font: match &at.font {
                    Some(entry) => Some(serde_json::to_value(entry).ok()?),
                    None => None,
                },
            },
        }),
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
        many if many.iter().all(|change| matches!(change, Change::Shape(Snapshot { image: Image::Format(_), cell: Some(_), .. }))) => {
            // A `format.*` step over N cells of one table: every cell's whole body is one image,
            // and they go back in one `shape.setCellBodies`, so a neighbour is never touched.
            let mut slide: Option<&SlideKey> = None;
            let mut shape: Option<&ShapeKey> = None;
            let mut cells = Vec::with_capacity(many.len());
            for change in many {
                let Change::Shape(at) = change else { return None };
                let Image::Format(body) = &at.image else { return None };
                let cell = at.cell?;
                if slide.is_some_and(|s| s != &at.slide) || shape.is_some_and(|s| s != &at.shape) {
                    return None;
                }
                slide = Some(&at.slide);
                shape = Some(&at.shape);
                cells.push(deckcraft_collab_proto::CellBody { cell, body: serde_json::to_value(body).ok()? });
            }
            Some(Restore { slide: Some(slide?.clone()), shape: shape.cloned(), shapes: Vec::new(), payload: Payload::CellsText { cells } })
        }
        many => {
            // A step over several objects of one slide is restored atomically. `arrange.align`
            // and `arrange.distribute` record one geometry image per moved shape and go back in
            // one `shape.setBoundsMany`; a group's fill/outline records an exact model value per
            // member and goes back in one `shape.setFields`.
            let mut slide: Option<&SlideKey> = None;
            for change in many {
                let Change::Shape(at) = change else { return None };
                if slide.is_some_and(|s| s != &at.slide) {
                    return None;
                }
                slide = Some(&at.slide);
            }
            let slide = slide?.clone();
            if many.iter().all(|change| matches!(change, Change::Shape(Snapshot { image: Image::Xfrm { .. }, .. }))) {
                let mut items = Vec::with_capacity(many.len());
                for change in many {
                    let Change::Shape(at) = change else { return None };
                    let Image::Xfrm { x, y, w, h } = at.image else { return None };
                    items.push(ShapeBounds { shape: at.shape.clone(), x, y, w, h });
                }
                return Some(Restore {
                    slide: Some(slide),
                    shape: None,
                    shapes: items.iter().map(|b| b.shape.clone()).collect(),
                    payload: Payload::ShapesSetBounds { items },
                });
            }
            let mut items = Vec::with_capacity(many.len());
            for change in many {
                let Change::Shape(at) = change else { return None };
                match at.image.payload(at.cell)? {
                    Payload::ShapeField { field, value } => items.push(ShapeFieldItem { shape: at.shape.clone(), field, value }),
                    _ => return None,
                }
            }
            Some(Restore { slide: Some(slide), shape: None, shapes: Vec::new(), payload: Payload::ShapeFields { items } })
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
        return Some(Restore { slide: Some(slide), shape: None, shapes, payload: Payload::ShapeDelete {} });
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
    Some(Restore { slide: Some(slide), shape: None, shapes: Vec::new(), payload: Payload::ShapeRestore { items, remove: Vec::new() } })
}

/// One operation that moves between a group and its members. The payload removes the shapes of
/// the *other* side and restores the images of this one in the same atomic step: undoing a group
/// removes the group and puts its members back at the top level; redoing it removes the members
/// and puts the whole group image back.
fn group_restore(at: &GroupImage) -> Restore {
    match &at.group {
        Some(value) => {
            let item =
                ShapeRestoreItem { shape: at.key.clone(), parent: None, index: at.index, value: value.clone(), animations: at.animations.clone() };
            Restore {
                slide: Some(at.slide.clone()),
                shape: None,
                shapes: Vec::new(),
                payload: Payload::ShapeRestore { items: vec![item], remove: at.children.clone() },
            }
        }
        None => {
            let items = at
                .members
                .iter()
                .map(|member| ShapeRestoreItem {
                    shape: member.key.clone(),
                    parent: None,
                    index: member.index,
                    value: member.shape.clone().unwrap_or(serde_json::Value::Null),
                    animations: member.animations.clone(),
                })
                .collect();
            Restore {
                slide: Some(at.slide.clone()),
                shape: None,
                shapes: Vec::new(),
                payload: Payload::ShapeRestore { items, remove: vec![at.key.clone()] },
            }
        }
    }
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
        assert_eq!(restore.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
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
        assert_eq!(restore.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        assert!(restore.shape.is_none(), "the restore addresses the slide and every member");
        let Payload::ShapeFields { items } = restore.payload else { panic!("one merge payload") };
        let keys: Vec<&str> = items.iter().map(|i| i.shape.0.as_str()).collect();
        assert_eq!(keys, ["g", "h1"]);
        assert!(items.iter().all(|i| i.field == ShapeField::Fill));
    }

    #[test]
    fn a_group_step_moves_between_the_group_and_its_members() {
        let member = |key: &str, id: u64, index: u32| ShapeImage {
            slide: "s1".into(),
            key: key.into(),
            parent: None,
            index,
            shape: Some(serde_json::json!({"id": id})),
            animations: Vec::new(),
        };
        let step = Step {
            op_id: "op".into(),
            version: 1,
            before: vec![Change::GroupSubtree(GroupImage {
                slide: "s1".into(),
                key: "g".into(),
                index: 0,
                group: None,
                children: Vec::new(),
                members: vec![member("h1", 1, 0), member("h2", 2, 1)],
                animations: Vec::new(),
            })],
            after: vec![Change::GroupSubtree(GroupImage {
                slide: "s1".into(),
                key: "g".into(),
                index: 1,
                group: Some(serde_json::json!({"id": 9, "kind": {"kind": "group", "children": [{"id": 1}, {"id": 2}]}})),
                children: vec!["h1".into(), "h2".into()],
                members: Vec::new(),
                animations: vec![serde_json::json!({"shape": 1})],
            })],
        };
        let undo = step.restoring_payload().expect("undo of a group");
        assert_eq!(undo.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        let Payload::ShapeRestore { items, remove } = undo.payload else { panic!("one atomic restore") };
        assert_eq!(remove, vec![ShapeKey("g".into())], "the group goes out first");
        let keys: Vec<&str> = items.iter().map(|i| i.shape.0.as_str()).collect();
        assert_eq!(keys, ["h1", "h2"]);
        assert!(items.iter().all(|i| i.parent.is_none() && i.index < 2));

        let redo = step.reapplying_payload().expect("redo of a group");
        let Payload::ShapeRestore { items, remove } = redo.payload else { panic!("one atomic restore") };
        assert_eq!(items.len(), 1, "one image carries the whole group");
        assert_eq!(items[0].shape.0, "g");
        assert_eq!(items[0].index, 1);
        assert_eq!(items[0].animations.len(), 1);
        assert_eq!(remove, vec![ShapeKey("h1".into()), ShapeKey("h2".into())], "the members go out first");
    }

    #[test]
    fn a_group_image_compares_the_group_or_every_member() {
        let member = |key: &str, id: u64| ShapeImage {
            slide: "s1".into(),
            key: key.into(),
            parent: None,
            index: 0,
            shape: Some(serde_json::json!({"id": id})),
            animations: Vec::new(),
        };
        let closed = Change::GroupSubtree(GroupImage {
            slide: "s1".into(),
            key: "g".into(),
            index: 0,
            group: Some(serde_json::json!({"id": 9})),
            children: vec!["h1".into()],
            members: Vec::new(),
            animations: Vec::new(),
        });
        let edited = Change::GroupSubtree(GroupImage {
            slide: "s1".into(),
            key: "g".into(),
            index: 0,
            group: Some(serde_json::json!({"id": 9, "name": "Edited"})),
            children: vec!["h1".into()],
            members: Vec::new(),
            animations: Vec::new(),
        });
        assert!(closed.same_state(&closed));
        assert!(!closed.same_state(&edited), "a member was edited inside the group");

        let open = Change::GroupSubtree(GroupImage {
            slide: "s1".into(),
            key: "g".into(),
            index: 0,
            group: None,
            children: Vec::new(),
            members: vec![member("h1", 1)],
            animations: Vec::new(),
        });
        let open_edited = Change::GroupSubtree(GroupImage {
            slide: "s1".into(),
            key: "g".into(),
            index: 0,
            group: None,
            children: Vec::new(),
            members: vec![member("h1", 2)],
            animations: Vec::new(),
        });
        assert!(open.same_state(&open));
        assert!(!open.same_state(&open_edited), "a former member changed after the ungroup");
        assert!(!open.same_state(&closed), "the group is present where the image says absent");
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
        assert_eq!(restore.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        assert!(restore.shape.is_none());
        let Payload::SlideRestore { index, sections, .. } = restore.payload else { panic!("a slide restore") };
        assert_eq!((index, sections), (2, vec![(0, 1)]));
        let redo = s.reapplying_payload().expect("slide delete");
        assert_eq!(redo.payload, Payload::SlideDelete {});
    }

    #[test]
    fn a_transition_step_restores_the_whole_value_it_replaced() {
        let fade = Transition { kind: "fade".into(), duration_ms: 700, advance_after_ms: Some(5000), ..Transition::default() };
        let step = Step {
            op_id: "op".into(),
            version: 3,
            before: vec![Change::SlideTransition(TransitionImage { slide: "s2".into(), transition: Some(fade.clone()) })],
            after: vec![Change::SlideTransition(TransitionImage { slide: "s2".into(), transition: None })],
        };
        let undo = step.restoring_payload().expect("transition restore");
        assert_eq!(undo.slide.as_ref().map(|s| s.0.as_str()), Some("s2"));
        assert!(undo.shape.is_none());
        let Payload::SlideTransition { transition: Some(value) } = undo.payload else { panic!("one whole transition") };
        assert_eq!((value.kind.as_str(), value.duration_ms, value.advance_after_ms), ("fade", 700, Some(5000)));
        let redo = step.reapplying_payload().expect("transition redo");
        assert_eq!(redo.payload, Payload::SlideTransition { transition: None });

        let at = Change::SlideTransition(TransitionImage { slide: "s2".into(), transition: Some(fade.clone()) });
        assert!(at.same_state(&at));
        let other = Change::SlideTransition(TransitionImage { slide: "s2".into(), transition: None });
        assert!(!at.same_state(&other), "someone replaced the transition after the step");
        let elsewhere = Change::SlideTransition(TransitionImage { slide: "s1".into(), transition: Some(fade) });
        assert!(!at.same_state(&elsewhere), "the image names one slide");
    }

    #[test]
    fn an_animation_step_restores_the_shapes_whole_list_around_it() {
        let fade = Animation { shape: deckcraft_model::ShapeId(4), effect: "fade".into(), ..Animation::default() };
        let pulse = Animation { shape: deckcraft_model::ShapeId(4), effect: "pulse".into(), duration_ms: 900, ..Animation::default() };
        let step = Step {
            op_id: "op".into(),
            version: 2,
            before: vec![Change::Shape(Snapshot { slide: "s1".into(), shape: "k4".into(), cell: None, image: Image::Animation(vec![fade.clone()]) })],
            after: vec![Change::Shape(Snapshot {
                slide: "s1".into(),
                shape: "k4".into(),
                cell: None,
                image: Image::Animation(vec![fade, pulse.clone()]),
            })],
        };
        let undo = step.restoring_payload().expect("animation restore");
        assert_eq!(undo.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        assert_eq!(undo.shape.as_ref().map(|k| k.0.as_str()), Some("k4"));
        let Payload::ShapeAnimations { animations } = &undo.payload else { panic!("the whole step list: {:?}", undo.payload) };
        assert_eq!(animations.len(), 1, "the step's before image keeps the other steps");
        assert_eq!(animations[0].get("effect").and_then(serde_json::Value::as_str), Some("fade"));
        let redo = step.reapplying_payload().expect("animation redo");
        assert!(matches!(&redo.payload, Payload::ShapeAnimations { animations } if animations.len() == 2));

        let at = Change::Shape(Snapshot { slide: "s1".into(), shape: "k4".into(), cell: None, image: Image::Animation(vec![pulse.clone()]) });
        assert!(at.same_state(&at));
        let other = Change::Shape(Snapshot {
            slide: "s1".into(),
            shape: "k4".into(),
            cell: None,
            image: Image::Animation(vec![Animation { effect: "wipe".into(), ..pulse }]),
        });
        assert!(!at.same_state(&other), "someone changed the shape's steps after the undo");
    }

    #[test]
    fn a_slide_animation_step_restores_the_whole_interleaved_sequence() {
        let a = deckcraft_model::ShapeId(4);
        let b = deckcraft_model::ShapeId(7);
        let fade = Animation { shape: a, effect: "fade".into(), ..Animation::default() };
        let zoom = Animation { shape: b, effect: "zoom".into(), ..Animation::default() };
        let pulse = Animation { shape: a, effect: "pulse".into(), duration_ms: 900, ..Animation::default() };
        let step = Step {
            op_id: "op".into(),
            version: 2,
            before: vec![Change::SlideAnimations(SlideAnimationsImage {
                slide: "s1".into(),
                animations: vec![fade.clone(), zoom.clone(), pulse.clone()],
            })],
            after: vec![Change::SlideAnimations(SlideAnimationsImage {
                slide: "s1".into(),
                animations: vec![zoom.clone(), fade.clone(), pulse.clone()],
            })],
        };
        let undo = step.restoring_payload().expect("slide animation restore");
        assert_eq!(undo.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        assert!(undo.shape.is_none(), "the address is the slide");
        let Payload::SlideAnimations { animations } = &undo.payload else { panic!("the whole sequence: {:?}", undo.payload) };
        let effects: Vec<&str> = animations.iter().filter_map(|a| a.get("effect").and_then(serde_json::Value::as_str)).collect();
        assert_eq!(effects, ["fade", "zoom", "pulse"], "the before sequence comes back in its order");
        assert_eq!(animations[1].get("shape").and_then(serde_json::Value::as_u64), Some(7), "each step keeps its own shape");
        let redo = step.reapplying_payload().expect("slide animation redo");
        assert!(matches!(&redo.payload, Payload::SlideAnimations { animations } if animations.len() == 3));
        assert_eq!(redo.slide, undo.slide);
        assert!(redo.shape.is_none());

        let at = Change::SlideAnimations(SlideAnimationsImage { slide: "s1".into(), animations: vec![fade.clone(), zoom.clone()] });
        assert!(at.same_state(&at));
        let moved = Change::SlideAnimations(SlideAnimationsImage { slide: "s1".into(), animations: vec![zoom, fade.clone()] });
        assert!(!at.same_state(&moved), "someone reordered the slide's steps after the undo");
        let elsewhere = Change::SlideAnimations(SlideAnimationsImage { slide: "s2".into(), animations: vec![fade, pulse] });
        assert!(!at.same_state(&elsewhere), "the image names one slide");
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
        assert_eq!(restore.slide.as_ref().map(|s| s.0.as_str()), Some("s1"));
        assert!(restore.shape.is_none(), "the payload names the shape");
        assert!(restore.shapes.is_empty());
        let Payload::ShapeRestore { items, .. } = restore.payload else { panic!("one shape restore") };
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
