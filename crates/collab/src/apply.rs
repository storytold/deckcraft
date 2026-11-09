//! The DeckCraft collision adapter: translates a normalised wire operation into the engine
//! command that performs it, then answers with an authoritative delta (ADR 0001, ADR 0004).
//!
//! The adapter is what makes "the server executes, the client only previews" true for `.pptx`.
//! Three rules shape it:
//!
//! 1. **Explicit address.** Everything is resolved from `Target` keys; nothing is read from the
//!    selection, so the same operation means the same thing on every replica.
//! 2. **Derived state stays home.** Group frames, connector routes and autofit are recomputed by
//!    the engine on every replica and never travel in a delta (ADR 0001 §4).
//! 3. **Order is an anchor on the wire, an index in the engine.** `slide.move` and
//!    `arrange.reorder` address positions; the adapter turns a `before` key into the index the
//!    command expects, against the authoritative head.

use std::collections::HashSet;

use base64::Engine as _;
use deckcraft_collab_proto::{
    ChangeKind, Delta, HistoryAction, MediaKind, OpEnvelope, OpKind, Payload, RejectCode, ShapeField, ShapeKey, SlideKey, Target,
};
use deckcraft_engine::{Session, Target as EngineTarget, TextSel};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, SlideId, find_shape, parent_of};
use serde_json::{Value, json};

use crate::delta::delta;
use crate::history::{Change, GroupImage, Histories, Image, Restore, ShapeImage, SlideImage, Snapshot, Step};
use crate::keys::{KeyIssuer, KeyMap};
use crate::media::MediaCache;

/// Why the adapter refused an operation. The server turns this into `Rejected`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub code: RejectCode,
    pub message: String,
    pub retryable: bool,
    pub hint_version: Option<u64>,
}

impl Rejection {
    /// The address or the base version moved; refetch and retry.
    pub fn stale(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Stale, message: message.into(), retryable: true, hint_version: version }
    }

    /// The object is gone for good; a retry cannot help (ADR 0002 §3).
    pub fn dead(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Stale, message: message.into(), retryable: false, hint_version: version }
    }

    pub fn forbidden(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Forbidden, message: message.into(), retryable: false, hint_version: version }
    }

    pub fn invalid(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Invalid, message: message.into(), retryable: false, hint_version: version }
    }

    pub fn unsupported(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Unsupported, message: message.into(), retryable: false, hint_version: version }
    }

    /// The operation refers to picture bytes this replica does not have. Fetching them and
    /// sending the same operation again can succeed, so the refusal is retryable.
    pub fn missing_media(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::Unsupported, message: message.into(), retryable: true, hint_version: version }
    }

    /// Undo would overwrite someone else's later change (ADR 0010 §3).
    pub fn undo_conflict(message: impl Into<String>, version: Option<u64>) -> Self {
        Rejection { code: RejectCode::UndoConflict, message: message.into(), retryable: false, hint_version: version }
    }
}

/// The adapter for one document session.
#[derive(Debug)]
pub struct Adapter {
    keys: KeyMap,
    issuer: KeyIssuer,
    /// Slides that were deleted in this session. Their keys never come back (ADR 0002 §3).
    dead_slides: HashSet<SlideKey>,
    /// Per-actor undo/redo (ADR 0010). One participant's undo never takes back another's edit.
    history: Histories,
    /// Picture bytes this replica holds, by digest. An operation only refers to them.
    media: MediaCache,
}

impl Adapter {
    /// `prefix` separates the keys of one session from another; the server passes a random one.
    pub fn new(prefix: impl Into<String>) -> Self {
        Adapter {
            keys: KeyMap::new(),
            issuer: KeyIssuer::new(prefix),
            dead_slides: HashSet::new(),
            history: Histories::default(),
            media: MediaCache::new(),
        }
    }

    /// Picture bytes this adapter can run an operation against.
    pub fn media(&self) -> &MediaCache {
        &self.media
    }

    /// The cache to fill before applying a `picture.insert` (server upload, local replica, test).
    pub fn media_mut(&mut self) -> &mut MediaCache {
        &mut self.media
    }

    /// Issue keys for everything in `doc`. Called at join and after every applied operation.
    pub fn join(&mut self, doc: &Presentation) {
        self.keys.sync(doc, &mut self.issuer);
    }

    /// The key map and issuer counter, for a base transfer (ADR-0002, ADR-0005).
    pub fn snapshot_keys(&self) -> deckcraft_collab_proto::SnapshotKeys {
        self.keys.to_snapshot(&self.issuer)
    }

    /// Replace the map with the one a base brought. Keys are session-scoped and cannot be
    /// re-derived after structural edits, so the server's map wins over the local one.
    pub fn restore_keys(&mut self, keys: &deckcraft_collab_proto::SnapshotKeys) {
        self.keys = KeyMap::from_snapshot(keys);
        self.issuer = KeyIssuer::with_next(self.issuer.prefix(), keys.next);
    }

    pub fn keys(&self) -> &KeyMap {
        &self.keys
    }

    /// The per-actor undo/redo stacks (diagnostics and tests).
    pub fn history(&self) -> &Histories {
        &self.history
    }

    /// Apply an operation to `session` and return the authoritative delta.
    ///
    /// `head` is the document version the operation was ordered against: a `structural`
    /// operation is only accepted when it was sent against the current head (ADR 0004 §2).
    pub fn apply(&mut self, session: &mut Session, head: u64, actor: &str, op: &OpEnvelope) -> Result<Delta, Rejection> {
        let class = class_of(&op.payload);
        if class != op.kind {
            return Err(Rejection::invalid(format!("`{}` is a {:?} operation, sent as {:?}", op.payload.command(), class, op.kind), Some(head)));
        }
        if op.kind == OpKind::Structural && op.base_version != head {
            return Err(Rejection::stale(format!("structural `{}` needs baseVersion == head ({head})", op.payload.command()), Some(head)));
        }

        // The named step must be the actor's current history head, or the history would move
        // for an operation it never recorded.
        let expected = match &op.history {
            Some(HistoryAction::Undo { op_id }) => Some((true, op_id)),
            Some(HistoryAction::Redo { op_id }) => Some((false, op_id)),
            None => None,
        };
        if let Some((undo, op_id)) = expected {
            let actor_history = self.history.get(actor);
            let head_of = |undo: bool| match (undo, actor_history) {
                (true, Some(h)) => h.peek_undo().map(|s| s.op_id.as_str()),
                (false, Some(h)) => h.peek_redo().map(|s| s.op_id.as_str()),
                _ => None,
            };
            if head_of(undo) != Some(op_id.as_str()) {
                return Err(Rejection::undo_conflict("that operation is not the latest step of this history", Some(head)));
            }
        }
        if op.history.is_some()
            || matches!(
                op.payload,
                Payload::ShapeField { .. } | Payload::ShapeFields { .. } | Payload::ShapeRestore { .. } | Payload::SlideRestore { .. }
            )
        {
            // A history move is the server's own before/after image, never a client edit: the
            // exact value, the object and a structural restore must all match the recorded step.
            self.check_restore(actor, op, head)?;
        }

        let before = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        let out = self.apply_confirmed(session, head, actor, op)?;
        let after = session.doc().map_err(|_| Rejection::invalid("the document closed while applying", Some(head)))?.doc.clone();

        match &op.history {
            Some(HistoryAction::Undo { .. }) => self.history.of(actor).commit_undo(),
            Some(HistoryAction::Redo { .. }) => self.history.of(actor).commit_redo(),
            None => {
                // What this step replaced and what it left, for the actor's own undo (ADR 0010).
                // A merge step may touch several objects (a group's members) or a whole slide.
                if let Some((old, new)) = self.capture_step(&before, &after, op) {
                    self.history.of(actor).record(Step { op_id: op.op_id.clone(), version: head.saturating_add(1), before: old, after: new });
                }
            }
        }
        Ok(out)
    }

    /// Apply one already-confirmed operation and answer its delta, without touching the actor's
    /// history. The client uses this for `applied`: the server sent the command it actually ran,
    /// so every replica can run the same command in the same order and converge. The server's
    /// own [`Self::apply`] uses it too, then records the undo step.
    pub fn apply_confirmed(&mut self, session: &mut Session, head: u64, _actor: &str, op: &OpEnvelope) -> Result<Delta, Rejection> {
        let before = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        self.run(session, &before, head, op)?;
        let after = session.doc().map_err(|_| Rejection::invalid("the document closed while applying", Some(head)))?.doc.clone();
        self.keys.sync(&after, &mut self.issuer);
        let out = delta(&self.keys, &before, &after);
        self.bury(&before, &out);
        // A restored slide or shape comes back from the grave: a late operation on it is valid
        // again, and it keeps the key it had, so the step that restored it stays addressable.
        for change in &out.changes {
            if change.kind != ChangeKind::Inserted {
                continue;
            }
            match &change.shape {
                None => {
                    self.dead_slides.remove(&change.slide);
                }
                Some(shape) => {
                    if let Some(slide) = self.keys.slide_id(&change.slide)
                        && let Some(id) = self.keys.shape_id(slide, shape)
                    {
                        self.keys.revive_shape(slide, id);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Build the operation that takes back `actor`'s last step (ADR 0010).
    ///
    /// Refused with `undo_conflict` when the objects no longer hold what that step left: someone
    /// else changed one of them after, and their edit must not be overwritten. The history is only
    /// moved when the returned operation is applied and accepted, so a refusal changes nothing.
    pub fn undo(&mut self, session: &Session, head: u64, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let step = self.history.of(actor).peek_undo().cloned().ok_or_else(|| Rejection::invalid("nothing to undo", Some(head)))?;
        let doc = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        if !self.step_matches(&doc, &step.after) {
            return Err(Rejection::undo_conflict("someone else changed this object after the step", Some(head)));
        }
        let Restore { slide, shape, shapes, payload } =
            step.restoring_payload().ok_or_else(|| Rejection::invalid("the recorded value cannot be restored", Some(head)))?;
        Ok(OpEnvelope {
            op_id: op_id.to_string(),
            doc: String::new(),
            base_version: head,
            kind: class_of(&payload),
            target: Target { slide: Some(slide), shape, shapes, ..Target::default() },
            payload,
            history: Some(HistoryAction::Undo { op_id: step.op_id.clone() }),
        })
    }

    /// Build the operation that re-applies `actor`'s last undone step.
    pub fn redo(&mut self, session: &Session, head: u64, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let step = self.history.of(actor).peek_redo().cloned().ok_or_else(|| Rejection::invalid("nothing to redo", Some(head)))?;
        let doc = session.doc().map_err(|_| Rejection::invalid("no document open", Some(head)))?.doc.clone();
        if !self.step_matches(&doc, &step.before) {
            return Err(Rejection::undo_conflict("someone else changed this object after the undo", Some(head)));
        }
        let Restore { slide, shape, shapes, payload } =
            step.reapplying_payload().ok_or_else(|| Rejection::invalid("the recorded value cannot be restored", Some(head)))?;
        Ok(OpEnvelope {
            op_id: op_id.to_string(),
            doc: String::new(),
            base_version: head,
            kind: class_of(&payload),
            target: Target { slide: Some(slide), shape, shapes, ..Target::default() },
            payload,
            history: Some(HistoryAction::Redo { op_id: step.op_id.clone() }),
        })
    }

    /// Whether the document still holds every value a step left (undo) or replaced (redo).
    fn step_matches(&self, doc: &Presentation, expected: &[Change]) -> bool {
        !expected.is_empty() && expected.iter().all(|at| self.capture_change(doc, at).is_some_and(|current| current.same_state(at)))
    }

    /// A restore payload may only repeat what this actor's own latest step recorded: it is a
    /// history move, not a way to set any model value or to address another object.
    fn check_restore(&self, actor: &str, op: &OpEnvelope, head: u64) -> Result<(), Rejection> {
        let history = self.history.get(actor);
        let step = match (&op.history, history) {
            (Some(HistoryAction::Undo { .. }), Some(h)) => h.peek_undo(),
            (Some(HistoryAction::Redo { .. }), Some(h)) => h.peek_redo(),
            _ => None,
        }
        .ok_or_else(|| Rejection::invalid("a restore is only sent as an undo or redo of this actor's own step", Some(head)))?;
        let redo = matches!(op.history, Some(HistoryAction::Redo { .. }));
        let expected = if redo { step.reapplying_payload() } else { step.restoring_payload() };
        match expected {
            Some(expected)
                if op.target.slide.as_ref() == Some(&expected.slide)
                    && op.target.shape == expected.shape
                    && op.target.shapes == expected.shapes
                    && op.payload == expected.payload =>
            {
                Ok(())
            }
            Some(_) => Err(Rejection::invalid("the restore does not match the value and object its step recorded", Some(head))),
            None => Err(Rejection::invalid("the recorded value cannot be restored", Some(head))),
        }
    }

    /// The before and after images of one operation, when it is undoable in v1.
    ///
    /// Shape value steps record one image per object a merge touched (a group's fill/outline
    /// records every member). `slide.delete`, `slide.new` and `slide.duplicate` record the whole
    /// slide, which is what structural undo restores (RFC §13, item 4). A shape insert (preset,
    /// picture, media) and `edit.delete` record the whole shape with its place in the z-order and
    /// its animations (RFC §6).
    fn capture_step(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let (cell, image) = match &op.payload {
            Payload::ShapeMove { .. } | Payload::ShapeSetBounds { .. } => (None, Image::Xfrm { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }),
            Payload::ShapesSetBounds { items } => return self.capture_bounds_step(before, after, op, items),
            Payload::TextSet { cell, .. } => (*cell, Image::Text(String::new())),
            Payload::TextInsert { .. } | Payload::TextDelete { .. } => (None, Image::Text(String::new())),
            Payload::ShapeFill { .. } => return self.capture_appearance(before, after, op, Image::Fill(None)),
            Payload::ShapeLine { .. } => return self.capture_appearance(before, after, op, Image::Line(None)),
            Payload::FormatSet { .. } => (None, Image::Format(TextBody::default())),
            Payload::ShapeField { field, .. } => (None, image_of_field(*field)),
            Payload::ShapeInsert { .. } | Payload::PictureInsert { .. } | Payload::MediaInsert { .. } => {
                return self.capture_shape_insert(before, after, op);
            }
            Payload::ShapeDelete {} => return self.capture_shape_delete(before, after, op),
            Payload::Group {} => return self.capture_group(before, after, op),
            Payload::Ungroup {} => return self.capture_ungroup(before, after, op),
            Payload::SlideDelete {} => return self.capture_slide_delete(before, after, op),
            Payload::SlideInsert { .. } | Payload::SlideDuplicate {} => return self.capture_slide_insert(before, after),
            _ => return None,
        };
        let template = Snapshot { slide: op.target.slide.clone()?, shape: op.target.shape.clone()?, cell, image };
        Some((vec![Change::Shape(self.capture_snapshot(before, &template)?)], vec![Change::Shape(self.capture_snapshot(after, &template)?)]))
    }

    /// Undo of `arrange.align`/`arrange.distribute`: one geometry image per target, so the step
    /// moves the whole selection back in one operation (`shape.setBoundsMany`) instead of one
    /// step per object. A member that went away makes the step unrecordable.
    fn capture_bounds_step(
        &self,
        before: &Presentation,
        after: &Presentation,
        op: &OpEnvelope,
        items: &[deckcraft_collab_proto::ShapeBounds],
    ) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide = op.target.slide.clone()?;
        let mut was = Vec::with_capacity(items.len());
        let mut now = Vec::with_capacity(items.len());
        for item in items {
            let template =
                Snapshot { slide: slide.clone(), shape: item.shape.clone(), cell: None, image: Image::Xfrm { x: 0.0, y: 0.0, w: 0.0, h: 0.0 } };
            was.push(Change::Shape(self.capture_snapshot(before, &template)?));
            now.push(Change::Shape(self.capture_snapshot(after, &template)?));
        }
        Some((was, now))
    }

    /// Undo of `shape.insert`, `insert.picture` or `insert.video`/`insert.audio`: the one top-most
    /// shape the step added, with its z-order and animations.
    fn capture_shape_insert(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide_key = op.target.slide.clone()?;
        let slide_id = self.keys.slide_id(&slide_key)?;
        let old = before.slide(slide_id)?;
        let new = after.slide(slide_id)?;
        let added = top_most_added(&old.shapes, &new.shapes)?;
        let key = self.keys.shape_key(slide_id, added)?.clone();
        let now = self.shape_image(after, &slide_key, slide_id, added)?;
        Some((vec![Change::ShapeBody(absent_shape(slide_key, key))], vec![Change::ShapeBody(now)]))
    }

    /// Undo of `edit.delete`: every top-most shape the step removed, with its whole subtree, its
    /// sibling position and the animations that named it. A group's image carries its children,
    /// so the step records only the shapes whose parent survived.
    fn capture_shape_delete(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide_key = op.target.slide.clone()?;
        let slide_id = self.keys.slide_id(&slide_key)?;
        let old = before.slide(slide_id)?;
        let new = after.slide(slide_id)?;
        let gone = top_most_gone(&old.shapes, &new.shapes);
        if gone.is_empty() {
            return None;
        }
        let mut was = Vec::with_capacity(gone.len());
        let mut now = Vec::with_capacity(gone.len());
        for id in gone {
            let key = self.keys.shape_key(slide_id, id)?.clone();
            was.push(Change::ShapeBody(self.shape_image(before, &slide_key, slide_id, id)?));
            now.push(Change::ShapeBody(absent_shape(slide_key.clone(), key)));
        }
        Some((was, now))
    }

    /// Undo of `arrange.group`: the new group on one side and the former members on the other.
    /// Both sides are whole-subtree images, so one atomic `shape.restore` moves between them
    /// (`remove` takes the other side's shapes out before the images go in).
    fn capture_group(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide_key = op.target.slide.clone()?;
        let slide_id = self.keys.slide_id(&slide_key)?;
        let old = before.slide(slide_id)?;
        let new = after.slide(slide_id)?;
        let group_id = top_most_added(&old.shapes, &new.shapes)?;
        let group_key = self.keys.shape_key(slide_id, group_id)?.clone();
        let image = self.shape_image(after, &slide_key, slide_id, group_id)?;
        let child_ids: Vec<ShapeId> = find_shape(&new.shapes, group_id)?.children().iter().map(|child| child.id).collect();
        if child_ids.len() < 2 {
            return None;
        }
        let children = child_ids.iter().map(|id| self.keys.shape_key(slide_id, *id).cloned()).collect::<Option<Vec<_>>>()?;
        let members = child_ids.iter().map(|id| self.shape_image(before, &slide_key, slide_id, *id)).collect::<Option<Vec<_>>>()?;
        let was = GroupImage {
            slide: slide_key.clone(),
            key: group_key.clone(),
            index: 0,
            group: None,
            children: Vec::new(),
            members,
            animations: Vec::new(),
        };
        let now = GroupImage {
            slide: slide_key,
            key: group_key,
            index: image.index,
            group: image.shape,
            children,
            members: Vec::new(),
            animations: image.animations,
        };
        Some((vec![Change::GroupSubtree(was)], vec![Change::GroupSubtree(now)]))
    }

    /// Undo of `arrange.ungroup`: the group on one side and its members after ungrouping on the
    /// other. The members carry the geometry `arrange.ungroup` gave them, so a redo puts the same
    /// values back instead of recomputing them.
    fn capture_ungroup(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide_key = op.target.slide.clone()?;
        let slide_id = self.keys.slide_id(&slide_key)?;
        let old = before.slide(slide_id)?;
        let new = after.slide(slide_id)?;
        let gone = top_most_gone(&old.shapes, &new.shapes);
        let [group_id] = gone.as_slice() else { return None };
        let group_key = self.keys.shape_key(slide_id, *group_id)?.clone();
        let child_ids: Vec<ShapeId> = find_shape(&old.shapes, *group_id)?.children().iter().map(|child| child.id).collect();
        if child_ids.len() < 2 {
            return None;
        }
        let children = child_ids.iter().map(|id| self.keys.shape_key(slide_id, *id).cloned()).collect::<Option<Vec<_>>>()?;
        let image = self.shape_image(before, &slide_key, slide_id, *group_id)?;
        let members = child_ids.iter().map(|id| self.shape_image(after, &slide_key, slide_id, *id)).collect::<Option<Vec<_>>>()?;
        let was = GroupImage {
            slide: slide_key.clone(),
            key: group_key.clone(),
            index: image.index,
            group: image.shape,
            children,
            members: Vec::new(),
            animations: image.animations,
        };
        let now = GroupImage { slide: slide_key, key: group_key, index: 0, group: None, children: Vec::new(), members, animations: Vec::new() };
        Some((vec![Change::GroupSubtree(was)], vec![Change::GroupSubtree(now)]))
    }

    /// The whole image of one present shape: its model value, the group that holds it, its place
    /// among that group's children and the animations that name it or a descendant (including an
    /// effect of another shape that it triggered).
    fn shape_image(&self, doc: &Presentation, slide_key: &SlideKey, slide_id: SlideId, shape_id: ShapeId) -> Option<ShapeImage> {
        let slide = doc.slide(slide_id)?;
        let shape = find_shape(&slide.shapes, shape_id)?;
        let (parent, index) = match deckcraft_model::parent_of(&slide.shapes, shape_id)? {
            None => (None, slide.shapes.iter().position(|s| s.id == shape_id)?),
            Some(group) => {
                let holder = find_shape(&slide.shapes, group)?;
                (self.keys.shape_key(slide_id, group).cloned(), holder.children().iter().position(|s| s.id == shape_id)?)
            }
        };
        let mut ids = Vec::new();
        walk_ids(std::slice::from_ref(shape), &mut ids, 0);
        let animations = slide
            .animations
            .iter()
            .filter(|a| ids.contains(&a.shape) || a.trigger.is_some_and(|t| ids.contains(&t)))
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect();
        Some(ShapeImage {
            slide: slide_key.clone(),
            key: self.keys.shape_key(slide_id, shape_id)?.clone(),
            parent,
            index: index as u32,
            shape: Some(serde_json::to_value(shape).ok()?),
            animations,
        })
    }

    /// The current state of one recorded whole-shape image, if this adapter knows its key.
    fn capture_shape_body(&self, doc: &Presentation, at: &ShapeImage) -> Option<ShapeImage> {
        let slide_id = self.keys.slide_id(&at.slide)?;
        let id = self.keys.shape_id(slide_id, &at.key)?;
        match doc.slide(slide_id).and_then(|slide| find_shape(&slide.shapes, id)) {
            Some(_) => self.shape_image(doc, &at.slide, slide_id, id),
            None => Some(absent_shape(at.slide.clone(), at.key.clone())),
        }
    }

    /// A group's fill or outline formats the group and every visible member; record one image
    /// per object and keep the ones the step actually changed.
    fn capture_appearance(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope, image: Image) -> Option<(Vec<Change>, Vec<Change>)> {
        let slide_key = op.target.slide.clone()?;
        let shape_key = op.target.shape.clone()?;
        let slide_id = self.keys.slide_id(&slide_key)?;
        let shape_id = self.keys.shape_id(slide_id, &shape_key)?;
        let slide = before.slide(slide_id)?;
        let shape = find_shape(&slide.shapes, shape_id)?;
        let mut templates = Vec::new();
        fn walk(shapes: &[Shape], slide_id: SlideId, slide_key: &SlideKey, image: &Image, keys: &KeyMap, out: &mut Vec<Snapshot>, depth: usize) {
            if depth > 64 {
                return;
            }
            for sh in shapes {
                if let Some(key) = keys.shape_key(slide_id, sh.id) {
                    out.push(Snapshot { slide: slide_key.clone(), shape: key.clone(), cell: None, image: image.clone() });
                }
                walk(sh.children(), slide_id, slide_key, image, keys, out, depth + 1);
            }
        }
        walk(std::slice::from_ref(shape), slide_id, &slide_key, &image, &self.keys, &mut templates, 0);
        let mut old = Vec::with_capacity(templates.len());
        let mut new = Vec::with_capacity(templates.len());
        for at in &templates {
            let (Some(was), Some(now)) = (self.capture_snapshot(before, at), self.capture_snapshot(after, at)) else {
                return None;
            };
            if was != now {
                old.push(Change::Shape(was));
                new.push(Change::Shape(now));
            }
        }
        (!old.is_empty()).then_some((old, new))
    }

    /// Undo of `slide.delete`: the whole slide was present before and is absent after.
    fn capture_slide_delete(&self, before: &Presentation, after: &Presentation, op: &OpEnvelope) -> Option<(Vec<Change>, Vec<Change>)> {
        let key = op.target.slide.clone()?;
        let id = self.keys.slide_id(&key)?;
        let was = self.slide_image(before, &key, id)?;
        debug_assert!(after.slide(id).is_none(), "the slide is gone after a delete");
        Some((vec![Change::Slide(was)], vec![Change::Slide(absent_slide(key))]))
    }

    /// Undo of `slide.new` or `slide.duplicate`: find the one slide the step added.
    fn capture_slide_insert(&self, before: &Presentation, after: &Presentation) -> Option<(Vec<Change>, Vec<Change>)> {
        let added: Vec<SlideId> = after.slides.iter().map(|s| s.id).filter(|id| before.slide(*id).is_none()).collect();
        let [id] = added.as_slice() else { return None };
        let key = self.keys.slide_key(*id)?.clone();
        let now = self.slide_image(after, &key, *id)?;
        Some((vec![Change::Slide(absent_slide(key))], vec![Change::Slide(now)]))
    }

    /// Read the current state of one recorded object, if this adapter knows its keys.
    fn capture_change(&self, doc: &Presentation, at: &Change) -> Option<Change> {
        match at {
            Change::Shape(snapshot) => self.capture_snapshot(doc, snapshot).map(Change::Shape),
            Change::ShapeBody(snapshot) => self.capture_shape_body(doc, snapshot).map(Change::ShapeBody),
            Change::GroupSubtree(snapshot) => self.capture_group_image(doc, snapshot).map(Change::GroupSubtree),
            Change::Slide(snapshot) => {
                let id = self.keys.slide_id(&snapshot.key)?;
                match doc.slide(id) {
                    Some(_) => Some(Change::Slide(self.slide_image(doc, &snapshot.key, id)?)),
                    None => Some(Change::Slide(absent_slide(snapshot.key.clone()))),
                }
            }
        }
    }

    /// The current state of a recorded group image: the group present with its children, or the
    /// group absent and its former members wherever they are now.
    fn capture_group_image(&self, doc: &Presentation, at: &GroupImage) -> Option<GroupImage> {
        let slide_id = self.keys.slide_id(&at.slide)?;
        let group_id = self.keys.shape_id(slide_id, &at.key)?;
        let slide = doc.slide(slide_id)?;
        match find_shape(&slide.shapes, group_id) {
            Some(shape) if shape.is_group() => {
                let image = self.shape_image(doc, &at.slide, slide_id, group_id)?;
                let children = shape.children().iter().map(|child| self.keys.shape_key(slide_id, child.id).cloned()).collect::<Option<Vec<_>>>()?;
                Some(GroupImage {
                    slide: at.slide.clone(),
                    key: at.key.clone(),
                    index: image.index,
                    group: image.shape,
                    children,
                    members: Vec::new(),
                    animations: image.animations,
                })
            }
            _ => {
                let mut members = Vec::with_capacity(at.members.len());
                for member in &at.members {
                    let current = match self.keys.shape_id(slide_id, &member.key) {
                        Some(id) => match find_shape(&slide.shapes, id) {
                            Some(_) => self.shape_image(doc, &at.slide, slide_id, id)?,
                            None => absent_shape(at.slide.clone(), member.key.clone()),
                        },
                        None => absent_shape(at.slide.clone(), member.key.clone()),
                    };
                    members.push(current);
                }
                Some(GroupImage {
                    slide: at.slide.clone(),
                    key: at.key.clone(),
                    index: 0,
                    group: None,
                    children: Vec::new(),
                    members,
                    animations: Vec::new(),
                })
            }
        }
    }

    /// The full image of one present slide: content, position and section/custom-show membership.
    fn slide_image(&self, doc: &Presentation, key: &SlideKey, id: SlideId) -> Option<SlideImage> {
        let slide = doc.slide(id)?;
        let sections = doc
            .sections
            .iter()
            .enumerate()
            .filter_map(|(i, section)| section.slides.iter().position(|x| *x == id).map(|p| (i as u32, p as u32)))
            .collect();
        let shows = doc
            .custom_shows
            .iter()
            .enumerate()
            .filter_map(|(i, show)| show.slides.iter().position(|x| *x == id).map(|p| (i as u32, p as u32)))
            .collect();
        Some(SlideImage {
            key: key.clone(),
            index: doc.slide_index(id).unwrap_or(0) as u32,
            slide: Some(serde_json::to_value(slide).ok()?),
            sections,
            shows,
        })
    }

    /// Read the current value of one shape, if this adapter knows its keys.
    fn capture_snapshot(&self, doc: &Presentation, at: &Snapshot) -> Option<Snapshot> {
        let slide_id = self.keys.slide_id(&at.slide)?;
        let shape_id = self.keys.shape_id(slide_id, &at.shape)?;
        let slide = doc.slide(slide_id)?;
        let shape = deckcraft_model::find_shape(&slide.shapes, shape_id)?;
        let image = match &at.image {
            Image::Xfrm { .. } => {
                let x = shape.xfrm?;
                Image::Xfrm { x: x.x, y: x.y, w: x.w, h: x.h }
            }
            Image::Text(_) => {
                let body = match (&shape.kind, at.cell) {
                    (ShapeKind::Table(t), Some((r, c))) => t.cell(r as usize, c as usize)?.text.clone(),
                    (ShapeKind::Table(_), None) => return None,
                    _ => shape.text.clone().unwrap_or_default(),
                };
                Image::Text(body.text())
            }
            // A group carries its own fill/outline too; the member images are captured by the
            // same traversal, so a group's appearance step is fully restorable.
            Image::Fill(_) => Image::Fill(shape.fill.clone()),
            Image::Line(_) => Image::Line(shape.line.clone()),
            Image::Format(_) => Image::Format(shape.text.clone()?),
        };
        Some(Snapshot { slide: at.slide.clone(), shape: at.shape.clone(), cell: at.cell, image })
    }

    /// Resolve the operation and run the engine command it normalises to.
    fn run(&mut self, session: &mut Session, before: &Presentation, head: u64, op: &OpEnvelope) -> Result<(), Rejection> {
        match &op.payload {
            Payload::ShapeMove { x, y } => {
                // The engine command acts on the selection: a replica whose own active slide is
                // another one must be put on this slide first, or a move on slide 5 is refused as
                // "no such shape" (TASK.md §8, two clients on different slides).
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let args = json!({"ids": [shape.0], "x": x, "y": y});
                self.call(session, "shape.move", args, head)
            }
            Payload::ShapeSetBounds { x, y, w, h } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let args = json!({"ids": [shape.0], "x": x, "y": y, "w": w, "h": h});
                self.call(session, "shape.setBounds", args, head)
            }
            Payload::ShapesSetBounds { items } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if items.is_empty() || items.len() > 4096 {
                    return Err(Rejection::invalid("a bounds step carries 1–4096 boxes", Some(head)));
                }
                // Resolve every object before the first write: `arrange.align`/`distribute` is one
                // atomic step, so a stale member must refuse it instead of moving half a selection.
                let mut seen = HashSet::with_capacity(items.len());
                let mut boxes = Vec::with_capacity(items.len());
                for item in items {
                    let shape = self.resolve_shape(before, slide, &item.shape, head)?;
                    if !seen.insert(shape) {
                        return Err(Rejection::invalid("a bounds step names the same shape twice", Some(head)));
                    }
                    for value in [item.x, item.y, item.w, item.h] {
                        if !value.is_finite() {
                            return Err(Rejection::invalid("a bounds value is not finite", Some(head)));
                        }
                    }
                    boxes.push(json!({"id": shape.0, "x": item.x, "y": item.y, "w": item.w, "h": item.h}));
                }
                self.select_slide(session, before, slide, head)?;
                self.call(session, "shape.setBoundsMany", json!({"items": boxes}), head)
            }
            Payload::TextSet { text, cell } => {
                // `text.set` looks the shape up through the engine selection; select it on its
                // own slide so a remote set on slide 5 is not refused by a replica on slide 0.
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let mut args = json!({"id": shape.0, "text": text});
                if let Some((r, c)) = cell {
                    args["cell"] = json!([r, c]);
                }
                self.call(session, "text.set", args, head)
            }
            Payload::TextInsert { at, text } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let body = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).and_then(|s| s.text.as_ref());
                let pos = offset_position(body, *at as usize);
                self.select_text(session, before, slide, shape, (pos, pos), head)?;
                self.call(session, "text.insert", json!({"text": text}), head)
            }
            Payload::TextDelete { from, to } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                if to <= from {
                    return Err(Rejection::invalid("the deleted range is empty", Some(head)));
                }
                let body = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape)).and_then(|s| s.text.as_ref());
                let start = offset_position(body, *from as usize);
                let end = offset_position(body, *to as usize);
                self.select_text(session, before, slide, shape, (start, end), head)?;
                self.call(session, "text.delete", json!({}), head)
            }
            Payload::ShapeInsert { preset, rect, text } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                self.select_slide(session, before, slide, head)?;
                let mut args = json!({"preset": preset, "rect": rect});
                if let Some(text) = text {
                    args["text"] = json!(text);
                }
                self.call(session, "shape.insert", args, head)
            }
            Payload::PictureInsert { sha256, name, rect } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                let bytes = self
                    .media
                    .get(sha256)
                    .ok_or_else(|| Rejection::missing_media(format!("picture `{sha256}` has not been uploaded to this session"), Some(head)))?;
                if let Some(kind) = self.media.kind(sha256)
                    && kind != crate::MediaType::Picture
                {
                    return Err(Rejection::invalid(format!("`{sha256}` is a {} object, not a picture", kind.noun()), Some(head)));
                }
                self.select_slide(session, before, slide, head)?;
                let mut args = json!({
                    "name": name,
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes.as_slice()),
                });
                if let Some(rect) = rect {
                    args["rect"] = json!(rect);
                }
                self.call(session, "insert.picture", args, head)
            }
            Payload::MediaInsert { kind, sha256, name, rect } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                let bytes = self
                    .media
                    .get(sha256)
                    .ok_or_else(|| Rejection::missing_media(format!("media `{sha256}` has not been uploaded to this session"), Some(head)))?;
                let expected = match kind {
                    MediaKind::Video => crate::MediaType::Video,
                    MediaKind::Audio => crate::MediaType::Audio,
                };
                if let Some(stored) = self.media.kind(sha256)
                    && stored != expected
                {
                    return Err(Rejection::invalid(format!("`{sha256}` is a {} object, not {}", stored.noun(), expected.noun()), Some(head)));
                }
                self.select_slide(session, before, slide, head)?;
                let mut args = json!({
                    "name": name,
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes.as_slice()),
                });
                if let Some(rect) = rect {
                    args["rect"] = json!(rect);
                }
                let cmd = match kind {
                    MediaKind::Video => "insert.video",
                    MediaKind::Audio => "insert.audio",
                };
                self.call(session, cmd, args, head)
            }
            Payload::ShapeFill { fill } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let mut args = fill.args();
                args["ids"] = json!([shape.0]);
                self.call(session, "shape.fill", args, head)
            }
            Payload::ShapeLine { line } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let mut args = line.args();
                args["ids"] = json!([shape.0]);
                self.call(session, "shape.line", args, head)
            }
            Payload::FormatSet { format, args } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let target = before.slide(slide).and_then(|s| find_shape(&s.shapes, shape));
                match target {
                    Some(sh) if matches!(sh.kind, ShapeKind::Table(_)) => {
                        return Err(Rejection::unsupported("formatting a table needs a cell address", Some(head)));
                    }
                    Some(sh) if sh.text.is_none() => {
                        return Err(Rejection::invalid("the shape has no text to format", Some(head)));
                    }
                    Some(_) => {}
                    None => return Err(Rejection::stale("the shape is gone", Some(head))),
                }
                self.select_shapes(session, before, slide, &[shape], head)?;
                if !format.accepts(args) {
                    return Err(Rejection::invalid(format!("`{}` does not take these arguments", format.id()), Some(head)));
                }
                self.call(session, format.id(), args.clone(), head)
            }
            Payload::ShapeField { field, value } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                self.select_shapes(session, before, slide, &[shape], head)?;
                let name = match field {
                    ShapeField::Fill => "fill",
                    ShapeField::Line => "line",
                    ShapeField::Text => "text",
                };
                self.call(session, "shape.setField", json!({"field": name, "value": value, "ids": [shape.0]}), head)
            }
            Payload::ShapeFields { items } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                // Resolve every object before the first write: the operation is one atomic step,
                // so a missing member must refuse it instead of applying half a group.
                let mut resolved = Vec::with_capacity(items.len());
                for item in items {
                    let shape = self.resolve_shape(before, slide, &item.shape, head)?;
                    resolved.push((shape, item));
                }
                if resolved.is_empty() {
                    return Err(Rejection::invalid("a merge restore carries no values", Some(head)));
                }
                self.select_slide(session, before, slide, head)?;
                let values: Vec<Value> = resolved
                    .iter()
                    .map(|(shape, item)| {
                        json!({
                            "id": shape.0,
                            "field": match item.field { ShapeField::Fill => "fill", ShapeField::Line => "line", ShapeField::Text => "text" },
                            "value": item.value.clone(),
                        })
                    })
                    .collect();
                self.call(session, "shape.setFields", json!({"items": values}), head)
            }
            Payload::ShapeRestore { items, remove } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                if items.is_empty() || items.len() > 64 || remove.len() > 64 {
                    return Err(Rejection::invalid("a shape restore carries 1–64 shapes and up to 64 removals", Some(head)));
                }
                // Resolve every parent and every removed shape before the first write: the
                // operation is one atomic step, so a missing group must refuse it instead of
                // putting half a subtree back.
                let mut parents = Vec::with_capacity(items.len());
                for item in items {
                    let parent = match &item.parent {
                        Some(key) => Some(self.resolve_shape(before, slide, key, head)?.0),
                        None => None,
                    };
                    parents.push(parent);
                }
                let mut removed = Vec::with_capacity(remove.len());
                for key in remove {
                    removed.push(self.resolve_shape(before, slide, key, head)?.0);
                }
                self.select_slide(session, before, slide, head)?;
                let values: Vec<Value> = items
                    .iter()
                    .zip(parents)
                    .map(|(item, parent)| json!({"value": item.value, "parent": parent, "index": item.index, "animations": item.animations}))
                    .collect();
                self.call(session, "shape.restore", json!({"items": values, "remove": removed}), head)
            }
            Payload::SlideRestore { index, slide: image, sections, shows } => {
                self.call(session, "slide.restore", json!({"index": index, "slide": image, "sections": sections, "shows": shows}), head)
            }
            Payload::ShapeDelete {} => {
                let (_, shapes) = self.resolve_many(before, &op.target, head)?;
                let ids: Vec<u32> = shapes.iter().map(|s| s.0).collect();
                self.call(session, "edit.delete", json!({"ids": ids}), head)
            }
            Payload::Reorder { before: anchor } => {
                let (slide, shape) = self.resolve_one(before, &op.target, head)?;
                let index = reorder_index(before, slide, shape, anchor.as_ref(), &self.keys, head)?;
                self.call(session, "arrange.reorder", json!({"id": shape.0, "index": index}), head)
            }
            Payload::Group {} => {
                let (_, shapes) = self.resolve_many(before, &op.target, head)?;
                if shapes.len() < 2 {
                    return Err(Rejection::invalid("a group needs two or more shapes", Some(head)));
                }
                let ids: Vec<u32> = shapes.iter().map(|s| s.0).collect();
                self.call(session, "arrange.group", json!({"ids": ids}), head)
            }
            Payload::Ungroup {} => {
                let (_, shape) = self.resolve_one(before, &op.target, head)?;
                self.call(session, "arrange.ungroup", json!({"ids": [shape.0]}), head)
            }
            Payload::SlideInsert { before: anchor, layout } => {
                let at = match anchor {
                    Some(key) => self.slide_index(before, key, head)?,
                    None => before.slides.len(),
                };
                let mut args = json!({"at": at});
                if let Some(l) = layout {
                    args["layout"] = json!(l);
                }
                self.call(session, "slide.new", args, head)
            }
            Payload::SlideDelete {} => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                self.call(session, "slide.delete", json!({"ids": [slide.0]}), head)
            }
            Payload::SlideMove { before: anchor } => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                let to = match anchor {
                    Some(key) => {
                        let anchor_id = self.resolve_slide(before, Some(key), head)?;
                        if anchor_id == slide {
                            return Err(Rejection::invalid("a slide cannot move before itself", Some(head)));
                        }
                        // `slide.move` counts the destination without the moved slide.
                        before
                            .slides
                            .iter()
                            .filter(|s| s.id != slide)
                            .position(|s| s.id == anchor_id)
                            .ok_or_else(|| Rejection::invalid("the anchor slide is not in this document", Some(head)))?
                    }
                    None => before.slides.len().saturating_sub(1),
                };
                self.call(session, "slide.move", json!({"ids": [slide.0], "to": to}), head)
            }
            Payload::SlideDuplicate {} => {
                let slide = self.resolve_slide(before, op.target.slide.as_ref(), head)?;
                let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
                self.call(session, "slide.duplicate", json!({"index": index}), head)
            }
        }
    }

    /// Put the engine's selection on one shape, so formatting and fill commands act on the
    /// authoritative shape rather than on whatever this replica had selected.
    fn select_shapes(&self, session: &mut Session, before: &Presentation, slide: SlideId, shapes: &[ShapeId], head: u64) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
        st.selection.target = EngineTarget::Slides;
        st.selection.slide = index;
        st.selection.slides.clear();
        st.selection.shapes = shapes.to_vec();
        st.selection.cells = None;
        st.selection.text = None;
        Ok(())
    }

    /// Put the engine's selection on one shape's text, so `text.insert`/`text.delete` act at the
    /// authoritative position rather than at whatever this replica had selected.
    fn select_text(
        &self,
        session: &mut Session,
        before: &Presentation,
        slide: SlideId,
        shape: ShapeId,
        (anchor, caret): ((usize, usize), (usize, usize)),
        head: u64,
    ) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
        st.selection.target = EngineTarget::Slides;
        st.selection.slide = index;
        st.selection.slides.clear();
        st.selection.shapes = vec![shape];
        st.selection.cells = None;
        st.selection.text = Some(TextSel { shape, anchor, caret, cell: None, notes: false, line_end: None });
        Ok(())
    }

    fn select_slide(&self, session: &mut Session, before: &Presentation, slide: SlideId, head: u64) -> Result<(), Rejection> {
        let index = before.slide_index(slide).ok_or_else(|| Rejection::stale("the slide is gone", Some(head)))?;
        let st = session.doc_mut().map_err(|_| Rejection::invalid("no document open", Some(head)))?;
        st.selection.target = EngineTarget::Slides;
        st.selection.slide = index;
        st.selection.slides.clear();
        st.selection.shapes.clear();
        st.selection.cells = None;
        st.selection.text = None;
        Ok(())
    }

    fn call(&self, session: &mut Session, cmd: &str, args: Value, head: u64) -> Result<(), Rejection> {
        // A remote operation never enters this session's undo history or journal (ADR 0010).
        session.apply_remote(cmd, &args).map(|_| ()).map_err(|e| Rejection::invalid(format!("{cmd}: {e}"), Some(head)))
    }

    /// The slide named by `key`, which must still exist in `doc`.
    fn resolve_slide(&self, doc: &Presentation, key: Option<&SlideKey>, head: u64) -> Result<SlideId, Rejection> {
        let key = key.ok_or_else(|| Rejection::invalid("the operation names no slide", Some(head)))?;
        if self.dead_slides.contains(key) {
            return Err(Rejection::dead(format!("slide `{key}` was deleted"), Some(head)));
        }
        let id = self.keys.slide_id(key).ok_or_else(|| Rejection::dead(format!("unknown slide `{key}`"), Some(head)))?;
        if doc.slide_index(id).is_none() {
            return Err(Rejection::dead(format!("slide `{key}` is gone"), Some(head)));
        }
        Ok(id)
    }

    fn resolve_shape(&self, doc: &Presentation, slide: SlideId, key: &ShapeKey, head: u64) -> Result<ShapeId, Rejection> {
        if self.keys.is_dead_shape(slide, key) {
            return Err(Rejection::dead(format!("shape `{key}` was deleted"), Some(head)));
        }
        let id = self.keys.shape_id(slide, key).ok_or_else(|| Rejection::dead(format!("unknown shape `{key}`"), Some(head)))?;
        let found = doc.slide(slide).is_some_and(|s| deckcraft_model::find_shape(&s.shapes, id).is_some());
        if !found {
            return Err(Rejection::dead(format!("shape `{key}` is gone"), Some(head)));
        }
        Ok(id)
    }

    fn resolve_one(&self, doc: &Presentation, target: &Target, head: u64) -> Result<(SlideId, ShapeId), Rejection> {
        let slide = self.resolve_slide(doc, target.slide.as_ref(), head)?;
        let key = target.shape.as_ref().ok_or_else(|| Rejection::invalid("the operation names no shape", Some(head)))?;
        let shape = self.resolve_shape(doc, slide, key, head)?;
        Ok((slide, shape))
    }

    fn resolve_many(&self, doc: &Presentation, target: &Target, head: u64) -> Result<(SlideId, Vec<ShapeId>), Rejection> {
        let slide = self.resolve_slide(doc, target.slide.as_ref(), head)?;
        let keys: Vec<&ShapeKey> = if target.shapes.is_empty() { target.shape.iter().collect() } else { target.shapes.iter().collect() };
        if keys.is_empty() {
            return Err(Rejection::invalid("the operation names no shapes", Some(head)));
        }
        let mut ids = Vec::with_capacity(keys.len());
        for key in keys {
            ids.push(self.resolve_shape(doc, slide, key, head)?);
        }
        Ok((slide, ids))
    }

    fn slide_index(&self, doc: &Presentation, key: &SlideKey, head: u64) -> Result<usize, Rejection> {
        let id = self.resolve_slide(doc, Some(key), head)?;
        doc.slides.iter().position(|s| s.id == id).ok_or_else(|| Rejection::dead("slide is gone", Some(head)))
    }

    /// Turn deleted objects into tombstones so a late operation cannot resurrect them.
    fn bury(&mut self, before: &Presentation, out: &Delta) {
        for change in &out.changes {
            match &change.kind {
                ChangeKind::Deleted => match &change.shape {
                    Some(shape_key) => {
                        if let Some(slide) = self.keys.slide_id(&change.slide)
                            && let Some(id) = self.keys.shape_id(slide, shape_key)
                        {
                            self.keys.bury_shape(slide, id);
                        }
                    }
                    None => {
                        self.dead_slides.insert(change.slide.clone());
                        if let Some(slide_id) = self.keys.slide_id(&change.slide)
                            && let Some(slide) = before.slides.iter().find(|s| s.id == slide_id)
                        {
                            let mut ids = Vec::new();
                            walk_ids(&slide.shapes, &mut ids, 0);
                            for id in ids {
                                self.keys.bury_shape(slide_id, id);
                            }
                        }
                    }
                },
                ChangeKind::Inserted | ChangeKind::Modified { .. } | ChangeKind::Reordered { .. } => {}
            }
        }
    }
}

fn walk_ids(shapes: &[Shape], out: &mut Vec<ShapeId>, depth: usize) {
    if depth > 64 {
        return;
    }
    for s in shapes {
        out.push(s.id);
        walk_ids(s.children(), out, depth + 1);
    }
}

/// The image of a slide that is not in the document.
fn absent_slide(key: SlideKey) -> SlideImage {
    SlideImage { key, index: 0, slide: None, sections: Vec::new(), shows: Vec::new() }
}

/// The image of a shape that is not in the document.
fn absent_shape(slide: SlideKey, key: ShapeKey) -> ShapeImage {
    ShapeImage { slide, key, parent: None, index: 0, shape: None, animations: Vec::new() }
}

/// The top-most shapes `after` has and `before` did not: the objects one insert added. A shape
/// whose ancestor is also new is not listed again, because an image carries its children.
fn top_most_added(before: &[Shape], after: &[Shape]) -> Option<ShapeId> {
    fn rec(before: &[Shape], after: &[Shape], inside_new: bool, out: &mut Vec<ShapeId>, depth: usize) {
        if depth > 64 {
            return;
        }
        for shape in after {
            let new = find_shape(before, shape.id).is_none();
            if new && !inside_new {
                out.push(shape.id);
            }
            rec(before, shape.children(), inside_new || new, out, depth + 1);
        }
    }
    let mut added = Vec::new();
    rec(before, after, false, &mut added, 0);
    match added.as_slice() {
        [id] => Some(*id),
        _ => None,
    }
}

/// The top-most shapes `before` had and `after` does not: the objects one delete removed.
fn top_most_gone(before: &[Shape], after: &[Shape]) -> Vec<ShapeId> {
    fn rec(before: &[Shape], after: &[Shape], inside_gone: bool, out: &mut Vec<ShapeId>, depth: usize) {
        if depth > 64 {
            return;
        }
        for shape in before {
            let gone = find_shape(after, shape.id).is_none();
            if gone && !inside_gone {
                out.push(shape.id);
            }
            rec(shape.children(), after, inside_gone || gone, out, depth + 1);
        }
    }
    let mut gone = Vec::new();
    rec(before, after, false, &mut gone, 0);
    gone
}

/// The kind of value a restore puts back. The document supplies the value itself; the payload
/// only says which field to read.
fn image_of_field(field: ShapeField) -> Image {
    match field {
        ShapeField::Fill => Image::Fill(None),
        ShapeField::Line => Image::Line(None),
        ShapeField::Text => Image::Format(TextBody::default()),
    }
}

/// Which kind an operation must carry. Structure-changing operations are exclusive.
/// Length of a text body in characters, counting one newline between paragraphs.
pub fn body_len(body: Option<&TextBody>) -> usize {
    let Some(body) = body else { return 0 };
    let text: usize = body.paragraphs.iter().map(|p| p.char_len()).sum();
    text + body.paragraphs.len().saturating_sub(1)
}

/// A paragraph/character position as a whole-body character offset.
pub fn position_offset(body: Option<&TextBody>, pos: (usize, usize)) -> usize {
    let Some(body) = body else { return 0 };
    let mut offset = 0;
    for (i, paragraph) in body.paragraphs.iter().enumerate() {
        if i > 0 {
            offset += 1;
        }
        if i == pos.0 {
            return offset + pos.1.min(paragraph.char_len());
        }
        offset += paragraph.char_len();
    }
    offset
}

/// A whole-body character offset as paragraph/character. Out-of-range offsets clamp to the end.
pub fn offset_position(body: Option<&TextBody>, offset: usize) -> (usize, usize) {
    let Some(body) = body else { return (0, 0) };
    let mut left = offset;
    for (i, paragraph) in body.paragraphs.iter().enumerate() {
        let len = paragraph.char_len();
        if left <= len {
            return (i, left);
        }
        left -= len;
        if i + 1 < body.paragraphs.len() {
            left = left.saturating_sub(1);
        }
    }
    (body.paragraphs.len().saturating_sub(1), body.paragraphs.last().map_or(0, |p| p.char_len()))
}

pub fn class_of(payload: &Payload) -> OpKind {
    match payload {
        Payload::ShapeMove { .. }
        | Payload::ShapeSetBounds { .. }
        | Payload::ShapesSetBounds { .. }
        | Payload::TextSet { .. }
        | Payload::TextInsert { .. }
        | Payload::TextDelete { .. }
        | Payload::ShapeFill { .. }
        | Payload::ShapeLine { .. }
        | Payload::FormatSet { .. }
        | Payload::ShapeField { .. }
        | Payload::ShapeFields { .. } => OpKind::Command,
        Payload::ShapeInsert { .. }
        | Payload::PictureInsert { .. }
        | Payload::MediaInsert { .. }
        | Payload::ShapeDelete {}
        | Payload::ShapeRestore { .. }
        | Payload::Reorder { .. }
        | Payload::Group {}
        | Payload::Ungroup {}
        | Payload::SlideInsert { .. }
        | Payload::SlideDelete {}
        | Payload::SlideRestore { .. }
        | Payload::SlideMove { .. }
        | Payload::SlideDuplicate {} => OpKind::Structural,
    }
}

/// Index `arrange.reorder` expects: a position among the shape's siblings **after** the shape is
/// taken out, `0` = back.
fn reorder_index(
    doc: &Presentation,
    slide: SlideId,
    shape: ShapeId,
    before: Option<&ShapeKey>,
    keys: &KeyMap,
    head: u64,
) -> Result<usize, Rejection> {
    let Some(slide_ref) = doc.slide(slide) else {
        return Err(Rejection::invalid("no such slide", Some(head)));
    };
    let siblings: &[Shape] = match parent_of(&slide_ref.shapes, shape) {
        None => return Err(Rejection::invalid("no such shape", Some(head))),
        Some(None) => &slide_ref.shapes,
        Some(Some(group)) => {
            let Some(g) = deckcraft_model::find_shape(&slide_ref.shapes, group) else {
                return Err(Rejection::invalid("no such group", Some(head)));
            };
            g.children()
        }
    };
    let Some(anchor_key) = before else {
        return Ok(siblings.len().saturating_sub(1));
    };
    let anchor = keys.shape_id(slide, anchor_key).ok_or_else(|| Rejection::dead(format!("unknown shape `{anchor_key}`"), Some(head)))?;
    if anchor == shape {
        return Ok(siblings.iter().position(|s| s.id == shape).unwrap_or(0));
    }
    for (index, sibling) in siblings.iter().filter(|s| s.id != shape).enumerate() {
        if sibling.id == anchor {
            return Ok(index);
        }
    }
    Err(Rejection::invalid("the anchor is not a sibling of the moved shape", Some(head)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::ActorHistory;
    use deckcraft_collab_proto::{FillSpec, FormatCmd, LineSpec, ShapeBounds, ShapeRestoreItem, TextCursor};

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

    fn doc_of(s: &Session) -> Presentation {
        s.doc().expect("doc").doc.as_ref().clone()
    }

    fn key_of(adapter: &Adapter, doc: &Presentation, shape: usize) -> (SlideKey, ShapeKey) {
        let slide = doc.slides.first().expect("slide");
        let slide_key = adapter.keys().slide_key(slide.id).expect("slide key").clone();
        let shape_key = adapter.keys().shape_key(slide.id, slide.shapes[shape].id).expect("shape key").clone();
        (slide_key, shape_key)
    }

    fn op(kind: OpKind, base: u64, target: Target, payload: Payload) -> OpEnvelope {
        OpEnvelope { op_id: "op-1".into(), doc: "d".into(), base_version: base, kind, target, payload, history: None }
    }

    #[test]
    fn two_clients_converge_on_a_move() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));

        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 0);
        let envelope =
            op(OpKind::Command, 0, Target { slide: Some(slide), shape: Some(shape), ..Target::default() }, Payload::ShapeMove { x: 300.0, y: 200.0 });

        let da = aa.apply(&mut a, 0, "a", &envelope).expect("apply a");
        let db = bb.apply(&mut b, 0, "b", &envelope).expect("apply b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        assert!(!da.is_empty());
    }

    /// Two slides, each with a shape: the receiver may be looking at either one.
    fn two_slide_deck() -> Session {
        let mut s = deck();
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second slide");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape on the second slide");
        s
    }

    #[test]
    fn an_operation_on_another_slide_applies_whatever_the_replica_is_looking_at() {
        let (mut a, mut b) = (two_slide_deck(), two_slide_deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        // The receiving replica looks at slide 0; the operation targets slide 1.
        b.doc_mut().expect("doc").selection.slide = 0;
        b.doc_mut().expect("doc").selection.shapes.clear();

        let doc = doc_of(&a);
        let slide_id = doc.slides[1].id;
        let slide = aa.keys().slide_key(slide_id).expect("slide").clone();
        let shape_id = doc.slides[1].shapes[0].id;
        let shape = aa.keys().shape_key(slide_id, shape_id).expect("shape").clone();
        let target = Target { slide: Some(slide), shape: Some(shape), ..Target::default() };

        let mv = op(OpKind::Command, 0, target.clone(), Payload::ShapeMove { x: 300.0, y: 200.0 });
        aa.apply(&mut a, 0, "a", &mv).expect("author moves");
        bb.apply(&mut b, 0, "b", &mv).expect("the other replica moves a shape on the slide it is not looking at");

        let set = op(OpKind::Command, 1, target, Payload::TextSet { text: "hello".into(), cell: None });
        aa.apply(&mut a, 1, "a", &set).expect("author sets text");
        bb.apply(&mut b, 1, "b", &set).expect("the other replica sets text on the slide it is not looking at");

        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge");
        let shape = &doc_of(&a).slides[1].shapes[0];
        assert_eq!(shape.xfrm.expect("xfrm").x, 300.0);
        assert_eq!(shape.text.as_ref().map(|text| text.text()).as_deref(), Some("hello"));
    }

    #[test]
    fn the_same_shape_ends_where_the_server_put_it() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, shape) = key_of(&aa, &doc, 0);

        for (i, x) in [200.0, 320.0].iter().enumerate() {
            let envelope = op(
                OpKind::Command,
                i as u64,
                Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
                Payload::ShapeMove { x: *x, y: 100.0 },
            );
            aa.apply(&mut a, i as u64, "a", &envelope).expect("a");
            bb.apply(&mut b, i as u64, "b", &envelope).expect("b");
        }
        assert_eq!(doc_of(&a), doc_of(&b));
        let x = doc_of(&a).slides[0].shapes[0].xfrm.expect("xfrm").x;
        assert_eq!(x, 320.0, "last writer wins");
    }

    #[test]
    fn moving_one_child_of_a_group_refits_it_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let ids: Vec<ShapeKey> = doc.slides[0].shapes.iter().map(|s| aa.keys().shape_key(doc.slides[0].id, s.id).expect("key").clone()).collect();

        let group = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), shapes: ids.clone(), ..Target::default() }, Payload::Group {});
        aa.apply(&mut a, 0, "a", &group).expect("group a");
        bb.apply(&mut b, 0, "b", &group).expect("group b");
        assert_eq!(doc_of(&a), doc_of(&b));

        // The child key comes from the same map the join issued.
        let child = {
            let after = doc_of(&a);
            let slide_id = after.slides[0].id;
            let child_id = after.slides[0].shapes[0].children()[0].id;
            aa.keys().shape_key(slide_id, child_id).expect("child key").clone()
        };

        let inside =
            op(OpKind::Command, 1, Target { slide: Some(slide), shape: Some(child), ..Target::default() }, Payload::ShapeMove { x: 60.0, y: 60.0 });
        aa.apply(&mut a, 1, "a", &inside).expect("move a");
        bb.apply(&mut b, 1, "b", &inside).expect("move b");
        assert_eq!(doc_of(&a), doc_of(&b), "group frame and children agree");
    }

    #[test]
    fn a_structural_operation_must_be_sent_against_the_head() {
        let mut a = deck();
        let mut aa = Adapter::new("k");
        aa.join(&doc_of(&a));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = aa.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape").clone();
        let stale = op(OpKind::Structural, 3, Target { slide: Some(slide), shape: Some(shape), ..Target::default() }, Payload::ShapeDelete {});
        let err = aa.apply(&mut a, 7, "a", &stale).expect_err("stale");
        assert_eq!(err.code, RejectCode::Stale);
        assert!(err.retryable);
        assert_eq!(err.hint_version, Some(7));
    }

    #[test]
    fn a_late_operation_on_a_deleted_slide_is_refused_and_not_resurrected() {
        let mut a = deck();
        let mut aa = Adapter::new("k");
        aa.join(&doc_of(&a));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = aa.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape").clone();
        let slides_before = doc_of(&a).slides.len();

        let delete = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), ..Target::default() }, Payload::SlideDelete {});
        aa.apply(&mut a, 0, "a", &delete).expect("delete");

        let late =
            op(OpKind::Command, 1, Target { slide: Some(slide), shape: Some(shape), ..Target::default() }, Payload::ShapeMove { x: 1.0, y: 1.0 });
        let err = aa.apply(&mut a, 1, "a", &late).expect_err("late operation");
        assert_eq!(err.code, RejectCode::Stale);
        assert!(!err.retryable, "a tombstone is final");
        assert_eq!(doc_of(&a).slides.len(), slides_before.saturating_sub(1));
    }

    #[test]
    fn text_set_addresses_one_body_and_needs_no_selection() {
        let mut a = deck();
        let mut aa = Adapter::new("k");
        aa.join(&doc_of(&a));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = aa.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[1].id).expect("shape").clone();
        let envelope = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(shape), text: Some(TextCursor::default()), ..Target::default() },
            Payload::TextSet { text: "Hello".into(), cell: None },
        );
        aa.apply(&mut a, 0, "a", &envelope).expect("text");
        assert_eq!(doc_of(&a).slides[0].shapes[1].text.as_ref().expect("text").text(), "Hello");
    }

    #[test]
    fn a_remote_operation_does_not_enter_the_local_undo_history() {
        let mut a = deck();
        let mut aa = Adapter::new("k");
        aa.join(&doc_of(&a));
        assert!(a.doc().expect("doc").history.undo.is_empty(), "setup used apply_remote");
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = aa.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape").clone();
        let envelope =
            op(OpKind::Command, 0, Target { slide: Some(slide), shape: Some(shape), ..Target::default() }, Payload::ShapeMove { x: 10.0, y: 10.0 });
        aa.apply(&mut a, 0, "a", &envelope).expect("move");
        assert!(a.doc().expect("doc").history.undo.is_empty(), "a collaborator's edit is not my undo step");
    }

    #[test]
    fn a_payload_must_match_its_kind() {
        let mut a = deck();
        let mut aa = Adapter::new("k");
        aa.join(&doc_of(&a));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let envelope = op(OpKind::Command, 0, Target { slide: Some(slide), ..Target::default() }, Payload::SlideDelete {});
        let err = aa.apply(&mut a, 0, "a", &envelope).expect_err("wrong kind");
        assert_eq!(err.code, RejectCode::Invalid);
    }

    #[test]
    fn a_slide_moves_before_its_anchor() {
        let (mut a, mut b) = (deck(), deck());
        for s in [&mut a, &mut b] {
            s.apply_remote("slide.new", &json!({"at": 1})).expect("slide 1");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let first = aa.keys().slide_key(doc.slides[0].id).expect("s0").clone();
        let second = aa.keys().slide_key(doc.slides[1].id).expect("s1").clone();
        assert_eq!(doc.slides.len(), 2);

        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(second.clone()), ..Target::default() },
            Payload::SlideMove { before: Some(first.clone()) },
        );
        aa.apply(&mut a, 0, "a", &envelope).expect("move a");
        bb.apply(&mut b, 0, "b", &envelope).expect("move b");
        assert_eq!(doc_of(&a), doc_of(&b));
        let order: Vec<SlideId> = doc_of(&a).slides.iter().map(|s| s.id).collect();
        assert_eq!(order[0], doc.slides[1].id, "the moved slide is now first");
    }

    fn shape_x(doc: &Presentation, index: usize) -> f64 {
        doc.slides[0].shapes[index].xfrm.expect("xfrm").x
    }

    fn move_envelope(slide: &SlideKey, shape: &ShapeKey, x: f64, base: u64) -> OpEnvelope {
        op(
            OpKind::Command,
            base,
            Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            Payload::ShapeMove { x, y: 100.0 },
        )
    }

    #[test]
    fn fill_line_format_and_a_slide_copy_converge_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, first) = key_of(&aa, &doc_of(&a), 0);
        let (slide_b, second) = key_of(&bb, &doc_of(&b), 1);
        assert_eq!(slide, slide_b);

        let steps = [
            (Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } }, first.clone()),
            (Payload::ShapeLine { line: LineSpec { color: Some("#0044ff".into()), width: Some(3.0), ..LineSpec::default() } }, first.clone()),
            (Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}) }, first.clone()),
            (Payload::FormatSet { format: FormatCmd::Size, args: json!({"size": 24.0}) }, second.clone()),
        ];
        for (version, (payload, shape)) in steps.into_iter().enumerate() {
            let envelope =
                op(OpKind::Command, version as u64, Target { slide: Some(slide.clone()), shape: Some(shape), ..Target::default() }, payload);
            let da = aa.apply(&mut a, version as u64, "a", &envelope).expect("apply a");
            let db = bb.apply(&mut b, version as u64, "b", &envelope).expect("apply b");
            assert_eq!(da, db, "the same operation has the same delta on both replicas");
        }
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same formatting");

        // The copy is structural: it lands on the head and both replicas keep the same order.
        let duplicate = op(OpKind::Structural, 4, Target { slide: Some(slide.clone()), ..Target::default() }, Payload::SlideDuplicate {});
        aa.apply(&mut a, 4, "a", &duplicate).expect("duplicate a");
        bb.apply(&mut b, 4, "b", &duplicate).expect("duplicate b");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same slide order");
        assert_eq!(doc_of(&a).slides.len(), 2);
        assert_ne!(doc_of(&a).slides[0].id, doc_of(&a).slides[1].id, "the copy has fresh ids");
    }

    #[test]
    fn a_format_operation_cannot_move_off_its_target_with_args() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        let envelope = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(first), ..Target::default() },
            Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true, "ids": [99]}) },
        );
        let err = ad.apply(&mut s, 0, "a", &envelope).expect_err("an address in args");
        assert_eq!(err.code, RejectCode::Invalid);
    }

    #[test]
    fn a_video_operation_runs_from_the_media_channel_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let bytes = mp4();
        let sha = aa.media_mut().put("clip.mp4", bytes.clone()).expect("media");
        bb.media_mut().insert(&sha, bytes).expect("the same object");

        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();
        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::MediaInsert { kind: MediaKind::Video, sha256: sha.clone(), name: "clip.mp4".into(), rect: Some([40.0, 40.0, 320.0, 180.0]) },
        );
        let da = aa.apply(&mut a, 0, "a", &envelope).expect("apply a");
        let db = bb.apply(&mut b, 0, "b", &envelope).expect("apply b");
        assert_eq!(da, db);
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same clip");
        let after = doc_of(&a);
        let clip = after.slides[0].shapes.iter().find(|s| matches!(s.kind, ShapeKind::Media(_))).expect("a media shape");
        assert_eq!(clip.xfrm.expect("xfrm").x, 40.0);
    }

    #[test]
    fn an_unplayable_clip_is_still_one_operation() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let bytes = b"RIFF\x0c\0\0\0WAVEfmt ".to_vec();
        let sha = ad.media_mut().put("hit.wav", bytes).expect("media");
        let (slide, _) = key_of(&ad, &doc_of(&s), 0);
        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::MediaInsert { kind: MediaKind::Audio, sha256: sha, name: "hit.wav".into(), rect: None },
        );
        ad.apply(&mut s, 0, "a", &envelope).expect("an unrecognised codec is a warning, not a refusal");
        assert!(doc_of(&s).slides[0].shapes.iter().any(|s| matches!(s.kind, ShapeKind::Media(ref m) if !m.video)));
    }

    /// A minimal ISO base media file: enough for the media allowlist, not for a decoder.
    fn mp4() -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0x18];
        bytes.extend(b"ftypisom\0\0\x02\x00isomiso2");
        bytes.extend([0u8; 32]);
        bytes
    }

    #[test]
    fn a_picture_may_not_name_an_audio_object() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let bytes = b"RIFF\x0c\0\0\0WAVEfmt ".to_vec();
        let sha = ad.media_mut().put("hit.wav", bytes).expect("media");
        let (slide, _) = key_of(&ad, &doc_of(&s), 0);
        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::PictureInsert { sha256: sha, name: "hit.wav".into(), rect: None },
        );
        let err = ad.apply(&mut s, 0, "a", &envelope).expect_err("an audio object is not a picture");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(err.message.contains("audio"), "{}", err.message);
    }

    #[test]
    fn undo_restores_my_change_and_leaves_another_shape_alone() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, first) = key_of(&ad, &doc, 0);
        let (_, second) = key_of(&ad, &doc, 1);

        ad.apply(&mut s, 0, "a", &move_envelope(&slide, &first, 300.0, 0)).expect("a moves");
        ad.apply(&mut s, 1, "b", &move_envelope(&slide, &second, 500.0, 1)).expect("b moves");
        let with_both = doc_of(&s);

        let undo = ad.undo(&s, 2, "a", "u-1").expect("a can undo");
        ad.apply(&mut s, 2, "a", &undo).expect("the undo is applied");

        let after = doc_of(&s);
        assert_eq!(shape_x(&after, 0), 100.0, "a's own move is taken back");
        assert_eq!(shape_x(&after, 1), 500.0, "b's move stays");
        assert_eq!(after.slides[0].shapes[1], with_both.slides[0].shapes[1]);
        assert_eq!(ad.history.get("a").map(ActorHistory::undo_len), Some(0));
        assert_eq!(ad.history.get("a").map(ActorHistory::redo_len), Some(1));
        assert_eq!(ad.history.get("b").map(ActorHistory::undo_len), Some(1), "b's history is untouched");
    }

    #[test]
    fn undo_after_another_edit_of_the_same_shape_is_refused() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);

        ad.apply(&mut s, 0, "a", &move_envelope(&slide, &shape, 300.0, 0)).expect("a moves");
        ad.apply(&mut s, 1, "b", &move_envelope(&slide, &shape, 700.0, 1)).expect("b moves");

        let err = ad.undo(&s, 2, "a", "u-1").expect_err("a cannot undo over b");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable, "a silent overwrite is exactly what the rule forbids");
        assert_eq!(shape_x(&doc_of(&s), 0), 700.0, "b's edit is not reverted");
        assert_eq!(ad.history.get("a").map(ActorHistory::undo_len), Some(1), "the history stays");
    }

    #[test]
    fn redo_puts_the_step_back() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);

        ad.apply(&mut s, 0, "a", &move_envelope(&slide, &shape, 300.0, 0)).expect("move");
        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo");
        ad.apply(&mut s, 1, "a", &undo).expect("apply undo");
        assert_eq!(shape_x(&doc_of(&s), 0), 100.0);

        let redo = ad.redo(&s, 2, "a", "r-1").expect("redo");
        ad.apply(&mut s, 2, "a", &redo).expect("apply redo");
        assert_eq!(shape_x(&doc_of(&s), 0), 300.0, "the step is back");
        assert_eq!(ad.history.get("a").map(ActorHistory::undo_len), Some(1));
        assert_eq!(ad.history.get("a").map(ActorHistory::redo_len), Some(0));
    }

    #[test]
    fn undo_of_a_text_step_restores_the_whole_body() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 1);
        let set = op(
            OpKind::Command,
            0,
            Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            Payload::TextSet { text: "Hello".into(), cell: None },
        );
        ad.apply(&mut s, 0, "a", &set).expect("text");
        assert_eq!(doc_of(&s).slides[0].shapes[1].text.as_ref().expect("text").text(), "Hello");

        let undo = ad.undo(&s, 1, "a", "u-1").expect("undo");
        ad.apply(&mut s, 1, "a", &undo).expect("apply undo");
        let body = doc_of(&s).slides[0].shapes[1].text.clone().unwrap_or_default();
        assert_eq!(body.text(), "", "the body the step replaced is back");
    }

    #[test]
    fn nothing_to_undo_is_an_invalid_operation_not_a_conflict() {
        let s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let err = ad.undo(&s, 0, "a", "u-1").expect_err("empty history");
        assert_eq!(err.code, RejectCode::Invalid);
    }

    #[test]
    fn a_named_step_must_be_the_history_head() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let (slide, shape) = key_of(&ad, &doc, 0);
        ad.apply(&mut s, 0, "a", &move_envelope(&slide, &shape, 300.0, 0)).expect("move");

        let mut undo = ad.undo(&s, 1, "a", "u-1").expect("undo");
        if let Some(HistoryAction::Undo { op_id }) = &mut undo.history {
            *op_id = "some-other-op".into();
        }
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("wrong step");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert_eq!(ad.history.get("a").map(ActorHistory::undo_len), Some(1));
    }

    /// 8×8 red PNG.
    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73,
            68, 65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    #[test]
    fn a_picture_operation_runs_from_the_media_channel_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let bytes = png();
        let sha = aa.media_mut().put("red.png", bytes.clone()).expect("media");
        bb.media_mut().insert(&sha, bytes).expect("the same object");

        let slide = aa.keys().slide_key(doc_of(&a).slides[0].id).expect("slide").clone();
        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::PictureInsert { sha256: sha.clone(), name: "red.png".into(), rect: Some([50.0, 60.0, 80.0, 80.0]) },
        );
        let da = aa.apply(&mut a, 0, "a", &envelope).expect("apply a");
        let db = bb.apply(&mut b, 0, "b", &envelope).expect("apply b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas hold the same picture");
        let after = doc_of(&a);
        let picture = after.slides[0].shapes.iter().find(|s| matches!(s.kind, ShapeKind::Picture { .. })).expect("a picture shape");
        assert_eq!(picture.xfrm.expect("xfrm").x, 50.0);
        assert_eq!(aa.media().len(), 1, "one object for one digest");
    }

    #[test]
    fn a_picture_operation_without_its_bytes_is_refused() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let slide = ad.keys().slide_key(doc_of(&s).slides[0].id).expect("slide").clone();
        let envelope = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::PictureInsert { sha256: crate::media::sha256_hex(&png()), name: "red.png".into(), rect: None },
        );
        let err = ad.apply(&mut s, 0, "a", &envelope).expect_err("no bytes");
        assert_eq!(err.code, RejectCode::Unsupported);
        assert!(err.retryable, "a fetch can make the same operation work");
    }

    fn command(op_id: &str, base: u64, slide: &SlideKey, shape: &ShapeKey, payload: Payload) -> OpEnvelope {
        OpEnvelope {
            op_id: op_id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            payload,
            history: None,
        }
    }

    fn fill_of(doc: &Presentation, index: usize) -> Option<deckcraft_model::Fill> {
        doc.slides[0].shapes[index].fill.clone()
    }

    #[test]
    fn undo_of_a_fill_and_an_outline_restores_the_values_they_replaced() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, first) = key_of(&aa, &doc_of(&a), 0);

        let red = Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } };
        let green = Payload::ShapeFill { fill: FillSpec::Solid { color: "#00ff00".into() } };
        let outline = Payload::ShapeLine { line: LineSpec { color: Some("#0000ff".into()), width: Some(2.0), ..LineSpec::default() } };
        aa.apply(&mut a, 0, "a", &command("op-1", 0, &slide, &first, red)).expect("red");
        let red_fill = fill_of(&doc_of(&a), 0).expect("a fill");
        aa.apply(&mut a, 1, "a", &command("op-2", 1, &slide, &first, green)).expect("green");
        aa.apply(&mut a, 2, "a", &command("op-3", 2, &slide, &first, outline)).expect("outline");
        // Replica b sees the same confirmed operations.
        for op in [
            command("op-1", 0, &slide, &first, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } }),
            command("op-2", 1, &slide, &first, Payload::ShapeFill { fill: FillSpec::Solid { color: "#00ff00".into() } }),
            command(
                "op-3",
                2,
                &slide,
                &first,
                Payload::ShapeLine { line: LineSpec { color: Some("#0000ff".into()), width: Some(2.0), ..LineSpec::default() } },
            ),
        ] {
            let base = op.base_version;
            bb.apply_confirmed(&mut b, base, "a", &op).expect("b applies the confirmed command");
        }
        assert_eq!(doc_of(&a), doc_of(&b));

        // Undo the outline, then the second fill: each puts back the value its own step replaced.
        let undo_outline = aa.undo(&a, 3, "a", "undo-1").expect("undo outline");
        assert!(matches!(undo_outline.payload, Payload::ShapeField { field: ShapeField::Line, .. }));
        aa.apply(&mut a, 3, "a", &undo_outline).expect("undo outline");
        bb.apply_confirmed(&mut b, 3, "a", &undo_outline).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "the restore converges");
        assert!(doc_of(&a).slides[0].shapes[0].line.is_none(), "the outline is gone again");

        let undo_green = aa.undo(&a, 4, "a", "undo-2").expect("undo green");
        aa.apply(&mut a, 4, "a", &undo_green).expect("undo green");
        bb.apply_confirmed(&mut b, 4, "a", &undo_green).expect("b applies the undo");
        assert_eq!(fill_of(&doc_of(&a), 0), Some(red_fill.clone()), "the first fill is back");
        assert_eq!(doc_of(&a), doc_of(&b));

        // Redo puts the green fill back as a new operation.
        let redo = aa.redo(&a, 5, "a", "redo-1").expect("redo");
        aa.apply(&mut a, 5, "a", &redo).expect("redo green");
        assert_ne!(fill_of(&doc_of(&a), 0), Some(red_fill), "green is back");
    }

    #[test]
    fn an_undo_after_someone_else_changed_the_same_fill_is_refused() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        ad.apply(&mut s, 0, "a", &command("op-1", 0, &slide, &first, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } }))
            .expect("a fills red");
        ad.apply(&mut s, 1, "b", &command("op-2", 1, &slide, &first, Payload::ShapeFill { fill: FillSpec::Solid { color: "#00ff00".into() } }))
            .expect("b fills green");

        let err = ad.undo(&s, 2, "a", "undo-1").expect_err("b changed the same object");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable, "an undo never overwrites someone else's edit");
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(1), "a refused undo moves nothing");
    }

    #[test]
    fn undo_of_a_format_step_restores_the_body_formatting() {
        let mut s = deck();
        s.apply_remote("text.set", &json!({"id": doc_of(&s).slides[0].shapes[0].id.0, "text": "Hello"})).expect("text");
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        let bold =
            |op_id: &str, base: u64| command(op_id, base, &slide, &first, Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}) });
        ad.apply(&mut s, 0, "a", &bold("op-1", 0)).expect("bold");
        let bold_of = |s: &Session| {
            doc_of(s).slides[0].shapes[0].text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).and_then(|r| r.props.bold)
        };
        assert_eq!(bold_of(&s), Some(true));

        let undo = ad.undo(&s, 1, "a", "undo-1").expect("undo bold");
        assert!(matches!(undo.payload, Payload::ShapeField { field: ShapeField::Text, .. }), "{undo:?}");
        ad.apply(&mut s, 1, "a", &undo).expect("undo bold");
        assert_eq!(bold_of(&s), None, "the body formatting went back to what it was");
        let redo = ad.redo(&s, 2, "a", "redo-1").expect("redo bold");
        ad.apply(&mut s, 2, "a", &redo).expect("redo bold");
        assert_eq!(bold_of(&s), Some(true));
    }

    #[test]
    fn an_undo_after_someone_else_formatted_the_same_text_is_refused() {
        let mut s = deck();
        s.apply_remote("text.set", &json!({"id": doc_of(&s).slides[0].shapes[0].id.0, "text": "Hello"})).expect("text");
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        let bold = command("op-1", 0, &slide, &first, Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}) });
        let italic = command("op-2", 1, &slide, &first, Payload::FormatSet { format: FormatCmd::Italic, args: json!({"on": true}) });
        ad.apply(&mut s, 0, "a", &bold).expect("a bolds");
        ad.apply(&mut s, 1, "b", &italic).expect("b italicises");
        let err = ad.undo(&s, 2, "a", "undo-1").expect_err("b changed the same body");
        assert_eq!(err.code, RejectCode::UndoConflict);
    }

    #[test]
    fn undo_of_a_group_fill_restores_the_group_and_every_member() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let ids: Vec<ShapeKey> = doc.slides[0].shapes.iter().map(|x| aa.keys().shape_key(doc.slides[0].id, x.id).expect("key").clone()).collect();
        let group = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), shapes: ids, ..Target::default() }, Payload::Group {});
        aa.apply(&mut a, 0, "a", &group).expect("group");
        bb.apply_confirmed(&mut b, 0, "a", &group).expect("b groups");
        let group_key = {
            let after = doc_of(&a);
            aa.keys().shape_key(after.slides[0].id, after.slides[0].shapes[0].id).expect("group key").clone()
        };

        let fill = command("op-2", 1, &slide, &group_key, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } });
        aa.apply(&mut a, 1, "a", &fill).expect("fill the group");
        bb.apply_confirmed(&mut b, 1, "a", &fill).expect("b fills the group");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(2), "the group step and its appearance step are recorded");

        let undo = aa.undo(&a, 2, "a", "undo-1").expect("undo the group fill");
        let Payload::ShapeFields { items } = &undo.payload else { panic!("a group restore is one merge payload: {:?}", undo.payload) };
        assert_eq!(items.len(), 3, "the group and its two members");
        assert_eq!(undo.kind, OpKind::Command);
        aa.apply(&mut a, 2, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 2, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the restore");
        let group = &doc_of(&a).slides[0].shapes[0];
        assert!(group.fill.is_none(), "the group's own fill is back to inherited");
        assert!(group.children().iter().all(|child| child.fill.is_none()), "every member's fill is back");

        let redo = aa.redo(&a, 3, "a", "redo-1").expect("redo the group fill");
        aa.apply(&mut a, 3, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 3, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        let group = &doc_of(&a).slides[0].shapes[0];
        assert!(matches!(group.fill, Some(deckcraft_model::Fill::Solid { .. })));
        assert!(group.children().iter().all(|child| matches!(child.fill, Some(deckcraft_model::Fill::Solid { .. }))));
    }

    #[test]
    fn a_group_undo_is_refused_when_someone_else_changed_one_member() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let doc = doc_of(&s);
        let slide = ad.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let ids: Vec<ShapeKey> = doc.slides[0].shapes.iter().map(|x| ad.keys().shape_key(doc.slides[0].id, x.id).expect("key").clone()).collect();
        ad.apply(
            &mut s,
            0,
            "a",
            &op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), shapes: ids, ..Target::default() }, Payload::Group {}),
        )
        .expect("group");
        let (group_key, member_key) = {
            let after = doc_of(&s);
            let group = after.slides[0].shapes[0].clone();
            let member = group.children()[0].id;
            (
                ad.keys().shape_key(after.slides[0].id, group.id).expect("group key").clone(),
                ad.keys().shape_key(after.slides[0].id, member).expect("member key").clone(),
            )
        };
        ad.apply(&mut s, 1, "a", &command("op-2", 1, &slide, &group_key, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } }))
            .expect("a fills the group");
        ad.apply(&mut s, 2, "b", &command("op-3", 2, &slide, &member_key, Payload::ShapeFill { fill: FillSpec::Solid { color: "#00ff00".into() } }))
            .expect("b repaints one member");
        let err = ad.undo(&s, 3, "a", "undo-1").expect_err("a member no longer holds the step's value");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(ad.history().get("a").map(|h| h.undo_len()), Some(2), "the refused undo moves nothing");
    }

    #[test]
    fn undo_of_a_slide_delete_restores_the_whole_slide_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        for s in [&mut a, &mut b] {
            s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second slide");
            s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Kept"})).expect("content");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let before = doc_of(&a);
        let slide = aa.keys().slide_key(before.slides[1].id).expect("slide").clone();

        let delete = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), ..Target::default() }, Payload::SlideDelete {});
        aa.apply(&mut a, 0, "a", &delete).expect("delete the slide");
        bb.apply_confirmed(&mut b, 0, "a", &delete).expect("b deletes it");
        assert_eq!(doc_of(&a).slides.len(), 1);
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(1), "a structural step is now undoable");

        let undo = aa.undo(&a, 1, "a", "undo-1").expect("undo the delete");
        assert!(matches!(undo.payload, Payload::SlideRestore { .. }), "{:?}", undo.payload);
        assert_eq!(undo.kind, OpKind::Structural, "putting a slide back changes every index after it");
        assert_eq!(undo.target.slide.as_ref(), Some(&slide));
        aa.apply(&mut a, 1, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the restored slide");
        assert_eq!(doc_of(&a).slides.len(), 2);
        assert_eq!(doc_of(&a).slides[1].shapes[0].text.as_ref().map(|t| t.text()), Some("Kept".into()), "the whole slide came back");

        let redo = aa.redo(&a, 2, "a", "redo-1").expect("redo the delete");
        assert_eq!(redo.payload, Payload::SlideDelete {});
        aa.apply(&mut a, 2, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides.len(), 1);
    }

    #[test]
    fn undo_of_a_slide_insert_deletes_it_and_redo_puts_it_back() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let insert = op(OpKind::Structural, 0, Target::default(), Payload::SlideInsert { before: None, layout: Some("blank".into()) });
        aa.apply(&mut a, 0, "a", &insert).expect("new slide");
        bb.apply_confirmed(&mut b, 0, "a", &insert).expect("b adds the slide");
        assert_eq!(doc_of(&a).slides.len(), 2);
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(1));

        let undo = aa.undo(&a, 1, "a", "undo-1").expect("undo the insert");
        assert_eq!(undo.payload, Payload::SlideDelete {});
        aa.apply(&mut a, 1, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides.len(), 1);

        let redo = aa.redo(&a, 2, "a", "redo-1").expect("redo the insert");
        assert!(matches!(redo.payload, Payload::SlideRestore { .. }), "{:?}", redo.payload);
        aa.apply(&mut a, 2, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides.len(), 2);
    }

    #[test]
    fn a_slide_restore_cannot_name_another_slide() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        // Add a second slide so a forged target exists.
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second slide");
        ad.join(&doc_of(&s));
        let key = |s: &Session, i: usize| ad.keys().slide_key(doc_of(s).slides[i].id).expect("slide key").clone();
        let (title, second) = (key(&s, 0), key(&s, 1));
        ad.apply(&mut s, 0, "a", &op(OpKind::Structural, 0, Target { slide: Some(title.clone()), ..Target::default() }, Payload::SlideDelete {}))
            .expect("delete");
        let mut undo = ad.undo(&s, 1, "a", "undo-1").expect("undo");
        undo.target.slide = Some(second);
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("a restore must address its own slide");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s).slides.len(), 1, "nothing was resurrected");
    }

    #[test]
    fn a_restore_value_is_refused_without_a_history_step() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        let forged = command(
            "op-1",
            0,
            &slide,
            &first,
            Payload::ShapeField { field: ShapeField::Fill, value: serde_json::to_value(deckcraft_model::Fill::Background).expect("json") },
        );
        let err = ad.apply(&mut s, 0, "a", &forged).expect_err("an exact value is not a user edit");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(fill_of(&doc_of(&s), 0).is_none(), "nothing was written");
    }

    #[test]
    fn an_undo_cannot_replace_its_recorded_value_with_another() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        ad.apply(&mut s, 0, "a", &command("op-1", 0, &slide, &first, Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } }))
            .expect("a fills red");
        let mut undo = ad.undo(&s, 1, "a", "undo-1").expect("undo");
        // A client could send its own history action; it may only repeat the value the step recorded.
        undo.payload = Payload::ShapeField { field: ShapeField::Fill, value: serde_json::to_value(deckcraft_model::Fill::Background).expect("json") };
        let err = ad.apply(&mut s, 1, "a", &undo).expect_err("a different value");
        assert_eq!(err.code, RejectCode::Invalid);
        assert!(matches!(fill_of(&doc_of(&s), 0), Some(deckcraft_model::Fill::Solid { .. })), "the recorded fill is still there");
    }

    #[test]
    fn undo_of_a_shape_insert_deletes_it_after_a_foreign_edit_of_a_neighbour() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let (slide, first) = key_of(&aa, &doc_of(&a), 0);
        let insert = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), ..Target::default() },
            Payload::ShapeInsert { preset: "star5".into(), rect: [200.0, 300.0, 120.0, 120.0], text: None },
        );
        aa.apply(&mut a, 0, "a", &insert).expect("a inserts");
        bb.apply_confirmed(&mut b, 0, "a", &insert).expect("b inserts");
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 3);
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(1), "a shape insert is undoable now");
        let (_, inserted) = key_of(&aa, &doc_of(&a), 2);

        // b moves a neighbour after the insert: that edit must survive a's undo.
        let move_b = command("op-b", 1, &slide, &first, Payload::ShapeMove { x: 520.0, y: 100.0 });
        aa.apply_confirmed(&mut a, 1, "b", &move_b).expect("a sees b's move");
        bb.apply(&mut b, 1, "b", &move_b).expect("b moves");

        let undo = aa.undo(&a, 2, "a", "undo-1").expect("undo the insert");
        assert_eq!(undo.payload, Payload::ShapeDelete {});
        assert_eq!(undo.kind, OpKind::Structural);
        assert_eq!(undo.target.shapes, vec![inserted], "the delete names the inserted shape");
        aa.apply(&mut a, 2, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 2, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the undo");
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 2, "the inserted shape is gone");
        assert_eq!(shape_x(&doc_of(&a), 0), 520.0, "b's move stays");

        let redo = aa.redo(&a, 3, "a", "redo-1").expect("redo the insert");
        assert!(matches!(redo.payload, Payload::ShapeRestore { .. }), "{:?}", redo.payload);
        aa.apply(&mut a, 3, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 3, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 3, "the shape came back at the top of the order");
    }

    #[test]
    fn undo_of_a_shape_insert_is_refused_after_someone_edited_the_inserted_shape() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, _) = key_of(&ad, &doc_of(&s), 0);
        ad.apply(
            &mut s,
            0,
            "a",
            &op(
                OpKind::Structural,
                0,
                Target { slide: Some(slide.clone()), ..Target::default() },
                Payload::ShapeInsert { preset: "star5".into(), rect: [200.0, 300.0, 120.0, 120.0], text: None },
            ),
        )
        .expect("insert");
        let (_, inserted) = key_of(&ad, &doc_of(&s), 2);
        ad.apply(&mut s, 1, "b", &command("op-b", 1, &slide, &inserted, Payload::ShapeFill { fill: FillSpec::Solid { color: "#00ff00".into() } }))
            .expect("b repaints the inserted shape");
        let err = ad.undo(&s, 2, "a", "undo-1").expect_err("deleting it would drop b's edit");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert!(!err.retryable);
        assert_eq!(doc_of(&s).slides[0].shapes.len(), 3, "nothing was deleted");
    }

    #[test]
    fn undo_of_a_shape_delete_restores_it_among_changed_neighbours_on_both_replicas() {
        let (mut a, mut b) = (deck(), deck());
        for s in [&mut a, &mut b] {
            let id = doc_of(s).slides[0].shapes[0].id.0;
            s.apply_remote("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id]})).expect("animation");
        }
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let first_id = doc_of(&a).slides[0].shapes[0].id;
        let (slide, first) = key_of(&aa, &doc_of(&a), 0);
        let (_, second) = key_of(&aa, &doc_of(&a), 1);
        let delete = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), shapes: vec![first.clone()], ..Target::default() },
            Payload::ShapeDelete {},
        );
        aa.apply(&mut a, 0, "a", &delete).expect("a deletes");
        bb.apply_confirmed(&mut b, 0, "a", &delete).expect("b deletes");
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 1);
        assert!(doc_of(&a).slides[0].animations.is_empty(), "the delete dropped the animation");

        // b moves the neighbour that is left.
        let move_b = command("op-b", 1, &slide, &second, Payload::ShapeMove { x: 600.0, y: 100.0 });
        aa.apply_confirmed(&mut a, 1, "b", &move_b).expect("a sees b's move");
        bb.apply(&mut b, 1, "b", &move_b).expect("b moves");

        let undo = aa.undo(&a, 2, "a", "undo-1").expect("undo the delete");
        assert!(matches!(undo.payload, Payload::ShapeRestore { .. }), "{:?}", undo.payload);
        assert_eq!(undo.kind, OpKind::Structural, "a restored shape shifts the z-order of its siblings");
        aa.apply(&mut a, 2, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 2, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the restored shape");
        let after = doc_of(&a);
        assert_eq!(after.slides[0].shapes.len(), 2);
        assert_eq!(after.slides[0].shapes[0].id, first_id, "the shape is back at the bottom of the order");
        assert_eq!(shape_x(&after, 1), 600.0, "the neighbour keeps the position b gave it");
        assert_eq!(after.slides[0].animations.len(), 1, "its animation came back");
        assert_eq!(after.slides[0].animations[0].shape, first_id);

        let redo = aa.redo(&a, 3, "a", "redo-1").expect("redo the delete");
        assert_eq!(redo.payload, Payload::ShapeDelete {});
        assert_eq!(redo.target.shapes, vec![first]);
        aa.apply(&mut a, 3, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 3, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 1);
    }

    #[test]
    fn undo_of_a_group_delete_restores_the_group_and_its_children_in_one_image() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let ids: Vec<ShapeKey> = doc.slides[0].shapes.iter().map(|x| aa.keys().shape_key(doc.slides[0].id, x.id).expect("key").clone()).collect();
        let group = op(OpKind::Structural, 0, Target { slide: Some(slide.clone()), shapes: ids, ..Target::default() }, Payload::Group {});
        aa.apply(&mut a, 0, "a", &group).expect("group");
        bb.apply_confirmed(&mut b, 0, "a", &group).expect("b groups");
        let group_id = doc_of(&a).slides[0].shapes[0].id;
        let (_, group_key) = key_of(&aa, &doc_of(&a), 0);

        let delete = op(OpKind::Structural, 1, Target { slide: Some(slide), shapes: vec![group_key], ..Target::default() }, Payload::ShapeDelete {});
        aa.apply(&mut a, 1, "a", &delete).expect("delete the group");
        bb.apply_confirmed(&mut b, 1, "a", &delete).expect("b deletes it");
        assert!(doc_of(&a).slides[0].shapes.is_empty());

        let undo = aa.undo(&a, 2, "a", "undo-1").expect("undo the delete");
        let Payload::ShapeRestore { items, .. } = &undo.payload else { panic!("a group restore: {:?}", undo.payload) };
        assert_eq!(items.len(), 1, "one image carries the whole group");
        assert_eq!(items[0].value.get("id").and_then(Value::as_u64), Some(group_id.0 as u64));
        assert_eq!(
            items[0].value.get("kind").and_then(|k| k.get("children")).and_then(Value::as_array).map(Vec::len),
            Some(2),
            "the children are inside the image"
        );
        aa.apply(&mut a, 2, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 2, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        let group = &doc_of(&a).slides[0].shapes[0];
        assert_eq!(group.id, group_id, "the group came back with its id");
        assert_eq!(group.children().len(), 2, "and its members");
    }

    /// The final boxes `arrange.align` would send for the two shapes of `deck()` aligned to the
    /// selection's centre (union `[100, 520]`, centre `310`).
    fn aligned_boxes(adapter: &Adapter, doc: &Presentation) -> Vec<ShapeBounds> {
        let (_, first) = key_of(adapter, doc, 0);
        let (_, second) = key_of(adapter, doc, 1);
        vec![
            ShapeBounds { shape: first, x: 210.0, y: 100.0, w: 200.0, h: 100.0 },
            ShapeBounds { shape: second, x: 250.0, y: 100.0, w: 120.0, h: 100.0 },
        ]
    }

    #[test]
    fn align_is_one_operation_and_one_undo_step_on_two_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let items = aligned_boxes(&aa, &doc);
        let target = Target { slide: Some(slide), shapes: items.iter().map(|b| b.shape.clone()).collect(), ..Target::default() };
        let envelope = op(OpKind::Command, 0, target, Payload::ShapesSetBounds { items });

        let da = aa.apply(&mut a, 0, "alice", &envelope).expect("apply a");
        let db = bb.apply(&mut b, 0, "bob", &envelope).expect("apply b");
        assert_eq!(da, db, "the same operation has the same delta on both replicas");
        assert_eq!(doc_of(&a), doc_of(&b), "the replicas converge");
        let xs: Vec<f64> = doc_of(&a).slides[0].shapes.iter().map(|sh| sh.xfrm.expect("xfrm").x).collect();
        assert_eq!(xs, vec![210.0, 250.0]);
        assert_eq!(aa.history().get("alice").map(|h| h.undo_len()), Some(1), "one step for the whole selection");

        // One undo moves both objects back, in one operation and one new version.
        let undo = aa.undo(&a, 1, "alice", "alice-undo").expect("undo");
        assert!(matches!(&undo.payload, Payload::ShapesSetBounds { items } if items.len() == 2));
        aa.apply(&mut a, 1, "alice", &undo).expect("undo a");
        bb.apply(&mut b, 1, "bob", &undo).expect("undo b");
        let xs: Vec<f64> = doc_of(&a).slides[0].shapes.iter().map(|sh| sh.xfrm.expect("xfrm").x).collect();
        assert_eq!(xs, vec![100.0, 400.0], "both objects are back");
        assert_eq!(doc_of(&a), doc_of(&b));

        let redo = aa.redo(&a, 2, "alice", "alice-redo").expect("redo");
        aa.apply(&mut a, 2, "alice", &redo).expect("redo a");
        bb.apply(&mut b, 2, "bob", &redo).expect("redo b");
        let xs: Vec<f64> = doc_of(&a).slides[0].shapes.iter().map(|sh| sh.xfrm.expect("xfrm").x).collect();
        assert_eq!(xs, vec![210.0, 250.0], "redo puts the aligned boxes back");
        assert_eq!(doc_of(&a), doc_of(&b));
    }

    #[test]
    fn an_align_undo_is_refused_after_someone_else_moved_a_member() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let slide = aa.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let items = aligned_boxes(&aa, &doc);
        let first = items[0].shape.clone();
        let target = Target { slide: Some(slide.clone()), shapes: items.iter().map(|b| b.shape.clone()).collect(), ..Target::default() };
        let align = op(OpKind::Command, 0, target, Payload::ShapesSetBounds { items });
        aa.apply(&mut a, 0, "alice", &align).expect("align a");
        bb.apply(&mut b, 0, "bob", &align).expect("align b");

        let foreign =
            op(OpKind::Command, 1, Target { slide: Some(slide), shape: Some(first), ..Target::default() }, Payload::ShapeMove { x: 700.0, y: 300.0 });
        bb.apply(&mut b, 1, "bob", &foreign).expect("bob moves a member");
        aa.apply(&mut a, 1, "bob", &foreign).expect("a sees bob's move");

        let err = aa.undo(&a, 2, "alice", "alice-undo").expect_err("undo conflict");
        assert_eq!(err.code, RejectCode::UndoConflict, "alice's step no longer holds the object");
    }

    #[test]
    fn undo_of_group_restores_members_and_redo_groups_them_on_two_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let before = doc_of(&a);
        let (slide, first) = key_of(&aa, &before, 0);
        let (_, second) = key_of(&aa, &before, 1);
        let group = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), shapes: vec![first.clone(), second.clone()], ..Target::default() },
            Payload::Group {},
        );
        aa.apply(&mut a, 0, "a", &group).expect("a groups");
        bb.apply_confirmed(&mut b, 0, "a", &group).expect("b groups");
        let grouped = doc_of(&a);
        assert_eq!(grouped.slides[0].shapes.len(), 1, "one group holds both shapes");
        let (_, group_key) = key_of(&aa, &grouped, 0);
        assert_eq!(aa.history().get("a").map(|h| h.undo_len()), Some(1), "grouping is one step");

        let undo = aa.undo(&a, 1, "a", "undo-1").expect("undo the group");
        let Payload::ShapeRestore { items, remove } = &undo.payload else { panic!("a group restore: {:?}", undo.payload) };
        assert_eq!(remove, &vec![group_key.clone()], "the group is taken out");
        assert_eq!(items.len(), 2, "both members come back at the top level");
        assert!(items.iter().all(|item| item.parent.is_none()));
        aa.apply(&mut a, 1, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 1, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b), "both replicas converge on the undo");
        assert_eq!(doc_of(&a).slides[0].shapes, before.slides[0].shapes, "the members are back exactly where they were");
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 2);

        let redo = aa.redo(&a, 2, "a", "redo-1").expect("redo the group");
        let Payload::ShapeRestore { items, remove } = &redo.payload else { panic!("a group restore: {:?}", redo.payload) };
        assert_eq!(items.len(), 1, "one image carries the whole group");
        assert_eq!(remove.len(), 2, "its members are taken out first");
        aa.apply(&mut a, 2, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 2, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a).slides[0].shapes, grouped.slides[0].shapes, "the group came back with its id and children");
    }

    #[test]
    fn undo_of_ungroup_restores_the_group_on_two_replicas() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, first) = key_of(&aa, &doc, 0);
        let (_, second) = key_of(&aa, &doc, 1);
        let group = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), shapes: vec![first.clone(), second.clone()], ..Target::default() },
            Payload::Group {},
        );
        aa.apply(&mut a, 0, "a", &group).expect("a groups");
        bb.apply_confirmed(&mut b, 0, "a", &group).expect("b groups");
        let grouped = doc_of(&a);
        let (_, group_key) = key_of(&aa, &grouped, 0);

        let ungroup =
            op(OpKind::Structural, 1, Target { slide: Some(slide), shape: Some(group_key.clone()), ..Target::default() }, Payload::Ungroup {});
        aa.apply(&mut a, 1, "a", &ungroup).expect("a ungroups");
        bb.apply_confirmed(&mut b, 1, "a", &ungroup).expect("b ungroups");
        let ungrouped = doc_of(&a);
        assert_eq!(ungrouped.slides[0].shapes.len(), 2, "the members are at the top level again");

        let undo = aa.undo(&a, 2, "a", "undo-1").expect("undo the ungroup");
        let Payload::ShapeRestore { items, remove } = &undo.payload else { panic!("a group restore: {:?}", undo.payload) };
        assert_eq!(items.len(), 1, "the whole group image goes back");
        assert_eq!(remove.len(), 2, "its members are taken out first");
        aa.apply(&mut a, 2, "a", &undo).expect("undo on a");
        bb.apply_confirmed(&mut b, 2, "a", &undo).expect("b applies the undo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a), grouped, "the group and its child values are back");

        let redo = aa.redo(&a, 3, "a", "redo-1").expect("redo the ungroup");
        let Payload::ShapeRestore { items, remove } = &redo.payload else { panic!("a group restore: {:?}", redo.payload) };
        assert_eq!(items.len(), 2, "the members with their ungrouped geometry");
        assert_eq!(remove, &vec![group_key], "the group is taken out");
        aa.apply(&mut a, 3, "a", &redo).expect("redo on a");
        bb.apply_confirmed(&mut b, 3, "a", &redo).expect("b applies the redo");
        assert_eq!(doc_of(&a), doc_of(&b));
        assert_eq!(doc_of(&a), ungrouped);
    }

    #[test]
    fn undo_of_a_group_is_refused_after_someone_moved_a_member() {
        let (mut a, mut b) = (deck(), deck());
        let mut aa = Adapter::new("k");
        let mut bb = Adapter::new("k");
        aa.join(&doc_of(&a));
        bb.join(&doc_of(&b));
        let doc = doc_of(&a);
        let (slide, first) = key_of(&aa, &doc, 0);
        let (_, second) = key_of(&aa, &doc, 1);
        let group = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide.clone()), shapes: vec![first.clone(), second], ..Target::default() },
            Payload::Group {},
        );
        aa.apply(&mut a, 0, "a", &group).expect("group");
        bb.apply_confirmed(&mut b, 0, "a", &group).expect("b groups");

        // b moves a member inside the group: the recorded group image no longer holds.
        let moved = command("op-b", 1, &slide, &first, Payload::ShapeMove { x: 520.0, y: 100.0 });
        aa.apply_confirmed(&mut a, 1, "b", &moved).expect("a sees b's move");
        bb.apply(&mut b, 1, "b", &moved).expect("b moves");
        let err = aa.undo(&a, 2, "a", "undo-1").expect_err("undo would drop b's move");
        assert_eq!(err.code, RejectCode::UndoConflict);
        assert_eq!(doc_of(&a).slides[0].shapes.len(), 1, "nothing was ungrouped");
    }

    #[test]
    fn a_shape_restore_is_refused_without_a_history_step() {
        let mut s = deck();
        let mut ad = Adapter::new("k");
        ad.join(&doc_of(&s));
        let (slide, first) = key_of(&ad, &doc_of(&s), 0);
        let forged = op(
            OpKind::Structural,
            0,
            Target { slide: Some(slide), ..Target::default() },
            Payload::ShapeRestore {
                items: vec![ShapeRestoreItem { shape: first, parent: None, index: 0, value: json!({"id": 999}), animations: Vec::new() }],
                remove: Vec::new(),
            },
        );
        let err = ad.apply(&mut s, 0, "a", &forged).expect_err("a restore is not a user edit");
        assert_eq!(err.code, RejectCode::Invalid);
        assert_eq!(doc_of(&s).slides[0].shapes.len(), 2, "nothing was inserted");
    }
}
